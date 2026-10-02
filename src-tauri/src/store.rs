//! Local SQLite store — the app's own database, one file on this machine.
//!
//! Everything belongs to an **owner**: a person identified by their DOP id
//! (a mobile number), who signs in with their own login password. Accounts,
//! lists, the DOP credential and the run history are all per-owner; several
//! owners can share one install without seeing each other's data.
//!
//! Takes a path rather than an `AppHandle` on purpose, so the whole schema and
//! every round trip can be exercised in tests against an in-memory database.

use crate::db::{DbList, DbListEntry, InputList};
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;

/// Bump when the schema changes; [`Store::migrate`] applies what is missing.
const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS owners (
    id           TEXT PRIMARY KEY,
    username     TEXT NOT NULL UNIQUE,
    login_hash   TEXT NOT NULL,
    kdf_salt     TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    last_used_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS accounts (
    id           TEXT PRIMARY KEY,
    owner_id     TEXT NOT NULL REFERENCES owners(id) ON DELETE CASCADE,
    number       TEXT NOT NULL,
    name         TEXT NOT NULL DEFAULT '',
    -- TEXT on purpose: the legacy Atlas import held this as both int32 and
    -- string, and the frontend treats it as a string. Storing it verbatim
    -- loses nothing.
    denomination TEXT NOT NULL DEFAULT '',
    cnumber      TEXT NOT NULL DEFAULT '',
    ref_number   TEXT NOT NULL DEFAULT '',
    added_in     TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS accounts_owner_number ON accounts(owner_id, number);

CREATE TABLE IF NOT EXISTS lists (
    id         TEXT PRIMARY KEY,
    owner_id   TEXT NOT NULL REFERENCES owners(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    active     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS lists_owner_name ON lists(owner_id, name);

CREATE TABLE IF NOT EXISTS list_entries (
    list_id    TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    rebate     INTEGER NOT NULL DEFAULT 0,
    position   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (list_id, account_id)
);
CREATE INDEX IF NOT EXISTS list_entries_account ON list_entries(account_id);

-- One row per owner: the DOP portal pair. The password is a Fernet token.
CREATE TABLE IF NOT EXISTS credentials (
    owner_id   TEXT PRIMARY KEY REFERENCES owners(id) ON DELETE CASCADE,
    username   TEXT NOT NULL,
    password   TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- One row per Generate run, so what was paid can be answered later.
CREATE TABLE IF NOT EXISTS runs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_id    TEXT,
    list_names  TEXT NOT NULL,
    status      TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    finished_at TEXT,
    detail      TEXT
);

-- Small key/value corner: which schema version wrote this, and anything
-- else that must survive a restart. Auth lives in `owners`, not here.
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

/// One signed-in identity: who, and what verifies their password.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct OwnerRow {
    pub id: String,
    pub username: String,
    pub has_credential: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OwnerAuth {
    pub username: String,
    pub login_hash: String,
    pub kdf_salt: String,
}

pub struct Store {
    conn: Connection,
}

fn now() -> String {
    crate::rfc3339_utc(crate::now_secs())
}

/// Does `table` already carry `column`? Drives the v1 → v2 migration.
fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let found = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .any(|name| name == column);
    Ok(found)
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
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        // v1 databases predate owners: their tables lack owner_id and their
        // single login lives in `meta`. Bring the whole file across in one
        // transaction before the v2 schema touches anything.
        let table_exists = |name: &str| -> Result<bool, String> {
            let count: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![name],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?;
            Ok(count > 0)
        };
        if version < 2
            && table_exists("accounts")?
            && !has_column(&self.conn, "accounts", "owner_id")?
        {
            self.migrate_v1_to_v2()?;
        }
        self.conn
            .execute_batch(SCHEMA)
            .map_err(|error| format!("schema failed: {error}"))?;
        self.conn
            .pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|error| error.to_string())
    }

    /// Move a single-user v1 database under a synthetic `default` owner. The
    /// owner's username is the DOP portal id the credential was saved with.
    fn migrate_v1_to_v2(&self) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let legacy_username: Option<String> = tx
            .query_row("SELECT username FROM credentials WHERE id = 1", [], |row| {
                row.get(0)
            })
            .ok();
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS owners_v1 (
                id           TEXT PRIMARY KEY,
                username     TEXT NOT NULL UNIQUE,
                login_hash   TEXT NOT NULL,
                kdf_salt     TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                last_used_at TEXT NOT NULL
            );",
        )
        .map_err(|error| error.to_string())?;
        let login_hash: String = tx
            .query_row(
                "SELECT value FROM meta WHERE key = 'login_hash'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_default();
        let kdf_salt: String = tx
            .query_row("SELECT value FROM meta WHERE key = 'kdf_salt'", [], |row| {
                row.get(0)
            })
            .unwrap_or_default();
        tx.execute(
            "INSERT OR IGNORE INTO owners_v1 (id, username, login_hash, kdf_salt, created_at, last_used_at)
             VALUES ('default', ?1, ?2, ?3, ?4, ?4)",
            params![
                legacy_username.as_deref().unwrap_or("owner"),
                login_hash,
                kdf_salt,
                now()
            ],
        )
        .map_err(|error| error.to_string())?;
        tx.execute_batch(
            "ALTER TABLE accounts ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'default';
             ALTER TABLE lists ADD COLUMN owner_id TEXT NOT NULL DEFAULT 'default';
             ALTER TABLE runs ADD COLUMN owner_id TEXT;
             DROP INDEX IF EXISTS accounts_number;
             DROP INDEX IF EXISTS lists_name;
             CREATE UNIQUE INDEX accounts_owner_number ON accounts(owner_id, number);
             CREATE UNIQUE INDEX lists_owner_name ON lists(owner_id, name);",
        )
        .map_err(|error| error.to_string())?;
        // `credentials` is keyed by a constant 1 in v1; rebuild it owner-keyed.
        tx.execute_batch(
            "CREATE TABLE credentials_v2 (
                owner_id   TEXT PRIMARY KEY,
                username   TEXT NOT NULL,
                password   TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
             INSERT OR REPLACE INTO credentials_v2 (owner_id, username, password, updated_at)
               SELECT 'default', username, password, updated_at FROM credentials WHERE id = 1;
             DROP TABLE credentials;
             ALTER TABLE credentials_v2 RENAME TO credentials;",
        )
        .map_err(|error| error.to_string())?;
        // Auth lives in `owners` from here on.
        tx.execute(
            "DELETE FROM meta WHERE key IN ('login_hash', 'kdf_salt')",
            [],
        )
        .map_err(|error| error.to_string())?;
        // Hold the migrated owners under its real name once the old tables are
        // gone (SQLite needs the name free before RENAME).
        tx.execute_batch("ALTER TABLE owners_v1 RENAME TO owners;")
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())
    }

    // ---- owners ----

    /// Every owner, newest activity last.
    pub fn owners(&self) -> Result<Vec<OwnerRow>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT o.id, o.username,
                        (SELECT COUNT(*) FROM credentials c WHERE c.owner_id = o.id)
                 FROM owners o ORDER BY o.last_used_at",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(OwnerRow {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    has_credential: row.get::<_, i64>(2)? > 0,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn has_owners(&self) -> Result<bool, String> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM owners", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        Ok(count > 0)
    }

    /// Register a new owner with their own login. The username is the DOP
    /// portal id (a mobile number) and must be unique on this machine.
    pub fn create_owner(
        &self,
        username: &str,
        login_hash: &str,
        kdf_salt: &str,
    ) -> Result<String, String> {
        let id = new_id();
        self.conn
            .execute(
                "INSERT INTO owners (id, username, login_hash, kdf_salt, created_at, last_used_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id, username, login_hash, kdf_salt, now()],
            )
            .map_err(|error| {
                if error.to_string().contains("UNIQUE") {
                    format!("{username} already has a workspace on this machine")
                } else {
                    error.to_string()
                }
            })?;
        Ok(id)
    }

    /// The login material for one owner, when the owner exists.
    pub fn owner_auth(&self, owner_id: &str) -> Result<Option<OwnerAuth>, String> {
        let found = self
            .conn
            .query_row(
                "SELECT username, login_hash, kdf_salt FROM owners WHERE id = ?1",
                params![owner_id],
                |row| {
                    Ok(OwnerAuth {
                        username: row.get(0)?,
                        login_hash: row.get(1)?,
                        kdf_salt: row.get(2)?,
                    })
                },
            )
            .ok();
        Ok(found)
    }

    /// Re-key one owner's login (password change).
    pub fn set_owner_auth(
        &self,
        owner_id: &str,
        login_hash: &str,
        kdf_salt: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE owners SET login_hash = ?2, kdf_salt = ?3 WHERE id = ?1",
                params![owner_id, login_hash, kdf_salt],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn touch_owner(&self, owner_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE owners SET last_used_at = ?2 WHERE id = ?1",
                params![owner_id, now()],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    // ---- accounts ----

    pub fn counts(&self, owner_id: &str) -> Result<Counts, String> {
        let one = |sql: &str| -> Result<i64, String> {
            self.conn
                .query_row(sql, params![owner_id], |row| row.get(0))
                .map_err(|error| error.to_string())
        };
        Ok(Counts {
            accounts: one("SELECT COUNT(*) FROM accounts WHERE owner_id = ?1")?,
            lists: one("SELECT COUNT(*) FROM lists WHERE owner_id = ?1")?,
            entries: one("SELECT COUNT(*) FROM list_entries e
                 JOIN lists l ON l.id = e.list_id WHERE l.owner_id = ?1")?,
        })
    }

    /// Every row in the file, across owners — what a whole-file backup holds.
    pub fn counts_all(&self) -> Result<Counts, String> {
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
    pub fn accounts(&self, owner_id: &str) -> Result<Vec<Value>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, number, name, denomination, cnumber, ref_number, added_in
                 FROM accounts WHERE owner_id = ?1 ORDER BY number",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![owner_id], |row| {
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

    /// Replace this owner's accounts with `rows`. Other owners are untouched.
    pub fn replace_accounts(&self, owner_id: &str, rows: &[Value]) -> Result<usize, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM accounts WHERE owner_id = ?1",
            params![owner_id],
        )
        .map_err(|error| error.to_string())?;
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
                   (id, owner_id, number, name, denomination, cnumber, ref_number, added_in, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    owner_id,
                    text("Number"),
                    text("Name"),
                    text("Denomination"),
                    text("CNumber"),
                    text("Ref_Number"),
                    text("addedIn"),
                    now(),
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
    pub fn add_account(&self, owner_id: &str, row: &Value) -> Result<String, String> {
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
                   (id, owner_id, number, name, denomination, cnumber, ref_number, added_in, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    owner_id,
                    text("Number"),
                    text("Name"),
                    text("Denomination"),
                    text("CNumber"),
                    text("Ref_Number"),
                    text("addedIn"),
                    now(),
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
    pub fn delete_account(&self, owner_id: &str, id: &str) -> Result<bool, String> {
        let removed = self
            .conn
            .execute(
                "DELETE FROM accounts WHERE id = ?1 AND owner_id = ?2",
                params![id, owner_id],
            )
            .map_err(|error| error.to_string())?;
        Ok(removed > 0)
    }

    // ---- lists ----

    pub fn lists(&self, owner_id: &str) -> Result<Vec<DbList>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, active FROM lists WHERE owner_id = ?1 ORDER BY name")
            .map_err(|error| error.to_string())?;
        let headers = statement
            .query_map(params![owner_id], |row| {
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
    pub fn save_lists(&self, owner_id: &str, lists: &[InputList]) -> Result<Vec<DbList>, String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;

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
                    "SELECT id FROM lists WHERE id = ?1 AND owner_id = ?2",
                    params![list.id.trim(), owner_id],
                    |row| row.get(0),
                )
                .ok()
            } else {
                tx.query_row(
                    "SELECT id FROM lists WHERE name = ?1 AND owner_id = ?2",
                    params![name, owner_id],
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
                "INSERT INTO lists (id, owner_id, name, active, created_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, active = excluded.active",
                params![id, owner_id, name, i64::from(list.active), now()],
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
                        "SELECT COUNT(*) FROM accounts WHERE id = ?1 AND owner_id = ?2",
                        params![entry.id, owner_id],
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

        // `active` is a single flag across this owner's lists.
        let active_id: Option<String> = tx
            .query_row(
                "SELECT id FROM lists WHERE owner_id = ?1 ORDER BY name LIMIT 1",
                params![owner_id],
                |row| row.get(0),
            )
            .ok();
        tx.execute(
            "UPDATE lists SET active = 0 WHERE owner_id = ?1",
            params![owner_id],
        )
        .map_err(|error| error.to_string())?;
        if lists.iter().any(|list| list.active) {
            for list in lists.iter().filter(|list| list.active) {
                tx.execute(
                    "UPDATE lists SET active = 1 WHERE owner_id = ?1 AND (name = ?2 OR id = ?2)",
                    params![owner_id, list.name.trim()],
                )
                .map_err(|error| error.to_string())?;
            }
        } else if let Some(id) = active_id {
            let _ = tx.execute(
                "UPDATE lists SET active = 1 WHERE id = ?1 AND owner_id = ?2",
                params![id, owner_id],
            );
        }

        tx.commit().map_err(|error| error.to_string())?;
        self.lists(owner_id)
    }

    /// Empty this owner's lists, keeping the lists. Returns how many entries went.
    pub fn clear_all_lists(&self, owner_id: &str) -> Result<usize, String> {
        let removed = self
            .conn
            .execute(
                "DELETE FROM list_entries WHERE list_id IN
                   (SELECT id FROM lists WHERE owner_id = ?1)",
                params![owner_id],
            )
            .map_err(|error| error.to_string())?;
        Ok(removed)
    }

    // ---- credentials ----

    pub fn credentials(&self, owner_id: &str) -> Result<Option<StoredCredentials>, String> {
        let found = self
            .conn
            .query_row(
                "SELECT username, password FROM credentials WHERE owner_id = ?1",
                params![owner_id],
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

    pub fn set_credentials(
        &self,
        owner_id: &str,
        username: &str,
        token: &str,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO credentials (owner_id, username, password, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(owner_id) DO UPDATE SET
                   username = excluded.username,
                   password = excluded.password,
                   updated_at = excluded.updated_at",
                params![owner_id, username, token, now()],
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

    pub fn start_run(&self, owner_id: &str, list_names: &str) -> Result<i64, String> {
        self.conn
            .execute(
                "INSERT INTO runs (owner_id, list_names, status, started_at)
                 VALUES (?1, ?2, 'running', ?3)",
                params![owner_id, list_names, now()],
            )
            .map_err(|error| error.to_string())?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_run(&self, id: i64, status: &str, detail: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE runs SET status = ?2, detail = ?3, finished_at = ?4 WHERE id = ?1",
                params![id, status, detail, now()],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn recent_runs(&self, owner_id: &str, limit: i64) -> Result<Vec<Value>, String> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT id, list_names, status, started_at, finished_at
                 FROM runs WHERE owner_id = ?1 ORDER BY id DESC LIMIT ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map(params![owner_id, limit], |row| {
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

    const OWNER: &str = "default";

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

    /// Register a second owner for isolation checks.
    fn second_owner(store: &Store) -> String {
        store
            .create_owner("9876543210", "phc-other", "salt-other")
            .unwrap()
    }

    #[test]
    fn creates_the_schema_and_can_be_reopened() {
        let store = store();
        assert!(!store.has_owners().unwrap());
        // migrate() runs on every open; running it again must not fail or wipe.
        store.migrate().unwrap();
        assert!(!store.has_owners().unwrap());
    }

    #[test]
    fn a_v1_database_migrates_under_a_default_owner() {
        // Build a v1-shaped database by hand.
        let dir = std::env::temp_dir().join(format!("autodop-v1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v1.db");
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA user_version = 1;
             CREATE TABLE accounts (id TEXT PRIMARY KEY, number TEXT NOT NULL, name TEXT NOT NULL DEFAULT '',
               denomination TEXT NOT NULL DEFAULT '', cnumber TEXT NOT NULL DEFAULT '',
               ref_number TEXT NOT NULL DEFAULT '', added_in TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL);
             CREATE UNIQUE INDEX accounts_number ON accounts(number);
             CREATE TABLE lists (id TEXT PRIMARY KEY, name TEXT NOT NULL, active INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL);
             CREATE UNIQUE INDEX lists_name ON lists(name);
             CREATE TABLE list_entries (list_id TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
               account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, rebate INTEGER NOT NULL DEFAULT 0,
               position INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (list_id, account_id));
             CREATE TABLE credentials (id INTEGER PRIMARY KEY CHECK (id = 1), username TEXT NOT NULL,
               password TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE runs (id INTEGER PRIMARY KEY AUTOINCREMENT, list_names TEXT NOT NULL,
               status TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT, detail TEXT);
             CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO accounts (id, number, name, created_at) VALUES ('a1', '111', 'One', 't');
             INSERT INTO lists (id, name, active, created_at) VALUES ('l1', 'A', 1, 't');
             INSERT INTO credentials (id, username, password, updated_at) VALUES (1, 'DOP.MI777', 'tok', 't');
             INSERT INTO meta (key, value) VALUES ('login_hash', '$argon2id$legacy');
             INSERT INTO meta (key, value) VALUES ('kdf_salt', 'legacy-salt');",
        )
        .unwrap();
        drop(conn);

        let store = Store::open(&path).expect("migration succeeds");
        let owners = store.owners().unwrap();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].username, "DOP.MI777");
        assert_eq!(owners[0].id, "default");

        let auth = store.owner_auth("default").unwrap().unwrap();
        assert_eq!(auth.login_hash, "$argon2id$legacy");
        assert_eq!(auth.kdf_salt, "legacy-salt");

        // Data came across and stays out of meta.
        assert_eq!(store.counts("default").unwrap().accounts, 1);
        assert_eq!(store.lists("default").unwrap().len(), 1);
        assert!(store.credentials("default").unwrap().unwrap().username == "DOP.MI777");
        assert!(store.meta("login_hash").is_none());
        assert!(store.meta("kdf_salt").is_none());

        // Reopening does not migrate again or lose anything.
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.counts("default").unwrap().accounts, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn owners_are_isolated() {
        let store = store();
        let mine = store
            .create_owner("9999999999", "phc-mine", "salt-mine")
            .unwrap();
        let other = second_owner(&store);

        store.replace_accounts(&mine, &[account("111")]).unwrap();
        store.replace_accounts(&other, &[account("222")]).unwrap();
        assert_eq!(store.counts(&mine).unwrap().accounts, 1);
        assert_eq!(store.counts(&other).unwrap().accounts, 1);
        assert_eq!(store.accounts(&mine).unwrap()[0]["Number"], json!("111"));

        // The same list name can exist under both owners.
        store
            .save_lists(
                &mine,
                &[InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: true,
                    entries: vec![],
                }],
            )
            .unwrap();
        store
            .save_lists(
                &other,
                &[InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: true,
                    entries: vec![],
                }],
            )
            .unwrap();
        assert_eq!(store.lists(&mine).unwrap().len(), 1);
        assert_eq!(store.lists(&other).unwrap().len(), 1);

        // Credentials are per-owner too.
        store.set_credentials(&mine, "DOP.MI1", "tok1").unwrap();
        store.set_credentials(&other, "DOP.MI2", "tok2").unwrap();
        assert_eq!(
            store.credentials(&mine).unwrap().unwrap().username,
            "DOP.MI1"
        );
        assert_eq!(
            store.credentials(&other).unwrap().unwrap().username,
            "DOP.MI2"
        );

        // One owner's replace does not touch the other's accounts.
        store.replace_accounts(&mine, &[account("333")]).unwrap();
        assert_eq!(store.counts(&mine).unwrap().accounts, 1);
        assert_eq!(store.counts(&other).unwrap().accounts, 1);
        assert_eq!(store.accounts(&other).unwrap()[0]["Number"], json!("222"));
    }

    #[test]
    fn a_duplicated_username_is_refused() {
        let store = store();
        store.create_owner("9999999999", "h", "s").unwrap();
        let error = store
            .create_owner("9999999999", "h", "s")
            .expect_err("username is taken");
        assert!(error.contains("already has a workspace"), "{error}");
    }

    #[test]
    fn replaces_accounts_and_reads_the_frontend_shape() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        let written = store
            .replace_accounts(&owner, &[account("111"), account("222")])
            .unwrap();
        assert_eq!(written, 2);

        let read = store.accounts(&owner).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0]["Number"], json!("111"));
        assert_eq!(read[0]["Name"], json!("Holder 111"));
        assert_eq!(read[0]["Denomination"], json!("2000"));
        assert_eq!(read[0]["CNumber"], json!("CN-1"));
        assert_eq!(read[0]["Ref_Number"], json!("REF-1"));
        assert_eq!(read[0]["addedIn"], json!("A"));
        assert!(read[0]["_id"].as_str().unwrap().len() == 24);

        // Replacing is a replace, not an append.
        store.replace_accounts(&owner, &[account("333")]).unwrap();
        assert_eq!(store.accounts(&owner).unwrap().len(), 1);
    }

    #[test]
    fn keeps_the_denomination_verbatim_even_when_numeric() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store
            .replace_accounts(&owner, &[json!({ "Number": "1", "Denomination": 2000 })])
            .unwrap();
        assert_eq!(
            store.accounts(&owner).unwrap()[0]["Denomination"],
            json!("2000")
        );
    }

    #[test]
    fn refuses_a_duplicate_account_number() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store.replace_accounts(&owner, &[account("111")]).unwrap();
        assert!(
            store.add_account(&owner, &account("111")).is_err(),
            "same number"
        );
        assert!(
            store.add_account(&owner, &account("222")).is_ok(),
            "new number"
        );
        // The same number under another owner is fine.
        let other = second_owner(&store);
        assert!(store.add_account(&other, &account("111")).is_ok());
    }

    #[test]
    fn cascades_an_account_delete_into_list_entries() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store
            .replace_accounts(&owner, &[account("111"), account("222")])
            .unwrap();
        let ids: Vec<String> = store
            .accounts(&owner)
            .unwrap()
            .iter()
            .map(|row| row["_id"].as_str().unwrap().to_string())
            .collect();

        store
            .save_lists(
                &owner,
                &[InputList {
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
                }],
            )
            .unwrap();
        assert_eq!(store.counts(&owner).unwrap().entries, 2);

        assert!(store.delete_account(&owner, &ids[0]).unwrap());
        assert_eq!(
            store.counts(&owner).unwrap().entries,
            1,
            "entry went with the account"
        );
        assert!(
            !store.delete_account(&owner, &ids[0]).unwrap(),
            "second delete is a no-op"
        );
    }

    #[test]
    fn round_trips_lists_with_order_and_rebates() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store
            .replace_accounts(&owner, &[account("111"), account("222"), account("333")])
            .unwrap();
        let ids: Vec<String> = store
            .accounts(&owner)
            .unwrap()
            .iter()
            .map(|row| row["_id"].as_str().unwrap().to_string())
            .collect();

        let saved = store
            .save_lists(
                &owner,
                &[InputList {
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
                }],
            )
            .unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "B");
        assert!(saved[0].active);
        assert_eq!(saved[0].entries[0].id, ids[2]);
        assert_eq!(saved[0].entries[0].rebate, 1);
        assert_eq!(saved[0].entries[1].id, ids[0]);

        let again = store
            .save_lists(
                &owner,
                &[InputList {
                    id: "client-side-id".into(),
                    name: "B".into(),
                    active: true,
                    entries: vec![InputEntry {
                        id: ids[1].clone(),
                        rebate: 3,
                    }],
                }],
            )
            .unwrap();
        assert_eq!(again[0].entries.len(), 1);
        assert_eq!(again[0].entries[0].rebate, 3);
        assert_eq!(store.counts(&owner).unwrap().entries, 1);
    }

    #[test]
    fn an_upsert_by_name_does_not_create_a_second_row() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        let first = store
            .save_lists(
                &owner,
                &[InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: true,
                    entries: vec![],
                }],
            )
            .unwrap();
        let second = store
            .save_lists(
                &owner,
                &[InputList {
                    id: String::new(),
                    name: "A".into(),
                    active: true,
                    entries: vec![],
                }],
            )
            .unwrap();
        assert_eq!(second.len(), 1, "one list, not two");
        assert_eq!(second[0].id, first[0].id, "id is stable across saves");
    }

    #[test]
    fn only_one_list_is_active() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store
            .save_lists(
                &owner,
                &[
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
                ],
            )
            .unwrap();
        let active: Vec<String> = store
            .lists(&owner)
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
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store.replace_accounts(&owner, &[account("111")]).unwrap();
        let real = store.accounts(&owner).unwrap()[0]["_id"]
            .as_str()
            .unwrap()
            .to_string();
        let saved = store
            .save_lists(
                &owner,
                &[InputList {
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
                }],
            )
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
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        store
            .replace_accounts(&owner, &[account("111"), account("222")])
            .unwrap();
        let ids: Vec<String> = store
            .accounts(&owner)
            .unwrap()
            .iter()
            .map(|row| row["_id"].as_str().unwrap().to_string())
            .collect();
        store
            .save_lists(
                &owner,
                &[
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
                ],
            )
            .unwrap();

        assert_eq!(store.clear_all_lists(&owner).unwrap(), 2);
        let counts = store.counts(&owner).unwrap();
        assert_eq!(counts.entries, 0);
        assert_eq!(counts.lists, 2, "lists themselves stay");
        assert_eq!(counts.accounts, 2, "accounts are untouched");
    }

    #[test]
    fn round_trips_the_stored_credential() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        assert!(store.credentials(&owner).unwrap().is_none());

        store
            .set_credentials(&owner, "DOP.MI123", "gAAAAABfake-token")
            .unwrap();
        let stored = store.credentials(&owner).unwrap().unwrap();
        assert_eq!(stored.username, "DOP.MI123");
        assert_eq!(
            stored.token, "gAAAAABfake-token",
            "token is stored verbatim"
        );

        // Overwriting keeps a single row.
        store
            .set_credentials(&owner, "DOP.MI999", "gAAAAABother")
            .unwrap();
        let stored = store.credentials(&owner).unwrap().unwrap();
        assert_eq!(stored.username, "DOP.MI999");
        assert_eq!(store.counts(&owner).unwrap().accounts, 0);
    }

    #[test]
    fn records_a_run_from_start_to_finish() {
        let store = store();
        let owner = store.create_owner("9999999999", "h", "s").unwrap();
        let id = store.start_run(&owner, "A, B").unwrap();
        let running = store.recent_runs(&owner, 10).unwrap();
        assert_eq!(running[0]["status"], json!("running"));
        assert_eq!(running[0]["finished_at"], Value::Null);

        store.finish_run(id, "ok", "2 accounts paid").unwrap();
        let done = store.recent_runs(&owner, 10).unwrap();
        assert_eq!(done[0]["status"], json!("ok"));
        assert_eq!(done[0]["lists"], json!("A, B"));
        assert!(done[0]["finished_at"].is_string());

        // Runs are owner-scoped.
        let other = second_owner(&store);
        assert!(store.recent_runs(&other, 10).unwrap().is_empty());
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
