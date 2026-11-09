# Phrase X — feature roadmap

Target: feature parity with commercial text expanders (clean-room: behaviour only, own code/UI/texts).
`[x]` done · `[ ]` to do. Milestones in the order they will be built.

## Done
- [x] Global keyboard hook on its own thread, Italian/any layout (AltGr, Shift, accents)
- [x] Abbreviation expansion: after delimiter or immediate, case-sensitive option, longest match wins
- [x] Reset on click / arrows / Enter / window switch; ignores own output; expands in its own editor too (not in abbreviation/shortcut/search fields)
- [x] Macros: `{#date fmt}` `{#time fmt}` `{#clipboard}` `{#input}` (text, multi-line, list, checkbox, date)
- [x] SQLite library: folders (top level = categories), phrases, tags, search
- [x] Desktop app: tree + editor, tray icon, pause, close to tray, Italian/English UI

## M1 — Insert from anywhere
- [x] Global hotkey popup: search box + phrase list, inserts into the previous app
- [x] Phrase tree in the tray menu
- [x] Per-phrase hotkey; same abbreviation or hotkey on several phrases opens a pick menu at the caret
- [x] Paste long/multi-line text via clipboard (restore the user's clipboard afterwards)
- [x] Fill-in forms for `{#input}`: text, multi-line, dropdown, checkbox, date

## M2 — Everyday app behaviour
- [x] Start with Windows, single instance
- [x] Settings: language, search hotkey, autostart, excluded programs — delimiters/delay still to do
- [x] Program rules: exclude expansion per application (global list) — per-folder rules still to do
- [x] Undo expansion: Backspace right after an expansion restores the abbreviation
- [x] Case adaptation: `btw`→by the way, `Btw`→By the way, `BTW`→BY THE WAY

## M3 — Clipboard
- [x] Clipboard history (text) with popup and search, private items skipped — images still to do
- [x] Turn a history item into a phrase

## M4 — Macros, advanced
- [x] Cursor position `{#cursor}`, keys `{#key Tab}` / `{#enter}`, `{#wait ms}`
- [x] Date arithmetic (`+1d`, `-1M`), day/month names in IT/EN
- [x] Insert another phrase, counters, random choice
- [x] Calculations `{#calc}` — variables and if/else still to do
- [x] Launch program / open URL / open file

## M5 — Writing assistance
- [x] Autocorrect list (typos), DOuble CApitals, sentence capitalization
- [ ] Autocomplete suggestions learned from frequently typed text
- [x] Spell check in the editor (WebView2 native, Windows spell-check languages)
- [x] Usage statistics (expansions, keystrokes saved)

## M6 — Library management
- [x] Box/Ctrl+click group selection: move/delete/drag in bulk
- [x] Run as administrator option (scheduled task, one UAC prompt) so it types into elevated windows
- [x] Drag & drop move, right-click menu (new/rename/duplicate/delete), duplicate phrase — manual reorder and copy/paste still to do
- [x] Import/export (JSON, CSV) from toolbar, single-folder export to share with other people/PCs, automatic daily backups, trash in the tree with restore / delete for good / empty, auto-emptied after 30 days, Del key + Undo, no confirm popups
- [ ] Rich text / HTML phrases with images
- [ ] Optional encryption of the library

## M7 — Sharing & distribution
- [ ] Typing into elevated ("Run as administrator") windows without running Phrase X as admin: signed build with `uiAccess=true`, installed in Program Files
- [ ] (Not possible by Windows design: the UAC consent prompt runs on the isolated Secure Desktop — no program can type there)
- [ ] Shared library on a network folder (multi-user, conflict-safe)
- [x] Cloud sync via "Accedi con Google" (2026-11-09): OAuth desktop flow (loopback + PKCE), library in Drive appDataFolder, per-item last-write-wins + tombstones (store V6), refresh token in Windows Credential Manager, Settings: sign in/out, last sync, "Sincronizza ora". Needs the user's OAuth client (GOOGLE-SETUP.md) — settings not synced yet
- [x] Windows installer (NSIS) — auto-update still to do; no licences, no limits, no telemetry
- [ ] macOS / Linux listener backends
