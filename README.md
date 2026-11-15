# Phrase X

Open-source text expander & macro automation for Windows. MIT, no usage limits, no telemetry.
UI in Italian / English. Website & privacy: https://assistenza-serenergy.github.io/phrasex/
Download: [Releases](https://github.com/Assistenza-Serenergy/phrasex/releases) (the app then updates itself).

Cloud sync: Settings → "Accedi con Google" keeps phrases and settings in the user's own Google Drive (hidden app folder).
Users configure nothing; only whoever builds Phrase X needs the OAuth client once: [GOOGLE-SETUP.md](GOOGLE-SETUP.md).

```
crates/engine     typing buffer + trigger matching + macros (pure, cross-platform, unit-tested)
crates/listener   Windows low-level keyboard hook + SendInput injector + clipboard
crates/store      SQLite data layer + cloud sync document (%APPDATA%\PhraseX\phrasex.db)
app/              desktop app: Tauri 2 + React (tree, editor, tray icon, sync, sharing, updates)
docs/             website + privacy policy (GitHub Pages)
scripts/          build, release, local reinstall, icon / installer art
```

Build (Git Bash, from this folder):
```
cargo test --workspace
sh scripts/build.sh      # signed installer: target/release/bundle/nsis/Phrase X_<version>_x64-setup.exe
```
`scripts/build.sh` needs the update signing key in `%USERPROFILE%\.tauri\phrasex-updater.key`.
