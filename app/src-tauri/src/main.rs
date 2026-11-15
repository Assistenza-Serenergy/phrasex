//! Phrase X desktop app: library editor (React UI) + tray icon + background expansion,
//! hotkey popup, fill-in forms, per-phrase hotkeys, settings.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipimg;
mod cloud;

use phrasex_engine::macros::{self, Action, Context, DateTime, Field, FieldKind};
use phrasex_engine::{Case, Engine, Key, Mode, Options, Snippet};
use phrasex_store::{Entry, Folder, Store};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WindowEvent, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

const DEFAULT_POPUP_HOTKEY: &str = "Ctrl+Shift+Space";
const DEFAULT_CLIP_HOTKEY: &str = "Ctrl+Alt+V";

struct AppState {
    store: Mutex<Store>,
    engine: Arc<Mutex<Engine>>,
    paused: Arc<AtomicBool>,
    excluded: Arc<Mutex<Vec<String>>>,
    /// global shortcut id -> phrase ids (several = the user picks one)
    hotkeys: Mutex<HashMap<u32, Vec<i64>>>,
    popup_hotkey: Mutex<Option<u32>>,
    clip_hotkey: Mutex<Option<u32>>,
    form: Mutex<Option<PendingForm>>,
    /// Phrases waiting for the user's pick in the chooser (shared trigger or hotkey).
    choice: Mutex<Option<Choice>>,
    /// Our main window (expansion works in it, except in fields marked no-expand).
    main_window: AtomicIsize,
    /// The focused field of the main window must not expand (abbreviation, shortcut, search).
    no_expand: AtomicBool,
    /// A shortcut field is recording: global hotkeys are off so it receives the keys.
    recording: AtomicBool,
}

struct Choice {
    ids: Vec<i64>,
    backspaces: usize,
    case: Case,
    suffix: String,
}

struct PendingForm {
    fields: Vec<FieldDto>,
    reply: mpsc::Sender<Option<HashMap<String, String>>>,
}

/// Serializes insertions (typed trigger, popup, tray, hotkey): one form / one injection at a time.
static DELIVER: Mutex<()> = Mutex::new(());

// ---------- DTOs shared with the UI ----------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryDto {
    id: i64,
    folder_id: Option<i64>,
    name: String,
    trigger: String,
    body: String,
    immediate: bool,
    case_sensitive: bool,
    enabled: bool,
    hotkey: String,
    #[serde(default)]
    uses: i64,
    /// Part of a library someone shared with this user: read-only.
    #[serde(default)]
    shared: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderDto {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    hotkey: String,
    color: String,
    shared: bool,
}

#[derive(Serialize)]
struct Library {
    folders: Vec<FolderDto>,
    entries: Vec<EntryDto>,
    paused: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Settings {
    lang: String,
    popup_hotkey: String,
    autostart: bool,
    run_as_admin: bool,
    excluded_apps: Vec<String>,
    clip_history: bool,
    clip_hotkey: String,
    fix_double_caps: bool,
    capitalize_sentences: bool,
    /// Order of the phrases in the popup and in the menus: "uses" (most used first), "name" (A-Z) or "tree" (as in the library).
    #[serde(default)]
    popup_sort: String,
    /// Emails that see this user's phrases (read-only) when they sign in to Phrase X.
    #[serde(default)]
    share_with: Vec<String>,
}

#[derive(Serialize)]
struct ClipDto {
    id: i64,
    text: String,
    /// data: URL of a copied picture ("" for text)
    image: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FieldDto {
    label: String,
    kind: &'static str,
    options: Vec<String>,
    check_text: String,
}

impl From<&Field> for FieldDto {
    fn from(f: &Field) -> Self {
        let (kind, options, check_text) = match &f.kind {
            FieldKind::Text => ("text", vec![], String::new()),
            FieldKind::Multiline => ("multiline", vec![], String::new()),
            FieldKind::Date => ("date", vec![], String::new()),
            FieldKind::Check(t) => ("check", vec![], t.clone()),
            FieldKind::Choice(o) => ("choice", o.clone(), String::new()),
        };
        Self { label: f.label.clone(), kind, options, check_text }
    }
}

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn to_dto(e: Entry, uses: &HashMap<i64, i64>) -> EntryDto {
    EntryDto {
        uses: uses.get(&e.id).copied().unwrap_or(0),
        id: e.id,
        folder_id: e.folder_id,
        name: e.name,
        trigger: e.trigger.unwrap_or_default(),
        body: e.body,
        immediate: e.mode == Mode::Immediate,
        case_sensitive: e.case_sensitive,
        enabled: e.enabled,
        hotkey: e.hotkey.unwrap_or_default(),
        shared: false,
    }
}

fn load_settings(app: &AppHandle, store: &Store) -> Settings {
    let get = |k: &str| store.setting(k).ok().flatten().unwrap_or_default();
    let or = |v: String, d: &str| if v.is_empty() { d.to_string() } else { v };
    Settings {
        lang: get("lang"),
        popup_hotkey: or(get("popup_hotkey"), DEFAULT_POPUP_HOTKEY),
        clip_hotkey: or(get("clip_hotkey"), DEFAULT_CLIP_HOTKEY),
        clip_history: get("clip_history") != "off",
        fix_double_caps: get("fix_double_caps") != "off",
        capitalize_sentences: get("capitalize_sentences") == "on",
        popup_sort: match get("popup_sort").as_str() {
            s @ ("name" | "tree") => s.into(),
            _ => "uses".into(),
        },
        share_with: emails(&get("share_with")),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        run_as_admin: get("run_as_admin") == "on",
        excluded_apps: get("excluded_apps").lines().map(str::to_string).filter(|s| !s.is_empty()).collect(),
    }
}

/// One email per line, lowercase, without blanks and duplicates.
fn emails(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text.lines().map(|l| l.trim().to_lowercase()).filter(|l| l.contains('@')).collect();
    out.sort();
    out.dedup();
    out
}

fn lang(store: &Store) -> String {
    store.setting("lang").ok().flatten().filter(|l| l == "en").unwrap_or_else(|| "it".into())
}

// ---------- reload: engine, tray menu, global hotkeys ----------

/// Applies the stored library and settings everywhere. Returns ids of phrases and of folders whose hotkey
/// could not be registered, and whether the popup hotkey failed.
fn reload(app: &AppHandle, store: &Store) -> Res<(Vec<i64>, Vec<i64>, bool)> {
    SYNC_DIRTY.store(true, Relaxed);
    let state = app.state::<AppState>();
    let s = load_settings(app, store);
    let opts = Options { fix_double_caps: s.fix_double_caps, capitalize_sentences: s.capitalize_sentences };
    let snippets = store.engine_snippets().map_err(err)?.into_iter().map(|s| Snippet { mode: Mode::Immediate, ..s }).collect();
    *state.engine.lock().unwrap() = Engine::with_options(snippets, opts);
    *state.excluded.lock().unwrap() = s.excluded_apps.iter().map(|s| s.to_lowercase()).collect();

    let menu = tray_menu(app, store, &lang(store), state.paused.load(Relaxed)).map_err(err)?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu)).map_err(err)?;
    }

    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if state.recording.load(Relaxed) {
        return Ok((vec![], vec![], false)); // a shortcut field is listening: re-registered when it loses focus
    }
    let mut popup_failed = false;
    let settings = load_settings(app, store);
    let mut register = |hk: &str| match hk.parse::<Shortcut>() {
        Ok(sc) if gs.register(sc).is_ok() => Some(sc.id()),
        _ => {
            popup_failed = true;
            None
        }
    };
    *state.popup_hotkey.lock().unwrap() = register(&settings.popup_hotkey);
    *state.clip_hotkey.lock().unwrap() = if settings.clip_history { register(&settings.clip_hotkey) } else { None };
    let mut map: HashMap<u32, Vec<i64>> = HashMap::new();
    let mut failed = Vec::new();
    let entries = store.entries().map_err(err)?;
    for e in entries.clone() {
        let Some(hk) = e.hotkey.filter(|h| !h.is_empty()) else { continue };
        let Ok(sc) = hk.parse::<Shortcut>() else {
            failed.push(e.id);
            continue;
        };
        if Some(sc.id()) == *state.popup_hotkey.lock().unwrap() || Some(sc.id()) == *state.clip_hotkey.lock().unwrap() {
            failed.push(e.id);
        } else if let Some(ids) = map.get_mut(&sc.id()) {
            ids.push(e.id); // shared shortcut: a menu lets the user pick
        } else if gs.register(sc).is_ok() {
            map.insert(sc.id(), vec![e.id]);
        } else {
            failed.push(e.id);
        }
    }
    // Folder shortcut: menu with the enabled phrases of the folder and its subfolders.
    let folders = store.folders().map_err(err)?;
    let links: Vec<(i64, Option<i64>)> = folders.iter().map(|f| (f.id, f.parent_id)).collect();
    let mut failed_folders = Vec::new();
    for f in folders {
        let Some(hk) = f.hotkey.filter(|h| !h.is_empty()) else { continue };
        let Ok(sc) = hk.parse::<Shortcut>() else {
            failed_folders.push(f.id);
            continue;
        };
        let inside = subtree(&links, f.id);
        let ids = entries.iter().filter(|e| e.enabled && e.folder_id.is_some_and(|id| inside.contains(&id))).map(|e| e.id);
        if Some(sc.id()) == *state.popup_hotkey.lock().unwrap() || Some(sc.id()) == *state.clip_hotkey.lock().unwrap() {
            failed_folders.push(f.id);
        } else if let Some(list) = map.get_mut(&sc.id()) {
            list.extend(ids.filter(|id| !list.contains(id)).collect::<Vec<_>>());
        } else if gs.register(sc).is_ok() {
            map.insert(sc.id(), ids.collect());
        } else {
            failed_folders.push(f.id);
        }
    }
    *state.hotkeys.lock().unwrap() = map;
    Ok((failed, failed_folders, popup_failed))
}

// ---------- cloud sync ("Accedi con Google") ----------

/// One sync at a time (background loop and "Sincronizza ora").
static SYNC: Mutex<()> = Mutex::new(());
/// The library changed since the last sync: the background loop syncs within a minute.
static SYNC_DIRTY: AtomicBool = AtomicBool::new(true);
const SYNC_EVERY: Duration = Duration::from_secs(300);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SyncStatus {
    configured: bool,
    email: String,
    last: String,
    error: String,
    share_error: String,
    /// The user's name on their Google account (empty until they sign in with the profile scope).
    name: String,
}

fn sync_status_of(store: &Store) -> SyncStatus {
    let get = |k: &str| store.setting(k).ok().flatten().unwrap_or_default();
    SyncStatus {
        configured: cloud::configured(),
        email: get("sync_email"),
        last: get("sync_last"),
        error: get("sync_error"),
        share_error: get("share_error"),
        name: get("sync_name"),
    }
}

/// Publishes this user's library for the emails in Settings (when it or the list changed), then
/// brings in the libraries other people share with this user (when they changed).
fn share_round(app: &AppHandle) -> Res<()> {
    let mut sharing = cloud::Sharing::connect()?;
    {
        // Google puts the name only in the sign-in token, not in refreshed ones: keep the one saved at sign-in
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        if sharing.name != sharing.email {
            store.set_setting("sync_name", &sharing.name).map_err(err)?;
        } else if let Some(saved) = store.setting("sync_name").ok().flatten().filter(|n| !n.trim().is_empty()) {
            sharing.name = saved;
        }
    }
    let state = app.state::<AppState>();
    let (readers, lib, published, received) = {
        let store = state.store.lock().unwrap();
        let get = |k: &str| store.setting(k).ok().flatten().unwrap_or_default();
        (emails(&get("share_with")), store.sync_snapshot().map_err(err)?, get("share_published"), get("share_received"))
    };
    let lib = phrasex_store::sync::Library { tombstones: vec![], settings: None, ..lib }; // readers get phrases only
    let stamp = if readers.is_empty() { String::new() } else { cloud::fingerprint(&serde_json::to_string(&(&readers, &lib, &sharing.name)).map_err(err)?) };
    if stamp != published {
        sharing.publish(&readers, &lib)?;
        state.store.lock().unwrap().set_setting("share_published", &stamp).map_err(err)?;
    }
    let shares = sharing.incoming()?;
    let stamp = cloud::fingerprint(&serde_json::to_string(&shares).map_err(err)?);
    if stamp != received {
        let mut store = state.store.lock().unwrap();
        store.set_shared(&shares).map_err(err)?;
        store.set_setting("share_received", &stamp).map_err(err)?;
        drop(store);
        let h = app.clone();
        app.run_on_main_thread(move || {
            let state = h.state::<AppState>();
            let _ = reload(&h, &state.store.lock().unwrap());
            let _ = h.emit("library-changed", ());
        })
        .map_err(err)?;
    }
    Ok(())
}

/// Download, merge (newest edit per phrase wins), upload if the cloud is behind, apply if this PC is behind.
fn sync_round(app: &AppHandle) -> Res<()> {
    let state = app.state::<AppState>();
    let (local, adopt, switched) = {
        let store = state.store.lock().unwrap();
        let get = |k: &str| store.setting(k).ok().flatten().unwrap_or_default();
        let local = store.sync_snapshot().map_err(err)?;
        // the local library belongs to another Google account: this account's cloud library replaces it
        let switched = !get("sync_account").is_empty() && !get("sync_account").eq_ignore_ascii_case(&get("sync_email"));
        let adopt = get("sync_last").is_empty() && (local.is_pristine() || switched);
        (local, adopt, switched)
    };
    let mut drive = cloud::Drive::connect()?; // network without holding the store lock
    let plan = cloud::plan(&local, drive.download()?.as_ref(), adopt);
    if plan.upload {
        drive.upload(&plan.merged)?;
    }
    if plan.apply {
        let mut store = state.store.lock().unwrap();
        let now = store.sync_snapshot().map_err(err)?;
        let changed_meanwhile = now != local;
        // edits made during the download are newer: they win and go up on the next round
        let target = if changed_meanwhile { phrasex_store::sync::merge(&now, &plan.merged) } else { plan.merged };
        if switched {
            // the previous account's library is in its own cloud; keep a local copy too
            let dir = data_dir().join("backups");
            let _ = std::fs::create_dir_all(&dir);
            let _ = store.backup_to(&dir.join(format!("phrasex-cambio-account-{}.db", today())));
        }
        store.apply_sync(&target).map_err(err)?;
        drop(store);
        // reload registers hotkeys on the main thread: never from here while holding the store lock
        let h = app.clone();
        app.run_on_main_thread(move || {
            let state = h.state::<AppState>();
            let _ = reload(&h, &state.store.lock().unwrap());
            SYNC_DIRTY.store(changed_meanwhile, Relaxed);
            let _ = h.emit("library-changed", ());
        })
        .map_err(err)?;
    }
    Ok(())
}

/// Runs a sync round and records its outcome for Settings (also sent as the "sync-status" event).
fn sync_now_inner(app: &AppHandle) -> SyncStatus {
    let (result, share_error) = {
        let _one = SYNC.lock().unwrap();
        SYNC_DIRTY.store(false, Relaxed);
        let result = sync_round(app);
        let share = if result.is_ok() && cloud::sharing_configured() { share_round(app).err().unwrap_or_default() } else { String::new() };
        (result, share)
    };
    let state = app.state::<AppState>();
    let store = state.store.lock().unwrap();
    let _ = store.set_setting("share_error", &share_error);
    match &result {
        Ok(()) => {
            let t = phrasex_listener::System.now();
            let when = format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute);
            let _ = store.set_setting("sync_last", &when);
            let _ = store.set_setting("sync_error", "");
            // whose library this PC now holds (kept on sign-out, compared on the next sign-in)
            let _ = store.set_setting("sync_account", &store.setting("sync_email").ok().flatten().unwrap_or_default());
        }
        Err(e) => {
            SYNC_DIRTY.store(true, Relaxed); // retry on the next tick
            let _ = store.set_setting("sync_error", e);
            if e == cloud::SCOPE_MISSING || e == "google_relogin" || e == "google_signed_out" {
                // retrying can't help: forget the account so Settings shows "Accedi con Google" again
                cloud::logout();
                let _ = store.set_setting("sync_email", "");
                let _ = store.set_setting("sync_last", "");
            }
        }
    }
    let status = sync_status_of(&store);
    let _ = app.emit("sync-status", &status);
    status
}

fn signed_in(app: &AppHandle) -> bool {
    !app.state::<AppState>().store.lock().unwrap().setting("sync_email").ok().flatten().unwrap_or_default().is_empty()
}

/// Background sync: at start, within a minute of a change, and every few minutes for other PCs' changes.
fn sync_loop(app: AppHandle) {
    let mut last: Option<std::time::Instant> = None;
    loop {
        let due = last.map_or(true, |t| t.elapsed() >= SYNC_EVERY) || SYNC_DIRTY.load(Relaxed);
        if due && signed_in(&app) {
            sync_now_inner(&app);
            last = Some(std::time::Instant::now());
        }
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[tauri::command]
fn sync_status(state: State<AppState>) -> SyncStatus {
    sync_status_of(&state.store.lock().unwrap())
}

/// Opens Google sign-in in the browser and waits (up to 5 minutes), then syncs at once.
#[tauri::command(async)]
fn sync_login(app: AppHandle) -> Res<SyncStatus> {
    let (email, name) = cloud::login(|url| {
        phrasex_listener::open_url(url);
        let _ = app.emit("sync-login-url", url); // Settings offers "copy link" if no browser shows up
    })?;
    // close the browser tab the sign-in ended on (once its title shows up), then back to Phrase X
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(200));
        if phrasex_listener::close_tab_titled(cloud::LOGIN_DONE_TITLE) {
            break;
        }
    }
    let h = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = h.get_webview_window("main") {
            let _ = w.show();
            let _ = w.unminimize();
        }
        phrasex_listener::force_foreground(h.state::<AppState>().main_window.load(Relaxed));
    });
    {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.set_setting("sync_email", if email.is_empty() { "Google" } else { &email }).map_err(err)?;
        store.set_setting("sync_name", &name).map_err(err)?;
        store.set_setting("sync_last", "").map_err(err)?; // first sync of this PC with this account
        store.set_setting("sync_error", "").map_err(err)?;
    }
    Ok(sync_now_inner(&app))
}

#[tauri::command]
fn sync_cancel_login() {
    cloud::CANCEL_LOGIN.store(true, Relaxed);
}

#[tauri::command(async)]
fn sync_logout(app: AppHandle) -> SyncStatus {
    let _one = SYNC.lock().unwrap();
    cloud::logout();
    let state = app.state::<AppState>();
    let mut store = state.store.lock().unwrap();
    for k in ["sync_email", "sync_name", "sync_last", "sync_error", "share_error", "share_received", "share_published"] {
        let _ = store.set_setting(k, "");
    }
    // what others shared came with this account: it goes with it
    if store.set_shared(&[]).is_ok() {
        let _ = reload(&app, &store);
        let _ = app.emit("library-changed", ());
    }
    let status = sync_status_of(&store);
    let _ = app.emit("sync-status", &status);
    status
}

#[tauri::command(async)]
fn sync_now(app: AppHandle) -> SyncStatus {
    sync_now_inner(&app)
}

// ---------- commands ----------

#[tauri::command]
fn library(state: State<AppState>) -> Res<Library> {
    let store = state.store.lock().unwrap();
    let folders = store.folders().map_err(err)?;
    let entries = store.entries().map_err(err)?;
    let uses: HashMap<i64, i64> = store.usage().map_err(err)?.into_iter().collect();
    let (shared_folders, shared_entries) = store.shared_ids().map_err(err)?;
    Ok(Library {
        folders: folders
            .into_iter()
            .map(|f| FolderDto { shared: shared_folders.contains(&f.id), id: f.id, parent_id: f.parent_id, name: f.name, hotkey: f.hotkey.unwrap_or_default(), color: f.color.unwrap_or_default() })
            .collect(),
        entries: entries
            .into_iter()
            .map(|e| EntryDto { shared: shared_entries.contains(&e.id), ..to_dto(e, &uses) })
            .collect(),
        paused: state.paused.load(Relaxed),
    })
}

/// Libraries shared with this user are read-only, and nothing of the user's own may go inside them
/// (the next refresh of the share would delete it).
fn own_only(store: &Store, entries: &[i64], folders: &[Option<i64>]) -> Res<()> {
    let (shared_folders, shared_entries) = store.shared_ids().map_err(err)?;
    if entries.iter().any(|e| shared_entries.contains(e)) || folders.iter().flatten().any(|f| shared_folders.contains(f)) {
        return Err("shared_read_only".into());
    }
    Ok(())
}

#[tauri::command]
fn save_entry(app: AppHandle, state: State<AppState>, entry: EntryDto) -> Res<i64> {
    let hotkey = entry.hotkey.trim().to_string();
    if !hotkey.is_empty() && hotkey.parse::<Shortcut>().is_err() {
        return Err("invalid_hotkey".into());
    }
    let store = state.store.lock().unwrap();
    own_only(&store, &[entry.id], &[entry.folder_id])?;
    let trigger = entry.trigger.trim();
    let id = store
        .save(&Entry {
            id: entry.id,
            folder_id: entry.folder_id,
            name: entry.name.trim().to_string(),
            trigger: (!trigger.is_empty()).then(|| trigger.to_string()),
            body: entry.body,
            mode: if entry.immediate { Mode::Immediate } else { Mode::WordEnd },
            case_sensitive: entry.case_sensitive,
            enabled: entry.enabled,
            hotkey: (!hotkey.is_empty()).then_some(hotkey),
        })
        .map_err(err)?;
    let (failed, _, _) = reload(&app, &store)?;
    if failed.contains(&id) {
        return Err(format!("hotkey_unavailable:{id}"));
    }
    Ok(id)
}

/// Manual order of one folder's phrases (dragged in the tree).
#[tauri::command]
fn reorder_entries(app: AppHandle, state: State<AppState>, ids: Vec<i64>) -> Res<()> {
    let mut store = state.store.lock().unwrap();
    own_only(&store, &ids, &[])?;
    store.reorder(&ids).map_err(err)?;
    reload(&app, &store).map(drop)
}

/// Folder colour: a CSS hex colour, or empty for the default.
#[tauri::command]
fn set_folder_color(app: AppHandle, state: State<AppState>, id: i64, color: String) -> Res<()> {
    let ok = color.is_empty() || (color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit()));
    if !ok {
        return Err("invalid_color".into());
    }
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[Some(id)])?;
    store.set_folder_color(id, (!color.is_empty()).then_some(color.as_str())).map_err(err)?;
    reload(&app, &store).map(drop)
}

/// "Ordina A-Z" on a folder: its phrases go back to alphabetical order.
#[tauri::command]
fn sort_folder(app: AppHandle, state: State<AppState>, folder_id: Option<i64>) -> Res<()> {
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[folder_id])?;
    store.sort_folder(folder_id).map_err(err)?;
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn delete_entry(app: AppHandle, state: State<AppState>, id: i64) -> Res<()> {
    let store = state.store.lock().unwrap();
    own_only(&store, &[id], &[])?;
    if let Some(mut e) = store.entries().map_err(err)?.into_iter().find(|e| e.id == id) {
        let (name, parent) = (e.name.clone(), e.folder_id.take());
        let file = snapshot(vec![], vec![e])?;
        store.add_trash(&name, false, parent, &serde_json::to_string(&file).map_err(err)?).map_err(err)?;
    }
    store.delete(id).map_err(err)?;
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn add_folder(app: AppHandle, state: State<AppState>, parent_id: Option<i64>, name: String) -> Res<i64> {
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[parent_id])?;
    let id = store.add_folder(parent_id, name.trim()).map_err(err)?;
    reload(&app, &store)?;
    Ok(id)
}

#[tauri::command]
fn rename_folder(app: AppHandle, state: State<AppState>, id: i64, name: String) -> Res<()> {
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[Some(id)])?;
    store.rename_folder(id, name.trim()).map_err(err)?;
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn set_folder_hotkey(app: AppHandle, state: State<AppState>, id: i64, hotkey: String) -> Res<()> {
    let hotkey = hotkey.trim();
    if !hotkey.is_empty() && hotkey.parse::<Shortcut>().is_err() {
        return Err("invalid_hotkey".into());
    }
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[Some(id)])?;
    store.set_folder_hotkey(id, Some(hotkey).filter(|h| !h.is_empty())).map_err(err)?;
    let (_, failed, _) = reload(&app, &store)?;
    if failed.contains(&id) {
        return Err("hotkey_unavailable".into());
    }
    Ok(())
}

#[tauri::command]
fn move_folder(app: AppHandle, state: State<AppState>, id: i64, parent_id: Option<i64>) -> Res<()> {
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[Some(id), parent_id])?;
    if !store.move_folder(id, parent_id).map_err(err)? {
        return Err("folder_loop".into());
    }
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn delete_folder(app: AppHandle, state: State<AppState>, id: i64) -> Res<()> {
    let store = state.store.lock().unwrap();
    own_only(&store, &[], &[Some(id)])?;
    let (file, name, parent) = folder_snapshot(&store, id)?;
    store.add_trash(&name, true, parent, &serde_json::to_string(&file).map_err(err)?).map_err(err)?;
    store.delete_folder(id).map_err(err)?;
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn set_paused(app: AppHandle, state: State<AppState>, paused: bool) -> Res<()> {
    state.paused.store(paused, Relaxed);
    let store = state.store.lock().unwrap();
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn settings(app: AppHandle, state: State<AppState>) -> Settings {
    load_settings(&app, &state.store.lock().unwrap())
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Res<()> {
    if settings.popup_hotkey.parse::<Shortcut>().is_err() || settings.clip_hotkey.parse::<Shortcut>().is_err() {
        return Err("invalid_hotkey".into());
    }
    let store = state.store.lock().unwrap();
    store.set_setting("lang", &settings.lang).map_err(err)?;
    store.set_setting("popup_hotkey", settings.popup_hotkey.trim()).map_err(err)?;
    store.set_setting("clip_hotkey", settings.clip_hotkey.trim()).map_err(err)?;
    store.set_setting("clip_history", if settings.clip_history { "on" } else { "off" }).map_err(err)?;
    store.set_setting("fix_double_caps", if settings.fix_double_caps { "on" } else { "off" }).map_err(err)?;
    store.set_setting("capitalize_sentences", if settings.capitalize_sentences { "on" } else { "off" }).map_err(err)?;
    store.set_setting("share_with", &emails(&settings.share_with.join("\n")).join("\n")).map_err(err)?;
    let sort = settings.popup_sort.as_str();
    store.set_setting("popup_sort", if matches!(sort, "name" | "tree") { sort } else { "uses" }).map_err(err)?;
    if !settings.clip_history {
        store.clear_clips().map_err(err)?;
    }
    let apps: Vec<String> = settings.excluded_apps.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
    store.set_setting("excluded_apps", &apps.join("\n")).map_err(err)?;
    store.touch_settings().map_err(err)?; // goes to the user's other PCs on the next sync round
    let auto = app.autolaunch();
    let _ = if settings.autostart { auto.enable() } else { auto.disable() };
    let was_admin = store.setting("run_as_admin").ok().flatten().as_deref() == Some("on");
    if settings.run_as_admin != was_admin {
        let ok = if settings.run_as_admin { admin::create_task() } else { admin::delete_task() };
        if !ok {
            return Err("admin_cancelled".into());
        }
        store.set_setting("run_as_admin", if settings.run_as_admin { "on" } else { "off" }).map_err(err)?;
        if settings.run_as_admin {
            // restart elevated now; the new instance must start after this one is gone (single instance)
            store.set_setting(admin::LAUNCH_FLAG, "show").map_err(err)?;
            admin::run_task_after_exit();
            app.exit(0);
            return Ok(());
        }
    }
    let (_, _, popup_failed) = reload(&app, &store)?;
    if popup_failed {
        return Err("popup_hotkey_unavailable".into());
    }
    Ok(())
}

/// "Run as administrator" without a UAC prompt at every start: a Task Scheduler task with the highest
/// run level starts Phrase X elevated. Creating/deleting it asks UAC once; a normal start (Start menu,
/// autostart) just runs the task and exits.
mod admin {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const TASK: &str = "Phrase X";
    const NO_WINDOW: u32 = 0x0800_0000;
    /// Set right before running the task; the elevated instance clears it ("show" or "min").
    pub const LAUNCH_FLAG: &str = "elevated_launch";

    /// Runs schtasks elevated (UAC prompt unless already admin); false if refused or failed.
    fn schtasks_admin(args: &str) -> bool {
        let status = if phrasex_listener::is_elevated() {
            Command::new("schtasks").raw_arg(args).creation_flags(NO_WINDOW).status()
        } else {
            let ps = format!(
                "try {{ $p = Start-Process schtasks.exe -ArgumentList '{}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1 }}",
                args.replace('\'', "''")
            );
            Command::new("powershell").args(["-NoProfile", "-Command", &ps]).creation_flags(NO_WINDOW).status()
        };
        status.is_ok_and(|s| s.success())
    }

    pub fn task_xml(exe: &str, user: &str) -> String {
        let esc = |v: &str| v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Starts Phrase X as administrator, so it also types into elevated windows.</Description></RegistrationInfo>
  <Principals><Principal id="Author"><UserId>{}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>HighestAvailable</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>Parallel</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{}</Command></Exec></Actions>
</Task>
"#,
            esc(user),
            esc(exe)
        )
    }

    pub fn create_task() -> bool {
        let Ok(exe) = std::env::current_exe() else { return false };
        let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
        let xml = task_xml(&exe.to_string_lossy(), &user);
        let path = std::env::temp_dir().join("phrasex-task.xml");
        let mut bytes = vec![0xFF, 0xFE]; // UTF-16 LE with BOM, what schtasks expects
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        if std::fs::write(&path, bytes).is_err() {
            return false;
        }
        let ok = schtasks_admin(&format!("/Create /F /TN \"{TASK}\" /XML \"{}\"", path.display()));
        let _ = std::fs::remove_file(path);
        ok
    }

    pub fn delete_task() -> bool {
        schtasks_admin(&format!("/Delete /F /TN \"{TASK}\""))
    }

    /// Starts the elevated instance (no prompt once the task exists); false if the task is missing.
    pub fn run_task() -> bool {
        Command::new("schtasks").args(["/Run", "/TN", TASK]).creation_flags(NO_WINDOW).status().is_ok_and(|s| s.success())
    }

    /// Same, two seconds from now, so this instance can exit first.
    pub fn run_task_after_exit() {
        let _ = Command::new("cmd")
            .raw_arg(format!("/c ping -n 3 127.0.0.1 >nul & schtasks /Run /TN \"{TASK}\""))
            .creation_flags(NO_WINDOW)
            .spawn();
    }
}

/// Executable of the window the user was in before opening Phrase X ("" if none), for the exclusion list.
#[tauri::command]
fn running_apps() -> Vec<String> {
    phrasex_listener::recent_apps()
}

#[tauri::command]
fn popup_insert(app: AppHandle, id: i64) {
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.hide();
    }
    insert_phrase(&app, id);
}

// ---------- import / export / backup ----------

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFolder {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    hotkey: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    color: String,
}

#[derive(Serialize, Deserialize)]
struct ExportFile {
    app: String,
    version: u32,
    folders: Vec<ExportFolder>,
    phrases: Vec<EntryDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportReport {
    phrases: usize,
    folders: usize,
}

fn data_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("PhraseX")
}

fn today() -> String {
    let t = phrasex_listener::System.now();
    format!("{:04}-{:02}-{:02}", t.year, t.month, t.day)
}

/// Folder ids in the subtree rooted at `root` (root included).
fn subtree(folders: &[(i64, Option<i64>)], root: i64) -> std::collections::HashSet<i64> {
    let mut out = std::collections::HashSet::from([root]);
    loop {
        let before = out.len();
        for &(id, parent) in folders {
            if parent.is_some_and(|p| out.contains(&p)) {
                out.insert(id);
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

/// Keeps only file-name-safe characters.
fn file_safe(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect::<String>().trim().to_string()
}

/// Snapshot of some folders and phrases in the export format.
fn snapshot(folders: Vec<Folder>, entries: Vec<Entry>) -> Res<ExportFile> {
    let uses = HashMap::new();
    Ok(ExportFile {
        app: "Phrase X".into(),
        version: 1,
        folders: folders
            .into_iter()
            .map(|f| ExportFolder { id: f.id, parent_id: f.parent_id, name: f.name, hotkey: f.hotkey.unwrap_or_default(), color: f.color.unwrap_or_default() })
            .collect(),
        phrases: entries.into_iter().map(|e| to_dto(e, &uses)).collect(),
    })
}

/// A folder with its subfolders and phrases; the folder itself is made top level.
/// Also returns its name and original parent.
fn folder_snapshot(store: &Store, root: i64) -> Res<(ExportFile, String, Option<i64>)> {
    let mut folders = store.folders().map_err(err)?;
    let mut entries = store.entries().map_err(err)?;
    let keep = subtree(&folders.iter().map(|f| (f.id, f.parent_id)).collect::<Vec<_>>(), root);
    folders.retain(|f| keep.contains(&f.id));
    entries.retain(|e| e.folder_id.is_some_and(|f| keep.contains(&f)));
    let f = folders.iter_mut().find(|f| f.id == root).ok_or("folder_missing")?;
    let (name, parent) = (f.name.clone(), f.parent_id.take());
    Ok((snapshot(folders, entries)?, name, parent))
}

/// Writes the whole library, or one folder with its subfolders, to
/// Documents\PhraseX[-folder]-YYYY-MM-DD.json and shows it in Explorer, ready to send to someone else.
#[tauri::command]
fn export_data(state: State<AppState>, folder_id: Option<i64>) -> Res<String> {
    let store = state.store.lock().unwrap();
    let (file, label) = match folder_id {
        Some(root) => {
            let (file, name, _) = folder_snapshot(&store, root)?;
            (file, format!("-{}", file_safe(&name)))
        }
        None => {
            let (shared_folders, shared_entries) = store.shared_ids().map_err(err)?;
            let mut folders = store.folders().map_err(err)?;
            let mut entries = store.entries().map_err(err)?;
            folders.retain(|f| !shared_folders.contains(&f.id));
            entries.retain(|e| !shared_entries.contains(&e.id));
            (snapshot(folders, entries)?, String::new())
        }
    };
    let docs = std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Documents");
    let path = docs.join(format!("PhraseX{label}-{}.json", today()));
    std::fs::write(&path, serde_json::to_string_pretty(&file).map_err(err)?).map_err(err)?;
    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
    Ok(path.display().to_string())
}

/// Saves an imported phrase.
fn import_phrase(store: &mut Store, report: &mut ImportReport, folder: Option<i64>, p: EntryDto, keep_hotkey: bool) -> Res<()> {
    let e = Entry {
        id: 0,
        folder_id: folder,
        name: p.name,
        trigger: Some(p.trigger.trim().to_string()).filter(|t| !t.is_empty()),
        body: p.body,
        mode: if p.immediate { Mode::Immediate } else { Mode::WordEnd },
        case_sensitive: p.case_sensitive,
        enabled: p.enabled,
        // shortcuts are machine-specific: only kept when restoring from the trash
        hotkey: Some(p.hotkey).filter(|h| keep_hotkey && !h.is_empty()),
    };
    store.save(&e).map_err(err)?;
    report.phrases += 1;
    Ok(())
}

/// Adds an export snapshot under `root` (None = top level), keeping its folder tree.
fn import_snapshot(store: &mut Store, report: &mut ImportReport, file: ExportFile, root: Option<i64>, keep_hotkeys: bool) -> Res<()> {
    let mut ids: HashMap<i64, i64> = HashMap::new();
    // parents before children: repeat until every folder is placed
    let mut pending = file.folders;
    while !pending.is_empty() {
        let before = pending.len();
        let mut rest = Vec::new();
        for f in pending {
            match f.parent_id {
                Some(p) if !ids.contains_key(&p) => rest.push(f),
                parent => {
                    let new_parent = parent.and_then(|p| ids.get(&p).copied()).or(root);
                    let id = store.add_folder(new_parent, &f.name).map_err(err)?;
                    if keep_hotkeys && !f.hotkey.is_empty() {
                        store.set_folder_hotkey(id, Some(&f.hotkey)).map_err(err)?;
                    }
                    if !f.color.is_empty() {
                        store.set_folder_color(id, Some(&f.color)).map_err(err)?;
                    }
                    ids.insert(f.id, id);
                    report.folders += 1;
                }
            }
        }
        if rest.len() == before {
            break; // broken parent links: drop them
        }
        pending = rest;
    }
    for p in file.phrases {
        let folder = p.folder_id.and_then(|f| ids.get(&f).copied()).or(root);
        import_phrase(store, report, folder, p, keep_hotkeys)?;
    }
    Ok(())
}

/// Ctrl+C / Ctrl+V in the tree: copies of folders (with their content) and phrases under `target`.
/// Abbreviations are kept, shortcuts are not (a shortcut works only once).
#[tauri::command]
fn paste_items(app: AppHandle, state: State<AppState>, folders: Vec<i64>, entries: Vec<i64>, target: Option<i64>) -> Res<ImportReport> {
    let mut store = state.store.lock().unwrap();
    own_only(&store, &[], &[target])?;
    let mut report = ImportReport { phrases: 0, folders: 0 };
    for id in folders {
        let (file, _, _) = folder_snapshot(&store, id)?;
        import_snapshot(&mut store, &mut report, file, target, false)?;
    }
    let mut picked: Vec<Entry> = store.entries().map_err(err)?.into_iter().filter(|e| entries.contains(&e.id)).collect();
    picked.iter_mut().for_each(|e| e.folder_id = None);
    let file = snapshot(vec![], picked)?;
    import_snapshot(&mut store, &mut report, file, target, false)?;
    reload(&app, &store)?;
    Ok(report)
}

/// Imports a Phrase X JSON export, or a CSV with columns description;abbreviation;text
/// (comma or semicolon separated, header optional). Everything lands in a new folder.
#[tauri::command]
fn import_data(app: AppHandle, state: State<AppState>, content: String, file_name: String) -> Res<ImportReport> {
    import_check(&content, &file_name)?;
    let mut store = state.store.lock().unwrap();
    let mut report = ImportReport { phrases: 0, folders: 0 };
    let file: Option<ExportFile> = if content.trim_start().starts_with('{') {
        Some(serde_json::from_str(&content).map_err(|_| "import_invalid".to_string())?)
    } else {
        None
    };
    let base = file_name.rsplit(['\\', '/']).next().unwrap_or("Import").to_string();
    let root = Some(store.add_folder(None, &format!("Import {base}")).map_err(err)?);
    report.folders += 1;
    match file {
        Some(file) => import_snapshot(&mut store, &mut report, file, root, false)?,
        None => {
            for (i, cols) in parse_csv(&content).into_iter().enumerate() {
                if cols.len() < 3 || (i == 0 && cols[2].to_lowercase().starts_with("te")) {
                    continue; // header or incomplete row
                }
                let p = EntryDto {
                    id: 0, folder_id: None, name: cols[0].clone(), trigger: cols[1].clone(), body: cols[2].clone(),
                    immediate: true, case_sensitive: false, enabled: true, hotkey: String::new(), uses: 0, shared: false,
                };
                import_phrase(&mut store, &mut report, root, p, false)?;
            }
        }
    }
    reload(&app, &store)?;
    Ok(report)
}

/// Refuses files that aren't text: PhraseExpress libraries/exports (.pexdb/.pxp are encrypted) and binaries,
/// which the browser hands over as text full of U+FFFD and would become hundreds of garbage phrases.
fn import_check(content: &str, file_name: &str) -> Result<(), String> {
    let ext = file_name.rsplit('.').next().unwrap_or("").to_lowercase();
    if ["pexdb", "pxp", "pex"].contains(&ext.as_str()) {
        return Err("import_phraseexpress".into());
    }
    let bad = content.chars().filter(|&c| c == '\u{FFFD}' || c == '\0').count();
    if bad > 3 && bad * 100 > content.chars().count() {
        return Err("import_invalid".into());
    }
    Ok(())
}

// ---------- trash ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrashDto {
    id: i64,
    label: String,
    is_folder: bool,
    deleted_at: String,
}

#[tauri::command]
fn trash(state: State<AppState>) -> Res<Vec<TrashDto>> {
    let store = state.store.lock().unwrap();
    Ok(store
        .trash()
        .map_err(err)?
        .into_iter()
        .map(|t| TrashDto { id: t.id, label: t.label, is_folder: t.is_folder, deleted_at: t.deleted_at })
        .collect())
}

/// Puts the item back where it was (top level if that folder is gone too).
#[tauri::command]
fn trash_restore(app: AppHandle, state: State<AppState>, id: i64) -> Res<ImportReport> {
    let mut store = state.store.lock().unwrap();
    let mut report = ImportReport { phrases: 0, folders: 0 };
    let Some((parent, payload)) = store.take_trash(id).map_err(err)? else { return Ok(report) };
    let file: ExportFile = serde_json::from_str(&payload).map_err(err)?;
    import_snapshot(&mut store, &mut report, file, parent, true)?;
    reload(&app, &store)?;
    Ok(report)
}

/// Deletes one trashed item for good.
#[tauri::command]
fn trash_delete(state: State<AppState>, id: i64) -> Res<()> {
    state.store.lock().unwrap().take_trash(id).map(drop).map_err(err)
}

#[tauri::command]
fn trash_clear(state: State<AppState>) -> Res<()> {
    state.store.lock().unwrap().clear_trash().map_err(err)
}

/// Minimal CSV: `,` or `;` separator (auto), double quotes with "" escapes, multi-line quoted cells.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let first = text.lines().next().unwrap_or("");
    let sep = if first.matches(';').count() > first.matches(',').count() { ';' } else { ',' };
    let (mut rows, mut row, mut cell, mut quoted) = (Vec::new(), Vec::new(), String::new(), false);
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => row.push(std::mem::take(&mut cell)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

/// Daily database copy in %APPDATA%\PhraseX\backups, keeping the 10 most recent.
fn daily_backup(store: &Store) {
    let dir = data_dir().join("backups");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join(format!("phrasex-{}.db", today()));
    if !path.exists() {
        let _ = store.backup_to(&path);
    }
    let mut files: Vec<_> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort();
    for old in files.iter().rev().skip(10) {
        let _ = std::fs::remove_file(old);
    }
}

/// Common Italian misspellings, as normal phrases in an "Autocorrezione" folder (editable, removable).
const AUTOCORRECT_IT: &[(&str, &str)] = &[
    ("perchè", "perché"), ("poichè", "poiché"), ("affinchè", "affinché"), ("benchè", "benché"), ("finchè", "finché"),
    ("nè", "né"), ("sè", "sé"), ("pò", "po'"), ("stò", "sto"), ("stà", "sta"), ("fà", "fa"), ("quì", "qui"),
    ("quà", "qua"), ("sù", "su"), ("qual'è", "qual è"), ("un'altro", "un altro"), ("qualcun'altro", "qualcun altro"),
    ("ad'esempio", "ad esempio"), ("sopratutto", "soprattutto"), ("propio", "proprio"), ("daccordo", "d'accordo"),
    ("pultroppo", "purtroppo"), ("inquanto", "in quanto"), ("aereoporto", "aeroporto"), ("areoplano", "aeroplano"),
    ("metereologico", "meteorologico"), ("eccezzionale", "eccezionale"), ("coscenza", "coscienza"),
    ("efficenza", "efficienza"), ("sufficente", "sufficiente"), ("beneficienza", "beneficenza"),
    ("ingegniere", "ingegnere"), ("accellerare", "accelerare"), ("celebrale", "cerebrale"),
];

#[tauri::command]
fn install_autocorrect(app: AppHandle, state: State<AppState>) -> Res<usize> {
    let store = state.store.lock().unwrap();
    let folder = store.add_folder(None, "Autocorrezione").map_err(err)?;
    let mut added = 0;
    for (wrong, right) in AUTOCORRECT_IT {
        if store.save(&Entry::new(Some(folder), &format!("{wrong} → {right}"), wrong, right)).is_ok() {
            added += 1;
        }
    }
    reload(&app, &store)?;
    Ok(added)
}

#[tauri::command]
fn open_backups() {
    let _ = std::process::Command::new("explorer.exe").arg(data_dir().join("backups")).spawn();
}

#[tauri::command]
fn clips(state: State<AppState>) -> Res<Vec<ClipDto>> {
    let store = state.store.lock().unwrap();
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(store
        .clips()
        .map_err(err)?
        .into_iter()
        .map(|(id, text, image)| match image {
            Some(png) => ClipDto { id, text: String::new(), image: format!("data:image/png;base64,{}", b64.encode(png)) },
            None => ClipDto { id, text, image: String::new() },
        })
        .collect())
}

/// Pastes a history item as is (no macros) into the last used app.
#[tauri::command]
fn clip_insert(app: AppHandle, state: State<AppState>, id: i64) {
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.hide();
    }
    let clip = state.store.lock().unwrap().clips().ok().and_then(|c| c.into_iter().find(|c| c.0 == id));
    if let Some((_, text, image)) = clip {
        std::thread::spawn(move || {
            let _one_at_a_time = DELIVER.lock().unwrap_or_else(|p| p.into_inner());
            phrasex_listener::restore_focus();
            match image {
                Some(png) => {
                    if let Some(dib) = clipimg::png_to_dib(&png) {
                        phrasex_listener::inject_image(&dib);
                    }
                }
                None => phrasex_listener::inject(0, &text),
            }
        });
    }
}

#[tauri::command]
fn clip_delete(state: State<AppState>, id: i64) -> Res<()> {
    state.store.lock().unwrap().delete_clip(id).map_err(err)
}

#[tauri::command]
fn clip_clear(state: State<AppState>) -> Res<()> {
    state.store.lock().unwrap().clear_clips().map_err(err)
}

/// Turns a history item into a phrase and opens it in the editor.
#[tauri::command]
fn clip_to_phrase(app: AppHandle, state: State<AppState>, id: i64) -> Res<i64> {
    let store = state.store.lock().unwrap();
    let text = store.clips().map_err(err)?.into_iter().find(|c| c.0 == id && c.2.is_none()).map(|c| c.1).ok_or("not found")?;
    let name: String = text.lines().next().unwrap_or("").chars().take(40).collect();
    let new_id = store.save(&Entry::new(None, name.trim(), "", &text)).map_err(err)?;
    reload(&app, &store)?;
    drop(store);
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.hide();
    }
    show_main(&app);
    let _ = app.emit_to("main", "open-entry", new_id);
    Ok(new_id)
}

#[tauri::command]
fn popup_hide(app: AppHandle, state: State<AppState>) {
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.hide();
    }
    state.choice.lock().unwrap().take(); // closing the chooser = cancel
}

/// Several phrases share what was typed (or pressed): ask which one, next to the caret.
fn offer_choice(app: &AppHandle, choice: Choice) {
    *app.state::<AppState>().choice.lock().unwrap() = Some(choice);
    show_popup(app, "choose");
}

/// Phrase ids the chooser must list, in order.
#[tauri::command]
fn choice_pending(state: State<AppState>) -> Option<Vec<i64>> {
    state.choice.lock().unwrap().as_ref().map(|c| c.ids.clone())
}

/// Inserts the picked phrase in place of the typed abbreviation; `None` = cancel (the text stays as typed).
#[tauri::command]
fn choice_pick(app: AppHandle, state: State<AppState>, id: Option<i64>) {
    if let Some(w) = app.get_webview_window("popup") {
        let _ = w.hide();
    }
    let Some(choice) = state.choice.lock().unwrap().take() else { return };
    let body = id.filter(|id| choice.ids.contains(id)).and_then(|id| {
        let store = state.store.lock().unwrap();
        store.entries().ok()?.into_iter().find(|e| e.id == id).map(|e| (id, e.body))
    });
    let app = app.clone();
    std::thread::spawn(move || match body {
        Some((id, body)) => {
            if deliver(&app, choice.backspaces, &format!("{body}{}", choice.suffix), choice.case).is_some() {
                let _ = app.state::<AppState>().store.lock().unwrap().count_use(id);
            }
        }
        None => phrasex_listener::restore_focus(),
    });
}

/// The main window tells which kind of field has the focus.
#[tauri::command]
fn set_no_expand(state: State<AppState>, on: bool) {
    state.no_expand.store(on, Relaxed);
}

/// A shortcut field starts/stops listening: global hotkeys off/on so it can record any combination.
#[tauri::command]
fn set_recording_hotkey(app: AppHandle, state: State<AppState>, on: bool) -> Res<()> {
    state.recording.store(on, Relaxed);
    let store = state.store.lock().unwrap();
    reload(&app, &store).map(drop)
}

#[tauri::command]
fn form_pending(state: State<AppState>) -> Option<Vec<FieldDto>> {
    state.form.lock().unwrap().as_ref().map(|p| p.fields.clone())
}

#[tauri::command]
fn form_submit(state: State<AppState>, answers: Option<HashMap<String, String>>) {
    if let Some(p) = state.form.lock().unwrap().take() {
        let _ = p.reply.send(answers);
    }
}

// ---------- inserting text ----------

/// Real clock/clipboard + the form window for fill-in fields.
struct AppContext<'a> {
    app: &'a AppHandle,
}

impl Context for AppContext<'_> {
    fn now(&self) -> DateTime {
        phrasex_listener::System.now()
    }

    fn clipboard(&self) -> String {
        phrasex_listener::System.clipboard()
    }

    fn form(&self, fields: &[Field]) -> Option<HashMap<String, String>> {
        let (tx, rx) = mpsc::channel();
        let dto: Vec<FieldDto> = fields.iter().map(FieldDto::from).collect();
        let state = self.app.state::<AppState>();
        *state.form.lock().unwrap() = Some(PendingForm { fields: dto.clone(), reply: tx });
        let w = self.app.get_webview_window("form")?;
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = self.app.emit_to("form", "form-request", &dto);
        let answer = rx.recv_timeout(Duration::from_secs(900)).ok().flatten();
        state.form.lock().unwrap().take();
        let _ = w.hide();
        answer
    }

    fn lang(&self) -> &str {
        let state = self.app.state::<AppState>();
        let store = state.store.lock().unwrap();
        if lang(&store) == "en" { "en" } else { "it" }
    }

    fn phrase(&self, key: &str) -> Option<String> {
        let state = self.app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let key = key.to_lowercase();
        store.entries().ok()?.into_iter().find(|e| {
            e.name.to_lowercase() == key || e.trigger.as_deref().is_some_and(|t| t.to_lowercase() == key)
        }).map(|e| e.body)
    }

    fn counter(&self, name: &str) -> u64 {
        let state = self.app.state::<AppState>();
        let store = state.store.lock().unwrap();
        let k = format!("counter:{name}");
        let next = store.setting(&k).ok().flatten().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) + 1;
        let _ = store.set_setting(&k, &next.to_string());
        next
    }

    fn random(&self, n: usize) -> usize {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        nanos as usize % n.max(1)
    }
}

/// Renders macros (maybe asking a form), returns focus to the user's app and inserts.
/// Returns how many characters were inserted (for undo), `None` if cancelled.
fn deliver(app: &AppHandle, backspaces: usize, body: &str, case: Case) -> Option<usize> {
    let _one_at_a_time = DELIVER.lock().unwrap_or_else(|p| p.into_inner());
    let actions = macros::render_actions(body, &AppContext { app });
    phrasex_listener::restore_focus();
    let actions = actions?;
    phrasex_listener::inject(backspaces, "");
    let (mut inserted, mut after_cursor, mut only_text, mut first) = (0usize, None::<usize>, true, true);
    for action in actions {
        match action {
            Action::Text(t) => {
                // Capitalize only the very start; upper case applies everywhere.
                let t = if first || case == Case::Upper { case.apply(&t) } else { t };
                first = false;
                phrasex_listener::inject(0, &t);
                let n = t.replace("\r\n", "\n").chars().count();
                inserted += n;
                if let Some(c) = after_cursor.as_mut() {
                    *c += n;
                }
            }
            Action::Key(k) => {
                only_text = false;
                phrasex_listener::send_key(&k);
            }
            Action::Wait(ms) => std::thread::sleep(Duration::from_millis(ms)),
            Action::Cursor => {
                only_text = false;
                after_cursor = Some(0);
            }
            Action::Open(target) => {
                only_text = false;
                phrasex_listener::open(&target);
            }
        }
    }
    if let Some(n) = after_cursor {
        phrasex_listener::press_left(n);
    }
    // Undo is only reliable when we typed plain text.
    only_text.then_some(inserted)
}

/// Inserts a stored phrase (popup, tray, hotkey) into the last used app, off the UI thread.
fn insert_phrase(app: &AppHandle, id: i64) {
    let body = {
        let state = app.state::<AppState>();
        let store = state.store.lock().unwrap();
        store.entries().ok().and_then(|es| es.into_iter().find(|e| e.id == id)).map(|e| e.body)
    };
    if let Some(body) = body {
        let app = app.clone();
        std::thread::spawn(move || {
            if deliver(&app, 0, &body, Case::AsIs).is_some() {
                let _ = app.state::<AppState>().store.lock().unwrap().count_use(id);
            }
        });
    }
}

/// Expansion loop: never touches the UI thread, so the editor can't slow down typing anywhere.
fn run_engine(app: AppHandle, keys: mpsc::Receiver<Key>) {
    let state = app.state::<AppState>();
    let (engine, paused, excluded) = (state.engine.clone(), state.paused.clone(), state.excluded.clone());
    let mut undo: Option<(usize, String)> = None; // (inserted chars, what the user typed)
    let mut exe = (0isize, String::new()); // foreground window -> executable, cached
    while let Ok(key) = keys.recv() {
        phrasex_listener::CAPITAL_NEXT.store(false, Relaxed); // set again below only when allowed here
        let fg = phrasex_listener::foreground_window();
        if fg != exe.0 {
            exe = (fg, phrasex_listener::foreground_exe());
        }
        // In our own windows only the main editor expands, and not in its abbreviation/shortcut/search fields.
        let own_blocked = phrasex_listener::foreground_is_own_process()
            && (fg != state.main_window.load(Relaxed) || state.no_expand.load(Relaxed));
        let blocked = paused.load(Relaxed) || own_blocked || excluded.lock().unwrap().contains(&exe.1);
        if blocked {
            engine.lock().unwrap().feed(Key::Reset);
            undo = None;
            continue;
        }
        // Backspace right after an expansion puts back exactly what was typed.
        if key == Key::Backspace {
            if let Some((inserted, typed)) = undo.take() {
                phrasex_listener::inject(inserted.saturating_sub(1), &typed);
                engine.lock().unwrap().feed(Key::Reset);
                continue;
            }
        }
        undo = None;
        let hit = engine.lock().unwrap().feed(key);
        if hit.is_none() {
            phrasex_listener::CAPITAL_NEXT.store(engine.lock().unwrap().capital_next(), Relaxed);
        }
        if let Some(e) = hit {
            if e.ids.len() > 1 {
                offer_choice(&app, Choice { ids: e.ids, backspaces: e.backspaces, case: e.case, suffix: e.suffix });
                continue;
            }
            undo = deliver(&app, e.backspaces, &e.text, e.case).map(|n| (n, e.typed.clone()));
            let _ = state.store.lock().unwrap().count_use_typed(&e.typed);
            while keys.try_recv().is_ok() {} // keys typed in a form or during injection are not triggers
            engine.lock().unwrap().feed(Key::Reset);
        }
    }
}

/// Polls the clipboard sequence number and stores what the user copies (when history is on).
fn watch_clipboard(app: AppHandle) {
    let mut seen = phrasex_listener::clipboard_sequence();
    loop {
        std::thread::sleep(Duration::from_millis(400));
        // scripts/reinstall.cmd asks us to quit: when we run as administrator it can't kill us
        if std::fs::remove_file(data_dir().join("quit-request")).is_ok() {
            app.exit(0);
            return;
        }
        let seq = phrasex_listener::clipboard_sequence();
        if seq == seen {
            continue;
        }
        seen = seq;
        let state = app.state::<AppState>();
        let enabled = state.store.lock().unwrap().setting("clip_history").ok().flatten().as_deref() != Some("off");
        if !enabled {
            continue;
        }
        if let Some(text) = phrasex_listener::copied_text() {
            let _ = state.store.lock().unwrap().add_clip(&text);
        } else if let Some(png) = phrasex_listener::copied_image().and_then(|dib| clipimg::dib_to_png(&dib)) {
            use sha2::Digest;
            let hash = sha2::Sha256::digest(&png);
            // dedup key in the text column; the control character keeps it apart from any copied text
            let key = format!("{}img:{}", char::from(1u8), hash[..8].iter().map(|b| format!("{b:02x}")).collect::<String>());
            let _ = state.store.lock().unwrap().add_image_clip(&key, &png);
        }
    }
}

// ---------- updates (GitHub Releases) ----------

/// Newer version found at start, shown as a button in the toolbar.
static UPDATE: Mutex<Option<String>> = Mutex::new(None);

async fn check_update(app: AppHandle) {
    use tauri_plugin_updater::UpdaterExt;
    let found = match app.updater() {
        Ok(u) => u.check().await,
        Err(e) => Err(e),
    };
    match found {
        Ok(Some(update)) => {
            *UPDATE.lock().unwrap() = Some(update.version.clone());
            let _ = app.emit("update-available", update.version);
        }
        Ok(None) => {}
        Err(_) => {} // offline or no release yet: try again next start
    }
}

#[tauri::command]
fn update_available() -> Option<String> {
    UPDATE.lock().unwrap().clone()
}

/// Downloads the new version and runs its installer (which closes Phrase X and starts the new one).
#[tauri::command]
async fn update_install(app: AppHandle) -> Res<()> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app.updater().map_err(err)?.check().await.map_err(err)?.ok_or("no_update")?;
    update.download_and_install(|_, _| {}, || {}).await.map_err(err)?;
    app.restart();
}

// ---------- windows & tray ----------

fn tr(lang: &str, key: &str) -> &'static str {
    let it = lang != "en";
    match key {
        "open" => if it { "Apri Phrase X" } else { "Open Phrase X" },
        "pause" => if it { "Pausa espansioni" } else { "Pause expansions" },
        "insert" => if it { "Inserisci frase" } else { "Insert phrase" },
        "search" => if it { "Cerca e inserisci…" } else { "Search and insert…" },
        "clips" => if it { "Cronologia appunti…" } else { "Clipboard history…" },
        "settings" => if it { "Impostazioni" } else { "Settings" },
        _ => if it { "Esci" } else { "Quit" },
    }
}

fn tray_menu(app: &AppHandle, store: &Store, lang: &str, paused: bool) -> tauri::Result<Menu<Wry>> {
    let folders = store.folders().unwrap_or_default();
    let entries = store.entries().unwrap_or_default();
    let phrases = phrase_items(app, &folders, &entries, None)?;
    let phrase_refs: Vec<&dyn IsMenuItem<Wry>> = phrases.iter().map(|b| b.as_ref()).collect();
    let open = MenuItem::with_id(app, "open", tr(lang, "open"), true, None::<&str>)?;
    let search = MenuItem::with_id(app, "search", tr(lang, "search"), true, None::<&str>)?;
    let clips = MenuItem::with_id(app, "clips", tr(lang, "clips"), true, None::<&str>)?;
    let insert = Submenu::with_items(app, tr(lang, "insert"), !phrase_refs.is_empty(), &phrase_refs)?;
    let pause = CheckMenuItem::with_id(app, "pause", tr(lang, "pause"), true, paused, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", tr(lang, "settings"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr(lang, "quit"), true, None::<&str>)?;
    let (s1, s2) = (PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?);
    Menu::with_items(app, &[&open, &search, &clips, &insert, &s1, &pause, &settings, &s2, &quit])
}

fn phrase_items(app: &AppHandle, folders: &[Folder], entries: &[Entry], parent: Option<i64>) -> tauri::Result<Vec<Box<dyn IsMenuItem<Wry>>>> {
    let mut out: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for f in folders.iter().filter(|f| f.parent_id == parent) {
        let children = phrase_items(app, folders, entries, Some(f.id))?;
        let refs: Vec<&dyn IsMenuItem<Wry>> = children.iter().map(|b| b.as_ref()).collect();
        out.push(Box::new(Submenu::with_items(app, f.name.replace('&', "&&"), !refs.is_empty(), &refs)?));
    }
    for e in entries.iter().filter(|e| e.folder_id == parent && e.enabled) {
        let label = if e.name.is_empty() { e.trigger.clone().unwrap_or_default() } else { e.name.clone() };
        out.push(Box::new(MenuItem::with_id(app, format!("phrase:{}", e.id), label.replace('&', "&&"), true, None::<&str>)?));
    }
    Ok(out)
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Opens the search popup ("phrases" or "clips") next to the mouse, inside the current monitor.
fn show_popup(app: &AppHandle, mode: &str) {
    let Some(w) = app.get_webview_window("popup") else { return };
    let choosing = mode == "choose";
    let (x, y) = if choosing { phrasex_listener::caret_pos() } else { None }.unwrap_or_else(phrasex_listener::cursor_pos);
    let (lw, lh) = if choosing {
        let rows = app.state::<AppState>().choice.lock().unwrap().as_ref().map_or(1, |c| c.ids.len().min(6));
        (440.0, 196.0 + 42.0 * rows as f64)
    } else {
        (460.0, 440.0)
    };
    let _ = w.set_size(tauri::LogicalSize::new(lw, lh));
    let scale = w.scale_factor().unwrap_or(1.0);
    let size = tauri::PhysicalSize::new((lw * scale) as u32, (lh * scale) as u32);
    let (mut px, mut py) = if choosing { (x, y + 6) } else { (x - 20, y + 16) };
    for m in w.available_monitors().unwrap_or_default() {
        let (p, s) = (m.position(), m.size());
        if x >= p.x && x < p.x + s.width as i32 && y >= p.y && y < p.y + s.height as i32 {
            px = px.clamp(p.x, p.x + s.width as i32 - size.width as i32);
            py = py.clamp(p.y, p.y + s.height as i32 - size.height as i32 - 48);
        }
    }
    let _ = w.set_position(PhysicalPosition::new(px, py));
    let _ = w.show();
    if let (true, Ok(hwnd)) = (choosing, w.hwnd()) {
        phrasex_listener::force_foreground(hwnd.0 as isize); // the user is typing in another app
    }
    let _ = w.set_focus();
    let _ = app.emit_to("popup", "popup-show", mode);
}

fn main() {
    let (store, _) = Store::open_default().expect("database");
    // "run as administrator": a normal start hands over to the elevated task and exits. The flag
    // marks the elevated start, so a failed elevation can never loop.
    let launch = store.setting(admin::LAUNCH_FLAG).ok().flatten().unwrap_or_default();
    let minimized = std::env::args().any(|a| a == "--minimized") || launch == "min";
    if !launch.is_empty() {
        let _ = store.set_setting(admin::LAUNCH_FLAG, "");
    } else if store.setting("run_as_admin").ok().flatten().as_deref() == Some("on") && !phrasex_listener::is_elevated() {
        let _ = store.set_setting(admin::LAUNCH_FLAG, if minimized { "min" } else { "show" });
        if admin::run_task() {
            return;
        }
        let _ = store.set_setting(admin::LAUNCH_FLAG, ""); // task gone: keep running normally
    }
    let engine = Arc::new(Mutex::new(Engine::new(store.engine_snippets().expect("read snippets"))));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let state = app.state::<AppState>();
                    if Some(shortcut.id()) == *state.popup_hotkey.lock().unwrap() {
                        show_popup(app, "phrases");
                    } else if Some(shortcut.id()) == *state.clip_hotkey.lock().unwrap() {
                        show_popup(app, "clips");
                    } else if let Some(ids) = state.hotkeys.lock().unwrap().get(&shortcut.id()).cloned() {
                        match ids[..] {
                            [] => {} // folder shortcut on a folder with no enabled phrases
                            [id] => insert_phrase(app, id),
                            _ => offer_choice(app, Choice { ids, backspaces: 0, case: Case::AsIs, suffix: String::new() }),
                        }
                    }
                })
                .build(),
        )
        .manage(AppState {
            store: Mutex::new(store),
            engine,
            paused: Arc::new(AtomicBool::new(false)),
            excluded: Arc::new(Mutex::new(Vec::new())),
            hotkeys: Mutex::new(HashMap::new()),
            popup_hotkey: Mutex::new(None),
            clip_hotkey: Mutex::new(None),
            form: Mutex::new(None),
            choice: Mutex::new(None),
            main_window: AtomicIsize::new(0),
            no_expand: AtomicBool::new(false),
            recording: AtomicBool::new(false),
        })
        .invoke_handler(tauri::generate_handler![
            library, save_entry, delete_entry, add_folder, rename_folder, set_folder_hotkey, reorder_entries, sort_folder, paste_items, set_folder_color, move_folder, delete_folder, set_paused,
            settings, save_settings, running_apps, popup_insert, popup_hide, form_pending, form_submit,
            clips, clip_insert, clip_delete, clip_clear, clip_to_phrase, export_data, import_data, open_backups, install_autocorrect,
            trash, trash_restore, trash_delete, trash_clear, choice_pending, choice_pick, set_no_expand, set_recording_hotkey,
            sync_status, sync_login, sync_cancel_login, sync_logout, sync_now, update_available, update_install
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/tray.png")) // own size: bold outline at 16-32 px
                .tooltip("Phrase X")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, ev| match ev.id().as_ref() {
                    "open" => show_main(app),
                    "search" => show_popup(app, "phrases"),
                    "clips" => show_popup(app, "clips"),
                    "settings" => {
                        show_main(app);
                        let _ = app.emit_to("main", "open-settings", ());
                    }
                    "pause" => {
                        let state = app.state::<AppState>();
                        let p = !state.paused.load(Relaxed);
                        state.paused.store(p, Relaxed);
                        let _ = app.emit("paused", p);
                    }
                    "quit" => app.exit(0),
                    id => {
                        if let Some(Ok(phrase)) = id.strip_prefix("phrase:").map(str::parse::<i64>) {
                            insert_phrase(app, phrase);
                        }
                    }
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            {
                let state = handle.state::<AppState>();
                let store = state.store.lock().unwrap();
                reload(&handle, &store)?;
                daily_backup(&store);
            }
            if let Some(hwnd) = app.get_webview_window("main").and_then(|w| w.hwnd().ok()) {
                let hwnd = hwnd.0 as isize;
                phrasex_listener::set_main_window(hwnd);
                handle.state::<AppState>().main_window.store(hwnd, Relaxed);
            }
            let keys = phrasex_listener::start()?;
            let watcher = handle.clone();
            std::thread::spawn(move || run_engine(handle, keys));
            std::thread::spawn(move || watch_clipboard(watcher));
            let syncer = app.handle().clone();
            std::thread::spawn(move || sync_loop(syncer));
            let updater = app.handle().clone();
            tauri::async_runtime::spawn(async move { check_update(updater).await });

            if !minimized {
                show_main(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing any window keeps Phrase X running in the tray; closing the form cancels it.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                if window.label() == "form" {
                    if let Some(p) = window.state::<AppState>().form.lock().unwrap().take() {
                        let _ = p.reply.send(None);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Phrase X");
}

#[cfg(test)]
mod tests {
    #[test]
    fn task_xml_escapes_and_runs_elevated() {
        let x = super::admin::task_xml(r"C:\A&B\phrasex-app.exe", r"PC\Marco");
        assert!(x.contains(r"<Command>C:\A&amp;B\phrasex-app.exe</Command>"));
        assert!(x.contains(r"<UserId>PC\Marco</UserId>"));
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(x.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(x.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    }

    #[test]
    fn csv_separators_quotes_multiline() {
        assert_eq!(super::import_check("x", "GMD.pexdb"), Err("import_phraseexpress".into()));
        assert_eq!(super::import_check("\u{FFFD}\u{FFFD}a\u{FFFD}\u{FFFD}\0", "a.txt"), Err("import_invalid".into()));
        assert_eq!(super::import_check("Firma;fir;Saluti \u{FFFD}", "a.csv"), Ok(()), "one odd char is fine");
        let rows = super::parse_csv("descrizione;abbreviazione;testo\nFirma;fir;\"Saluti\nMarco\"\n\"A \"\"B\"\"\";ab;x\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], ["Firma", "fir", "Saluti\nMarco"]);
        assert_eq!(rows[2], ["A \"B\"", "ab", "x"]);
        assert_eq!(super::parse_csv("a,b,c")[0], ["a", "b", "c"]);
    }

    #[test]
    fn export_subtree_and_file_names() {
        let folders = [(1, None), (2, Some(1)), (3, Some(2)), (4, None), (5, Some(4))];
        let mut got: Vec<_> = super::subtree(&folders, 1).into_iter().collect();
        got.sort();
        assert_eq!(got, [1, 2, 3]);
        assert_eq!(super::file_safe("Clienti/Fornitori: 2026"), "Clienti_Fornitori_ 2026");
    }

    #[test]
    fn folder_snapshot_restores_tree_and_hotkeys() {
        use super::*;
        let mut s = Store::open_in_memory().unwrap();
        let top = s.add_folder(None, "Top").unwrap();
        let a = s.add_folder(Some(top), "A").unwrap();
        let b = s.add_folder(Some(a), "B").unwrap();
        s.set_folder_hotkey(a, Some("Ctrl+Alt+A")).unwrap();
        let mut e = Entry::new(Some(b), "Firma", "fir", "Saluti");
        e.hotkey = Some("Ctrl+Alt+F".into());
        s.save(&e).unwrap();
        let (file, name, parent) = folder_snapshot(&s, a).unwrap();
        assert_eq!((name.as_str(), parent), ("A", Some(top)));
        s.delete_folder(a).unwrap();
        let mut report = ImportReport { phrases: 0, folders: 0 };
        import_snapshot(&mut s, &mut report, file, parent, true).unwrap();
        assert_eq!((report.folders, report.phrases), (2, 1));
        let folders = s.folders().unwrap();
        let new_a = folders.iter().find(|f| f.name == "A").unwrap();
        let new_b = folders.iter().find(|f| f.name == "B").unwrap();
        assert_eq!((new_a.parent_id, new_b.parent_id), (Some(top), Some(new_a.id)));
        assert_eq!(new_a.hotkey.as_deref(), Some("Ctrl+Alt+A"), "folder shortcut restored");
        let restored = s.entries().unwrap().into_iter().find(|e| e.name == "Firma").unwrap();
        assert_eq!((restored.folder_id, restored.trigger.as_deref(), restored.hotkey.as_deref()), (Some(new_b.id), Some("fir"), Some("Ctrl+Alt+F")));
    }
}
