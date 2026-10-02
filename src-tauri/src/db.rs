//! Local configuration and the shared list/credential types.
//!
//! Once a SQLite store existed, Atlas was only ever a one-time import source
//! and a credential fallback. Both went when the data was confirmed local —
//! the import happened on this machine, the live database is backed up, and
//! nothing here reaches the network. What remains is the `.env` reader
//! (which still feeds `AUTODOP_PYTHON`, `AUTODOP_SCRAPER`, legacy keys) and
//! the list/credential shapes the frontend and the store share.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

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

pub fn parse_env_file(path: &Path) -> Vec<(String, String)> {
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
pub fn setting(app: Option<&AppHandle>, key: &str) -> Option<String> {
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

/// The legacy Fernet key, exactly as the Python app read it (`FERNET_KEY`).
///
/// Only used to migrate a credential from the old `credentials.json`; the
/// local store derives its own key from the login password.
pub fn fernet_key(app: Option<&AppHandle>) -> Option<String> {
    setting(app, "FERNET_KEY").filter(|key| !key.trim().is_empty())
}

// --------------------------------------------------------------------------- //
// lists                                                                       //
// --------------------------------------------------------------------------- //

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbListEntry {
    pub id: String,
    pub rebate: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

// --------------------------------------------------------------------------- //
// DOP portal credentials                                                      //
// --------------------------------------------------------------------------- //

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
}

/// What the UI may know about the credentials. Never carries the password.
#[derive(Debug, Serialize)]
pub struct DopCredentialStatus {
    pub username: String,
    pub source: CredentialSource,
    pub has_password: bool,
    /// Why no credentials were found, when none were.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_file_parsing_skips_comments_and_blanks() {
        let dir = std::env::temp_dir().join("autodop-env-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        std::fs::write(
            &path,
            "# comment\n\nAUTODOP_PYTHON=/usr/bin/python3\nDOP_USERNAME=u\nBROKEN\n",
        )
        .unwrap();
        let pairs = parse_env_file(&path);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, "AUTODOP_PYTHON");
        assert_eq!(pairs[0].1, "/usr/bin/python3");
        assert_eq!(pairs[1].0, "DOP_USERNAME");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn input_list_deserializes_from_frontend_shape() {
        let list: InputList = serde_json::from_str(
            r#"{"id":"uuid-1","name":"A","active":true,"entries":[{"id":"probe-1"}]}"#,
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
