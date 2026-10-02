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
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn reads_a_key_from_a_dotenv_file() {
        let path = std::env::temp_dir().join(format!("autodop-env-{}", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "# a comment").unwrap();
        writeln!(file, "FERNET_KEY=\"abc=\"").unwrap();
        writeln!(file, "EMPTY=").unwrap();
        writeln!(file, "not a pair").unwrap();
        drop(file);

        let pairs = parse_env_file(&path);
        assert_eq!(pairs, vec![("FERNET_KEY".to_string(), "abc=".to_string())]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_yields_nothing() {
        assert!(parse_env_file(Path::new("/nonexistent/.env")).is_empty());
    }
}
