# Phrase X

Open-source text expander & macro automation for Windows. MIT, no usage limits, no telemetry.
UI in Italian / English. Roadmap: [ROADMAP.md](ROADMAP.md). Website & privacy: https://assistenza-serenergy.github.io/phrasex/

Cloud sync: Settings → "Accedi con Google" keeps the library in the user's own Google Drive (hidden app folder).
Users configure nothing; only whoever builds Phrase X needs the OAuth client once: [GOOGLE-SETUP.md](GOOGLE-SETUP.md).

```
crates/engine     typing buffer + trigger matching + macros (pure, cross-platform, unit-tested)
crates/listener   Windows low-level keyboard hook + SendInput injector + headless demo
crates/store      SQLite data layer: folders, phrases, tags (%APPDATA%\PhraseX\phrasex.db)
app/              desktop app: Tauri 2 + React (tree, editor, tray icon, Google Drive sync)
docs/             website + privacy policy (GitHub Pages)
```

Build (PowerShell, from this folder):
```
cargo test --workspace
cd app; npm.cmd install; npx.cmd tauri build
# installer: target\release\bundle\nsis\Phrase X_0.1.0_x64-setup.exe
```
