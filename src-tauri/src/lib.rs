use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub mod backup;
pub mod crypt;
pub mod db;
pub mod store;

const SCRAPER_TIMEOUT_SECS: u64 = 3600;

const PROGRESS_EVENT: &str = "scraper-progress";

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

    pub version: String,

    pub build: String,
    pub scraper: String,
    pub scraper_present: bool,

    pub scraper_source: String,

    pub scraper_kind: String,
    pub credentials: bool,
    pub python: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct ScraperLocation {
    pub path: String,

    pub source: String,

    pub kind: String,
    pub present: bool,
}

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

fn bundled_sidecar_in(resource_dir: &Path) -> Option<PathBuf> {
    let candidate = resource_dir.join("binaries").join(sidecar_name());
    candidate.is_file().then_some(candidate)
}

fn bundled_scraper_in(resource_dir: &Path) -> Option<PathBuf> {
    ["scraper.py", "_up_/scraper.py"]
        .iter()
        .map(|relative| resource_dir.join(relative))
        .find(|path| path.is_file())
}

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

fn locate_scraper(app: Option<&AppHandle>) -> ScraperLocation {
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

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn rfc3339_utc(secs: u64) -> String {
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

fn compact_timestamp(secs: u64) -> String {
    rfc3339_utc(secs).replace(['-', ':'], "").replace('T', "-")
}

#[derive(Clone)]
struct RunLog {
    file: Arc<Mutex<File>>,
    path: PathBuf,
}

impl RunLog {
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

#[derive(Default)]
struct Unlock(Mutex<Option<Session>>);

#[derive(Clone)]
struct Session {
    owner_id: String,
    key: String,
}

fn current_session(app: &AppHandle) -> Option<Session> {
    app.state::<Unlock>()
        .0
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

fn current_owner(app: &AppHandle) -> Result<String, String> {
    current_session(app)
        .map(|session| session.owner_id)
        .ok_or_else(|| "Sign in first.".into())
}

fn unlocked_key(app: &AppHandle) -> Option<String> {
    current_session(app).map(|session| session.key)
}

fn set_unlocked_session(app: &AppHandle, session: Session) {
    if let Ok(mut slot) = app.state::<Unlock>().0.lock() {
        *slot = Some(session);
    }
}

fn clear_unlocked_key(app: &AppHandle) {
    if let Ok(mut slot) = app.state::<Unlock>().0.lock() {
        *slot = None;
    }
}

fn unlock_with(app: &AppHandle, owner_id: &str, password: &str) -> Result<String, String> {
    let auth = open_store(app)?
        .owner_auth(owner_id)?
        .ok_or("that workspace no longer exists")?;
    let key = crypt::derive_key(password, &auth.kdf_salt)?;
    set_unlocked_session(
        app,
        Session {
            owner_id: owner_id.to_string(),
            key: key.clone(),
        },
    );
    Ok(key)
}

fn legacy_keys(app: &AppHandle) -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(path) = key_path(app) {
        if let Some(key) = read_key_file(&path) {
            keys.push(key);
        }
    }
    if let Some(key) = db::fernet_key(Some(app)) {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn migrate_credential(app: &AppHandle, owner_id: &str) -> Result<bool, String> {
    let key = unlocked_key(app).ok_or("Sign in first.")?;
    let store = open_store(app)?;
    let migrated = migrate_credential_with(&store, owner_id, &key, &legacy_keys(app))?;
    if migrated {
        if let Ok(path) = key_path(app) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(migrated)
}

fn migrate_credential_with(
    store: &store::Store,
    owner_id: &str,
    key: &str,
    legacy: &[String],
) -> Result<bool, String> {
    let Some(stored) = store.credentials(owner_id)? else {
        return Ok(false);
    };

    if crypt::decrypt(key, &stored.token).is_ok() {
        return Ok(false);
    }

    for old in legacy {
        if let Ok(plain) = crypt::decrypt(old, &stored.token) {
            store.set_credentials(owner_id, &stored.username, &crypt::encrypt(key, &plain)?)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("no app-config directory: {error}"))?;
    Ok(dir.join("autodop.db"))
}

fn open_store(app: &AppHandle) -> Result<store::Store, String> {
    store::Store::open(&store_path(app)?)
}

fn decrypt_stored(app: &AppHandle, stored: &str) -> Option<String> {
    if !stored.starts_with("gAAAA") {
        return Some(stored.to_string());
    }
    unlocked_key(app).and_then(|key| crypt::decrypt(&key, stored).ok())
}

fn env_credentials() -> Option<db::DopCredentials> {
    let user = std::env::var("DOP_USERNAME").unwrap_or_default();
    let password = std::env::var("DOP_PASSWORD").unwrap_or_default();
    if user.trim().is_empty() || password.trim().is_empty() {
        return None;
    }
    Some(db::DopCredentials {
        username: user,
        password,
        source: db::CredentialSource::Env,
    })
}

fn sqlite_credentials(app: &AppHandle) -> Option<db::DopCredentials> {
    let owner = current_owner(app).ok()?;
    let stored = open_store(app).ok()?.credentials(&owner).ok().flatten()?;
    let password = decrypt_stored(app, &stored.token)?;
    if stored.username.trim().is_empty() || password.trim().is_empty() {
        return None;
    }
    Some(db::DopCredentials {
        username: stored.username,
        password,
        source: db::CredentialSource::Local,
    })
}

fn file_credentials(app: &AppHandle) -> Option<db::DopCredentials> {
    let text = std::fs::read_to_string(credentials_path(app).ok()?).unwrap_or_default();
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
    let password = decrypt_stored(app, &stored)?;
    Some(db::DopCredentials {
        username,
        password,
        source: db::CredentialSource::Config,
    })
}

async fn resolve_credentials(app: &AppHandle) -> Result<db::DopCredentials, String> {
    if let Some(credentials) = env_credentials() {
        return Ok(credentials);
    }
    if let Some(credentials) = sqlite_credentials(app) {
        return Ok(credentials);
    }
    if let Some(credentials) = file_credentials(app) {
        return Ok(credentials);
    }
    Err("No DOP password saved yet — add it in Manage.".into())
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

#[derive(Debug, Serialize, Default)]
pub struct SavedCredentials {
    pub stored_encrypted: bool,

    pub location: String,
}

#[tauri::command]
fn set_credentials(
    app: AppHandle,
    username: String,
    password: String,
) -> Result<SavedCredentials, String> {
    let username = username.trim().to_string();
    if username.is_empty() || password.is_empty() {
        return Err("Username and password are both required.".into());
    }

    let owner = current_owner(&app)?;
    let key = unlocked_key(&app).ok_or(
        "Sign in first — the DOP password is encrypted with a key derived from your password.",
    )?;
    let token = crypt::encrypt(&key, &password)?;

    open_store(&app)?.set_credentials(&owner, &username, &token)?;

    Ok(SavedCredentials {
        stored_encrypted: true,
        location: store_path(&app)?.display().to_string(),
    })
}

#[derive(Debug, Serialize)]
pub struct OwnerInfo {
    pub id: String,
    pub username: String,
    pub has_credentials: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthStatus {
    pub configured: bool,
    pub unlocked: bool,
    pub owners: Vec<OwnerInfo>,
    pub current: Option<String>,
}

#[tauri::command]
fn auth_status(app: AppHandle) -> AuthStatus {
    let store = open_store(&app);
    let owners = store
        .as_ref()
        .map(|store| store.owners().unwrap_or_default())
        .unwrap_or_default();
    let current = current_session(&app).map(|session| session.owner_id);
    AuthStatus {
        configured: !owners.is_empty(),
        unlocked: current.is_some(),
        owners: owners
            .into_iter()
            .map(|row| OwnerInfo {
                id: row.id,
                username: row.username,
                has_credentials: row.has_credential,
            })
            .collect(),
        current,
    }
}

#[tauri::command]
fn setup_login(app: AppHandle, username: String, password: String) -> Result<OwnerInfo, String> {
    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("The username (your DOP portal id) is required.".into());
    }
    if password.is_empty() {
        return Err("The password cannot be empty.".into());
    }
    let store = open_store(&app)?;
    let owner_id = store.create_owner(
        &username,
        &crypt::hash_login(&password)?,
        &crypt::new_salt()?,
    )?;
    unlock_with(&app, &owner_id, &password)?;

    let _ = migrate_credential(&app, &owner_id);
    Ok(OwnerInfo {
        id: owner_id,
        username,
        has_credentials: false,
    })
}

#[tauri::command]
fn login(app: AppHandle, owner_id: String, password: String) -> Result<OwnerInfo, String> {
    let store = open_store(&app)?;
    let Some(auth) = store.owner_auth(&owner_id)? else {
        return Err("That workspace no longer exists on this machine.".into());
    };
    if !crypt::verify_login(&password, &auth.login_hash) {
        clear_unlocked_key(&app);
        return Err("Incorrect password.".into());
    }
    unlock_with(&app, &owner_id, &password)?;
    store.touch_owner(&owner_id)?;

    let _ = migrate_credential(&app, &owner_id);
    let has_credentials = store.credentials(&owner_id)?.is_some();
    Ok(OwnerInfo {
        id: owner_id,
        username: auth.username,
        has_credentials,
    })
}

#[tauri::command]
fn logout(app: AppHandle) {
    clear_unlocked_key(&app);
}

#[tauri::command]
fn change_login_password(
    app: AppHandle,
    old_password: String,
    new_password: String,
) -> Result<(), String> {
    if new_password.is_empty() {
        return Err("The new password cannot be empty.".into());
    }

    let owner = current_owner(&app)?;
    let store = open_store(&app)?;
    let Some(auth) = store.owner_auth(&owner)? else {
        return Err("No login password is set yet.".into());
    };
    if !crypt::verify_login(&old_password, &auth.login_hash) {
        return Err("The current password is not right.".into());
    }
    let current = unlocked_key(&app).ok_or("Sign in first, then change the password.")?;

    let salt = crypt::new_salt()?;
    let new_key = crypt::derive_key(&new_password, &salt)?;

    if let Some(stored) = store.credentials(&owner)? {
        let plain = crypt::decrypt(&current, &stored.token).map_err(|_| {
            "The stored DOP password could not be read with the current key.".to_string()
        })?;
        store.set_credentials(&owner, &stored.username, &crypt::encrypt(&new_key, &plain)?)?;
    }

    store.set_owner_auth(&owner, &crypt::hash_login(&new_password)?, &salt)?;
    set_unlocked_session(
        &app,
        Session {
            owner_id: owner,
            key: new_key,
        },
    );
    Ok(())
}

#[tauri::command]
fn scraper_location(app: AppHandle) -> ScraperLocation {
    locate_scraper(Some(&app))
}

#[tauri::command]
fn set_scraper_path(app: AppHandle, path: String) -> Result<ScraperLocation, String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_file() {
        return Err(format!("{} is not a file", candidate.display()));
    }
    write_scraper_override(&app, Some(&candidate.display().to_string()))?;
    Ok(locate_scraper(Some(&app)))
}

#[tauri::command]
fn clear_scraper_path(app: AppHandle) -> Result<ScraperLocation, String> {
    write_scraper_override(&app, None)?;
    Ok(locate_scraper(Some(&app)))
}

pub fn build_stamp() -> String {
    let commit = option_env!("AUTODOP_BUILD_STAMP").unwrap_or("unknown");
    match option_env!("AUTODOP_BUILD_EPOCH").and_then(|value| value.parse::<u64>().ok()) {
        Some(epoch) if epoch > 0 => format!("{commit} · {}", iso_minute(epoch)),
        _ => commit.to_string(),
    }
}

fn iso_minute(epoch: u64) -> String {
    let full = rfc3339_utc(epoch);
    if full.len() >= 17 {
        format!("{}Z", &full[..16])
    } else {
        full
    }
}

#[tauri::command]
async fn generate_lists(app: AppHandle, lists: Vec<GenList>) -> GenResult {
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

    let run_id = open_store(&app).ok().and_then(|store| {
        current_owner(&app)
            .ok()
            .and_then(|owner| store.start_run(&owner, &names.join(", ")).ok())
    });

    let app_for_run = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        run_scraper(&app_for_run, &location, &user, &password, &lists_json)
    })
    .await;

    let outcome = match result {
        Ok(gen_result) => gen_result,
        Err(e) => GenResult::err(format!("scraper task failed: {e}")),
    };

    if let Some(run_id) = run_id {
        if let Ok(store) = open_store(&app) {
            let (status, detail) = if outcome.ok {
                let processed = outcome.results.as_ref().map_or(0, Vec::len);
                ("ok", format!("{processed} list(s) processed"))
            } else {
                (
                    "failed",
                    outcome
                        .error
                        .clone()
                        .unwrap_or_else(|| "unknown error".into()),
                )
            };
            let _ = store.finish_run(run_id, status, &detail);
        }
    }

    outcome
}

fn run_scraper(
    app: &AppHandle,
    location: &ScraperLocation,
    user: &str,
    password: &str,
    lists_json: &str,
) -> GenResult {
    let target = PathBuf::from(&location.path);

    let is_sidecar = location.kind == "sidecar";

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

#[derive(Debug, Serialize)]
pub struct LocalStatus {
    pub path: String,
    pub accounts: i64,
    pub lists: i64,
    pub entries: i64,
    pub has_credentials: bool,
    pub error: Option<String>,
}

#[tauri::command]
fn local_status(app: AppHandle) -> LocalStatus {
    let error = |error: String| LocalStatus {
        path: String::new(),
        accounts: 0,
        lists: 0,
        entries: 0,
        has_credentials: false,
        error: Some(error),
    };
    let path = match store_path(&app) {
        Ok(path) => path,
        Err(message) => return error(message),
    };

    match open_store(&app) {
        Ok(store) => match current_owner(&app).and_then(|owner| store.counts(&owner)) {
            Ok(counts) => {
                let has_credentials = current_owner(&app)
                    .ok()
                    .and_then(|owner| store.credentials(&owner).ok().flatten())
                    .is_some_and(|stored| !stored.token.trim().is_empty());
                LocalStatus {
                    path: path.display().to_string(),
                    accounts: counts.accounts,
                    lists: counts.lists,
                    entries: counts.entries,
                    has_credentials,
                    error: None,
                }
            }
            Err(message) => error(message),
        },
        Err(message) => error(message),
    }
}

#[tauri::command]
fn load_accounts(app: AppHandle) -> Result<Vec<Value>, String> {
    open_store(&app)?.accounts(&current_owner(&app)?)
}

#[tauri::command]
fn load_lists(app: AppHandle) -> Result<Vec<db::DbList>, String> {
    open_store(&app)?.lists(&current_owner(&app)?)
}

#[tauri::command]
fn save_lists(app: AppHandle, lists: Vec<db::InputList>) -> Result<Vec<db::DbList>, String> {
    open_store(&app)?.save_lists(&current_owner(&app)?, &lists)
}

#[derive(Debug, Serialize)]
pub struct PdfImportReport {
    pub imported: usize,

    pub skipped_duplicates: usize,

    pub unparsed: usize,
}

#[tauri::command]
fn import_accounts_pdf(app: AppHandle, path: String) -> Result<PdfImportReport, String> {
    let source = expand_home(&path);
    if !source.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    let text =
        pdf_extract::extract_text(&source.display().to_string())
            .map_err(|error| format!("could not read the PDF: {error}"))?;
    import_pdf_text(app, &text)
}

/// Same import, from bytes the frontend read with a file picker — the picker
/// gives content, not paths, so the payload arrives base64-wrapped and is
/// written to a temp file for the extractor.
#[tauri::command]
fn import_accounts_pdf_bytes(
    app: AppHandle,
    data_base64: String,
) -> Result<PdfImportReport, String> {
    let bytes = base64_decode(&data_base64)?;
    let temp = write_temp("autodop-import.pdf", &bytes)?;
    let result = (|| {
        let text = pdf_extract::extract_text(&temp.display().to_string())
            .map_err(|error| format!("could not read the PDF: {error}"))?;
        import_pdf_text(app, &text)
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

fn import_pdf_text(app: AppHandle, text: &str) -> Result<PdfImportReport, String> {
    let owner = current_owner(&app)?;
    let store = open_store(&app)?;

    let mut imported = 0usize;
    let mut skipped_duplicates = 0usize;
    let mut unparsed = 0usize;

    let mut carry = String::new();
    for line in text.lines() {
        let combined = if carry.is_empty() {
            line.to_string()
        } else {
            format!("{carry} {line}")
        };
        let looks_like_data = line
            .split_whitespace()
            .any(|token| token.len() >= 9 && token.chars().all(|c| c.is_ascii_digit()));
        match parse_deposit_row(&combined) {
            Ok(Some((number, name, denomination))) => {
                carry.clear();
                let row = json!({
                    "Number": number,
                    "Name": name,
                    "Denomination": denomination,
                    "CNumber": "",
                    "Ref_Number": "",
                    "addedIn": "",
                });
                match store.add_account(&owner, &row) {
                    Ok(_) => imported += 1,
                    Err(error) if error.contains("already exists") => skipped_duplicates += 1,
                    Err(other) => return Err(other),
                }
            }
            Ok(None) => {
                carry = if looks_like_data && !line.contains("Cr.") {
                    combined
                } else {
                    String::new()
                };
            }
            Err(_) => {
                carry.clear();
                unparsed += 1;
            }
        }
    }

    if imported + skipped_duplicates == 0 {
        return Err(
            "No deposit-account rows were found in that PDF — is it the agent \
             portal's \"Deposit Accounts\" printout?"
                .into(),
        );
    }
    Ok(PdfImportReport {
        imported,
        skipped_duplicates,
        unparsed,
    })
}

pub fn parse_deposit_row(line: &str) -> Result<Option<(String, String, String)>, &'static str> {
    let is_amount = |token: &str| -> bool {
        let Some((whole, fraction)) = token.split_once('.') else {
            return false;
        };
        fraction.len() == 2
            && fraction.chars().all(|c| c.is_ascii_digit())
            && !whole.is_empty()
            && whole.chars().all(|c| c.is_ascii_digit() || c == ',')
    };

    let tokens: Vec<&str> = line.split_whitespace().collect();

    let mut amount_at = None;
    for (index, token) in tokens.iter().enumerate() {
        if is_amount(token)
            && tokens
                .get(index + 1)
                .is_some_and(|next| next.starts_with("Cr"))
        {
            amount_at = Some(index);
            break;
        }
    }
    let Some(amount_at) = amount_at else {
        return Ok(None);
    };

    let before = &tokens[..amount_at];

    let number_at = before
        .iter()
        .rposition(|token| {
            !token.is_empty() && token.chars().all(|c| c.is_ascii_digit()) && token.len() >= 9
        })
        .ok_or("no account number on the line")?;
    let number = tokens[number_at].to_string();
    let name = before[number_at + 1..].join(" ");
    if name.is_empty() {
        return Err("no holder name on the line");
    }

    let mut denomination: String = tokens[amount_at].replace(',', "");
    if let Some((whole, _)) = denomination.split_once('.') {
        denomination = whole.to_string();
    }
    Ok(Some((number, name, denomination)))
}

#[tauri::command]
fn save_account(app: AppHandle, account: Value) -> Result<String, String> {
    let owner = current_owner(&app)?;
    let store = open_store(&app)?;
    let number = account
        .get("Number")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if number.is_empty() {
        return Err("Account number is required.".into());
    }

    let existing: Option<String> = {
        let id = account.get("_id").and_then(Value::as_str).unwrap_or("");
        if id.is_empty() {
            store
                .accounts(&owner)?
                .into_iter()
                .find(|row| row.get("Number").and_then(Value::as_str) == Some(number.as_str()))
                .and_then(|row| row.get("_id").and_then(Value::as_str).map(str::to_string))
        } else {
            Some(id.to_string())
        }
    };
    let mut row = account.clone();
    if let Some(id) = &existing {
        row["_id"] = Value::String(id.clone());
    } else {
        row["_id"] = Value::String(String::new());
    }
    store.add_account(&owner, &row)
}

#[tauri::command]
fn delete_account(app: AppHandle, id: String) -> Result<bool, String> {
    open_store(&app)?.delete_account(&current_owner(&app)?, &id)
}

#[derive(Debug, Serialize)]
pub struct BackupOutcome {
    pub accounts: i64,
    pub lists: i64,
    pub entries: i64,
    pub has_credentials: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

#[tauri::command]
fn export_backup(app: AppHandle, path: String) -> Result<BackupOutcome, String> {
    let destination = expand_home(&path);
    if destination.extension().is_none() {
        return Err("give the backup a file name, e.g. autodop-backup.db".into());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let live = store_path(&app)?;
    let snapshot = backup::export(&live, &destination).map_err(fs_hint)?;
    Ok(BackupOutcome {
        accounts: snapshot.accounts,
        lists: snapshot.lists,
        entries: snapshot.entries,
        has_credentials: snapshot.has_credentials,
        previous: Some(destination.display().to_string()),
    })
}

#[tauri::command]
fn import_backup(app: AppHandle, path: String) -> Result<BackupOutcome, String> {
    let source = expand_home(&path);
    if !source.exists() {
        return Err(format!("{} does not exist", source.display()));
    }
    restore_from_file(&app, &source)
}

/// Same restore, from bytes the frontend read with a file picker.
#[tauri::command]
fn import_backup_bytes(app: AppHandle, data_base64: String) -> Result<BackupOutcome, String> {
    let bytes = base64_decode(&data_base64)?;
    let temp = write_temp("autodop-restore.db", &bytes)?;
    let result = restore_from_file(&app, &temp);
    let _ = std::fs::remove_file(&temp);
    result
}

fn restore_from_file(app: &AppHandle, source: &Path) -> Result<BackupOutcome, String> {
    let snapshot = backup::inspect(source).map_err(fs_hint)?;
    let live = store_path(&app)?;

    let safety = live.with_extension(format!("db.pre-restore-{}", compact_timestamp(now_secs())));
    backup::export(&live, &safety)?;
    let previous = safety.display().to_string();

    clear_unlocked_key(&app);

    for sidecar in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(live.with_extension(format!("db{sidecar}")));
    }
    std::fs::copy(&source, &live)
        .map_err(|error| fs_hint(format!("cannot restore over {}: {error}", live.display())))?;
    for sidecar in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(live.with_extension(format!("db{sidecar}")));
    }

    let store = open_store(&app)?;
    let counts = store.counts_all()?;
    if counts.accounts != snapshot.accounts
        || counts.lists != snapshot.lists
        || counts.entries != snapshot.entries
    {
        return Err(format!(
            "restored file does not match the backup (expected {:?}, got {:?}); \
             the replaced data was kept at {previous}",
            snapshot, counts
        ));
    }
    Ok(BackupOutcome {
        accounts: counts.accounts,
        lists: counts.lists,
        entries: counts.entries,
        has_credentials: snapshot.has_credentials,
        previous: Some(previous),
    })
}

/// Decode a base64 payload from the frontend (file pickers hand over bytes,
/// which are wrapped because `invoke` speaks JSON).
fn base64_decode(data: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|error| format!("malformed file payload: {error}"))
}

/// A uniquely named scratch file holding `bytes`, for commands that receive
/// content from a file picker but need a path (SQLite restore, PDF extract).
fn write_temp(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!(
        "{}-{}",
        compact_timestamp(now_secs()),
        name
    ));
    std::fs::write(&path, bytes)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path)
}

fn fs_hint(error: String) -> String {
    if error.contains("read-only file system") || error.contains("os error 30") {
        return format!(
            "{error} — AutoDOP is running from the installer DMG (read-only). \
             Drag the app to Applications (or anywhere on disk) and run it from there."
        );
    }
    error
}

#[tauri::command]
fn export_portable_backup(app: AppHandle, path: String) -> Result<BackupOutcome, String> {
    let destination = expand_home(&path);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let owner = current_owner(&app)?;
    let store = open_store(&app)?;
    let credentials = match store.credentials(&owner)? {
        Some(stored) if !stored.token.trim().is_empty() => {
            let auth = store
                .owner_auth(&owner)?
                .ok_or("this workspace has no login record")?;
            Some(backup::PortableCredentials {
                username: stored.username,
                token: stored.token,
                salt: auth.kdf_salt,
                login_hash: auth.login_hash,
            })
        }
        _ => None,
    };
    let written = backup::portable_export(
        &store,
        &owner,
        &destination,
        &rfc3339_utc(now_secs()),
        credentials,
    )
    .map_err(fs_hint)?;
    Ok(BackupOutcome {
        accounts: written.accounts.len() as i64,
        lists: written.lists.len() as i64,
        entries: written.lists.iter().map(|l| l.entries.len() as i64).sum(),
        has_credentials: written.credentials.is_some(),
        previous: Some(destination.display().to_string()),
    })
}

#[tauri::command]
fn import_portable_backup(
    app: AppHandle,
    path: String,
    login_password: String,
) -> Result<BackupOutcome, String> {
    let source = expand_home(&path);
    let backup = backup::portable_read(&source, &login_password)?;
    portable_import_run(app, &backup, &login_password)
}

/// Same import, from bytes the frontend read with a file picker.
#[tauri::command]
fn import_portable_backup_bytes(
    app: AppHandle,
    data_base64: String,
    login_password: String,
) -> Result<BackupOutcome, String> {
    let bytes = base64_decode(&data_base64)?;
    let temp = write_temp("autodop-import.json", &bytes)?;
    let result = (|| {
        let backup = backup::portable_read(&temp, &login_password)?;
        portable_import_run(app, &backup, &login_password)
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

fn portable_import_run(
    app: AppHandle,
    backup: &backup::PortableBackup,
    login_password: &str,
) -> Result<BackupOutcome, String> {
    let live = store_path(&app)?;
    let safety = live.with_extension(format!("db.pre-restore-{}", compact_timestamp(now_secs())));
    backup::export(&live, &safety).map_err(fs_hint)?;

    let owner = current_owner(&app)?;
    let store = open_store(&app)?;
    let (accounts, lists, entries) = backup::portable_import(&store, &owner, &backup)?;
    let mut has_credentials = false;
    if backup.credentials.is_some() {
        let key = unlocked_key(&app)
            .ok_or("Sign in first — the DOP password is re-encrypted with your login's key.")?;
        backup::portable_import_credentials(&store, &owner, &backup, login_password, &key)?;
        has_credentials = true;
    }
    Ok(BackupOutcome {
        accounts,
        lists,
        entries,
        has_credentials,
        previous: Some(safety.display().to_string()),
    })
}

#[tauri::command]
async fn dop_credentials_status(app: AppHandle) -> db::DopCredentialStatus {
    match resolve_credentials(&app).await {
        Ok(credentials) => db::DopCredentialStatus {
            username: credentials.username,
            source: credentials.source,
            has_password: !credentials.password.trim().is_empty(),
            detail: None,
        },
        Err(error) => db::DopCredentialStatus {
            username: String::new(),
            source: db::CredentialSource::Local,
            has_password: false,
            detail: Some(error),
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Unlock::default())
        .setup(|app| {
            db::hydrate_env(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            auth_status,
            setup_login,
            login,
            logout,
            change_login_password,
            set_credentials,
            generate_lists,
            dop_credentials_status,
            local_status,
            load_accounts,
            load_lists,
            save_lists,
            save_account,
            delete_account,
            import_accounts_pdf,
            import_accounts_pdf_bytes,
            import_backup_bytes,
            import_portable_backup_bytes,
            export_backup,
            import_backup,
            export_portable_backup,
            import_portable_backup,
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

        std::fs::write(&nested, "# placeholder").unwrap();
        assert_eq!(bundled_scraper_in(&dir).unwrap(), nested);

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
    fn migrates_a_credential_from_the_old_key_file() {
        let store = store::Store::open_in_memory().unwrap();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        let old = crypt::generate_key().unwrap();
        let new = crypt::derive_key("my login password", &crypt::new_salt().unwrap()).unwrap();

        store
            .set_credentials(
                &owner,
                "DOP.MI1",
                &crypt::encrypt(&old, "portal-secret").unwrap(),
            )
            .unwrap();
        let before = store.credentials(&owner).unwrap().unwrap();
        assert!(
            crypt::decrypt(&new, &before.token).is_err(),
            "the new key must not open the old ciphertext"
        );

        assert!(migrate_credential_with(&store, &owner, &new, &[old]).unwrap());
        let after = store.credentials(&owner).unwrap().unwrap();
        assert_eq!(crypt::decrypt(&new, &after.token).unwrap(), "portal-secret");
        assert_eq!(after.username, "DOP.MI1", "the portal id is untouched");

        assert!(!migrate_credential_with(&store, &owner, &new, &[]).unwrap());
    }

    #[test]
    fn leaves_a_credential_alone_when_no_legacy_key_opens_it() {
        let store = store::Store::open_in_memory().unwrap();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        let stranger = crypt::generate_key().unwrap();
        let new = crypt::derive_key("pw", &crypt::new_salt().unwrap()).unwrap();
        store
            .set_credentials(&owner, "DOP.MI1", &crypt::encrypt(&stranger, "x").unwrap())
            .unwrap();

        assert!(
            !migrate_credential_with(&store, &owner, &new, &[crypt::generate_key().unwrap()])
                .unwrap()
        );
        let stored = store.credentials(&owner).unwrap().unwrap();
        assert_eq!(crypt::decrypt(&stranger, &stored.token).unwrap(), "x");
    }

    #[test]
    fn migrating_with_no_credential_is_a_no_op() {
        let store = store::Store::open_in_memory().unwrap();
        let key = crypt::generate_key().unwrap();
        assert!(!migrate_credential_with(&store, "owner-1", &key, &[]).unwrap());
    }

    #[test]
    fn shortens_the_build_time_to_minutes() {
        assert_eq!(iso_minute(1_758_468_000), "2025-09-21T15:20Z");
        assert_eq!(iso_minute(0), "1970-01-01T00:00Z");
    }

    #[test]
    fn parses_a_deposit_row_from_the_printout() {
        let (number, name, denomination) =
            parse_deposit_row("3 020001994152 REMYA C V 1,500.00 Cr. 22 23-Nov-2022")
                .expect("parses")
                .expect("a row");
        assert_eq!(number, "020001994152");
        assert_eq!(name, "REMYA C V");
        assert_eq!(denomination, "1500");
    }

    #[test]
    fn a_wrapped_name_and_a_missing_date_both_parse() {
        let (number, name, _) = parse_deposit_row("62 3827124362 KAVITHA G NAIR 1,500.00 Cr. 60")
            .expect("parses")
            .expect("a row");
        assert_eq!(number, "3827124362");
        assert_eq!(name, "KAVITHA G NAIR");
    }

    #[test]
    fn headers_and_footers_are_not_rows() {
        for line in [
            "DEPOSIT ACCOUNTS",
            "Select Mode: Account Id(s): Deposit Accounts List",
            "Select Account No Account Name Denomination Month Paid Upto Next RD Installment Due Date",
            "1 of 5 11/10/22, 02:07",
            "Printed on 09-Nov-2022 15:06:32 PM",
        ] {
            assert_eq!(parse_deposit_row(line).unwrap(), None, "{line}");
        }
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
            return;
        }

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
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        assert_eq!(rfc3339_utc(1_758_468_000), "2025-09-21T15:20:00Z");
        assert_eq!(rfc3339_utc(2_000_000_000), "2033-05-18T03:33:20Z");
    }

    #[test]
    fn log_filenames_are_safe_on_every_platform() {
        let name = compact_timestamp(1_758_468_000);
        assert_eq!(name, "20250921-152000Z");

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
