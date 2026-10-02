//! Backup / restore of the local SQLite database.
//!
//! A backup is a plain copy of `autodop.db` made with SQLite's own backup
//! API — which matters because the store runs in WAL mode and a raw `cp` of
//! the main file can miss pages still sitting in `-wal`. The backup carries
//! the accounts, lists, the encrypted DOP password, and the login hash +
//! KDF salt, so restoring it is only safe together with the login password
//! that key was derived from. That is the documented caveat: *the backup is
//! the database file plus you remembering the password.*

use crate::store::Store;
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// What a database file holds — shown in the UI before a restore, so you can
/// see what you are about to replace the live data with.
#[derive(Debug, Serialize, Default, PartialEq)]
pub struct Snapshot {
    pub accounts: i64,
    pub lists: i64,
    pub entries: i64,
    pub has_credentials: bool,
}

/// Every AutoDOP database has a `meta` table; anything without one is not
/// ours, and must not be restored over live data or clobbered by an export.
fn require_autodop(conn: &Connection, path: &Path) -> Result<(), String> {
    let found: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if found == 0 {
        return Err(format!(
            "{} is not an AutoDOP database (no meta table)",
            path.display()
        ));
    }
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if ok != "ok" {
        return Err(format!(
            "{} failed the integrity check: {ok}",
            path.display()
        ));
    }
    Ok(())
}

fn open_read_only(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))
}

/// Open a file, require it to be an AutoDOP database, and count what is in it.
pub fn inspect(path: &Path) -> Result<Snapshot, String> {
    let conn = open_read_only(path)?;
    require_autodop(&conn, path)?;
    let one = |sql: &str| -> Result<i64, String> {
        conn.query_row(sql, [], |row| row.get(0))
            .map_err(|error| error.to_string())
    };
    let has_credentials: bool = one("SELECT COUNT(*) FROM credentials")? > 0
        && one("SELECT COUNT(*) FROM credentials WHERE TRIM(password) <> ''")? > 0;
    Ok(Snapshot {
        accounts: one("SELECT COUNT(*) FROM accounts")?,
        lists: one("SELECT COUNT(*) FROM lists")?,
        entries: one("SELECT COUNT(*) FROM list_entries")?,
        has_credentials,
    })
}

/// Copy `source` into `destination` with SQLite's backup API, then verify the
/// copy by reading it back. Refuses to clobber an existing file that is not
/// an AutoDOP database — a mistyped path must not destroy unrelated data.
pub fn export(source: &Path, destination: &Path) -> Result<Snapshot, String> {
    if destination == source {
        return Err("the backup path is the live database itself".into());
    }
    if destination.exists() {
        inspect(destination).map_err(|error| format!("refusing to overwrite: {error}"))?;
    }
    let src = open_read_only(source)?;
    require_autodop(&src, source)?;
    let mut dst = Connection::open(destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
    Backup::new(&src, &mut dst)
        .map_err(|error| format!("backup failed: {error}"))?
        .run_to_completion(64, Duration::from_millis(5), None)
        .map_err(|error| format!("backup failed: {error}"))?;
    drop(dst);
    let snapshot = inspect(destination)?;
    let original = inspect(source)?;
    if snapshot != original {
        return Err(format!(
            "backup verification failed: wrote {:?}, source holds {:?}",
            snapshot, original
        ));
    }
    Ok(snapshot)
}

// --------------------------------------------------------------------------- //
// portable JSON backup                                                        //
// --------------------------------------------------------------------------- //

/// The JSON shape written by [`portable_export`] and read by [`portable_read`].
///
/// Accounts and lists travel as plain JSON so the file is inspectable. The DOP
/// password travels as the same Fernet token the local store holds — encrypted
/// with a key derived (Argon2id) from the **app login password** of the machine
/// that made the backup. The importer asks for that login password, verifies it
/// against the carried `login_hash`, decrypts the token, and re-encrypts the
/// password under the *new* machine's login. The login password itself travels
/// nowhere — only its PHC hash, which verifies without revealing.
#[derive(Debug, Serialize, Deserialize)]
pub struct PortableBackup {
    pub format: String,
    pub version: u32,
    pub created_at: String,
    /// Accounts as the store holds them, `_id` included — list entries point
    /// at those ids.
    pub accounts: Vec<Value>,
    pub lists: Vec<crate::db::DbList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<PortableCredentials>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PortableCredentials {
    pub username: String,
    /// The Fernet token exactly as the local store holds it.
    pub token: String,
    /// The Argon2id salt (base64) the token's key was derived from.
    pub salt: String,
    /// PHC string verifying the login password that key was derived from.
    pub login_hash: String,
}

pub const PORTABLE_FORMAT: &str = "autodop-portable-backup";
pub const PORTABLE_VERSION: u32 = 1;

/// A wrong password is indistinguishable from a broken file here; say so.
const PASSWORD_MISMATCH: &str =
    "that is not the app login password this backup was made with — the DOP password in it cannot be read";

/// Gather the store into a [`PortableBackup`]. `credentials` is the stored
/// credential triple (username, token, salt) plus the login hash, supplied by
/// the caller so this stays testable; `None` when no password is stored.
pub fn portable_backup(
    store: &Store,
    owner_id: &str,
    credentials: Option<PortableCredentials>,
    created_at: &str,
) -> Result<PortableBackup, String> {
    Ok(PortableBackup {
        format: PORTABLE_FORMAT.into(),
        version: PORTABLE_VERSION,
        created_at: created_at.to_string(),
        accounts: store.accounts(owner_id)?,
        lists: store.lists(owner_id)?,
        credentials,
    })
}

/// Write the JSON file.
pub fn portable_export(
    store: &Store,
    owner_id: &str,
    destination: &Path,
    created_at: &str,
    credentials: Option<PortableCredentials>,
) -> Result<PortableBackup, String> {
    let backup = portable_backup(store, owner_id, credentials, created_at)?;
    let text = serde_json::to_string_pretty(&backup).map_err(|error| error.to_string())?;
    std::fs::write(destination, text)
        .map_err(|error| format!("cannot write {}: {error}", destination.display()))?;
    Ok(backup)
}

/// Read a JSON file, verify the shape, and verify the login password it was
/// encrypted with.
pub fn portable_read(source: &Path, login_password: &str) -> Result<PortableBackup, String> {
    let backup = portable_parse(source)?;
    if let Some(credentials) = &backup.credentials {
        if !crate::crypt::verify_login(login_password, &credentials.login_hash) {
            return Err(PASSWORD_MISMATCH.into());
        }
        // The token must actually open with this password, not merely pass a
        // stale hash — the file may have been written by a different build.
        let key = crate::crypt::derive_key(login_password, &credentials.salt)?;
        let password = crate::crypt::decrypt(&key, &credentials.token)
            .map_err(|_| PASSWORD_MISMATCH.to_string())?;
        if password.trim().is_empty() {
            return Err(PASSWORD_MISMATCH.into());
        }
    }
    Ok(backup)
}

/// Parse and shape-check a portable backup without a password.
pub fn portable_parse(source: &Path) -> Result<PortableBackup, String> {
    let text = std::fs::read_to_string(source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
    let backup: PortableBackup = serde_json::from_str(&text).map_err(|error| {
        format!(
            "{} is not an AutoDOP portable backup: {error}",
            source.display()
        )
    })?;
    if backup.format != PORTABLE_FORMAT {
        return Err(format!(
            "{} is not an AutoDOP portable backup (format {:?})",
            source.display(),
            backup.format
        ));
    }
    Ok(backup)
}

/// Put a [`PortableBackup`] into `store`, replacing what is there.
///
/// List entries pointing at accounts that are not in the file are dropped,
/// the same way `save_lists` handles stale references. The DOP password is
/// imported separately by [`portable_import_credentials`].
pub fn portable_import(
    store: &Store,
    owner_id: &str,
    backup: &PortableBackup,
) -> Result<(i64, i64, i64), String> {
    store.replace_accounts(owner_id, &backup.accounts)?;
    let input: Vec<crate::db::InputList> = backup
        .lists
        .iter()
        .map(|list| crate::db::InputList {
            id: list.id.clone(),
            name: list.name.clone(),
            active: list.active,
            entries: list
                .entries
                .iter()
                .map(|entry| crate::db::InputEntry {
                    id: entry.id.clone(),
                    rebate: entry.rebate,
                })
                .collect(),
        })
        .collect();
    store.save_lists(owner_id, &input)?;

    let counts = store.counts(owner_id)?;
    Ok((counts.accounts, counts.lists, counts.entries))
}

/// Import the DOP password, re-encrypting it under the current login key.
///
/// `login_password` is the app login password of the machine that made the
/// backup; it has already been verified by [`portable_read`].
pub fn portable_import_credentials(
    store: &Store,
    owner_id: &str,
    backup: &PortableBackup,
    login_password: &str,
    key: &str,
) -> Result<(), String> {
    let Some(credentials) = &backup.credentials else {
        return Ok(());
    };
    let derived = crate::crypt::derive_key(login_password, &credentials.salt)?;
    let password = crate::crypt::decrypt(&derived, &credentials.token)
        .map_err(|_| PASSWORD_MISMATCH.to_string())?;
    let token = crate::crypt::encrypt(key, &password)?;
    store.set_credentials(owner_id, &credentials.username, &token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::path::PathBuf;

    fn temp_file(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("autodop-backup-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn seeded(path: &Path) {
        let store = Store::open(path).expect("store opens");
        let owner = store.create_owner("9999999999", "h", "s").expect("owner");
        store
            .replace_accounts(
                &owner,
                &[
                    serde_json::json!({"Number": "111", "Name": "One", "Denomination": 100}),
                    serde_json::json!({"Number": "222", "Name": "Two", "Denomination": "100"}),
                ],
            )
            .expect("accounts saved");
        store
            .set_credentials(&owner, "DOP.MI.TEST", "fernet-token")
            .expect("credential saved");
    }

    #[test]
    fn export_copies_everything_and_verifies() {
        let src = temp_file("src.db");
        let dst = temp_file("dst.db");
        seeded(&src);
        let snapshot = export(&src, &dst).expect("export succeeds");
        assert_eq!(
            snapshot,
            Snapshot {
                accounts: 2,
                lists: 0,
                entries: 0,
                has_credentials: true
            }
        );
        // Reading the copy back through the store confirms it really opens.
        let store = Store::open(&dst).expect("backup opens as a store");
        assert_eq!(store.counts_all().expect("counts").accounts, 2);
        assert!(store
            .owners()
            .expect("owners")
            .iter()
            .any(|o| o.has_credential));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn export_refuses_to_clobber_a_foreign_file() {
        let src = temp_file("src2.db");
        let dst = temp_file("foreign.db");
        seeded(&src);
        let alien = Connection::open(&dst).expect("alien opens");
        alien
            .execute("CREATE TABLE notes (body TEXT)", [])
            .expect("table made");
        drop(alien);
        let error = export(&src, &dst).expect_err("export must refuse");
        assert!(error.contains("refusing to overwrite"), "{error}");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    #[test]
    fn inspect_rejects_a_file_that_is_not_a_database() {
        let path = temp_file("text.db");
        std::fs::write(&path, b"this is not sqlite at all").expect("written");
        assert!(inspect(&path).is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_store_backs_up_and_reads_back_empty() {
        let src = temp_file("empty-src.db");
        let dst = temp_file("empty-dst.db");
        Store::open(&src).expect("store opens");
        let snapshot = export(&src, &dst).expect("export succeeds");
        assert_eq!(snapshot, Snapshot::default());
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }

    fn portable_seed(path: &Path, login_password: &str) -> (Store, String) {
        let store = Store::open(path).expect("store opens");
        let owner = store
            .create_owner("9999999999", "unused", "unused")
            .expect("owner");
        store
            .replace_accounts(
                &owner,
                &[
                    serde_json::json!({"_id": "acc-1", "Number": "111", "Name": "One", "Denomination": 100}),
                    serde_json::json!({"_id": "acc-2", "Number": "222", "Name": "Two", "Denomination": "100"}),
                ],
            )
            .expect("accounts saved");
        store
            .save_lists(
                &owner,
                &[crate::db::InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: true,
                    entries: vec![
                        crate::db::InputEntry {
                            id: "acc-1".into(),
                            rebate: 4,
                        },
                        crate::db::InputEntry {
                            id: "acc-2".into(),
                            rebate: 0,
                        },
                    ],
                }],
            )
            .expect("list saved");
        // The owner's login material, the way `setup_login` writes it, and the
        // DOP password encrypted under that login's derived key.
        let salt = crate::crypt::new_salt().expect("salt");
        let hash = crate::crypt::hash_login(login_password).expect("hash");
        store
            .set_owner_auth(&owner, &hash, &salt)
            .expect("auth set");
        let key = crate::crypt::derive_key(login_password, &salt).expect("key");
        let token = crate::crypt::encrypt(&key, "the-portal-password").expect("token");
        store
            .set_credentials(&owner, "DOP.MI.TEST", &token)
            .expect("credential saved");
        (store, owner)
    }

    /// What the real export command assembles from the owner's row.
    fn portable_creds(store: &Store, owner: &str) -> PortableCredentials {
        let stored = store
            .credentials(owner)
            .expect("creds")
            .expect("credential");
        let auth = store.owner_auth(owner).expect("auth").expect("auth");
        PortableCredentials {
            username: stored.username,
            token: stored.token,
            salt: auth.kdf_salt,
            login_hash: auth.login_hash,
        }
    }

    #[test]
    fn portable_json_roundtrips_accounts_lists_and_password() {
        let src = temp_file("portable-src.db");
        let dst = temp_file("portable-dst.db");
        let json = temp_file("portable.json");
        let login_password = "login-pass-1";
        let (store, owner) = portable_seed(&src, login_password);
        let credentials = Some(portable_creds(&store, &owner));
        let backup = portable_export(&store, &owner, &json, "2026-10-02T00:00:00Z", credentials)
            .expect("export succeeds");
        drop(store);
        assert_eq!(backup.accounts.len(), 2);
        assert_eq!(backup.lists.len(), 1);
        assert!(backup.credentials.is_some());

        // The file is plain JSON and never carries the plaintext password.
        let text = std::fs::read_to_string(&json).expect("json read");
        assert!(text.contains("autodop-portable-backup"));
        assert!(!text.contains("the-portal-password"));

        // A fresh store, a different machine, a different login password.
        let fresh = Store::open(&dst).expect("fresh store");
        let new_owner = fresh
            .create_owner("8888888888", "unused", "unused")
            .expect("owner");
        let read = portable_read(&json, login_password).expect("read succeeds");
        portable_import(&fresh, &new_owner, &read).expect("import succeeds");
        // The new machine's login-derived key is a real Fernet key.
        let new_key = crate::crypt::generate_key().expect("key");
        portable_import_credentials(&fresh, &new_owner, &read, login_password, &new_key)
            .expect("credentials import");

        let counts = fresh.counts(&new_owner).expect("counts");
        assert_eq!((counts.accounts, counts.lists, counts.entries), (2, 1, 2));
        let stored = fresh
            .credentials(&new_owner)
            .expect("creds")
            .expect("credential");
        assert_eq!(stored.username, "DOP.MI.TEST");
        // Re-encrypted under the *new* login key, readable with it.
        assert_eq!(
            crate::crypt::decrypt(&new_key, &stored.token).expect("decrypts"),
            "the-portal-password"
        );

        // A wrong login password is refused with the helpful message.
        let error = portable_read(&json, "wrong").expect_err("wrong password refused");
        assert!(error.contains("not the app login password"), "{error}");

        // A foreign JSON file is not an AutoDOP backup.
        std::fs::write(&json, r#"{"format":"other"}"#).expect("foreign written");
        assert!(portable_read(&json, login_password).is_err());

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
        let _ = std::fs::remove_file(&json);
    }

    #[test]
    fn portable_export_carries_nothing_when_no_password_is_stored() {
        let src = temp_file("portable-nopass-src.db");
        let json = temp_file("portable-nopass.json");
        let (store, owner) = portable_seed(&src, "login-pass-1");
        store.delete_account(&owner, "acc-1").expect("deleted");
        let backup = portable_export(&store, &owner, &json, "t", None).expect("export succeeds");
        assert!(backup.credentials.is_none());
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&json);
    }
}
