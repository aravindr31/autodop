//! Read-only Atlas connectivity probe.
//!
//! Verifies the MONGO_URI from `src-tauri/.env` works and reports the *shape* of
//! the stored documents (field names only — never values, never credentials).
//!
//!     cd src-tauri && cargo run --example db_probe

use autodop_lib::db::{
    connect, fernet_key, fetch_atlas_credentials_with, fetch_lists_with, load_db_config,
    save_lists_with, InputEntry, InputList,
};
use futures_util::StreamExt;
use mongodb::bson::{doc, Document};

fn main() {
    tauri::async_runtime::block_on(async {
        let Some(cfg) = load_db_config(None) else {
            eprintln!("NO_MONGO_URI: no MONGO_URI in the environment or src-tauri/.env");
            std::process::exit(1);
        };
        println!("target: db={} collection={}", cfg.db, cfg.collection);

        let client = match connect(&cfg).await {
            Ok(client) => {
                println!("CONNECTED");
                client
            }
            Err(error) => {
                eprintln!("CONNECT_FAILED: {error}");
                std::process::exit(1);
            }
        };

        let databases = client.list_database_names().await.unwrap_or_default();
        println!("databases visible: {databases:?}");

        let collection = client
            .database(&cfg.db)
            .collection::<Document>(&cfg.collection);
        let count = collection.estimated_document_count().await.unwrap_or(0);
        println!("documents in {}.{}: {count}", cfg.db, cfg.collection);

        match collection.find(mongodb::bson::doc! {}).limit(1).await {
            Ok(mut cursor) => match cursor.next().await {
                Some(Ok(doc)) => {
                    let keys: Vec<&str> = doc.keys().map(String::as_str).collect();
                    println!("first document keys: {keys:?}");
                }
                Some(Err(error)) => println!("first document read failed: {error}"),
                None => println!("collection is empty"),
            },
            Err(error) => println!("find failed: {error}"),
        }

        // The old app also kept lists here; report whether it still does.
        let names = client
            .database(&cfg.db)
            .list_collection_names()
            .await
            .unwrap_or_default();
        println!("collections in {}: {names:?}", cfg.db);
        for name in names {
            let estimated = client
                .database(&cfg.db)
                .collection::<Document>(&name)
                .estimated_document_count()
                .await
                .unwrap_or(0);
            println!("  {name}: {estimated} docs");
        }

        // What may this user actually do? Safe, read-only command.
        match client
            .database(&cfg.db)
            .run_command(doc! { "connectionStatus": 1 })
            .await
        {
            Ok(reply) => {
                let roles: Vec<String> = reply
                    .get_document("authInfo")
                    .ok()
                    .and_then(|info| info.get_array("authenticatedUserRoles").ok())
                    .map(|list| {
                        list.iter()
                            .filter_map(|item| {
                                item.as_document()
                                    .and_then(|entry| entry.get_str("role").ok())
                                    .map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let writable = roles.iter().any(|role| {
                    role.contains("readWrite") || role == "root" || role == "atlasAdmin"
                });
                println!("auth roles: {roles:?}");
                println!("writable (can save lists): {writable}");
            }
            Err(error) => println!("connectionStatus failed: {error}"),
        }

        // DOP credentials: confirm the Fernet key really opens the stored
        // password. Only the length is reported — never the value.
        match fernet_key(None) {
            Some(key) => match fetch_atlas_credentials_with(&cfg, &key).await {
                Ok(creds) => println!(
                    "dop credentials OK: source={:?} DOP_ID={:?} password={} chars (value withheld)",
                    creds.source,
                    creds.username,
                    creds.password.len()
                ),
                Err(error) => println!("dop credentials FAILED: {error}"),
            },
            None => println!("FERNET_KEY not configured — credential check skipped"),
        }

        // Exercise the real list reader — the production path, not a copy of it.
        match fetch_lists_with(&cfg).await {
            Ok(lists) => {
                let with_entries = lists.iter().filter(|l| !l.entries.is_empty()).count();
                println!(
                    "fetch_lists: {} lists, {} with entries, active={:?}",
                    lists.len(),
                    with_entries,
                    lists.iter().find(|l| l.active).map(|l| l.name.as_str()),
                );
                println!(
                    "names: {:?}",
                    lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>()
                );
            }
            Err(error) => println!("fetch_lists failed: {error}"),
        }

        // Opt-in write round-trip. Only runs when MONGO_LISTS_COLLECTION points
        // at a throwaway collection, so the real `savedList` is never touched.
        let lists_name =
            std::env::var("MONGO_LISTS_COLLECTION").unwrap_or_else(|_| "savedList".to_string());
        if std::env::var("MONGO_LISTS_COLLECTION").is_ok() {
            println!("write test targeting {lists_name:?}");
            let probe_name = "__probe__";
            let payload = vec![InputList {
                id: String::new(),
                name: probe_name.to_string(),
                active: true,
                entries: vec![InputEntry {
                    id: "67822e54900a10d40ce71722".to_string(),
                    rebate: 3,
                }],
            }];
            match save_lists_with(&cfg, payload).await {
                Ok(saved) => {
                    let found = saved.iter().find(|list| list.name == probe_name);
                    println!(
                        "write round-trip: {} list(s); probe entries={:?} rebate={:?}",
                        saved.len(),
                        found.map(|list| list.entries.len()),
                        found
                            .and_then(|list| list.entries.first())
                            .map(|entry| entry.rebate),
                    );
                }
                Err(error) => println!("write round-trip failed: {error}"),
            }

            if let Err(error) = client
                .database(&cfg.db)
                .collection::<Document>(&lists_name)
                .drop()
                .await
            {
                println!("probe cleanup failed: {error}");
            } else {
                println!("probe collection {lists_name:?} dropped");
            }
        }
    });
}
