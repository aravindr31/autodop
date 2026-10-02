//! Local SQLite store — the app's own database, one file on this machine.
//!
//! Replaces the Atlas-backed path: an agent's accounts, lists and DOP password
//! belong to that machine, and nothing is shared. Atlas is only read, once, by
//! [`crate::db`] to import existing data.
//!
//! Takes a path rather than an `AppHandle` on purpose, so the whole schema and
//! every round trip can be exercised in tests against an in-memory database.

use crate::db::{DbList, DbListEntry, InputList};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

/// Bump when the schema changes; [`Store::migrate`] applies what is missing.
const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS accounts (
    id           TEXT PRIMARY KEY,
    number       TEXT NOT NULL,
    name         TEXT NOT NULL DEFAULT '',
    -- TEXT on purpose: Atlas holds this as both int32 and string, and the
    -- frontend treats it as a string. Storing it verbatim loses nothing.
    denomination TEXT NOT NULL DEFAULT '',
    cnumber      TEXT NOT NULL DEFAULT '',
    ref_number   TEXT NOT NULL DEFAULT '',
    added_in     TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS accounts_number ON accounts(number);

CREATE TABLE IF NOT EXISTS lists (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    active     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS lists_name ON lists(name);

CREATE TABLE IF NOT EXISTS list_entries (
    list_id    TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    rebate     INTEGER NOT NULL DEFAULT 0,
    position   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (list_id, account_id)
);
CREATE INDEX IF NOT EXISTS list_entries_account ON list_entries(account_id);

-- Exactly one row: the DOP portal pair. The password is a Fernet token.
CREATE TABLE IF NOT EXISTS credentials (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    username   TEXT NOT NULL,
    password   TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- One row per Generate run, so what was paid can be answered later.
CREATE TABLE IF NOT EXISTS runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    list_names  TEXT NOT NULL,
    status      TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    detail      TEXT
);

-- Small key/value corner: which schema version wrote this, whether the
-- import has run, and anything else that must survive a restart.
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct Counts {
    pub accounts: i64,
    pub lists: i64,
    pub entries: i64,
}

#[derive(Debug, Clone)]
pub struct StoredCredentials {
    pub username: String,
    /// Fernet token, never the plaintext.
    pub token: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (creating if needed) the database at `path`, and migrate it.
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|error| error.to_string())?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, String> {
        // WAL keeps reads working while a run holds a write; foreign keys make
        // the cascades above real.
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(|error| error.to_string())?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), String> {
        self.conn
            .execute_batch(SCHEMA)
            .map_err(|error| format!("schema failed: {error}"))?;
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|error| error.to_string())
    }

    // ---- accounts ----

    pub fn counts(&self) -> Result<Counts, String> {
        let one = |sql: &str| -> Result<i64, String> {
            self.conn
                .query_row(sql, [], |row| row.get(0))
                .map_err(|error| error.to_string())
        };
        Ok(Counts {
            accounts: one("SELECT COUNT(*) FROM accounts")?,
            lists: one("SELECT COUNT(*) FROM lists")?,
            entries: one("SELECT COUNT(*) FROM list_entries")?,
        })
    }

    /// Accounts in the same JSON shape the frontend already expects.
    pub fn accounts(&self) -> Result<Vec<Value>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, number, name, denomination, cnumber, ref_number, added_in
                 FROM accounts ORDER BY number",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(json!({
                    "_id": row.get::<_, String>(0)?,
                    "Number": row.get::<_, String>(1)?,
                    "Name": row.get::<_, String>(2)?,
                    "Denomination": row.get::<_, String>(3)?,
                    "CNumber": row.get::<_, String>(4)?,
                    "Ref_Number": row.get::<_, String>(5)?,
                    "addedIn": row.get::<_, String>(6)?,
                }))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    /// Replace every account.
    pub fn replace_accounts(&self, rows: &[Value]) -> Result<usize, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM accounts", [])
            .map_err(|error| error.to_string())?;
        let now = crate::rfc3339_utc(crate::now_secs());
        let mut inserted = 0usize;
        for row in rows {
            let text = |key: &str| {
                row.get(key)
                    .map(|value| match value {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default()
            };
            let id = match text("_id") {
                id if id.is_empty() => new_id(),
                id => id,
            };
            tx.execute(
                "INSERT OR REPLACE INTO accounts
                   (id, number, name, denomination, cnumber, ref_number, added_in, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    text("Number"),
                    text("Name"),
                    text("Denomination"),
                    text("CNumber"),
                    text("Ref_Number"),
                    text("addedIn"),
                    now,
                ],
            )
            .map_err(|error| format!("account {}: {error}", text("Number")))?;
            inserted += 1;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(inserted)
    }

    /// Add one account. A duplicate number is refused rather than silently
    /// merging, because the number is the account's identity.
    pub fn add_account(&self, row: &Value) -> Result<String, String> {
        let text = |key: &str| {
            row.get(key)
                .map(|value| match value {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                })
                .unwrap_or_default()
        };
        let id = match text("_id") {
            id if id.is_empty() => new_id(),
            id => id,
        };
        self.conn
            .execute(
                "INSERT INTO accounts
                   (id, number, name, denomination, cnumber, ref_number, added_in, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    text("Number"),
                    text("Name"),
                    text("Denomination"),
                    text("CNumber"),
                    text("Ref_Number"),
                    text("addedIn"),
                    crate::rfc3339_utc(crate::now_secs()),
                ],
            )
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    format!("account number {} already exists", text("Number"))
                } else {
                    error.to_string()
                }
            })?;
        Ok(id)
    }

    /// Remove an account; its list entries go with it (ON DELETE CASCADE).
    pub fn delete_account(&self, id: &str) -> Result<bool, String> {
        let removed = self
            .conn
            .execute("DELETE FROM accounts WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        Ok(removed > 0)
    }

    // ---- lists ----

    pub fn lists(&self) -> Result<Vec<DbList>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, active FROM lists ORDER BY name")
            .map_err(|error| error.to_string())?;
        let headers = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;

        let mut entries_statement = self
            .conn
            .prepare(
                "SELECT account_id, rebate FROM list_entries
                 WHERE list_id = ?1 ORDER BY position",
            )
            .map_err(|error| error.to_string())?;

        let mut lists = Vec::with_capacity(headers.len());
        for (id, name, active) in headers {
            let entries = entries_statement
                .query_map(params![id], |row| {
                    Ok(DbListEntry {
                        id: row.get(0)?,
                        rebate: row.get(1)?,
                    })
                })
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            lists.push(DbList {
                id,
                name,
                active,
                entries,
            });
        }
        Ok(lists)
    }

    /// Upsert lists by id/name and replace their contents. Mirrors what the
    /// old writer did, so the frontend contract is unchanged.
    pub fn save_lists(&self, lists: &[InputList]) -> Result<Vec<DbList>, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        let now = crate::rfc3339_utc(crate::now_secs());

        for list in lists {
            let trimmed = list.name.trim();
            let name = if trimmed.is_empty() {
                "Unnamed"
            } else {
                trimmed
            };

            // Prefer the id; fall back to matching on the name (ids are created
            // client-side and can be missing on a first save).
            let existing: Option<String> = if !list.id.trim().is_empty() {
                tx.query_row(
                    "SELECT id FROM lists WHERE id = ?1",
                    params![list.id.trim()],
                    |row| row.get(0),
                )
                .ok()
            } else {
                tx.query_row(
                    "SELECT id FROM lists WHERE name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .ok()
            };
            let id = existing.unwrap_or_else(|| {
                if list.id.trim().is_empty() {
                    new_id()
                } else {
                    list.id.trim().to_string()
                }
            });

            tx.execute(
                "INSERT INTO lists (id, name, active, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, active = excluded.active",
                params![id, name, i64::from(list.active), now],
            )
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    format!("two lists cannot both be called {name}")
                } else {
                    error.to_string()
                }
            })?;

            tx.execute("DELETE FROM list_entries WHERE list_id = ?1", params![id])
                .map_err(|error| error.to_string())?;
            for (position, entry) in list.entries.iter().enumerate() {
                // Skip ids that are not real accounts, rather than failing the
                // whole save on one stale reference.
                let known: i64 = tx
                    .query_row(
                        "SELECT COUNT(*) FROM accounts WHERE id = ?1",
                        params![entry.id],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                if known == 0 {
                    continue;
                }
                tx.execute(
                    "INSERT OR REPLACE INTO list_entries (list_id, account_id, rebate, position)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id, entry.id, entry.rebate, position as i64],
                )
                .map_err(|error| error.to_string())?;
            }
        }

        // `active` is a single flag across lists.
        let active_id: Option<String> = tx
            .query_row("SELECT id FROM lists ORDER BY name LIMIT 1", [], |row| {
                row.get(0)
            })
            .ok();
        tx.execute("UPDATE lists SET active = 0", [])
            .map_err(|error| error.to_string())?;
        if lists.iter().any(|list| list.active) {
            for list in lists.iter().filter(|list| list.active) {
                tx.execute(
                    "UPDATE lists SET active = 1 WHERE name = ?1 OR id = ?1",
                    params![list.name.trim()],
                )
                .map_err(|error| error.to_string())?;
            }
        } else if let Some(id) = active_id {
            let _ = tx.execute("UPDATE lists SET active = 1 WHERE id = ?1", params![id]);
        }

        tx.commit().map_err(|error| error.to_string())?;
        self.lists()
    }

    /// Empty every list, keeping the lists. Returns how many entries went.
    pub fn clear_all_lists(&self) -> Result<usize, String> {
        let removed = self
            .conn
            .execute("DELETE FROM list_entries", [])
            .map_err(|error| error.to_string())?;
        Ok(removed)
    }

    // ---- credentials ----

    pub fn credentials(&self) -> Result<Option<StoredCredentials>, String> {
        let found = self
            .conn
            .query_row(
                "SELECT username, password FROM credentials WHERE id = 1",
                [],
                |row| {
                    Ok(StoredCredentials {
                        username: row.get(0)?,
                        token: row.get(1)?,
                    })
                },
            )
            .ok();
        Ok(found)
    }

    pub fn set_credentials(&self, username: &str, token: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO credentials (id, username, password, updated_at)
                 VALUES (1, ?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET
                   username = excluded.username,
                   password = excluded.password,
                   updated_at = excluded.updated_at",
                params![username, token, crate::rfc3339_utc(crate::now_secs())],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    // ---- meta ----

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    // ---- runs ----

    pub fn start_run(&self, list_names: &str) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO runs (list_names, status, started_at) VALUES (?1, 'running', ?2)",
                params![list_names, crate::rfc3339_utc(crate::now_secs())],
            )
            .map_err(|error| error.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, id: i64, status: &str, detail: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE runs SET status = ?2, detail = ?3, finished_at = ?4 WHERE id = ?1",
                params![id, status, detail, crate::rfc3339_utc(crate::now_secs())],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn recent_runs(&self, limit: i64) -> Result<Vec<Value>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, list_names, status, started_at, finished_at
                 FROM runs ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![limit], |row| {
                Ok(json!({
                    "id": row.get::<_, i64>(0)?,
                    "lists": row.get::<_, String>(1)?,
                    "status": row.get::<_, String>(2)?,
                    "started_at": row.get::<_, String>(3)?,
                    "finished_at": row.get::<_, Option<String>>(4)?,
                }))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
}

/// A 24-hex id in the same shape as a Mongo ObjectId, so ids stay interchangeable
/// with anything imported.
pub fn new_id() -> String {
    let mut bytes = [0u8; 12];
    if getrandom::getrandom(&mut bytes).is_err() {
        // Never worth failing a write over an id.
        return format!("{:024x}", std::process::id() as u128);
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{InputEntry, InputList};
    use serde_json::json;

    fn account(number: &str) -> Value {
        json!({
            "Number": number,
            "Name": format!("Holder {number}"),
            "Denomination": "2000",
            "CNumber": "CN-1",
            "Ref_Number": "REF-1",
            "addedIn": "A",
        })
    }

    fn store() -> Store {
        Store::open_in_memory().expect("in-memory store")
    }

    #[test]
    fn creates_the_schema_and_can_be_reopened() {
        let store = store();
        assert_eq!(store.counts().unwrap().accounts, 0);
        // migrate() runs on every open; running it again must not fail or wipe.
        store.migrate().unwrap();
        assert_eq!(store.counts().unwrap().accounts, 0);
    }

    #[test]
    fn replaces_accounts_and_reads_the_frontend_shape() {
        let store = store();
        let written = store
            .replace_accounts(&[account("111"), account("222")])
            .unwrap();
        assert_eq!(written, 2);

        let read = store.accounts().unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0]["Number"], json!("111"));
        assert_eq!(read[0]["Name"], json!("Holder 111"));
        assert_eq!(read[0]["Denomination"], json!("2000"));
        assert_eq!(read[0]["CNumber"], json!("CN-1"));
        assert_eq!(read[0]["Ref_Number"], json!("REF-1"));
        assert_eq!(read[0]["addedIn"], json!("A"));
        assert!(read[0]["_id"].as_str().unwrap().len() == 24);

        // Replacing is a replace, not an append.
        store.replace_accounts(&[account("333")]).unwrap();
        assert_eq!(store.accounts().unwrap().len(), 1);
    }

    #[test]
    fn keeps_the_denomination_verbatim_even_when_numeric() {
        let store = store();
        store
            .replace_accounts(&[json!({ "Number": "1", "Denomination": 2000 })])
            .unwrap();
        assert_eq!(store.accounts().unwrap()[0]["Denomination"], json!("2000"));
    }

    #[test]
    fn refuses_a_duplicate_account_number() {
        let store = store();
        store.replace_accounts(&[account("111")]).unwrap();
        assert!(store.add_account(&account("111")).is_err(), "same number");
        assert!(store.add_account(&account("222")).is_ok(), "new number");
    }

    #[test]
    fn cascades_an_account_delete_into_list_entries() {
        let store = store();
        store
            .replace_accounts(&[account("111"), account("222")])
            .unwrap();
        let ids: Vec<String> = store
            .accounts()
            .unwrap()
            .iter()
            .map(|row| row["_id"].as_str().unwrap().to_string())
            .collect();

        store
            .save_lists(&[InputList {
                id: String::new(),
                name: "A".into(),
                active: true,
                entries: vec![
                    InputEntry {
                        id: ids[0].clone(),
                        rebate: 0,
                    },
                    InputEntry {
                        id: ids[1].clone(),
                        rebate: 2,
                    },
                ],
            }])
            .unwrap();
        assert_eq!(store.counts().unwrap().entries, 2);

        assert!(store.delete_account(&ids[0]).unwrap());
        assert_eq!(
            store.counts().unwrap().entries,
            1,
            "entry went with the account"
        );
        assert!(
            !store.delete_account(&ids[0]).unwrap(),
            "second delete is a no-op"
        );
    }

    #[test]
    fn round_trips_lists_with_order_and_rebates() {
        let store = store();
        store
            .replace_accounts(&[account("111"), account("222"), account("333")])
            .unwrap();
        let ids: Vec<String> = store
            .accounts()
            .unwrap()
            .iter()
            .map(|row| row["_id"].as_str().unwrap().to_string())
            .collect();

        let saved = store
            .save_lists(&[InputList {
                id: "client-side-id".into(),
                name: "B".into(),
                active: true,
                entries: vec![
                    InputEntry {
                        id: ids[2].clone(),
                        rebate: 1,
                    },
                    InputEntry {
                        id: ids[0].clone(),
                        rebate: 0,
                    },
                ],
            }])
            .unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "B");
        assert!(saved[0].active);
        // Order is the payload's order, and 1 survives — it means "skip the
        // rebate step" in scraper.py, not "unset".
        assert_eq!(saved[0].entries[0].id, ids[2]);
        assert_eq!(saved[0].entries[0].rebate, 1);
        assert_eq!(saved[0].entries[1].id, ids[0]);

        // Saving again replaces rather than duplicating.
        let again = store
            .save_lists(&[InputList {
                id: "client-side-id".into(),
                name: "B".into(),
                active: true,
                entries: vec![InputEntry {
                    id: ids[1].clone(),
                    rebate: 3,
                }],
            }])
            .unwrap();
        assert_eq!(again[0].entries.len(), 1);
        assert_eq!(again[0].entries[0].rebate, 3);
        assert_eq!(store.counts().unwrap().entries, 1);
    }

    #[test]
    fn an_upsert_by_name_does_not_create_a_second_row() {
        let store = store();
        let first = store
            .save_lists(&[InputList {
                id: String::new(),
                name: "A".into(),
                active: true,
                entries: vec![],
            }])
            .unwrap();
        let second = store
            .save_lists(&[InputList {
                id: String::new(),
                name: "A".into(),
                active: true,
                entries: vec![],
            }])
            .unwrap();
        assert_eq!(second.len(), 1, "one list, not two");
        assert_eq!(second[0].id, first[0].id, "id is stable across saves");
    }

    #[test]
    fn only_one_list_is_active() {
        let store = store();
        store
            .save_lists(&[
                InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: false,
                    entries: vec![],
                },
                InputList {
                    id: String::new(),
                    name: "B".into(),
                    active: true,
                    entries: vec![],
                },
            ])
            .unwrap();
        let active: Vec<String> = store
            .lists()
            .unwrap()
            .into_iter()
            .filter(|list| list.active)
            .map(|list| list.name)
            .collect();
        assert_eq!(active, vec!["B"]);
    }

    #[test]
    fn skips_entries_that_point_at_no_account() {
        let store = store();
        store.replace_accounts(&[account("111")]).unwrap();
        let real = store.accounts().unwrap()[0]["_id"]
            .as_str()
            .unwrap()
            .to_string();
        let saved = store
            .save_lists(&[InputList {
                id: String::new(),
                name: "A".into(),
                active: true,
                entries: vec![
                    InputEntry {
                        id: "deadbeefdeadbeefdeadbeef".into(),
                        rebate: 0,
                    },
                    InputEntry {
                        id: real.clone(),
                        rebate: 0,
                    },
                ],
            }])
            .unwrap();
        assert_eq!(
            saved[0].entries.len(),
            1,
            "stale reference dropped, save still succeeds"
        );
        assert_eq!(saved[0].entries[0].id, real);
    }

    #[test]
    fn clears_every_list_but_keeps_the_lists() {
        let store = store();
        store
            .replace_accounts(&[account("111"), account("222")])
            .unwrap();
        let ids: Vec<String> = store
            .accounts()
            .unwrap()
            .iter()
            .map(|row| row["_id"].as_str().unwrap().to_string())
            .collect();
        store
            .save_lists(&[
                InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: true,
                    entries: vec![InputEntry {
                        id: ids[0].clone(),
                        rebate: 0,
                    }],
                },
                InputList {
                    id: String::new(),
                    name: "B".into(),
                    active: false,
                    entries: vec![InputEntry {
                        id: ids[1].clone(),
                        rebate: 0,
                    }],
                },
            ])
            .unwrap();

        assert_eq!(store.clear_all_lists().unwrap(), 2);
        let counts = store.counts().unwrap();
        assert_eq!(counts.entries, 0);
        assert_eq!(counts.lists, 2, "lists themselves stay");
        assert_eq!(counts.accounts, 2, "accounts are untouched");
    }

    #[test]
    fn round_trips_the_stored_credential() {
        let store = store();
        assert!(store.credentials().unwrap().is_none());

        store
            .set_credentials("DOP.MI123", "gAAAAABfake-token")
            .unwrap();
        let stored = store.credentials().unwrap().unwrap();
        assert_eq!(stored.username, "DOP.MI123");
        assert_eq!(
            stored.token, "gAAAAABfake-token",
            "token is stored verbatim"
        );

        // Overwriting keeps a single row.
        store.set_credentials("DOP.MI999", "gAAAAABother").unwrap();
        let stored = store.credentials().unwrap().unwrap();
        assert_eq!(stored.username, "DOP.MI999");
        assert_eq!(store.counts().unwrap().accounts, 0);
    }

    #[test]
    fn records_a_run_from_start_to_finish() {
        let store = store();
        let id = store.start_run("A, B").unwrap();
        let running = store.recent_runs(10).unwrap();
        assert_eq!(running[0]["status"], json!("running"));
        assert_eq!(running[0]["finished_at"], Value::Null);

        store.finish_run(id, "ok", "2 accounts paid").unwrap();
        let done = store.recent_runs(10).unwrap();
        assert_eq!(done[0]["status"], json!("ok"));
        assert_eq!(done[0]["lists"], json!("A, B"));
        assert!(done[0]["finished_at"].is_string());
    }

    #[test]
    fn remembers_meta_across_reopen() {
        let store = store();
        assert!(store.meta("atlas_imported_at").is_none());
        store
            .set_meta("atlas_imported_at", "2026-10-01T00:00:00Z")
            .unwrap();
        assert_eq!(
            store.meta("atlas_imported_at"),
            Some("2026-10-01T00:00:00Z".to_string())
        );
        // Overwriting, not duplicating.
        store
            .set_meta("atlas_imported_at", "2026-10-02T00:00:00Z")
            .unwrap();
        assert_eq!(
            store.meta("atlas_imported_at"),
            Some("2026-10-02T00:00:00Z".to_string())
        );
    }

    #[test]
    fn generates_objectid_shaped_ids() {
        let id = new_id();
        assert_eq!(id.len(), 24);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "got {id}");
        assert_ne!(id, new_id());
    }
}
