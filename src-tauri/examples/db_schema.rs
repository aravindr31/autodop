//! Read-only schema dump of the Atlas database.
//!
//! Reports collections, document counts, indexes, and the *shape* of the stored
//! documents — field names and BSON types only. Never values: `accountHolders`
//! holds account numbers and names, and `users` holds a password ciphertext.
//!
//!     cd src-tauri && cargo run --example db_schema

use autodop_lib::db::{connect, load_db_config};
use futures_util::StreamExt;
use mongodb::bson::{Bson, Document};

fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "document",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::ObjectId(_) => "objectId",
        Bson::DateTime(_) => "date",
        Bson::Binary(_) => "binary",
        Bson::Decimal128(_) => "decimal128",
        Bson::Timestamp(_) => "timestamp",
        Bson::RegularExpression(_) => "regex",
        _ => "other",
    }
}

/// Collect `path -> type` pairs, stepping into subdocuments and into the first
/// element of an array (that is where `savedList.accounts` hides its shape).
fn shape(doc: &Document, prefix: &str, out: &mut Vec<(String, String)>) {
    for (key, value) in doc {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Bson::Document(inner) => {
                out.push((path.clone(), "document".to_string()));
                shape(inner, &path, out);
            }
            Bson::Array(items) => {
                out.push((path.clone(), format!("array(n={})", items.len())));
                if let Some(first) = items.iter().find(|item| !matches!(item, Bson::Null)) {
                    match first {
                        Bson::Document(inner) => shape(inner, &format!("{path}[]"), out),
                        other => out.push((format!("{path}[]"), type_name(other).to_string())),
                    }
                }
            }
            other => out.push((path, type_name(other).to_string())),
        }
    }
}

fn main() {
    tauri::async_runtime::block_on(async {
        let Some(cfg) = load_db_config(None) else {
            eprintln!("NO_MONGO_URI: no MONGO_URI in the environment or src-tauri/.env");
            std::process::exit(1);
        };
        let client = match connect(&cfg).await {
            Ok(client) => client,
            Err(error) => {
                eprintln!("CONNECT_FAILED: {error}");
                std::process::exit(1);
            }
        };

        let database = client.database(&cfg.db);
        let mut names = database.list_collection_names().await.unwrap_or_default();
        names.sort();

        println!("database: {}\n", cfg.db);
        for name in names {
            let collection = database.collection::<Document>(&name);
            let count = collection.estimated_document_count().await.unwrap_or(0);
            println!("=== {name}  ({count} docs)");

            // indexes
            match collection.list_indexes().await {
                Ok(mut indexes) => {
                    let mut shown = Vec::new();
                    while let Some(Ok(index)) = indexes.next().await {
                        let keys: Vec<String> = index
                            .keys
                            .iter()
                            .map(|(field, direction)| format!("{field}:{direction}"))
                            .collect();
                        let unique = index
                            .options
                            .as_ref()
                            .and_then(|options| options.unique)
                            .unwrap_or(false);
                        shown.push(format!(
                            "{}{}",
                            keys.join(", "),
                            if unique { " (unique)" } else { "" }
                        ));
                    }
                    println!("    indexes: {}", if shown.is_empty() { "none".into() } else { shown.join(" | ") });
                }
                Err(error) => println!("    indexes: unavailable ({error})"),
            }

            // shapes, sampled across a few hundred documents
            match collection.find(mongodb::bson::doc! {}).limit(200).await {
                Ok(mut cursor) => {
                    let mut fields: Vec<(String, String)> = Vec::new();
                    let mut sampled = 0;
                    while let Some(Ok(doc)) = cursor.next().await {
                        sampled += 1;
                        shape(&doc, "", &mut fields);
                    }
                    if sampled == 0 {
                        println!("    (empty)");
                    } else {
                        fields.sort();
                        fields.dedup();
                        println!("    fields (sampled {sampled}):");
                        for (path, kind) in fields {
                            // An array's element count differs per document; it is
                            // not part of the schema.
                            let kind = if kind.starts_with("array(") { "array".to_string() } else { kind };
                            println!("      {path}: {kind}");
                        }
                    }
                }
                Err(error) => println!("    read failed: {error}"),
            }
            println!();
        }

        println!("(no values read: names, numbers and ciphertexts are not printed)");
    });
}
