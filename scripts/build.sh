#!/bin/sh
# Builds the signed installer (Git Bash): target/release/bundle/nsis/Phrase X_<version>_x64-setup.exe + .sig
# The updater key lives outside the repo: %USERPROFILE%\.tauri\phrasex-updater.key (keep a backup!).
export PATH="$HOME/.cargo/bin:/c/Program Files/nodejs:/c/Windows/System32:/c/Windows:$PATH"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$USERPROFILE/.tauri/phrasex-updater.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
cd "$(dirname "$0")/../app" && node node_modules/@tauri-apps/cli/tauri.js build
