//! Read-only Atlas connectivity probe.
//!
//! Verifies the MONGO_URI from `src-tauri/.env` works and reports the *shape* of
//! the stored documents (field names only — never values, never credentials).
//!
//!     cd src-tauri && cargo run --example db_probe

use autodop_lib::db::{connect, load_db_config};
use futures_util::StreamExt;
use mongodb::bson::Document;

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
    });
}
