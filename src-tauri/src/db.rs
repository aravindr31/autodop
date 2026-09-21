//! MongoDB Atlas access.
//!
//! The connection string is read from the environment or a gitignored `.env`
//! next to `Cargo.toml` — it is never exposed to the webview.
//!
//! A client is created per call and dropped afterwards: loads are infrequent
//! (startup + manual reload), and this keeps URI changes from going stale.

use futures_util::StreamExt;
use mongodb::bson::{Bson, Document};
use mongodb::Client;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Verified against the live cluster: database `accounts` holds
/// `accountHolders` (the account documents), `savedList` (lists + rebates),
/// `users`, `admin` and an empty `list`. `accounts.accounts` is empty.
pub const DEFAULT_DB: &str = "accounts";
pub const DEFAULT_COLLECTION: &str = "accountHolders";

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
            count: None,
            error: Some("No MONGO_URI configured (add it to src-tauri/.env).".into()),
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
            DbStatus {
                configured: true,
                connected: true,
                db: cfg.db,
                collection: cfg.collection,
                count,
                error: None,
            }
        }
        Err(error) => DbStatus {
            configured: true,
            connected: false,
            db: cfg.db,
            collection: cfg.collection,
            count: None,
            error: Some(error),
        },
    }
}

pub async fn fetch_accounts(app: &AppHandle) -> Result<Vec<Value>, String> {
    let cfg =
        load_db_config(Some(app)).ok_or("No MONGO_URI configured (add it to src-tauri/.env).")?;
    let client = connect(&cfg).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::{doc, oid::ObjectId};

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
    }
}
