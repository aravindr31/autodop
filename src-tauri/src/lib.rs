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
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
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
    /// Where the full run log was written, so a failure can be inspected later
    /// instead of vanishing with the progress area.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
}

impl GenResult {
    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            results: None,
            error: Some(message.into()),
            returncode: None,
            log: None,
            log_path: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub desktop: bool,
    /// App version, from `tauri.conf.json` — also the name of the installer.
    pub version: String,
    /// `<short sha> <commit date>` for the build you are running.
    pub build: String,
    pub scraper: String,
    pub scraper_present: bool,
    /// `chosen` | `env` | `sidecar` | `bundled` | `repo` | `cwd`.
    pub scraper_source: String,
    /// `sidecar` (self-contained) | `script` (needs Python).
    pub scraper_kind: String,
    pub credentials: bool,
    pub python: String,
}

// --------------------------------------------------------------------------- //
// paths + credentials                                                         //
// --------------------------------------------------------------------------- //

/// Where the runner was found, and where it came from.
#[derive(Debug, Serialize, Clone)]
pub struct ScraperLocation {
    pub path: String,
    /// `chosen` | `env` | `sidecar` | `bundled` | `repo` | `cwd`.
    pub source: String,
    /// `sidecar` (a self-contained executable) | `script` (needs Python).
    pub kind: String,
    pub present: bool,
}

/// A `.py` needs an interpreter; anything else is treated as a frozen helper.
fn kind_of(path: &Path) -> &'static str {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
    {
        "script"
    } else {
        "sidecar"
    }
}

/// Name of the packaged helper for this platform, e.g. `scraper-macos-arm64`.
/// Produced by `npm run build:sidecar` — see the README.
fn sidecar_name() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    format!("scraper-{os}-{arch}{suffix}")
}

/// The frozen helper inside a resource directory, if one was bundled.
fn bundled_sidecar_in(resource_dir: &Path) -> Option<PathBuf> {
    let candidate = resource_dir.join("binaries").join(sidecar_name());
    candidate.is_file().then_some(candidate)
}

/// The bundled script inside a resource directory.
///
/// Tauri rewrites a `../` resource path into `_up_` while copying, so the file
/// can end up at either spelling — accept both rather than guess.
fn bundled_scraper_in(resource_dir: &Path) -> Option<PathBuf> {
    ["scraper.py", "_up_/scraper.py"]
        .iter()
        .map(|relative| resource_dir.join(relative))
        .find(|path| path.is_file())
}

/// Make sure a bundled executable can be launched.
///
/// Bundlers do not always carry the executable bit across, and a helper that is
/// present but not runnable fails in a thoroughly confusing way.
fn ensure_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let mode = metadata.permissions().mode();
            if mode & 0o111 == 0 {
                let _ =
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o755));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Resolve `scraper.py`, most specific first:
///
/// 1. a path chosen in the UI (app-config `settings.json`)
/// 2. `AUTODOP_SCRAPER` (environment or `.env`)
/// 3. the copy bundled inside the app — so a shipped build needs no setup
/// 4. the repo root, which only exists when running from source
/// 5. the working directory
fn locate_scraper(app: Option<&AppHandle>) -> ScraperLocation {
    // (path, source, kind)
    let mut candidates: Vec<(PathBuf, &str, &str)> = Vec::new();
    let mut add = |path: PathBuf, source: &'static str| {
        let kind = kind_of(&path);
        candidates.push((path, source, kind));
    };

    if let Some(app) = app {
        if let Some(chosen) = saved_scraper_override(app) {
            add(chosen, "chosen");
        }
    }
    if let Ok(value) = std::env::var("AUTODOP_SCRAPER") {
        if !value.trim().is_empty() {
            add(PathBuf::from(value.trim()), "env");
        }
    }
    if let Some(app) = app {
        if let Ok(dir) = app.path().resource_dir() {
            // The frozen helper comes first: it needs no Python at all, which is
            // the only configuration a fresh install can be expected to satisfy.
            if let Some(sidecar) = bundled_sidecar_in(&dir) {
                add(sidecar, "sidecar");
            }
            if let Some(script) = bundled_scraper_in(&dir) {
                add(script, "bundled");
            }
        }
    }
    if let Some(root) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        add(root.join("scraper.py"), "repo");
    }
    if let Ok(cwd) = std::env::current_dir() {
        add(cwd.join("scraper.py"), "cwd");
    }

    for (path, source, kind) in &candidates {
        if path.is_file() {
            return ScraperLocation {
                path: path.display().to_string(),
                source: (*source).to_string(),
                kind: (*kind).to_string(),
                present: true,
            };
        }
    }

    // Nothing usable — report what was tried first so the UI can say so.
    let (path, source, kind) =
        candidates
            .first()
            .cloned()
            .unwrap_or((PathBuf::from("scraper.py"), "cwd", "script"));
    ScraperLocation {
        path: path.display().to_string(),
        source: source.to_string(),
        kind: kind.to_string(),
        present: false,
    }
}

/// The app's own small settings file, next to `credentials.json`.
fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("no app-config directory: {error}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    Ok(dir.join("settings.json"))
}

fn read_settings(app: &AppHandle) -> Value {
    settings_path(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// A `scraper.py` path chosen in the UI, if that file still exists.
fn saved_scraper_override(app: &AppHandle) -> Option<PathBuf> {
    let value = read_settings(app)
        .get("scraper_path")?
        .as_str()?
        .trim()
        .to_string();
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_file().then_some(path)
}

/// Persist the chosen `scraper.py`, or drop the override to fall back.
fn write_scraper_override(app: &AppHandle, path: Option<&str>) -> Result<(), String> {
    let file = settings_path(app)?;
    let mut settings = read_settings(app);
    if !settings.is_object() {
        settings = json!({});
    }
    match path {
        Some(value) => settings["scraper_path"] = json!(value),
        None => {
            if let Some(map) = settings.as_object_mut() {
                map.remove("scraper_path");
            }
        }
    }
    let body = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    std::fs::write(&file, body).map_err(|error| format!("cannot write {}: {error}", file.display()))
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

// --------------------------------------------------------------------------- //
// run logs                                                                    //
// --------------------------------------------------------------------------- //

/// Seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Format an epoch timestamp as RFC 3339 UTC.
///
/// Hand-rolled so the app does not carry a date library for one string; the
/// civil-date step is Howard Hinnant's `civil_from_days`.
fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Filename-safe timestamp: `20260921-195210Z` (colons are illegal on Windows).
fn compact_timestamp(secs: u64) -> String {
    rfc3339_utc(secs).replace(['-', ':'], "").replace('T', "-")
}

/// The log file for a single scraper run.
#[derive(Clone)]
struct RunLog {
    file: Arc<Mutex<File>>,
    path: PathBuf,
}

impl RunLog {
    /// Opens `<app log dir>/scraper-<timestamp>.log`.
    ///
    /// Returns `None` if that path cannot be used: logging is diagnostics, so
    /// it must never be the reason a run fails.
    fn open(app: &AppHandle) -> Option<Self> {
        let dir = app.path().app_log_dir().ok()?;
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("scraper-{}.log", compact_timestamp(now_secs())));
        let file = File::create(&path).ok()?;
        Some(Self {
            file: Arc::new(Mutex::new(file)),
            path,
        })
    }

    fn write(&self, line: &str) {
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
        }
    }

    fn path_string(&self) -> String {
        self.path.display().to_string()
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

/// This app's own key file: generated on first use, beside `credentials.json`.
fn key_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("no app-config directory: {error}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    Ok(dir.join("key"))
}

fn read_key_file(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// The key for anything this app encrypts itself — a configured `FERNET_KEY`
/// if there is one, otherwise a key generated on first use.
///
/// Generating one is what lets a fresh install store a DOP password without
/// anyone hand-editing a `.env`. It is deliberately *not* the key used for
/// reading the Atlas copy: a brand-new key cannot decrypt that, and generating
/// one eagerly would turn an honest "FERNET_KEY is not configured" into a
/// misleading HMAC failure.
fn app_key(app: &AppHandle) -> Result<String, String> {
    if let Some(key) = db::fernet_key(Some(app)) {
        return Ok(key);
    }
    let path = key_path(app)?;
    if let Some(existing) = read_key_file(&path) {
        return Ok(existing);
    }

    let key = crypt::generate_key()?;
    std::fs::write(&path, &key)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

/// Read that key without creating one — for decrypting a file that exists.
fn existing_app_key(app: &AppHandle) -> Option<String> {
    if let Some(key) = db::fernet_key(Some(app)) {
        return Some(key);
    }
    read_key_file(&key_path(app).ok()?)
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
    let stored = value
        .get("password")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if username.trim().is_empty() || stored.trim().is_empty() {
        return None;
    }
    // The file holds a Fernet token; a plaintext value from an older build still
    // works. A token that will not decrypt (wrong or missing FERNET_KEY) is
    // skipped rather than passed on as if it were the password — falling back to
    // Atlas beats sending a wall of base64 to the portal.
    let password = if stored.starts_with("gAAAA") {
        match existing_app_key(app).and_then(|key| crypt::decrypt(&key, &stored).ok()) {
            Some(plain) => plain,
            None => return None,
        }
    } else {
        stored
    };
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
    let scraper = locate_scraper(Some(&app));
    AppInfo {
        desktop: true,
        version: app.package_info().version.to_string(),
        build: build_stamp(),
        scraper: scraper.path,
        scraper_present: scraper.present,
        scraper_source: scraper.source,
        scraper_kind: scraper.kind,
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

/// Where a saved DOP password ended up, so the UI can say so plainly.
#[derive(Debug, Serialize, Default)]
pub struct SavedCredentials {
    /// The password was written to the app-config file, encrypted.
    pub stored_encrypted: bool,
    /// The `users` document in Atlas was updated too.
    pub atlas_updated: bool,
    /// Why the Atlas copy was left alone (read-only role, offline, ...).
    pub atlas_note: Option<String>,
}

/// Store the DOP portal password, encrypted, and try to update the Atlas copy.
///
/// DOP passwords expire every 180 days: change it on the portal, then save it
/// here. The app-config file always wins over Atlas, so a save takes effect
/// immediately even while the Atlas role is read-only.
#[tauri::command]
async fn set_credentials(
    app: AppHandle,
    username: String,
    password: String,
) -> Result<SavedCredentials, String> {
    let username = username.trim().to_string();
    if username.is_empty() || password.is_empty() {
        return Err("Username and password are both required.".into());
    }

    // Encrypt before anything touches the disk — never store it in the clear.
    // `app_key` generates a key on first use, so this works on a fresh install
    // with no .env at all.
    let key = app_key(&app)?;
    let token = crypt::encrypt(&key, &password)?;

    let path = credentials_path(&app)?;
    let body = serde_json::to_string_pretty(&json!({
        "username": username,
        "password": token,
        "encrypted": true,
    }))
    .map_err(|error| error.to_string())?;
    std::fs::write(&path, body)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    let mut saved = SavedCredentials {
        stored_encrypted: true,
        ..SavedCredentials::default()
    };

    // Best effort, never fatal: the stored Atlas copy is the same ciphertext in
    // the same format, but the configured role is read-only today.
    if db::load_db_config(Some(&app)).is_none() {
        saved.atlas_note = Some("no MONGO_URI configured".to_string());
    } else {
        match db::update_atlas_dop_password(&app, &token).await {
            Ok(()) => saved.atlas_updated = true,
            Err(error) => saved.atlas_note = Some(error),
        }
    }

    Ok(saved)
}

#[tauri::command]
fn scraper_location(app: AppHandle) -> ScraperLocation {
    locate_scraper(Some(&app))
}

/// Point the app at a different `scraper.py`.
#[tauri::command]
fn set_scraper_path(app: AppHandle, path: String) -> Result<ScraperLocation, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_file() {
        return Err(format!("{} is not a file", candidate.display()));
    }
    write_scraper_override(&app, Some(&candidate.display().to_string()))?;
    Ok(locate_scraper(Some(&app)))
}

/// Drop the override and go back to the bundled copy.
#[tauri::command]
fn clear_scraper_path(app: AppHandle) -> Result<ScraperLocation, String> {
    write_scraper_override(&app, None)?;
    Ok(locate_scraper(Some(&app)))
}

/// `"<short sha> · <when this build was produced>"`.
///
/// The commit alone is not enough: rebuild the same commit and the stamp would
/// be identical, which is exactly the confusion this exists to prevent. The
/// minutes-resolution build time makes every artifact tell itself apart.
pub fn build_stamp() -> String {
    let commit = option_env!("AUTODOP_BUILD_STAMP").unwrap_or("unknown");
    match option_env!("AUTODOP_BUILD_EPOCH").and_then(|value| value.parse::<u64>().ok()) {
        Some(epoch) if epoch > 0 => format!("{commit} · {}", iso_minute(epoch)),
        _ => commit.to_string(),
    }
}

/// `2026-10-01T00:18Z` — `rfc3339_utc` without the seconds.
fn iso_minute(epoch: u64) -> String {
    let full = rfc3339_utc(epoch);
    if full.len() >= 17 {
        format!("{}Z", &full[..16])
    } else {
        full
    }
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
                "No DOP credentials available: {error}. Set them in Manage → DOP portal password."
            ))
        }
    };
    let user = credentials.username;
    let password = credentials.password;
    let location = locate_scraper(Some(&app));
    if !location.present {
        return GenResult::err(format!(
            "No runner found — looked at {}. Choose one in Manage → Scraper script.",
            location.path
        ));
    }

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
        run_scraper(&app, &location, &user, &password, &lists_json)
    })
    .await;

    match result {
        Ok(gen_result) => gen_result,
        Err(e) => GenResult::err(format!("scraper task failed: {e}")),
    }
}

fn run_scraper(
    app: &AppHandle,
    location: &ScraperLocation,
    user: &str,
    password: &str,
    lists_json: &str,
) -> GenResult {
    let target = PathBuf::from(&location.path);
    // A frozen helper takes the arguments directly; a script needs an interpreter
    // in front of it.
    let is_sidecar = location.kind == "sidecar";

    // Mirror the run to a file: the progress area is cleared as soon as the
    // window moves on, and a failure has to stay inspectable afterwards.
    let run_log = RunLog::open(app);
    if let Some(log) = &run_log {
        log.write("autodop - scraper run");
        log.write(&format!("started:  {}", rfc3339_utc(now_secs())));
        if is_sidecar {
            log.write(&format!("runner:   bundled helper ({})", target.display()));
        } else {
            log.write(&format!("python:   {}", python_bin()));
            log.write(&format!("script:   {}", target.display()));
        }
        log.write(&format!("user:     {user}"));
        log.write("password: <redacted>");
        log.write(&format!("payload:  {lists_json}"));
        log.write("------------------------------------------------------------");
    }

    if is_sidecar {
        ensure_executable(&target);
    }
    let mut command = if is_sidecar {
        Command::new(&target)
    } else {
        let mut python = Command::new(python_bin());
        python.arg(&target);
        python
    };
    command
        .arg(user)
        .arg(password)
        .arg(lists_json)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return GenResult::err(format!(
                "could not start {}: {error}",
                if is_sidecar {
                    "the bundled runner"
                } else {
                    "python"
                }
            ))
        }
    };

    // Drain both pipes on their own threads: streaming stdout to the UI as
    // progress, so a full pipe buffer can never deadlock the child.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let app_for_stdout = app.clone();
    let log_for_stdout = run_log.clone();
    let stdout_thread = std::thread::spawn(move || {
        let mut collected = String::new();
        if let Some(pipe) = stdout_pipe {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                let _ = app_for_stdout.emit(PROGRESS_EVENT, line.clone());
                if let Some(log) = &log_for_stdout {
                    log.write(&line);
                }
                collected.push_str(&line);
                collected.push('\n');
            }
        }
        collected
    });
    let app_for_stderr = app.clone();
    let log_for_stderr = run_log.clone();
    let stderr_thread = std::thread::spawn(move || {
        let mut collected = String::new();
        if let Some(pipe) = stderr_pipe {
            // Stream stderr to the UI too. A Python traceback is the single most
            // useful thing a failed run produces, and it used to be collected
            // silently, truncated into `log`, and then never rendered.
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                let _ = app_for_stderr.emit(PROGRESS_EVENT, line.clone());
                if let Some(log) = &log_for_stderr {
                    log.write(&line);
                }
                collected.push_str(&line);
                collected.push('\n');
            }
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
                log_path: run_log.as_ref().map(RunLog::path_string),
            }
        }
        None => {
            let mut result = GenResult::err("Scraper produced no parseable result.");
            result.returncode = status.map(|s| s.code().unwrap_or(-1));
            result.log_path = run_log.as_ref().map(RunLog::path_string);
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
        .setup(|app| {
            // Make `.env` values visible to code that reads `std::env` directly
            // (`AUTODOP_PYTHON`, `AUTODOP_SCRAPER`); `db::setting` only looks up.
            db::hydrate_env(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            set_credentials,
            generate_lists,
            dop_credentials_status,
            db_status,
            load_accounts,
            load_lists,
            save_lists,
            scraper_location,
            set_scraper_path,
            clear_scraper_path
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
    fn finds_the_bundled_script_in_either_layout() {
        let dir = std::env::temp_dir().join(format!("autodop-bundle-{}", std::process::id()));
        let flat = dir.join("scraper.py");
        let nested = dir.join("_up_/scraper.py");
        std::fs::create_dir_all(dir.join("_up_")).unwrap();

        // A `../scraper.py` resource is copied to `_up_/` — what the .app ships.
        std::fs::write(&nested, "# placeholder").unwrap();
        assert_eq!(bundled_scraper_in(&dir).unwrap(), nested);

        // A flat resource wins when both are present.
        std::fs::write(&flat, "# placeholder").unwrap();
        assert_eq!(bundled_scraper_in(&dir).unwrap(), flat);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_nothing_when_no_bundled_script_exists() {
        let dir = std::env::temp_dir().join(format!("autodop-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(bundled_scraper_in(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_the_sidecar_for_this_platform() {
        let name = sidecar_name();
        assert!(name.starts_with("scraper-"), "got {name}");
        assert!(name.contains(std::env::consts::OS), "got {name}");
        if cfg!(windows) {
            assert!(name.ends_with(".exe"), "got {name}");
        } else {
            assert!(!name.ends_with(".exe"), "got {name}");
        }
        // Must match the name scripts/build-sidecar.mjs writes.
        if cfg!(target_arch = "aarch64") {
            assert!(name.contains("arm64"), "got {name}");
        }
        if cfg!(target_arch = "x86_64") {
            assert!(name.contains("x64"), "got {name}");
        }
    }

    #[test]
    fn tells_a_script_from_a_frozen_helper() {
        assert_eq!(kind_of(Path::new("/x/scraper.py")), "script");
        assert_eq!(kind_of(Path::new("/x/SCRAPER.PY")), "script");
        assert_eq!(kind_of(Path::new("/x/scraper-macos-arm64")), "sidecar");
        assert_eq!(kind_of(Path::new("/x/scraper-windows-x64.exe")), "sidecar");
    }

    #[test]
    fn finds_a_bundled_sidecar_by_platform_name() {
        let dir = std::env::temp_dir().join(format!("autodop-side-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("binaries")).unwrap();
        assert_eq!(bundled_sidecar_in(&dir), None, "nothing bundled yet");

        let expected = dir.join("binaries").join(sidecar_name());
        std::fs::write(&expected, b"#!/bin/sh\n").unwrap();
        assert_eq!(bundled_sidecar_in(&dir).unwrap(), expected);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shortens_the_build_time_to_minutes() {
        assert_eq!(iso_minute(1_758_468_000), "2025-09-21T15:20Z");
        assert_eq!(iso_minute(0), "1970-01-01T00:00Z");
    }

    #[test]
    fn stamps_the_build_with_a_commit_and_a_time() {
        let stamp = build_stamp();
        assert_eq!(
            stamp,
            build_stamp(),
            "baked in at compile time, so it must not drift"
        );
        if stamp == "unknown" {
            return; // no git checkout — nothing to assert about the commit
        }

        // Expect "<short sha>[+dirty] · YYYY-MM-DDTHH:MMZ".
        let (sha, when) = stamp
            .split_once(" · ")
            .unwrap_or_else(|| panic!("no build time in {stamp:?} — was AUTODOP_BUILD_EPOCH set?"));
        let sha = sha.strip_suffix("+dirty").unwrap_or(sha);
        assert!((7..=12).contains(&sha.len()), "commit looks wrong: {sha}");
        assert_eq!(when.len(), 17, "timestamp looks wrong: {when}");
        assert!(when.starts_with("20"), "timestamp year: {when}");
        assert_eq!(&when[10..11], "T", "timestamp separator: {when}");
        assert!(when.ends_with('Z'), "timestamp zone: {when}");
    }

    #[test]
    fn keeps_every_list_in_one_payload() {
        // "Generate All Lists" hands every list to a single scraper.py run.
        let payload = build_payload(vec![
            list("A", &["111"], &[0]),
            list("B", &["222", "333"], &[0, 2]),
        ]);
        assert_eq!(payload.len(), 2);
        assert_eq!(payload[0]["name"], json!("A"));
        assert_eq!(payload[1]["name"], json!("B"));
        assert_eq!(payload[1]["numbers"], json!(["222", "333"]));
        assert_eq!(payload[1]["rebate"], json!([0, 2]));
    }

    #[test]
    fn formats_epoch_timestamps_as_utc() {
        // Expected values cross-checked against Python's datetime.
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(rfc3339_utc(1_758_468_000), "2025-09-21T15:20:00Z");
        assert_eq!(rfc3339_utc(2_000_000_000), "2033-05-18T03:33:20Z");
    }

    #[test]
    fn log_filenames_are_safe_on_every_platform() {
        let name = compact_timestamp(1_758_468_000);
        assert_eq!(name, "20250921-152000Z");
        // Windows rejects ':' in file names.
        assert!(!name.contains(':'));
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
