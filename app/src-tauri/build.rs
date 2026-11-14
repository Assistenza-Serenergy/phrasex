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
        if let Some(project) = client["project_id"].as_str() {
            println!("cargo:rustc-env=PHRASEX_GOOGLE_PROJECT_ID={project}");
        }
    }
    // Sharing (Firebase in the same Google project): the web API key from Project settings.
    println!("cargo:rerun-if-changed=firebase.json");
    if let Ok(text) = std::fs::read_to_string("firebase.json") {
        let json: serde_json::Value = serde_json::from_str(&text).expect("firebase.json is not valid JSON");
        let key = json["apiKey"].as_str().expect("firebase.json: apiKey missing");
        println!("cargo:rustc-env=PHRASEX_FIREBASE_API_KEY={key}");
    }
    tauri_build::build()
}
