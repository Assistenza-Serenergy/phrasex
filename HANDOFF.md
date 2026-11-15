# Phrase X — handoff (2026-11-15)

Open-source text expander (clean-room alternative to PhraseExpress) for X Holding. MIT, no limits, IT/EN UI.
Public repo https://github.com/Assistenza-Serenergy/phrasex (gh logged in as Assistenza-Serenergy), branch main,
GitHub Pages from /docs (website + privacy). Current version 0.2.0, published as a GitHub Release.

## User rules
- Italian, short answers (≤10 lines), first line = concrete action, plain words, no AI slop in UI texts.
- After each group of changes: commit + push on main, then build + reinstall — unless the user says to hold commits.
- **No GUI tests / computer use on the user's PC** unless they ask; verify with tests, typecheck, build, and the
  screenshot mode below (fake backend in the browser pane).
- Ask before anything outward-facing (GitHub Release, website wording about data, privacy page).
- Clean-room: behaviour like PhraseExpress, never its code/assets/texts/trade dress. No elevation tricks
  (the UAC secure desktop can't be typed into).
- Irreversible actions in the UI: two-click ConfirmButton, never native confirm(). Deletes go to the trash with Undo.

## Build, install, release (Git Bash)
- PATH="$HOME/.cargo/bin:/c/Program Files/nodejs:/c/Windows/System32:/c/Windows:$PATH" (Git's link.exe shadows MSVC).
- Tests `cargo test --workspace`; typecheck `node node_modules/typescript/bin/tsc --noEmit` in app/.
- Build ONLY with `sh scripts/build.sh`: sets the updater signing key (`%USERPROFILE%/.tauri/phrasex-updater.key`,
  no password, outside the repo; the user must keep a backup) and writes the NSIS installer + .sig.
- Reinstall the user's copy (%LOCALAPPDATA%\Phrase X): `explorer.exe 'C:\Users\Marco\Desktop\PhraseX\scripts\reinstall.vbs'`
  (absolute path; a relative one does nothing). reinstall.cmd asks the app to quit (`%APPDATA%\PhraseX\quit-request`,
  polled in watch_clipboard: taskkill can't stop an elevated app), runs the newest *-setup.exe /S, restarts it,
  writes scripts/reinstall.log. Processes started by Claude are MSIX-sandboxed: real AppData/registry only via explorer.exe.
- Release: bump the version in tauri.conf.json, Cargo workspace and app/package.json, build, ask the user, then
  `python scripts/release.py "notes"` (uploads PhraseX_<v>_x64-setup.exe + latest.json). The app checks
  `releases/latest/download/latest.json` at start and shows "Aggiorna".
- After icon changes: `touch app/src-tauri/build.rs app/src-tauri/src/main.rs`. Logo: `app/make-icons.py`
  (user's choice "B": glossy black bubble with the X Holding X from app/x-mark.png); `python scripts/icon-preview.py`
  before changing it. Installer art: `scripts/make-installer-art.py` (approved).
- Website screenshots: `.claude/launch.json` "shots" = the UI with a fake backend (app/vite.shots.config.ts,
  app/shots/*); headless Chrome `--screenshot` (popup through shots/frame.html: headless has a minimum width).
- Python edit scripts: heredocs in Git Bash mangle backslashes; write the script with the Write tool instead.
- Local only, gitignored: Installer/ and "Installa Phrase X.exe" (old 0.1.0 handout), google-oauth.json, firebase.json.

## Code map
- `crates/engine` — typing buffer, triggers (case adapt, undo data; `reload` in main.rs forces every phrase to
  immediate, the user's wish, so the word-end mode is unused by the app), writing assistance
  (double caps; `capital_next` for sentence capitals), macros (`macros.rs`).
- `crates/listener` — Windows LL keyboard/mouse hooks, focus tracking, injector, clipboard. Own keystrokes tagged
  `OWN_INPUT_TAG`; real layout keys via VkKeyScanEx + hardware scan codes (WinUI apps need them); text > 2 chars pasted
  via the clipboard and restored. Sentence capital: the hook swallows the lowercase key and types the capital
  (`CAPITAL_NEXT`, set by run_engine) — erasing and retyping raced with fast typing.
- `crates/store` — SQLite, migrations V1–V13 (foreign keys off while migrating). V6 sync uids/tombstones, V7 folder
  hotkey, V8 shared rows, V9 manual order, V10 folder colour, V11 clipboard images, V12 drops tags, V13 phrase colour.
  `sync.rs`: sync document (`Library` incl. `settings`), `merge` (newest per uid; tombstones), `apply_sync`, `set_shared`.
- `app/src-tauri/src` — `main.rs` (commands, tray, global shortcuts, windows, sync loop, sharing round, import/export,
  trash, updater), `cloud.rs` (Google OAuth + Drive + Firebase sharing), `clipimg.rs` (DIB <-> PNG).
- `app/src` — `App.tsx` (toolbar, tree, editors, trash, shortcuts, autosave), `Settings.tsx` (side tabs),
  `Popup.tsx` (search / clipboard / chooser), `Form.tsx`, `icons.tsx`, `theme.ts`, `locales/*.json`, `styles.css`.

## Behaviour worth knowing
- Autosave everywhere (600 ms after the last change, saves chained); the draft is never overwritten by the saved
  copy (it would eat spaces while typing). No Save button.
- Tree: drag only from the name; beside the name = box selection; Ctrl+click toggles; Del, F2, Ctrl+N,
  Ctrl+Shift+N, Ctrl+F, Ctrl+C/V (`paste_items`: keeps abbreviations, drops shortcuts), arrows/Enter.
  Manual order by dragging onto another phrase of the same folder; "Ordina A-Z" resets it.
- Several phrases may share an abbreviation or shortcut: a chooser opens at the caret. A folder shortcut opens the
  chooser with every enabled phrase of the folder and its subfolders.
- Colours: folders ("" = yellow), phrases ("" grey, "folder" = same as its folder, or hex);
  "Applica a tutto il contenuto della cartella" sets subfolders to the colour and phrases to "folder".
- Expansion also works in our main window except in fields with `data-noexpand`.
- Run as administrator: Task Scheduler task "Phrase X" (one UAC prompt to create/delete); normal starts run the task.
  Phrase X then runs elevated, so computer use can't click in it (UIPI).
- Trash: JSON snapshot per deletion, restore into the original parent, purged after 30 days.
- Clipboard history: text and images (PNG, max 20 images of 200 items); password-manager content skipped.
- Theme: system / light / dark (`data-theme` on <html> + window setTheme).

## Cloud sync, sharing
- "Accedi con Google": OAuth loopback + PKCE (openid email profile drive.appdata), refresh token in Credential
  Manager, Drive file `phrasex-library.json` in appDataFolder. Sync at start, ≤1 min after a change, every 5 min.
  Settings in SYNCED_SETTINGS travel too (newest `settings_updated_at`); autostart/admin stay local.
  Reload with hotkey registration runs on the main thread: never call reload off-main while holding the store lock.
- Account switch: backup `backups\phrasex-cambio-account-<date>.db`, then that account's cloud library replaces the local one.
- Google Cloud project `phrase-x` (spagnuolomarco01@gmail.com): External, in production, non-sensitive scopes.
- Sharing via Firebase (same project, Spark): Settings email list `share_with`; readers get a read-only top-level
  folder named after the owner. Firestore `shares/{uid}` {ownerName, ownerEmail, readers, library} (1 MB cap);
  rules: owner writes, verified reader emails read. Settings are never sent to readers. Recipient side not tested yet.
- Settings are stamped (`touch_settings`) only when a synced value really changes and never on the first start
  (a new PC would push its defaults everywhere, and an empty share list deletes the share).
- Known limits: no conditional upload (same-second edits on two PCs resend next round); tombstones never pruned.
- Privacy page mentions settings on Drive but not Firebase sharing yet (ask the user).

## Not now / no
PhraseExpress import (the user will move phrases by hand), rich text, encryption, network-folder library,
uiAccess build, macOS/Linux.
