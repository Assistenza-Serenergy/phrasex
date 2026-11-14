//! "Accedi con Google": OAuth for desktop apps (loopback redirect + PKCE), refresh token in the
//! Windows Credential Manager, library as one JSON file in the hidden Drive appDataFolder.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use phrasex_store::sync::{merge, Library, Share};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// From app/src-tauri/google-oauth.json via build.rs (see GOOGLE-SETUP.md).
const CLIENT_ID: Option<&str> = option_env!("PHRASEX_GOOGLE_CLIENT_ID");
const CLIENT_SECRET: Option<&str> = option_env!("PHRASEX_GOOGLE_CLIENT_SECRET");
/// profile = the user's name, shown to the people they share their phrases with.
const SCOPES: &str = "openid email profile https://www.googleapis.com/auth/drive.appdata";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";
const FILE_NAME: &str = "phrasex-library.json";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

type Res<T> = Result<T, String>;

/// Set by "Annulla" while the browser sign-in is pending.
pub static CANCEL_LOGIN: AtomicBool = AtomicBool::new(false);

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn configured() -> bool {
    client().is_ok()
}

fn client() -> Res<(&'static str, &'static str)> {
    match (CLIENT_ID, CLIENT_SECRET) {
        (Some(id), Some(secret)) if !id.is_empty() => Ok((id, secret)),
        _ => Err("google_not_configured".into()),
    }
}

fn credential() -> Res<keyring::Entry> {
    keyring::Entry::new("Phrase X", "google-refresh-token").map_err(err)
}

fn agent() -> Res<ureq::Agent> {
    let tls = native_tls::TlsConnector::new().map_err(err)?; // Windows' own TLS (schannel)
    Ok(ureq::AgentBuilder::new().tls_connector(Arc::new(tls)).timeout(Duration::from_secs(30)).build())
}

fn http(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let body = r.into_string().unwrap_or_default();
            if body.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT") {
                return SCOPE_MISSING.into();
            }
            format!("HTTP {code}: {body}")
        }
        e => e.to_string(),
    }
}

/// The user left Google's "Drive" checkbox unticked on the consent screen.
pub const SCOPE_MISSING: &str = "google_scope_missing";
const DRIVE_SCOPE: &str = "https://www.googleapis.com/auth/drive.appdata";

fn has_drive(scope: Option<&str>) -> bool {
    scope.map_or(true, |s| s.split_whitespace().any(|x| x == DRIVE_SCOPE)) // absent = unchanged grant
}

fn random(bytes: usize) -> Res<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(err)?;
    Ok(B64.encode(buf))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let (b, mut out, mut i) = (s.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        let hex = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok());
        let (byte, step) = match (b[i], hex) {
            (b'%', Some(v)) => (v, 3),
            (b'+', _) => (b' ', 1),
            (c, _) => (c, 1),
        };
        out.push(byte);
        i += step;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Deserialize)]
struct Token {
    access_token: String,
    scope: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

/// Opens Google's sign-in in the browser, waits for the redirect, stores the refresh token.
/// Returns the account's email and the user's name (as on their Google account).
pub fn login(open_browser: impl Fn(&str)) -> Res<(String, String)> {
    let (id, secret) = client()?;
    CANCEL_LOGIN.store(false, Relaxed);
    let (verifier, state) = (random(32)?, random(16)?);
    let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
    let listener = TcpListener::bind("127.0.0.1:0").map_err(err)?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(err)?.port());
    open_browser(&format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}\
         &code_challenge={challenge}&code_challenge_method=S256&state={state}&access_type=offline&prompt=consent",
        encode(id),
        encode(&redirect),
        encode(SCOPES)
    ));
    let code = wait_for_code(&listener, &state)?;
    let tok: Token = agent()?
        .post(TOKEN_URL)
        .send_form(&[
            ("code", code.as_str()),
            ("client_id", id),
            ("client_secret", secret),
            ("redirect_uri", &redirect),
            ("grant_type", "authorization_code"),
            ("code_verifier", &verifier),
        ])
        .map_err(http)?
        .into_json()
        .map_err(err)?;
    if !has_drive(tok.scope.as_deref()) {
        let _ = agent()?.post("https://oauth2.googleapis.com/revoke").send_form(&[("token", tok.access_token.as_str())]);
        return Err(SCOPE_MISSING.into());
    }
    credential()?.set_password(&tok.refresh_token.ok_or("google_no_refresh_token")?).map_err(err)?;
    let id = tok.id_token.unwrap_or_default();
    Ok((email_from_id_token(&id).unwrap_or_default(), claim(&id, "name").unwrap_or_default()))
}

/// The email claim of a Google id_token (already trusted: it came straight from the token endpoint over TLS).
fn email_from_id_token(jwt: &str) -> Option<String> {
    claim(jwt, "email")
}

fn claim(jwt: &str, key: &str) -> Option<String> {
    let payload = B64.decode(jwt.split('.').nth(1)?.trim_end_matches('=')).ok()?;
    serde_json::from_slice::<serde_json::Value>(&payload).ok()?[key].as_str().map(str::to_string)
}

fn wait_for_code(listener: &TcpListener, state: &str) -> Res<String> {
    listener.set_nonblocking(true).map_err(err)?;
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    loop {
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if CANCEL_LOGIN.load(Relaxed) {
                    return Err("google_login_cancelled".into());
                }
                if Instant::now() > deadline {
                    return Err("google_login_timeout".into());
                }
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) => return Err(err(e)),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let _ = BufReader::new(&stream).read_line(&mut line);
        match parse_redirect(&line, state) {
            None => reply(&mut stream, "404 Not Found", ""), // favicon and the like
            Some(result) => {
                let ok = result.is_ok();
                reply(&mut stream, "200 OK", if ok { PAGE_OK } else { PAGE_FAIL });
                return result;
            }
        }
    }
}

/// `GET /?state=..&code=.. HTTP/1.1` → Some(Ok(code)); an error or a wrong state → Some(Err); other requests → None.
fn parse_redirect(request_line: &str, state: &str) -> Option<Res<String>> {
    let query = request_line.split_whitespace().nth(1)?.split_once('?')?.1;
    let param = |k: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(n, _)| *n == k).map(|(_, v)| decode(v)));
    match (param("code"), param("error")) {
        (Some(code), _) if param("state").as_deref() == Some(state) => Some(Ok(code)),
        (Some(_), _) => Some(Err("google_login_failed: state".into())),
        (None, Some(e)) => Some(Err(format!("google_login_failed: {e}"))),
        (None, None) => None,
    }
}

/// The tab tries to close itself (browsers allow it only for some tabs); Phrase X comes to the front anyway.
const PAGE_OK: &str = "<!doctype html><meta charset=utf-8><title>Phrase X</title><body style=\"font:15px system-ui;padding:3em;color:#777\">\
<p>Fatto, torna a Phrase X.</p><script>window.close()</script>";
const PAGE_FAIL: &str = "<!doctype html><meta charset=utf-8><title>Phrase X</title><body style=\"font:16px system-ui;padding:3em\">\
<h1>Phrase X</h1><p>Accesso non riuscito: riprova da Impostazioni.</p><p>Sign-in failed: try again from Settings.</p>";

fn reply(stream: &mut TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Forgets the account on this PC (revokes the token at Google, best effort). The cloud copy stays.
pub fn logout() {
    if let Ok(c) = credential() {
        if let (Ok(token), Ok(a)) = (c.get_password(), agent()) {
            let _ = a.post("https://oauth2.googleapis.com/revoke").send_form(&[("token", token.as_str())]);
        }
        let _ = c.delete_credential();
    }
}

fn access_token(a: &ureq::Agent) -> Res<String> {
    Ok(tokens(a)?.access_token)
}

/// Fresh tokens from the stored refresh token (the id_token says who the user is).
fn tokens(a: &ureq::Agent) -> Res<Token> {
    let (id, secret) = client()?;
    let refresh = credential()?.get_password().map_err(|_| "google_signed_out".to_string())?;
    let form = [("client_id", id), ("client_secret", secret), ("refresh_token", refresh.as_str()), ("grant_type", "refresh_token")];
    match a.post(TOKEN_URL).send_form(&form) {
        Ok(r) => {
            let tok = r.into_json::<Token>().map_err(err)?;
            if !has_drive(tok.scope.as_deref()) {
                return Err(SCOPE_MISSING.into());
            }
            Ok(tok)
        }
        Err(ureq::Error::Status(400 | 401, _)) => Err("google_relogin".into()), // revoked or expired
        Err(e) => Err(http(e)),
    }
}

/// What one sync round does with the local and cloud libraries.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub merged: Library,
    pub upload: bool,
    pub apply: bool,
}

/// `adopt_cloud`: if the cloud already has a library, it replaces the local one instead of merging
/// (a new PC with only the untouched examples, or another Google account signed in on this PC).
pub fn plan(local: &Library, remote: Option<&Library>, adopt_cloud: bool) -> Plan {
    let empty = Library::default();
    let mine = if adopt_cloud && remote.is_some() { &empty } else { local };
    let merged = merge(mine, remote.unwrap_or(&empty));
    Plan { upload: remote.map_or(true, |r| merge(r, &empty) != merged), apply: merged != *local, merged }
}

/// The Drive side of a sync round.
pub struct Drive {
    agent: ureq::Agent,
    bearer: String,
    file: Option<String>,
}

impl Drive {
    pub fn connect() -> Res<Self> {
        let agent = agent()?;
        let bearer = format!("Bearer {}", access_token(&agent)?);
        #[derive(Deserialize)]
        struct Files {
            files: Vec<File>,
        }
        #[derive(Deserialize)]
        struct File {
            id: String,
        }
        let list: Files = agent
            .get(FILES_URL)
            .set("Authorization", &bearer)
            .query("spaces", "appDataFolder")
            .query("q", &format!("name = '{FILE_NAME}' and trashed = false"))
            .query("fields", "files(id)")
            .call()
            .map_err(http)?
            .into_json()
            .map_err(err)?;
        Ok(Self { agent, bearer, file: list.files.into_iter().next().map(|f| f.id) })
    }

    pub fn download(&self) -> Res<Option<Library>> {
        let Some(id) = &self.file else { return Ok(None) };
        let text = self
            .agent
            .get(&format!("{FILES_URL}/{id}"))
            .set("Authorization", &self.bearer)
            .query("alt", "media")
            .call()
            .map_err(http)?
            .into_string()
            .map_err(err)?;
        serde_json::from_str(&text).map(Some).map_err(|e| format!("cloud file unreadable: {e}"))
    }

    /// ponytail: no conditional write, two PCs uploading in the same second can lose one round;
    /// the next sync of the loser re-uploads its changes (they are still in its local DB).
    pub fn upload(&mut self, lib: &Library) -> Res<()> {
        let json = serde_json::to_string(lib).map_err(err)?;
        if let Some(id) = &self.file {
            self.agent
                .request("PATCH", &format!("{UPLOAD_URL}/{id}?uploadType=media"))
                .set("Authorization", &self.bearer)
                .set("Content-Type", "application/json; charset=UTF-8")
                .send_string(&json)
                .map_err(http)?;
            return Ok(());
        }
        let boundary = format!("phrasex{}", random(18)?.replace(['-', '_'], "x"));
        let body = format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{{\"name\":\"{FILE_NAME}\",\"parents\":[\"appDataFolder\"]}}\r\n\
             --{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{json}\r\n--{boundary}--"
        );
        #[derive(Deserialize)]
        struct Created {
            id: String,
        }
        let created: Created = self
            .agent
            .post(&format!("{UPLOAD_URL}?uploadType=multipart&fields=id"))
            .set("Authorization", &self.bearer)
            .set("Content-Type", &format!("multipart/related; boundary={boundary}"))
            .send_string(&body)
            .map_err(http)?
            .into_json()
            .map_err(err)?;
        self.file = Some(created.id);
        Ok(())
    }
}

// ---------- sharing: one Firestore document per owner, readable by the emails they listed ----------

const FIREBASE_KEY: Option<&str> = option_env!("PHRASEX_FIREBASE_API_KEY");
const PROJECT: Option<&str> = option_env!("PHRASEX_GOOGLE_PROJECT_ID");

pub fn sharing_configured() -> bool {
    matches!((FIREBASE_KEY, PROJECT), (Some(k), Some(p)) if !k.is_empty() && !p.is_empty())
}

/// Short stable digest, to tell whether something changed since last time.
pub fn fingerprint(s: &str) -> String {
    B64.encode(Sha256::digest(s.as_bytes()))
}

/// Signed in to Firebase with the user's Google account (no extra sign-in: the stored Google token is reused).
pub struct Sharing {
    agent: ureq::Agent,
    bearer: String,
    base: String,
    uid: String,
    pub email: String,
    pub name: String,
}

impl Sharing {
    pub fn connect() -> Res<Self> {
        let (Some(key), Some(project)) = (FIREBASE_KEY, PROJECT) else { return Err("sharing_not_configured".into()) };
        let agent = agent()?;
        let google = tokens(&agent)?.id_token.ok_or("google_relogin")?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SignIn {
            id_token: String,
            local_id: String,
            email: Option<String>,
            display_name: Option<String>,
        }
        let r: SignIn = agent
            .post(&format!("https://identitytoolkit.googleapis.com/v1/accounts:signInWithIdp?key={key}"))
            .send_json(serde_json::json!({
                "postBody": format!("id_token={google}&providerId=google.com"),
                "requestUri": "http://localhost",
                "returnSecureToken": true,
            }))
            .map_err(http)?
            .into_json()
            .map_err(err)?;
        let email = r.email.or_else(|| claim(&google, "email")).unwrap_or_default().to_lowercase();
        // accounts signed in before the profile scope have no name until they sign in again: email then
        let name = claim(&google, "name").or(r.display_name).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| email.clone());
        Ok(Self {
            agent,
            bearer: format!("Bearer {}", r.id_token),
            base: format!("https://firestore.googleapis.com/v1/projects/{project}/databases/(default)/documents"),
            uid: r.local_id,
            email,
            name,
        })
    }

    /// Puts the user's library where the listed emails can read it; no emails = stop sharing.
    /// ponytail: one document, Firestore caps it at 1 MB (thousands of phrases); split per folder if a library outgrows it.
    pub fn publish(&self, readers: &[String], lib: &Library) -> Res<()> {
        let url = format!("{}/shares/{}", self.base, self.uid);
        if readers.is_empty() {
            return match self.agent.delete(&url).set("Authorization", &self.bearer).call() {
                Ok(_) | Err(ureq::Error::Status(404, _)) => Ok(()),
                Err(e) => Err(http(e)),
            };
        }
        let text = |v: &str| serde_json::json!({ "stringValue": v });
        let body = serde_json::json!({ "fields": {
            "ownerName": text(&self.name),
            "ownerEmail": text(&self.email),
            "readers": { "arrayValue": { "values": readers.iter().map(|r| text(r)).collect::<Vec<_>>() } },
            "library": text(&serde_json::to_string(lib).map_err(err)?),
        }});
        self.agent.request("PATCH", &url).set("Authorization", &self.bearer).send_json(body).map_err(http)?;
        Ok(())
    }

    /// Libraries other people share with this user's email.
    pub fn incoming(&self) -> Res<Vec<Share>> {
        let query = serde_json::json!({ "structuredQuery": {
            "from": [{ "collectionId": "shares" }],
            "where": { "fieldFilter": { "field": { "fieldPath": "readers" }, "op": "ARRAY_CONTAINS", "value": { "stringValue": self.email } } },
        }});
        let rows: Vec<serde_json::Value> = self
            .agent
            .post(&format!("{}:runQuery", self.base))
            .set("Authorization", &self.bearer)
            .send_json(query)
            .map_err(http)?
            .into_json()
            .map_err(err)?;
        Ok(shares_from(&rows, &self.uid))
    }
}

/// runQuery rows → shares (a row without `document` = no results; the user's own document is skipped).
fn shares_from(rows: &[serde_json::Value], me: &str) -> Vec<Share> {
    let mut out: Vec<Share> = rows
        .iter()
        .filter_map(|row| {
            let doc = row.get("document")?;
            let owner = doc["name"].as_str()?.rsplit('/').next()?.to_string();
            let field = |k: &str| doc["fields"][k]["stringValue"].as_str().unwrap_or_default().to_string();
            let library = serde_json::from_str(&field("library")).ok()?;
            let name = Some(field("ownerName")).filter(|n| !n.is_empty()).unwrap_or_else(|| field("ownerEmail"));
            (owner != me).then_some(Share { owner, name, library })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use phrasex_store::sync::{SyncSnippet, EPOCH};

    fn snippet(uid: &str, at: &str) -> SyncSnippet {
        SyncSnippet {
            uid: uid.into(),
            folder: None,
            name: uid.into(),
            trigger: None,
            body: "x".into(),
            immediate: false,
            case_sensitive: false,
            enabled: true,
            hotkey: None,
            tags: vec![],
            updated_at: at.into(),
        }
    }

    #[test]
    fn firestore_rows_to_shares() {
        let lib = Library { snippets: vec![snippet("a", EPOCH)], ..Default::default() };
        let doc = |id: &str, name: &str| {
            serde_json::json!({ "document": { "name": format!("projects/p/databases/(default)/documents/shares/{id}"), "fields": {
                "ownerName": { "stringValue": name }, "ownerEmail": { "stringValue": "x@y.it" },
                "library": { "stringValue": serde_json::to_string(&lib).unwrap() } } } })
        };
        let rows = vec![doc("u2", "Zeno"), doc("me", "Io"), doc("u1", ""), serde_json::json!({ "readTime": "t" })];
        let got = shares_from(&rows, "me");
        assert_eq!(got.iter().map(|s| (s.owner.as_str(), s.name.as_str())).collect::<Vec<_>>(), [("u1", "x@y.it"), ("u2", "Zeno")]);
        assert_eq!(got[0].library, lib);
        assert!(shares_from(&[serde_json::json!({ "readTime": "t" })], "me").is_empty());
    }

    #[test]
    fn redirect_parsing() {
        let ok = parse_redirect("GET /?state=abc&code=4%2F0Ab-x&scope=email HTTP/1.1\r\n", "abc");
        assert_eq!(ok, Some(Ok("4/0Ab-x".into())));
        assert!(matches!(parse_redirect("GET /?state=evil&code=1 HTTP/1.1", "abc"), Some(Err(_))));
        assert_eq!(parse_redirect("GET /?error=access_denied&state=abc HTTP/1.1", "abc"), Some(Err("google_login_failed: access_denied".into())));
        assert_eq!(parse_redirect("GET /favicon.ico HTTP/1.1", "abc"), None);
        assert_eq!(encode("http://127.0.0.1:5 a"), "http%3A%2F%2F127.0.0.1%3A5%20a");
        let jwt = format!("h.{}.s", B64.encode(br#"{"email":"me@example.com"}"#));
        assert_eq!(email_from_id_token(&jwt).as_deref(), Some("me@example.com"));
        assert!(has_drive(Some("openid https://www.googleapis.com/auth/drive.appdata https://www.googleapis.com/auth/userinfo.email")));
        assert!(!has_drive(Some("openid https://www.googleapis.com/auth/userinfo.email")), "Drive box left unticked");
        assert!(has_drive(None));
    }

    #[test]
    fn plan_first_sync_and_steady_state() {
        let seed = Library { snippets: vec![snippet("seed", EPOCH)], ..Default::default() };
        let cloud = Library { snippets: vec![snippet("mine", "2026-11-09 10:00:00.000")], ..Default::default() };
        // new PC: the cloud library replaces the untouched examples, nothing to upload
        let p = plan(&seed, Some(&cloud), true);
        assert_eq!((p.merged.clone(), p.upload, p.apply), (cloud.clone(), false, true));
        // first PC (empty cloud): its examples are uploaded
        let p = plan(&seed, None, true);
        assert_eq!((p.merged, p.upload, p.apply), (seed.clone(), true, false));
        // in sync: nothing to do
        let p = plan(&cloud, Some(&cloud), false);
        assert!(!p.upload && !p.apply);
        // already synced PC keeps even untouched examples (they are in the cloud too)
        let p = plan(&seed, Some(&cloud), false);
        assert!(p.upload && p.apply && p.merged.snippets.len() == 2);
        // another account signed in: its cloud library replaces the local one
        let other = Library { snippets: vec![snippet("other", "2026-11-09 11:00:00.000")], ..Default::default() };
        let p = plan(&other, Some(&cloud), true);
        assert_eq!((p.merged, p.upload, p.apply), (cloud.clone(), false, true));
        // ...but an empty cloud gets the local library
        let p = plan(&other, None, true);
        assert_eq!((p.merged, p.upload, p.apply), (other, true, false));
    }
}
