fn main() {
    // Google OAuth client for cloud sync (see GOOGLE-SETUP.md); without it sync says "not configured".
    println!("cargo:rerun-if-changed=google-oauth.json");
    if let Ok(text) = std::fs::read_to_string("google-oauth.json") {
        let json: serde_json::Value = serde_json::from_str(&text).expect("google-oauth.json is not valid JSON");
        let client = &json["installed"];
        for (key, env) in [("client_id", "PHRASEX_GOOGLE_CLIENT_ID"), ("client_secret", "PHRASEX_GOOGLE_CLIENT_SECRET")] {
            let value = client[key].as_str().unwrap_or_else(|| panic!("google-oauth.json: installed.{key} missing (client type must be Desktop app)"));
            println!("cargo:rustc-env={env}={value}");
        }
    }
    tauri_build::build()
}
