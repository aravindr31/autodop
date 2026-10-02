use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn env_file_candidates(app: Option<&AppHandle>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("AUTODOP_ENV_FILE") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit.trim()));
        }
    }

    if let Some(manifest) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        paths.push(manifest.join("src-tauri").join(".env"));
        paths.push(manifest.join(".env"));
    }

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

pub fn hydrate_env(app: &AppHandle) {
    for path in env_file_candidates(Some(app)) {
        for (key, value) in parse_env_file(&path) {
            if std::env::var_os(&key).is_none() {
                std::env::set_var(key, value);
            }
        }
    }
}

pub fn fernet_key(app: Option<&AppHandle>) -> Option<String> {
    setting(app, "FERNET_KEY").filter(|key| !key.trim().is_empty())
}

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

fn default_rebate() -> i64 {
    0
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CredentialSource {
    Env,

    Local,

    Config,
}

#[derive(Debug, Serialize)]
pub struct DopCredentialStatus {
    pub username: String,
    pub source: CredentialSource,
    pub has_password: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

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
