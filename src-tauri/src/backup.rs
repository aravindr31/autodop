//! Backup / restore of the local SQLite database.
//!
//! A backup is a plain copy of `autodop.db` made with SQLite's own backup
//! API — which matters because the store runs in WAL mode and a raw `cp` of
//! the main file can miss pages still sitting in `-wal`. The backup carries
//! the accounts, lists, the encrypted DOP password, and the login hash +
//! KDF salt, so restoring it is only safe together with the login password
//! that key was derived from. That is the documented caveat: *the backup is
//! the database file plus you remembering the password.*

use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
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
        store
            .replace_accounts(&[
                serde_json::json!({"Number": "111", "Name": "One", "Denomination": 100}),
                serde_json::json!({"Number": "222", "Name": "Two", "Denomination": "100"}),
            ])
            .expect("accounts saved");
        store
            .set_credentials("DOP.MI.TEST", "fernet-token")
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
        assert_eq!(store.counts().expect("counts").accounts, 2);
        assert!(store.credentials().expect("creds").is_some());
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
}
