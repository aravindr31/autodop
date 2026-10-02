//! MongoDB Atlas access.
//!
//! The connection string is read from the environment or a gitignored `.env`
//! next to `Cargo.toml` — it is never exposed to the webview.
//!
//! A client is created per call and dropped afterwards: loads are infrequent
//! (startup + manual reload), and this keeps URI changes from going stale.

use futures_util::StreamExt;
use mongodb::bson::oid::ObjectId;
use mongodb::bson::{doc, Bson, Document};
use mongodb::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Verified against the live cluster: database `accounts` holds
/// `accountHolders` (the account documents), `savedList` (lists + rebates),
/// `users`, `admin` and an empty `list`. `accounts.accounts` is empty.
pub const DEFAULT_DB: &str = "accounts";
pub const DEFAULT_COLLECTION: &str = "accountHolders";
/// Lists (and their per-account rebates) — the old app's `savedList`.
pub const LISTS_COLLECTION: &str = "savedList";
/// Error text shared by the list/account readers.
pub const NO_URI: &str = "No MONGO_URI configured (add it to src-tauri/.env).";

/// `savedList` unless `MONGO_LISTS_COLLECTION` overrides it (used by probes).
fn lists_collection() -> String {
    std::env::var("MONGO_LISTS_COLLECTION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| LISTS_COLLECTION.to_string())
}

#[derive(Debug, Clone)]
pub struct DbConfig {
    pub uri: String,
    pub db: String,
    pub collection: String,
}

#[derive(Debug, Serialize)]
pub struct DbStatus {
    pub configured: bool,
    pub connected: bool,
    pub db: String,
    pub collection: String,
    /// False when the Atlas user can only read — saving lists would fail.
    pub writable: bool,
    /// The authenticated user's roles, e.g. `["readAnyDatabase"]`.
    pub roles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Read the connection's roles. Read-only and safe whatever the privileges.
pub async fn connection_roles(client: &Client, db: &str) -> Vec<String> {
    client
        .database(db)
        .run_command(doc! { "connectionStatus": 1 })
        .await
        .ok()
        .and_then(|reply| reply.get_document("authInfo").ok().cloned())
        .and_then(|info| info.get_array("authenticatedUserRoles").ok().cloned())
        .map(|list| {
            list.iter()
                .filter_map(|item| {
                    item.as_document()
                        .and_then(|entry| entry.get_str("role").ok())
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether those roles permit writes.
pub fn can_write(roles: &[String]) -> bool {
    roles.iter().any(|role| {
        role.contains("readWrite") || role == "root" || role == "atlasAdmin" || role == "dbOwner"
    })
}

// --------------------------------------------------------------------------- //
// configuration                                                               //
// --------------------------------------------------------------------------- //

/// Where a `.env` may live, most specific first.
pub fn env_file_candidates(app: Option<&AppHandle>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("AUTODOP_ENV_FILE") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit.trim()));
        }
    }
    // Dev layout: <repo>/src-tauri/.env and <repo>/.env
    if let Some(manifest) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        paths.push(manifest.join("src-tauri").join(".env"));
        paths.push(manifest.join(".env"));
    }
    // Packaged app: the OS config directory.
    if let Some(handle) = app {
        if let Ok(dir) = handle.path().app_config_dir() {
            paths.push(dir.join(".env"));
        }
    }
    paths
}

fn parse_env_file(path: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut pairs = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        let value = value
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if !key.is_empty() && !value.is_empty() {
            pairs.push((key, value));
        }
    }
    pairs
}

/// Resolve a setting: environment wins, then the first `.env` that defines it.
fn setting(app: Option<&AppHandle>, key: &str) -> Option<String> {
    if let Ok(value) = std::env::var(key) {
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    for path in env_file_candidates(app) {
        for (file_key, file_value) in parse_env_file(&path) {
            if file_key == key {
                return Some(file_value);
            }
        }
    }
    None
}

/// Copy every `.env` entry into the process environment, without clobbering a
/// variable that is already set.
///
/// [`setting`] only *looks up* `.env` values, so anything read straight from
/// `std::env` — `AUTODOP_PYTHON`, `AUTODOP_SCRAPER` — would otherwise ignore the
/// file entirely and silently fall back to `python3` on `PATH`.
pub fn hydrate_env(app: &AppHandle) {
    for path in env_file_candidates(Some(app)) {
        for (key, value) in parse_env_file(&path) {
            if std::env::var_os(&key).is_none() {
                std::env::set_var(key, value);
            }
        }
    }
}

pub fn load_db_config(app: Option<&AppHandle>) -> Option<DbConfig> {
    let uri = setting(app, "MONGO_URI")?;
    Some(DbConfig {
        uri,
        db: setting(app, "MONGO_DB").unwrap_or_else(|| DEFAULT_DB.to_string()),
        collection: setting(app, "MONGO_COLLECTION")
            .unwrap_or_else(|| DEFAULT_COLLECTION.to_string()),
    })
}

// --------------------------------------------------------------------------- //
// mapping                                                                     //
// --------------------------------------------------------------------------- //

/// Coerce BSON values to the plain strings the frontend `Account` type expects.
fn bson_to_string(value: Option<&Bson>) -> String {
    match value {
        Some(Bson::String(s)) => s.clone(),
        Some(Bson::Int32(i)) => i.to_string(),
        Some(Bson::Int64(i)) => i.to_string(),
        Some(Bson::Double(f)) => {
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Some(Bson::Decimal128(d)) => d.to_string(),
        Some(Bson::ObjectId(o)) => o.to_hex(),
        _ => String::new(),
    }
}

fn object_id(value: Option<&Bson>) -> String {
    match value {
        Some(Bson::ObjectId(o)) => o.to_hex(),
        Some(Bson::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Map one Atlas document onto the frontend `Account` shape (spec §2).
pub fn account_from_doc(doc: &Document) -> Value {
    json!({
        "_id": object_id(doc.get("_id")),
        "Number": bson_to_string(doc.get("Number")),
        "Name": bson_to_string(doc.get("Name")),
        "Denomination": bson_to_string(doc.get("Denomination")),
        "CNumber": bson_to_string(doc.get("CNumber")),
        "Ref_Number": bson_to_string(doc.get("Ref_Number")),
        "addedIn": bson_to_string(doc.get("addedIn")),
    })
}

// --------------------------------------------------------------------------- //
// access                                                                      //
// --------------------------------------------------------------------------- //

pub async fn connect(cfg: &DbConfig) -> Result<Client, String> {
    let client = Client::with_uri_str(&cfg.uri)
        .await
        .map_err(|e| format!("connection string rejected: {e}"))?;
    // Force a round-trip so a bad host/credential is reported now, not later.
    client
        .database(&cfg.db)
        .run_command(mongodb::bson::doc! { "ping": 1 })
        .await
        .map_err(|e| format!("cannot reach the cluster: {e}"))?;
    Ok(client)
}

pub async fn fetch_status(app: &AppHandle) -> DbStatus {
    let Some(cfg) = load_db_config(Some(app)) else {
        return DbStatus {
            configured: false,
            connected: false,
            db: DEFAULT_DB.to_string(),
            collection: DEFAULT_COLLECTION.to_string(),
            writable: false,
            roles: Vec::new(),
            count: None,
            error: Some(NO_URI.into()),
        };
    };
    match connect(&cfg).await {
        Ok(client) => {
            let count = client
                .database(&cfg.db)
                .collection::<Document>(&cfg.collection)
                .estimated_document_count()
                .await
                .ok();
            let roles = connection_roles(&client, &cfg.db).await;
            DbStatus {
                configured: true,
                connected: true,
                db: cfg.db,
                collection: cfg.collection,
                writable: can_write(&roles),
                roles,
                count,
                error: None,
            }
        }
        Err(error) => DbStatus {
            configured: true,
            connected: false,
            db: cfg.db,
            collection: cfg.collection,
            writable: false,
            roles: Vec::new(),
            count: None,
            error: Some(error),
        },
    }
}

pub async fn fetch_accounts(app: &AppHandle) -> Result<Vec<Value>, String> {
    let cfg =
        load_db_config(Some(app)).ok_or("No MONGO_URI configured (add it to src-tauri/.env).")?;
    fetch_accounts_with(&cfg).await
}

/// The query itself, taking a config so the import can be exercised without an
/// app handle.
pub async fn fetch_accounts_with(cfg: &DbConfig) -> Result<Vec<Value>, String> {
    let client = connect(cfg).await?;
    let mut cursor = client
        .database(&cfg.db)
        .collection::<Document>(&cfg.collection)
        .find(mongodb::bson::doc! {})
        .await
        .map_err(|e| format!("query failed: {e}"))?;

    let mut accounts = Vec::new();
    while let Some(doc) = cursor
        .next()
        .await
        .transpose()
        .map_err(|e| format!("reading results failed: {e}"))?
    {
        accounts.push(account_from_doc(&doc));
    }
    Ok(accounts)
}

// --------------------------------------------------------------------------- //
// lists (savedList)                                                            //
// --------------------------------------------------------------------------- //

#[derive(Debug, Serialize, Clone)]
pub struct DbListEntry {
    pub id: String,
    pub rebate: i64,
}

#[derive(Debug, Serialize, Clone)]
pub struct DbList {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub entries: Vec<DbListEntry>,
}

#[derive(Debug, Deserialize)]
pub struct InputEntry {
    pub id: String,
    #[serde(default = "default_rebate")]
    pub rebate: i64,
}

#[derive(Debug, Deserialize)]
pub struct InputList {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub entries: Vec<InputEntry>,
}

/// Default rebate when a payload omits it: `0`, matching the Streamlit UI's
/// `acc.get("Rebate", 0)`.
fn default_rebate() -> i64 {
    0
}

/// Map a `savedList` document onto the frontend list shape.
///
/// Entries are `{id: ObjectId, rebate: int}`; a bare ObjectId is also accepted
/// and treated as the default rebate.
pub fn list_from_doc(doc: &Document) -> DbList {
    let entries = doc
        .get_array("accounts")
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Bson::Document(entry) => Some(DbListEntry {
                        id: object_id(entry.get("id")),
                        rebate: entry
                            .get_i64("rebate")
                            .or_else(|_| entry.get_i32("rebate").map(i64::from))
                            .unwrap_or(default_rebate()),
                    }),
                    Bson::ObjectId(oid) => Some(DbListEntry {
                        id: oid.to_hex(),
                        rebate: default_rebate(),
                    }),
                    _ => None,
                })
                .filter(|entry| !entry.id.is_empty())
                .collect()
        })
        .unwrap_or_default();

    DbList {
        id: object_id(doc.get("_id")),
        name: doc.get_str("listName").unwrap_or("").to_string(),
        active: doc.get_bool("active").unwrap_or(false),
        entries,
    }
}

pub async fn fetch_lists(app: &AppHandle) -> Result<Vec<DbList>, String> {
    let cfg = load_db_config(Some(app)).ok_or(NO_URI)?;
    fetch_lists_with(&cfg).await
}

/// The actual query — takes a config so it can be exercised without an app.
pub async fn fetch_lists_with(cfg: &DbConfig) -> Result<Vec<DbList>, String> {
    let client = connect(cfg).await?;
    let mut cursor = client
        .database(&cfg.db)
        .collection::<Document>(&lists_collection())
        .find(doc! {})
        .await
        .map_err(|e| format!("query failed: {e}"))?;

    let mut lists = Vec::new();
    while let Some(document) = cursor
        .next()
        .await
        .transpose()
        .map_err(|e| format!("reading results failed: {e}"))?
    {
        lists.push(list_from_doc(&document));
    }
    lists.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(lists)
}

/// Upsert the supplied lists and return the stored result.
///
/// Never deletes: documents are matched by `_id` when the id is a real
/// ObjectId, otherwise by `listName` (local ids are UUIDs, and the existing
/// A–Z documents must be updated rather than duplicated).
pub async fn save_lists(app: &AppHandle, lists: Vec<InputList>) -> Result<Vec<DbList>, String> {
    let cfg = load_db_config(Some(app)).ok_or(NO_URI)?;
    save_lists_with(&cfg, lists).await
}

/// The actual upsert — takes a config so it can be exercised without an app.
pub async fn save_lists_with(cfg: &DbConfig, lists: Vec<InputList>) -> Result<Vec<DbList>, String> {
    let client = connect(cfg).await?;
    let collection = client
        .database(&cfg.db)
        .collection::<Document>(&lists_collection());

    for list in &lists {
        let accounts: Vec<Bson> = list
            .entries
            .iter()
            .filter_map(|entry| {
                ObjectId::parse_str(&entry.id)
                    .ok()
                    .map(|oid| Bson::Document(doc! { "id": oid, "rebate": entry.rebate }))
            })
            .collect();

        let filter = match ObjectId::parse_str(&list.id) {
            Ok(oid) => doc! { "_id": oid },
            Err(_) => doc! { "listName": &list.name },
        };
        let update = doc! { "$set": {
            "listName": &list.name,
            "active": list.active,
            "accounts": Bson::Array(accounts),
        } };

        collection
            .update_one(filter, update)
            .upsert(true)
            .await
            .map_err(|e| format!("could not save list {:?}: {e}", list.name))?;
    }

    fetch_lists_with(cfg).await
}

// --------------------------------------------------------------------------- //
// DOP portal credentials (users collection, Fernet-encrypted)                   //
// --------------------------------------------------------------------------- //

/// The user the old app read (`main.py`: `$match: {_id: ObjectId(...)}`).
pub const USER_ID: &str = "5fbf919c87da8228f87bd62f";
/// Collection holding the login record and `UserInfo`.
pub const USERS_COLLECTION: &str = "users";

/// Where the DOP portal credentials came from.
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CredentialSource {
    /// `DOP_USERNAME` / `DOP_PASSWORD` in the environment or a `.env`.
    Env,
    /// The local database — the normal case.
    Local,
    /// The older app-config `credentials.json`.
    Config,
    /// The `users` collection in Atlas, decrypted with `FERNET_KEY`.
    Atlas,
}

/// What the UI may know about the credentials. Never carries the password.
#[derive(Debug, Serialize)]
pub struct DopCredentialStatus {
    pub username: String,
    pub source: CredentialSource,
    pub has_password: bool,
    /// Whether Atlas holds a user document with a password we can decrypt.
    pub atlas_available: bool,
    /// Why Atlas could not supply them, when it could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// DOP portal credentials, Rust-side only.
///
/// Deliberately **not** `Serialize`: the password goes to `scraper.py` and
/// nowhere else, so it cannot leak into the webview by accident.
#[derive(Clone)]
pub struct DopCredentials {
    pub username: String,
    pub password: String,
    pub source: CredentialSource,
}

/// The Fernet key, exactly as the Python app read it (`FERNET_KEY`).
pub fn fernet_key(app: Option<&AppHandle>) -> Option<String> {
    setting(app, "FERNET_KEY").filter(|key| !key.trim().is_empty())
}

fn bson_type_name(value: &Bson) -> &'static str {
    match value {
        Bson::String(_) => "string",
        Bson::Binary(_) => "binary",
        Bson::ObjectId(_) => "objectId",
        Bson::Document(_) => "document",
        Bson::Array(_) => "array",
        Bson::Double(_) => "double",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        _ => "other",
    }
}

/// Decrypt `UserInfo.DOP_password`, accepting either storage type.
fn decrypt_stored_password(key: &str, stored: &Bson) -> Result<String, String> {
    let token = match stored {
        Bson::String(text) => text.clone(),
        Bson::Binary(binary) => String::from_utf8(binary.bytes.clone())
            .map_err(|_| "UserInfo.DOP_password binary is not UTF-8".to_string())?,
        other => {
            return Err(format!(
                "UserInfo.DOP_password is a {}, expected string or binary",
                bson_type_name(other)
            ))
        }
    };
    crate::crypt::decrypt(key, &token)
}

/// Load the DOP credentials from the `users` collection, decrypting the password.
pub async fn fetch_atlas_credentials(app: &AppHandle) -> Result<DopCredentials, String> {
    let cfg = load_db_config(Some(app)).ok_or(NO_URI)?;
    let key =
        fernet_key(Some(app)).ok_or("FERNET_KEY is not configured (add it to src-tauri/.env).")?;
    fetch_atlas_credentials_with(&cfg, &key).await
}

/// The filter that finds the single login record the old app used.
fn user_filter() -> Document {
    match ObjectId::parse_str(USER_ID) {
        Ok(oid) => doc! { "_id": oid },
        Err(_) => doc! { "_id": USER_ID },
    }
}

/// The actual query + decryption — takes a config and key so it can be
/// exercised without an app handle.
pub async fn fetch_atlas_credentials_with(
    cfg: &DbConfig,
    key: &str,
) -> Result<DopCredentials, String> {
    let client = connect(cfg).await?;

    let filter = user_filter();
    let user = client
        .database(&cfg.db)
        .collection::<Document>(USERS_COLLECTION)
        .find_one(filter)
        .await
        .map_err(|error| format!("query failed: {error}"))?
        .ok_or_else(|| format!("no user {USER_ID} in {}.{USERS_COLLECTION}", cfg.db))?;

    let info = user
        .get_document("UserInfo")
        .ok()
        .cloned()
        .unwrap_or_default();
    let username = info.get_str("DOP_ID").unwrap_or("").trim().to_string();
    if username.is_empty() {
        return Err("UserInfo.DOP_ID is empty".to_string());
    }

    let stored = info
        .get("DOP_password")
        .ok_or("UserInfo.DOP_password is missing")?;
    let password = decrypt_stored_password(&key, stored)?;
    if password.trim().is_empty() {
        return Err("the decrypted DOP password is empty".to_string());
    }

    Ok(DopCredentials {
        username,
        password,
        source: CredentialSource::Atlas,
    })
}

/// Replace `UserInfo.DOP_password` with an already-encrypted token.
///
/// This is the app's only write path to `users`, and it needs a read-write Atlas
/// role — the role currently configured is read-only, so callers must treat a
/// failure here as an expected outcome, not a bug.
pub async fn update_atlas_dop_password(app: &AppHandle, token: &str) -> Result<(), String> {
    let cfg = load_db_config(Some(app)).ok_or(NO_URI)?;
    let client = connect(&cfg).await?;

    let result = client
        .database(&cfg.db)
        .collection::<Document>(USERS_COLLECTION)
        .update_one(
            user_filter(),
            doc! { "$set": { "UserInfo.DOP_password": token } },
        )
        .await
        .map_err(|error| error.to_string())?;

    if result.matched_count == 0 {
        return Err(format!(
            "no user {USER_ID} in {}.{USERS_COLLECTION}",
            cfg.db
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_expected_user_filter() {
        // Must be a real ObjectId; a string here silently matches nothing.
        assert!(matches!(user_filter().get("_id"), Some(Bson::ObjectId(_))));
    }

    fn sample_doc() -> Document {
        doc! {
            "_id": ObjectId::parse_str("67822e54900a10d40ce71722").unwrap(),
            "Number": "4999087654321",
            "Name": "SBI OPC",
            "Denomination": 100,
            "CNumber": "CN-1",
            "Ref_Number": "REF-1",
            "addedIn": "A",
        }
    }

    #[test]
    fn maps_object_id_to_hex_string() {
        let mapped = account_from_doc(&sample_doc());
        assert_eq!(mapped["_id"], json!("67822e54900a10d40ce71722"));
    }

    #[test]
    fn coerces_numeric_denomination_to_string() {
        let mapped = account_from_doc(&sample_doc());
        assert_eq!(mapped["Denomination"], json!("100"));
    }

    #[test]
    fn keeps_string_fields() {
        let mapped = account_from_doc(&sample_doc());
        assert_eq!(mapped["Number"], json!("4999087654321"));
        assert_eq!(mapped["Name"], json!("SBI OPC"));
        assert_eq!(mapped["addedIn"], json!("A"));
    }

    #[test]
    fn missing_fields_become_empty_strings() {
        let mapped = account_from_doc(&doc! { "_id": ObjectId::new() });
        assert_eq!(mapped["Number"], json!(""));
        assert_eq!(mapped["Name"], json!(""));
        assert_eq!(mapped["Denomination"], json!(""));
    }

    #[test]
    fn tolerates_float_and_decimal_denominations() {
        let mapped = account_from_doc(&doc! { "_id": ObjectId::new(), "Denomination": 100.0 });
        assert_eq!(mapped["Denomination"], json!("100"));
        let mapped = account_from_doc(&doc! { "_id": ObjectId::new(), "Denomination": 55.5 });
        assert_eq!(mapped["Denomination"], json!("55.5"));
    }

    #[test]
    fn env_file_parsing_skips_comments_and_blanks() {
        let dir = std::env::temp_dir().join("autodop-env-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        std::fs::write(
            &path,
            "# comment\n\nMONGO_URI=mongodb+srv://u:p@host/db\nMONGO_DB=accounts\nBROKEN\n",
        )
        .unwrap();
        let pairs = parse_env_file(&path);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "MONGO_URI");
        assert_eq!(pairs[0].1, "mongodb+srv://u:p@host/db");
        assert_eq!(pairs[1].0, "MONGO_DB");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn defaults_match_the_live_cluster() {
        assert_eq!(DEFAULT_DB, "accounts");
        assert_eq!(DEFAULT_COLLECTION, "accountHolders");
        assert_eq!(LISTS_COLLECTION, "savedList");
    }

    #[test]
    fn detects_read_only_roles() {
        assert!(!can_write(&[]));
        assert!(!can_write(&["readAnyDatabase".to_string()]));
        assert!(!can_write(&["read".to_string()]));
        assert!(can_write(&["readWrite".to_string()]));
        assert!(can_write(&["readWriteAnyDatabase".to_string()]));
        assert!(can_write(&["atlasAdmin".to_string()]));
    }

    #[test]
    fn maps_a_saved_list_document() {
        let oid = ObjectId::parse_str("67822e54900a10d40ce71722").unwrap();
        let mapped = list_from_doc(&doc! {
            "_id": ObjectId::new(),
            "listName": "A",
            "active": true,
            "accounts": [ { "id": oid, "rebate": 4 } ],
        });
        assert_eq!(mapped.name, "A");
        assert!(mapped.active);
        assert_eq!(mapped.entries.len(), 1);
        assert_eq!(mapped.entries[0].id, "67822e54900a10d40ce71722");
        assert_eq!(mapped.entries[0].rebate, 4);
    }

    #[test]
    fn defaults_rebate_and_tolerates_bare_ids() {
        let oid = ObjectId::new();
        let mapped = list_from_doc(&doc! {
            "_id": ObjectId::new(),
            "listName": "B",
            "accounts": [ { "id": oid }, Bson::ObjectId(oid) ],
        });
        assert_eq!(mapped.entries.len(), 2);
        assert!(mapped.entries.iter().all(|entry| entry.rebate == 0));
        assert_eq!(mapped.entries[1].id, oid.to_hex());
    }

    #[test]
    fn empty_or_missing_lists_map_cleanly() {
        let mapped = list_from_doc(&doc! { "_id": ObjectId::new(), "listName": "Z" });
        assert!(mapped.entries.is_empty());
        assert!(!mapped.active);
        let mapped =
            list_from_doc(&doc! { "_id": ObjectId::new(), "listName": "C", "accounts": [] });
        assert!(mapped.entries.is_empty());
    }

    #[test]
    fn input_list_deserializes_from_frontend_shape() {
        let list: InputList = serde_json::from_str(
            r#"{"id":"uuid-1","name":"A","active":true,"entries":[{"id":"67822e54900a10d40ce71722"}]}"#,
        )
        .unwrap();
        assert_eq!(list.name, "A");
        assert!(list.active);
        assert_eq!(list.entries.len(), 1);
        assert_eq!(
            list.entries[0].rebate, 0,
            "rebate defaults to 0 when omitted"
        );
    }
}
