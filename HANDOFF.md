# AutoDOP — session handoff

Context for continuing work. Delete this file once it stops being useful.

## What this is

A desktop app replacing a Python/Streamlit tool that drives the **India Post DOP
portal** through Selenium: it keeps account holders, groups them into lists
(A–Z), and pays RD installments by running a scraper against the real portal.

- Repo: `/Users/aravind/repos/AutoDOP`, branch `feat/astro-frontend-spec`
- Stack: Astro + React + Tailwind frontend, **Tauri v2** desktop shell, **Rust**
  backend, one **SQLite** file per install
- Current version: **0.4.0** · latest commit `9861e70`
- `main.py`, `scraper.py` are the original app and are **left untouched**

## Where things stand

Data lives in one SQLite file, created on first run:

```
~/Library/Application Support/in.aravind.autodop/autodop.db
```

| Table | Holds |
| --- | --- |
| `accounts` | one row per holder; `number` unique |
| `lists` | A–Z, one flagged `active` |
| `list_entries` | membership, ordered, with the rebate (cascades from both) |
| `credentials` | DOP id + a Fernet token |
| `runs` | one row per Generate: what was asked, how it ended |
| `meta` | schema version, `atlas_imported_at`, `login_hash`, `kdf_salt` |

Verified state on this machine: **148 accounts, 26 lists, 0 entries, 1 credential**.
The 26 lists are empty shells in the source data, so there is no membership or
rebate data — only the account master and the password.

## What changed in this session

| Commit | What |
| --- | --- |
| `91de4f7` | Generate All Lists (one run, all lists); rebate default `0` + editable per account; scraper stderr surfaced + per-run log files |
| `da70b84` | Rotate the DOP password from the app; bundle `scraper.py` as a Tauri resource |
| `19daa69` | Clear all lists at once; fixed the count badge hiding under the delete ✕ |
| `5113d06` | Version + build stamp visible in the app; PyInstaller sidecar; a `.dmg` step that actually builds |
| `8de521f`, `992267c` | Fixed the stamp lagging a commit behind; scoped `+dirty` |
| `37ef972` | Read-only Atlas schema dump (`cargo run --example db_schema`) |
| `a3ef3de`, `f765304` | **SQLite became the store**; Atlas reduced to a one-time import; 0.3.0 |
| `ed9c500` | **DOP password encrypted with a key derived from the login password**; 0.4.0 |
| `9861e70` | rustfmt |

## How it works now

**Storage.** SQLite, one file per machine, nothing shared. Atlas is read in only
two places: the one-time import, and a credential fallback that exists until a
local password is saved. **Nothing is ever written to Atlas.**

**Credentials.** The DOP password is encrypted with a key derived from the login
password via **Argon2id** over a stored salt. The key is never written to disk —
it lives in memory only while signed in. Deliberately *not* a bare SHA-256: that
is fast, and the threat model is someone holding the database file. Resolution
order: env → local database → legacy `credentials.json` → Atlas.

**Login.** Verified in Rust (Argon2id PHC string in `meta`). The webview no
longer stores any password hash. Changing the password **re-encrypts** the stored
DOP password in the same step — without that the credential would be
unrecoverable. Forgetting the password means re-entering the DOP password, not
losing data.

**The runner.** `scraper.py` is bundled, and a PyInstaller sidecar
(`src-tauri/binaries/scraper-<os>-<arch>`) is preferred when present, so a
shipped app needs no Python. Chrome is still required. Order: chosen → env →
sidecar → bundled `.py` → repo → cwd. Build it with `npm run build:sidecar` (per
OS — PyInstaller cannot cross-compile).

**Builds.** `npm run build` = `scripts/build.mjs`: Tauri builds the `.app`, then
the DMG is made by staging a pristine copy in the temp dir and one
`hdiutil create`, then **mounted read-only and checked** before success is
reported. Tauri's own create-dmg step is bypassed on purpose — it writes its
scratch image into the folder it is about to copy, so it nests and fails
misleadingly. `npm run build:tauri` keeps the old path.

**Which build is running.** `build.rs` stamps each build with the commit and the
build time; it shows at the bottom of the list panel, on the sign-in screen, and
as "This build" in Manage. `npm run version:bump -- 0.4.0` updates all four
version fields. `cargo run --example build_info` prints it from a build.

## Verification status

Proven:

- `cargo test` **63/63**, no warnings; `astro check` 0/0/0; `npm run smoke` passes
- The live import: 148 accounts, 26 lists, credential, via
  `cargo run --example import_probe`
- The **upgrade path ran for real** on this machine: `key` file deleted,
  `login_hash` + `kdf_salt` written, credential intact
- The sidecar runs with no Python on the system (venv hidden, empty environment)
- The DMG mounts and contains the app, the script and the runner

Not proven:

- **Generate has never been run end-to-end against the DOP portal.** It pays real
  installments — try a one-account list first.
- The GUI flows are not driven by tests: they are verified by the Rust/TS
  contract plus one real use of the import.

## Open work

1. **Remove Atlas entirely** — `db.rs` and the `mongodb` dependency are still
   present purely for the import (~100 MB of build deps). Two read call sites in
   `lib.rs`; the rest is deletion. Do this only once the data is confirmed local
   (it is, on this machine).
2. **Local multi-account** — mentioned as a possibility, not started. Would need
   an owner column on `accounts` and `lists`. Cheaper now, while lists are empty.
3. **Backup/restore via a real file dialog** — implemented with typed paths; a
   native picker would need `tauri-plugin-dialog`.

## Backup / restore (done)

`Manage → Backup & restore` (`backup.rs` + `export_backup` / `import_backup` in
`lib.rs`). The backup is a complete SQLite file made with SQLite's own backup
API — required because the store runs in WAL and a raw `cp` can miss `-wal`
pages. It carries accounts, lists, the encrypted DOP password, the login hash
and the KDF salt. The standing caveat: **the backup is the file plus you
remembering the login password** — the DOP password key is derived from it, so
restoring on another machine means signing in with that same password.

Restore refuses non-AutoDOP files (`meta` table + `integrity_check` required),
keeps the replaced database as `autodop.db.pre-restore-<stamp>` next to the live
file, removes stale `-wal`/`-shm` sidecars, and signs you out (the in-memory key
belongs to the old login password). Exports refuse to overwrite an existing file
that is not an AutoDOP database. Verified by `cargo test` (backup module, 4
tests) and the roundtrip probe:

    cd src-tauri && cargo run --example backup_probe            # full seed→backup→damage→restore→verify
    cargo run --example backup_probe -- export /path/to.db     # real backup of the live store
    cargo run --example backup_probe -- inspect /path/to.db    # what a backup holds

A real export of the live database exists at `~/Backups/autodop-backup-2026-10-02.db`
(148 accounts, 26 lists, credential).

## Facts worth knowing

- `scraper.py:283` exits on missing args **before** touching the portal, so the
  runner can be smoke-tested without a failed DOP login.
- Rebate semantics: `1` means *skip the rebate step*; `0` is the default and is
  actively set. `scraper.py:165` is the branch.
- `accountHolders.Denomination` is mixed int32/string in the source data; the
  mapper handles both, and SQLite stores it verbatim as TEXT.
- The accounts count reads 148 now (an earlier probe said 149) —
  `estimated_document_count` vs an exact count.
- Unsigned builds: macOS Gatekeeper needs right-click → Open. The macOS bundle is
  arm64-only.
- Probes, all read-only and value-free: `db_schema`, `db_probe`, `import_probe`,
  `fernet_probe`, `build_info`.