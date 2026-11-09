//! Headless demo: global hook + engine + snippets from %APPDATA%\PhraseX\phrasex.db. Open Notepad and type.
#[cfg(windows)]
fn main() {
    use phrasex_engine::{macros, Engine};

    let (store, db_path) = phrasex_store::Store::open_default().expect("database");
    let snippets = store.engine_snippets().expect("read");
    println!("Phrase X attivo / active: {} snippet da / from {}  (Ctrl+C per uscire / to quit)", snippets.len(), db_path.display());
    let mut engine = Engine::new(snippets);

    let keys = phrasex_listener::start().expect("listener");
    let debug = std::env::var_os("PHRASEX_DEBUG").is_some();
    for key in keys {
        let hit = engine.feed(key);
        if debug {
            eprintln!("{key:?} -> {hit:?}");
        }
        if let Some(e) = hit {
            if let Some(text) = macros::render(&e.text, &phrasex_listener::System) {
                phrasex_listener::inject(e.backspaces, &text);
            }
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Phrase X listener: only Windows is supported for now.");
}
