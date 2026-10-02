//! Run the one-time Atlas → SQLite import without the app.
//!
//!     cd src-tauri && cargo run --example import_probe                 # temp db
//!     cd src-tauri && cargo run --example import_probe -- /path/to.db  # real file
//!
//! Reads Atlas (read-only) and writes a local database, then reports what the
//! database holds and what the list reader returns. Never prints a password.
//!
//! With no argument it writes to a throwaway file and deletes it, so it is safe
//! to run at any time; pass a path to prepare the real database.

use autodop_lib::db::{fernet_key, load_db_config};
use autodop_lib::store::Store;
use std::path::PathBuf;

fn main() {
    tauri::async_runtime::block_on(async {
        let (target, temporary) = match std::env::args().nth(1) {
            Some(path) => (PathBuf::from(path), false),
            None => (
                std::env::temp_dir().join(format!("autodop-import-{}.db", std::process::id())),
                true,
            ),
        };
        println!("target: {}", target.display());

        let Some(cfg) = load_db_config(None) else {
            eprintln!("NO_MONGO_URI: no MONGO_URI in the environment or src-tauri/.env");
            std::process::exit(1);
        };
        let Some(key) = fernet_key(None) else {
            eprintln!("NO_FERNET_KEY: the Atlas copy cannot be read without FERNET_KEY");
            std::process::exit(1);
        };

        let store = match Store::open(&target) {
            Ok(store) => store,
            Err(error) => {
                eprintln!("OPEN_FAILED: {error}");
                std::process::exit(1);
            }
        };

        let dump = match autodop_lib::read_atlas(&cfg, &key).await {
            Ok(dump) => dump,
            Err(error) => {
                eprintln!("IMPORT_FAILED: {error}");
                std::process::exit(1);
            }
        };

        match autodop_lib::write_import(&store, &dump, &key) {
            Ok(report) => {
                println!(
                    "imported: {} accounts, {} lists, {} entries, credentials={}",
                    report.accounts, report.lists, report.entries, report.credentials
                );
                for warning in &report.warnings {
                    println!("  warning: {warning}");
                }
            }
            Err(error) => {
                eprintln!("IMPORT_FAILED: {error}");
                std::process::exit(1);
            }
        }

        let counts = store.counts().unwrap_or_else(|error| {
            eprintln!("COUNTS_FAILED: {error}");
            std::process::exit(1);
        });
        println!(
            "database now: {} accounts, {} lists, {} entries",
            counts.accounts, counts.lists, counts.entries
        );

        match store.accounts() {
            Ok(accounts) => println!(
                "first account keys: {:?}",
                accounts
                    .first()
                    .and_then(|row| row.as_object())
                    .map(|object| object.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            ),
            Err(error) => println!("reading accounts failed: {error}"),
        }

        match store.lists() {
            Ok(lists) => {
                println!(
                    "lists: {} (active={:?})",
                    lists.len(),
                    lists
                        .iter()
                        .find(|list| list.active)
                        .map(|list| list.name.as_str())
                );
            }
            Err(error) => println!("reading lists failed: {error}"),
        }

        let credential = match store.credentials() {
            Ok(Some(stored)) => format!("{} (token {} chars)", stored.username, stored.token.len()),
            Ok(None) => "none".to_string(),
            Err(error) => format!("error: {error}"),
        };
        println!("stored credential: {credential}");
        println!("atlas_imported_at: {:?}", store.meta("atlas_imported_at"));

        if temporary {
            let _ = std::fs::remove_file(&target);
            println!("(throwaway database removed)");
        }
    });
}
