//! Exercise backup / restore without the app.
//!
//!     cd src-tauri && cargo run --example backup_probe                  # temp roundtrip
//!     cargo run --example backup_probe -- export /path/to/backup.db    # real backup
//!     cargo run --example backup_probe -- inspect /path/to/backup.db   # what it holds
//!
//! With no argument: builds a scratch database, backs it up, mutates the
//! live one, restores, and verifies everything came back. Never touches the
//! real database and never prints a password.
//!
//! `export` and `inspect` with a path are read-only against the live store
//! (export writes only the destination file).

use autodop_lib::backup;
use autodop_lib::store::Store;
use serde_json::json;
use std::path::PathBuf;

fn main() {
    match std::env::args().nth(1) {
        Some(command) if command == "inspect" => {
            let path = PathBuf::from(std::env::args().nth(2).expect("a path to inspect"));
            match backup::inspect(&path) {
                Ok(snapshot) => println!("{snapshot:?}"),
                Err(error) => {
                    eprintln!("INSPECT_FAILED: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some(command) if command == "export" => {
            let destination = PathBuf::from(std::env::args().nth(2).expect("a destination path"));
            let source = live_store_path();
            println!("source: {}", source.display());
            match backup::export(&source, &destination) {
                Ok(snapshot) => println!("exported: {snapshot:?} -> {}", destination.display()),
                Err(error) => {
                    eprintln!("EXPORT_FAILED: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some(command) if command == "json" => {
            let destination = PathBuf::from(std::env::args().nth(2).expect("a destination path"));
            let source = live_store_path();
            println!("source: {}", source.display());
            let store = Store::open(&source).expect("live store opens");
            // The portable export needs no password: the DOP token, the salt
            // and the login hash travel as the store already holds them.
            let credentials = store.credentials().expect("creds").and_then(|stored| {
                Some(autodop_lib::backup::PortableCredentials {
                    username: stored.username,
                    token: stored.token,
                    salt: store.meta("kdf_salt")?,
                    login_hash: store.meta("login_hash")?,
                })
            });
            let written = backup::portable_export(
                &store,
                &destination,
                &autodop_lib::rfc3339_utc(autodop_lib::now_secs()),
                credentials,
            )
            .expect("portable export succeeds");
            let parsed = backup::portable_parse(&destination).expect("parses back");
            assert_eq!(parsed.accounts.len() as i64, store.counts().expect("counts").accounts);
            println!(
                "exported: {} accounts, {} list(s), credentials: {} -> {}",
                written.accounts.len(),
                written.lists.len(),
                written.credentials.is_some(),
                destination.display()
            );
        }
        Some(other) => {
            eprintln!(
                "unknown command: {other} (use export, inspect, json, or nothing for the roundtrip)"
            );
            std::process::exit(2);
        }
        None => roundtrip(),
    }
}

/// The path the running app uses, when it can be found — informational only.
fn live_store_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let path =
            PathBuf::from(home).join("Library/Application Support/in.aravind.autodop/autodop.db");
        if path.exists() {
            return path;
        }
    }
    PathBuf::from("autodop.db")
}

fn roundtrip() {
    let dir = std::env::temp_dir().join(format!("autodop-backup-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let live = dir.join("autodop.db");
    let backup_path = dir.join("autodop-backup.db");

    // 1. Seed a store the way the app would have, then close it — nothing
    // may hold the live file open across the backup or the restore.
    let ids = {
        let store = Store::open(&live).expect("store opens");
        store
            .replace_accounts(&[
            json!({"_id": "probe-acc-1", "Number": "4999087654321", "Name": "Probe One", "Denomination": 100}),
            json!({"_id": "probe-acc-2", "Number": "4999087654322", "Name": "Probe Two", "Denomination": "100"}),
            json!({"_id": "probe-acc-3", "Number": "4999087654323", "Name": "Probe Three", "Denomination": 200}),
        ])
            .expect("accounts saved");
        store
            .save_lists(&[autodop_lib::db::InputList {
                id: String::new(),
                name: "PROBE LIST".into(),
                active: true,
                entries: vec![
                    autodop_lib::db::InputEntry {
                        id: "probe-acc-1".into(),
                        rebate: 1,
                    },
                    autodop_lib::db::InputEntry {
                        id: "probe-acc-2".into(),
                        rebate: 1,
                    },
                ],
            }])
            .expect("list saved");
        store
            .set_credentials("DOP.MI.PROBE", "fernet-token-never-shown")
            .expect("credential saved");
        println!("seeded: 3 accounts, 1 list with 2 entries, 1 credential");
        store
    };
    drop(ids);

    // 2. Back it up.
    let snapshot = backup::export(&live, &backup_path).expect("export succeeds");
    println!("backup:  {snapshot:?} -> {}", backup_path.display());

    // 3. Wreck the live one, in its own scope.
    {
        let store = Store::open(&live).expect("store reopens");
        store.clear_all_lists().expect("lists cleared");
        store
            .delete_account("probe-acc-1")
            .expect("account deleted");
        let before = store.counts().expect("counts");
        println!("damaged: {before:?}");
    }

    // 4. Restore exactly the way `import_backup` does: drop every connection,
    // remove WAL sidecars, copy, remove sidecars again.
    for sidecar in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.join(format!("autodop.db{sidecar}")));
    }
    std::fs::copy(&backup_path, &live).expect("restore copy");
    for sidecar in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.join(format!("autodop.db{sidecar}")));
    }
    let restored = Store::open(&live).expect("restored store opens");
    let after = restored.counts().expect("counts");
    let credentials = restored.credentials().expect("credentials read");
    println!(
        "restored: {after:?}, credential present: {}",
        credentials.is_some()
    );
    assert_eq!(after.accounts, 3, "accounts came back");
    assert_eq!(after.lists, 1, "lists came back");
    assert_eq!(after.entries, 2, "entries came back");
    assert!(credentials.is_some(), "credential came back");
    assert_eq!(
        snapshot,
        backup::inspect(&backup_path).expect("backup still reads")
    );

    // 5. Safety: a foreign file must not be overwritten.
    let foreign = dir.join("foreign.db");
    let alien = rusqlite::Connection::open(&foreign).expect("alien opens");
    alien
        .execute("CREATE TABLE notes (body TEXT)", [])
        .expect("table made");
    drop(alien);
    let error = backup::export(&live, &foreign).expect_err("export must refuse a foreign file");
    println!("refused: {error}");

    let _ = std::fs::remove_dir_all(&dir);
    println!("BACKUP PROBE: ALL GOOD");
}
