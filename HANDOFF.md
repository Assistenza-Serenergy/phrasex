# Phrase X — handoff (2026-11-08)

Open-source text expander (clean-room alternative to PhraseExpress) for X Holding. MIT, no limits, IT/EN UI.

## Stack and layout
- `crates/engine` — typing buffer, triggers (word/immediate, case adapt, undo data), writing assistance (double caps, sentence caps), macros (`macros.rs`: date±offset, names, time, clipboard, input forms, phrase, counter, random, calc, cursor, key/enter/tab, wait, open). 16 tests.
- `crates/store` — SQLite (rusqlite bundled), migrations V1–V6 (V6 = sync uids/tombstones; V5 rebuilds `snippet` without UNIQUE(trigger); foreign keys are off during migrations): folder tree, snippet (+hotkey, uses), tag, setting, clip, trash. `move_folder` loop-safe. 10 tests.
- `crates/listener` — Windows LL keyboard/mouse hooks, focus tracking, injector. Own keystrokes tagged `dwExtraInfo = OWN_INPUT_TAG`; other tools' input (VK_PACKET) counts as typing. Real layout keys via VkKeyScanEx + **hardware scan codes** (WinUI apps ignore modifiers without them); text > 2 chars pasted via clipboard with full restore + exclusion format. `restore_focus` only acts when Phrase X holds focus.
- `app/` — Tauri 2 + React 18 + Vite 5. `src-tauri/src/main.rs` (commands, tray, hotkeys via global-shortcut, single-instance, autostart, form/popup windows, clipboard watcher, import/export JSON+CSV, daily backups in %APPDATA%\PhraseX\backups). Frontend: `App.tsx` (tree + editor + welcome/stats), `Popup.tsx` (phrases/clipboard tabs), `Form.tsx`, `Settings.tsx`, `HotkeyInput.tsx`, `locales/it.json|en.json`, `styles.css`.
- Brand: X Holding pure black/white, serif headings. **Logo (user's choice "B", 2026-11-08)**: own drawing in `app/make-icons.py` (run in app/): glossy black oval bubble 1.25:1 (80 % of the height), short comma tail pointing down-left, light grey rim, X Holding X from `app/x-mark.png`; edge to edge, no CSS halo. User asked for an identical copy of PhraseExpress's icon: refused (their trade dress), proportions only. Measured on the user's taskbar: same 48x48 / 32x32 box as PhraseExpress, it cannot get bigger. Previous logo (0.1.0 ZIP) kept in `app/logo.ico`. Windows needs scripts/reinstall.cmd via the app's Terminal panel if `explorer.exe` doesn't start it.
- Roadmap with done/todo: `ROADMAP.md`.

## Tree UI (done 2026-11-08)
- Right-click menu on rows/empty area (`ContextMenu` in App.tsx), empty-area click = deselect, HTML5 drag & drop (needs `"dragDropEnabled": false` on the main window in tauri.conf.json, otherwise WebView2 swallows DnD).
- Toolbar Importa/Esporta; shared helpers `exportLibrary`/`importPicked` in Settings.tsx. `export_data(folderId?)` exports one folder subtree (becomes top level on import) to Documents.
- Folder order is by name (no manual reorder yet).
- Trash: store V4 `trash` table holds an export-format JSON snapshot per deletion (`delete_entry`/`delete_folder` in main.rs); restore = `import_snapshot` into the original parent (top level if gone), hotkeys kept. UI: "Cestino" node at the bottom of the tree (TrashNode/TrashView in App.tsx, user asked for it like a folder): greyed children, click = detail with Ripristina / Elimina definitivamente, node = list + Svuota cestino; irreversible actions use the two-click ConfirmButton, never native confirm(). Items older than 30 days are purged by the store (TRASH_DAYS, no count limit). Trash has its own group selection (`trashMulti`, rows carry data-tkey; box over trash rows or Ctrl+click) with Restore / Delete for good; Del does nothing in the trash (irreversible). Import refuses PhraseExpress files (.pexdb/.pxp are encrypted, entropy 8) and binaries (`import_check` in main.rs); PhraseExpress Free exports only Email attachment / Text file: the Text-file format is still to be seen.

## Shared triggers / hotkeys, expansion in our window (2026-11-08)
- Several phrases may share an abbreviation or a shortcut: the engine returns all ids (`Expansion.ids`, `suffix`), `offer_choice` stores a `Choice` and opens the popup window in mode `choose` at the text caret (`caret_pos`, mouse fallback); `choice_pick` inserts or cancels. Editor shows "also used by".
- Expansion works in the main window too: the listener remembers it as an insertion target (`set_main_window`); fields with `data-noexpand` (abbreviation, shortcut, search) report via `set_no_expand`. A shortcut field turns global hotkeys off while focused (`set_recording_hotkey`).
- Run as administrator (Settings toggle, user chose it): Task Scheduler task "Phrase X" (HighestAvailable, no trigger, Parallel, no battery/time limits) created/deleted via elevated schtasks (one UAC prompt). Every normal start runs the task and exits; DB flag `elevated_launch` (show/min) marks the elevated start so it never loops. Autostart stays on the Run key.
- Tree group selection: box-select on the empty tree area or Ctrl+click → `multi` state; group panel (move to folder / delete), group drag, Esc clears. `topLevel()` drops items inside a selected folder. Delete has no confirm popup (user hates native confirm): items go to the trash at once, status bar shows "{n} nel cestino · Annulla" (restores the newest n trash ids). Del/Canc key (window listener, ignored in inputs) deletes the group or the selected row; a single Ctrl+clicked row counts as a group.

## Cloud sync "Accedi con Google" (2026-11-09)
- Store V6: `uid` (random hex, same on every PC) on folder/snippet, `updated_at` UTC text with ms (`NOW` in store), `tombstone` table filled by AFTER DELETE triggers (cascades included). Seeded examples get `sync::EPOCH` stamps = "pristine": on a PC's first sync, if the cloud has a library, untouched examples are dropped.
- `crates/store/src/sync.rs`: `Library` doc (uids instead of ids, tags by name), `merge` (newest per uid wins, tombstone newer than every version deletes, output sorted by uid), `sync_snapshot`, `apply_sync` (local := doc; missing/looping parents → top level; usage counts stay local).
- `app/src-tauri/src/cloud.rs`: OAuth loopback+PKCE (scopes openid email drive.appdata), refresh token in Credential Manager via `keyring` ("Phrase X"/"google-refresh-token"), Drive file `phrasex-library.json` in appDataFolder (ureq + native-tls/schannel), `plan()` decides upload/apply.
- main.rs: `sync_loop` thread (at start, ≤1 min after a change via `SYNC_DIRTY` set in `reload`, every 5 min), commands sync_status/login/logout/now, events `sync-status` and `library-changed`. Apply happens under the store lock, `reload` then runs on the main thread (hotkey registration needs it: never call reload off-main while holding the store lock).
- OAuth client: `app/src-tauri/google-oauth.json` (Google's "Download JSON" for a Desktop client) read by build.rs into `PHRASEX_GOOGLE_CLIENT_ID/SECRET`; missing = "non configurato". Setup guide: GOOGLE-SETUP.md.
- Known limits: no conditional upload (two PCs in the same second can lose one round, re-sent next round); tombstones never pruned; settings are not synced.

## Build (Git Bash)
- PATH="$HOME/.cargo/bin:/c/Program Files/nodejs:/c/Windows/System32:/c/Windows" (Git's /usr/bin/link shadows MSVC link.exe).
- Tests: `cargo test --workspace`. Typecheck: `node node_modules/typescript/bin/tsc --noEmit` in app/.
- Build: in app/ `node node_modules/@tauri-apps/cli/tauri.js build --no-bundle` (exe: target\release\phrasex-app.exe); kill phrasex-app.exe first (exe locked). Installer (`build` without flag) only at the end, per user.
- After icon changes: `touch app/src-tauri/build.rs app/src-tauri/src/main.rs`.
- Processes started by Claude are MSIX-sandboxed (AppData/registry virtualized): anything that must be real goes through `explorer.exe <path>`.

## User rules
- Italian, short answers (≤10 lines), first line = concrete action, no recap fluff.
- Keep going through the roadmap without asking between modules.
- **No GUI tests / computer use on the user's PC** unless they ask again; verify with tests + typecheck + build only.
- Clean-room: behaviour like PhraseExpress, never its code/assets/texts/trade dress. UAC secure desktop can't be typed into; no elevation tricks.

## Local install loop (2026-11-08)
- User runs the INSTALLED copy (%LOCALAPPDATA%\Phrase X). After `tauri build`, reinstall with `explorer.exe scripts\reinstall.cmd` (kills app, silent NSIS /S, refreshes icon cache, restarts). Must go through explorer.exe to escape Claude's MSIX sandbox.
- `python scripts/icon-preview.py` renders the icon in every placement (titlebar, in-app, welcome, taskbar, tray) — show it to the user before changing the logo.
- Installer/ (ZIP for other people): do NOT refresh unless the user asks.
