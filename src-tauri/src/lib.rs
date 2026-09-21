//! AutoDOP desktop backend.
//!
//! Runs the Astro frontend in a native webview and exposes commands the page
//! calls with `invoke(...)`. The important one is [`generate_lists`], which runs
//! `scraper.py` as a local subprocess — something a plain browser page cannot do.
//!
//! Credentials live in the OS app-config directory (or the environment), never
//! in the webview's storage.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub mod crypt;
pub mod db;

/// Selenium waits up to 360s for the DOP login alone; allow a long ceiling.
const SCRAPER_TIMEOUT_SECS: u64 = 3600;
/// Event name the frontend subscribes to for live scraper output.
const PROGRESS_EVENT: &str = "scraper-progress";

// --------------------------------------------------------------------------- //
// types                                                                       //
// --------------------------------------------------------------------------- //

#[derive(Debug, Deserialize)]
pub struct GenList {
    pub name: String,
    pub numbers: Vec<String>,
    #[serde(default)]
    pub rebate: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct GenResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returncode: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
}

impl GenResult {
    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            results: None,
            error: Some(message.into()),
            returncode: None,
            log: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub desktop: bool,
    pub scraper: String,
    pub scraper_present: bool,
    pub credentials: bool,
    pub python: String,
}

// --------------------------------------------------------------------------- //
// paths + credentials                                                         //
// --------------------------------------------------------------------------- //

/// `scraper.py` location: env override, then the repo root (dev), then CWD.
fn scraper_path() -> Option<PathBuf> {
    if let Ok(value) = std::env::var("AUTODOP_SCRAPER") {
        let candidate = PathBuf::from(value.trim());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Some(root) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        let candidate = root.join("scraper.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let candidate = std::env::current_dir().ok()?.join("scraper.py");
    candidate.is_file().then_some(candidate)
}

/// Python interpreter: env override, else `python` on Windows / `python3` elsewhere.
fn python_bin() -> String {
    if let Ok(value) = std::env::var("AUTODOP_PYTHON") {
        if !value.trim().is_empty() {
            return value.trim().to_string();
        }
    }
    if cfg!(windows) {
        "python".to_string()
    } else {
        "python3".to_string()
    }
}

fn credentials_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("no config dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir.join("credentials.json"))
}

/// Credentials from the environment/`.env` or the app-config file, if present.
fn load_local_credentials(app: &AppHandle) -> Option<db::DopCredentials> {
    let env_user = std::env::var("DOP_USERNAME").unwrap_or_default();
    let env_pass = std::env::var("DOP_PASSWORD").unwrap_or_default();
    if !env_user.trim().is_empty() && !env_pass.trim().is_empty() {
        return Some(db::DopCredentials {
            username: env_user,
            password: env_pass,
            source: db::CredentialSource::Env,
        });
    }
    let path = credentials_path(app).ok()?;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let username = value
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let password = value
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if username.trim().is_empty() || password.trim().is_empty() {
        return None;
    }
    Some(db::DopCredentials {
        username,
        password,
        source: db::CredentialSource::Config,
    })
}

/// Resolve DOP credentials, most explicit source first:
///
/// 1. `DOP_USERNAME` / `DOP_PASSWORD` (environment or a `.env`)
/// 2. the app-config `credentials.json`
/// 3. the `users` collection in Atlas, decrypted with `FERNET_KEY`
///
/// The password stays on this side — it is only ever handed to `scraper.py`.
async fn resolve_credentials(app: &AppHandle) -> Result<db::DopCredentials, String> {
    if let Some(credentials) = load_local_credentials(app) {
        return Ok(credentials);
    }
    db::fetch_atlas_credentials(app).await
}

#[tauri::command]
async fn app_info(app: AppHandle) -> AppInfo {
    let credentials = resolve_credentials(&app).await;
    let scraper = scraper_path();
    AppInfo {
        desktop: true,
        scraper: scraper
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not found".into()),
        scraper_present: scraper.is_some(),
        credentials: credentials.is_ok(),
        python: python_bin(),
    }
}

// --------------------------------------------------------------------------- //
// payload + result parsing (mirrors desktop/main.py --selftest behaviour)      //
// --------------------------------------------------------------------------- //

/// Normalize to the shape `scraper.py` expects.
///
/// `process_lists` zips `numbers` with `rebate`, so both arrays must be the same
/// length — pad missing rebates with `1` ("no rebate, just pay").
fn build_payload(lists: Vec<GenList>) -> Vec<Value> {
    let mut payload = Vec::new();
    for list in lists {
        let numbers: Vec<String> = list
            .numbers
            .iter()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        if numbers.is_empty() {
            continue;
        }
        let mut rebate = list.rebate;
        if rebate.len() < numbers.len() {
            rebate.resize(numbers.len(), 1);
        }
        rebate.truncate(numbers.len());
        payload.push(json!({
            "name": if list.name.trim().is_empty() { "Unnamed" } else { list.name.trim() },
            "numbers": numbers,
            "rebate": rebate,
        }));
    }
    payload
}

/// Extract the JSON array `scraper.py` prints after its log lines.
fn parse_results(stdout: &str) -> Option<Vec<Value>> {
    let bytes = stdout.as_bytes();
    let starts: Vec<usize> = bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'[')
        .map(|(i, _)| i)
        .collect();
    for index in starts.into_iter().rev() {
        if let Ok(value) = serde_json::from_str::<Value>(&stdout[index..]) {
            if let Value::Array(items) = value {
                return Some(items);
            }
        }
    }
    None
}

// --------------------------------------------------------------------------- //
// commands                                                                    //
// --------------------------------------------------------------------------- //

#[tauri::command]
fn set_credentials(app: AppHandle, username: String, password: String) -> Result<(), String> {
    let username = username.trim().to_string();
    if username.is_empty() || password.is_empty() {
        return Err("Username and password are both required.".into());
    }
    let path = credentials_path(&app)?;
    let body = serde_json::to_string_pretty(&json!({ "username": username, "password": password }))
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Run `scraper.py` for the supplied lists. Streams each stdout line to the
/// frontend as a `scraper-progress` event and returns the parsed result array.
#[tauri::command]
async fn generate_lists(app: AppHandle, lists: Vec<GenList>) -> GenResult {
    // Credentials: env/`.env` first, then the config file, then the `users`
    // collection in Atlas (Fernet-decrypted). The password never leaves Rust.
    let credentials = match resolve_credentials(&app).await {
        Ok(credentials) => credentials,
        Err(error) => {
            return GenResult::err(format!(
                "No DOP credentials available: {error}. Set them in Manage → DOP Credentials, \
                 or add FERNET_KEY to src-tauri/.env so they can be read from Atlas."
            ))
        }
    };
    let user = credentials.username;
    let password = credentials.password;
    let Some(script) = scraper_path() else {
        return GenResult::err("scraper.py not found (set AUTODOP_SCRAPER to its path).");
    };

    let payload = build_payload(lists);
    if payload.is_empty() {
        return GenResult::err("No account numbers to generate — add accounts to the list first.");
    }

    let names: Vec<String> = payload
        .iter()
        .filter_map(|v| v.get("name").and_then(Value::as_str).map(str::to_string))
        .collect();
    let _ = app.emit(PROGRESS_EVENT, format!("Starting: {}", names.join(", ")));

    let lists_json = match serde_json::to_string(&payload) {
        Ok(json) => json,
        Err(e) => return GenResult::err(format!("could not serialize payload: {e}")),
    };

    let result = tauri::async_runtime::spawn_blocking(move || {
        run_scraper(&app, &script, &user, &password, &lists_json)
    })
    .await;

    match result {
        Ok(gen_result) => gen_result,
        Err(e) => GenResult::err(format!("scraper task failed: {e}")),
    }
}

fn run_scraper(
    app: &AppHandle,
    script: &std::path::Path,
    user: &str,
    password: &str,
    lists_json: &str,
) -> GenResult {
    let mut child = match Command::new(python_bin())
        .arg(script)
        .arg(user)
        .arg(password)
        .arg(lists_json)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return GenResult::err(format!("could not start python: {e}")),
    };

    // Drain both pipes on their own threads: streaming stdout to the UI as
    // progress, so a full pipe buffer can never deadlock the child.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let app_for_stdout = app.clone();
    let stdout_thread = std::thread::spawn(move || {
        let mut collected = String::new();
        if let Some(pipe) = stdout_pipe {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                let _ = app_for_stdout.emit(PROGRESS_EVENT, line.clone());
                collected.push_str(&line);
                collected.push('\n');
            }
        }
        collected
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut collected = String::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_string(&mut collected);
        }
        collected
    });

    // Poll-with-deadline so a hung driver cannot wedge the app forever.
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if started.elapsed() > Duration::from_secs(SCRAPER_TIMEOUT_SECS) {
                    let _ = child.kill();
                    let _ = child.wait();
                    timed_out = true;
                    break None;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => return GenResult::err(format!("cannot wait on python: {e}")),
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    if timed_out {
        return GenResult::err(format!("Scraper timed out after {SCRAPER_TIMEOUT_SECS}s."));
    }

    match parse_results(&stdout) {
        Some(results) => {
            let code = status.map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let _ = app.emit(PROGRESS_EVENT, "Scraper finished.".to_string());
            GenResult {
                ok: code == 0,
                results: Some(results),
                error: None,
                returncode: Some(code),
                log: tail(&stderr, 1500),
            }
        }
        None => {
            let mut result = GenResult::err("Scraper produced no parseable result.");
            result.returncode = status.map(|s| s.code().unwrap_or(-1));
            result.log = tail(
                if stderr.trim().is_empty() {
                    &stdout
                } else {
                    &stderr
                },
                1500,
            );
            result
        }
    }
}

fn tail(text: &str, limit: usize) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= limit {
        return Some(trimmed.to_string());
    }
    let mut start = trimmed.len() - limit;
    while start < trimmed.len() && !trimmed.is_char_boundary(start) {
        start += 1;
    }
    Some(trimmed[start..].to_string())
}

// --------------------------------------------------------------------------- //
// entrypoint                                                                  //
// --------------------------------------------------------------------------- //

/// Report whether an Atlas connection is configured and reachable.
#[tauri::command]
async fn db_status(app: AppHandle) -> db::DbStatus {
    db::fetch_status(&app).await
}

/// Load every account document from Atlas, mapped to the frontend shape.
#[tauri::command]
async fn load_accounts(app: AppHandle) -> Result<Vec<Value>, String> {
    db::fetch_accounts(&app).await
}

/// Load the saved lists (and per-account rebates) from Atlas.
#[tauri::command]
async fn load_lists(app: AppHandle) -> Result<Vec<db::DbList>, String> {
    db::fetch_lists(&app).await
}

/// Upsert the supplied lists to Atlas and return what is now stored.
#[tauri::command]
async fn save_lists(app: AppHandle, lists: Vec<db::InputList>) -> Result<Vec<db::DbList>, String> {
    db::save_lists(&app, lists).await
}

/// Which DOP credentials the app would use, and where they come from.
///
/// Never returns the password — only the portal id and the source.
#[tauri::command]
async fn dop_credentials_status(app: AppHandle) -> db::DopCredentialStatus {
    match resolve_credentials(&app).await {
        Ok(credentials) => db::DopCredentialStatus {
            username: credentials.username,
            source: credentials.source,
            has_password: !credentials.password.trim().is_empty(),
            atlas_available: true,
            detail: None,
        },
        Err(error) => db::DopCredentialStatus {
            username: String::new(),
            source: db::CredentialSource::Atlas,
            has_password: false,
            atlas_available: false,
            detail: Some(error),
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            set_credentials,
            generate_lists,
            dop_credentials_status,
            db_status,
            load_accounts,
            load_lists,
            save_lists
        ])
        .run(tauri::generate_context!())
        .expect("error while running AutoDOP");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(name: &str, numbers: &[&str], rebate: &[i64]) -> GenList {
        GenList {
            name: name.to_string(),
            numbers: numbers.iter().map(|s| s.to_string()).collect(),
            rebate: rebate.to_vec(),
        }
    }

    #[test]
    fn payload_pads_rebates_to_match_numbers() {
        let payload = build_payload(vec![list("A", &["111", "222", "333"], &[4])]);
        assert_eq!(payload[0]["rebate"], json!([4, 1, 1]));
        assert_eq!(payload[0]["name"], json!("A"));
        assert_eq!(payload[0]["numbers"], json!(["111", "222", "333"]));
    }

    #[test]
    fn payload_skips_empty_and_trims() {
        assert!(build_payload(vec![list("B", &[], &[])]).is_empty());
        let payload = build_payload(vec![list("C", &[" 9 ", " "], &[1, 1])]);
        assert_eq!(payload[0]["numbers"], json!(["9"]));
        assert_eq!(payload[0]["rebate"], json!([1]));
    }

    #[test]
    fn payload_truncates_extra_rebates() {
        let payload = build_payload(vec![list("D", &["1"], &[1, 2, 3])]);
        assert_eq!(payload[0]["rebate"], json!([1]));
    }

    #[test]
    fn payload_defaults_blank_name() {
        let payload = build_payload(vec![list("   ", &["1"], &[1])]);
        assert_eq!(payload[0]["name"], json!("Unnamed"));
    }

    #[test]
    fn parses_trailing_json_array_after_logs() {
        let sample = concat!(
            "Inside Login Page\n",
            "Processing list: A\n",
            "Account numbers: ['111']\n",
            "[\n  {\n    \"list_name\": \"A\",\n    \"status\": \"success\",\n",
            "    \"details\": {\"gen_number\": \"1234567890\"}\n  }\n]\n"
        );
        let parsed = parse_results(sample).expect("array parsed");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["list_name"], json!("A"));
    }

    #[test]
    fn parse_returns_none_without_json() {
        assert!(parse_results("no json here").is_none());
    }
}
