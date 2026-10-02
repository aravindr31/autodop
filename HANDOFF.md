# AutoDOP — session handoff

Context for continuing work. Delete this file once it stops being useful.

## What this is

A desktop app replacing a Python/Streamlit tool that drives the **India Post DOP
portal** through Selenium: it keeps account holders, groups them into lists
(A–Z), and pays RD installments by running a scraper against the real portal.

- Repo: `/Users/aravind/repos/AutoDOP`, branch `feat/astro-frontend-spec`
- Stack: Astro + React + Tailwind frontend, **Tauri v2** desktop shell, **Rust**
  backend, one **SQLite** file per install
- Current version: **0.6.0** · latest commit (see `git log -1`)
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

**Storage.** SQLite, one file per machine, nothing shared. **Atlas is gone
entirely** — `mongodb` and `futures-util` are out of `Cargo.toml`, `db.rs` is
now just the `.env` reader plus the shared list/credential types, and the
import machinery (`import_from_atlas`, `read_atlas`, `write_import`, the four
Atlas probes) is deleted. Credential resolution: env → local database → legacy
`credentials.json`.

**Credentials.** The DOP password is encrypted with a key derived from the login
password via **Argon2id** over a stored salt. The key is never written to disk —
it lives in memory only while signed in. Deliberately *not* a bare SHA-256: that
is fast, and the threat model is someone holding the database file. Resolution
order: env → local database → legacy `credentials.json`.

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

## Local multi-account (done)

**Workspaces.** A `owners` table keys each person by their DOP portal id (a
mobile number); `accounts`, `lists`, `runs` carry `owner_id`, and
`credentials` is owner-keyed. Schema version 2: an existing single-user v1
database migrates in one transaction under a synthetic `default` owner named
after the saved portal id, with its `login_hash`/`kdf_salt` moved out of
`meta` into the owner row. The migration was verified against a copy of the
live store (148 accounts, 26 lists, credential, argon2 hash carried over).

**Auth.** `setup_login` takes username + password and creates the owner;
`login` takes `owner_id` + password and derives that owner's key. The
frontend remembers the last owner id in localStorage (`autodop-last-owner`)
so the login screen asks only for the password, with a "Not you?" switcher
listing the other workspaces. A second person: pick/add a workspace, sign in
with their own password, import their portable JSON backup — data lands in
their space, isolated from yours. Password change re-keys one owner.
`counts(owner)` is scoped; `counts_all()` is what a whole-file backup holds.

## PDF import (first-run import, done)

`Manage → Import from PDF` (`import_accounts_pdf` + `parse_deposit_row` in
`lib.rs`, `pdf-extract` crate). Point at the agent portal's **Deposit
Accounts** PDF printout; every row becomes an account — number, name,
denomination (commas stripped, `1500.00`), REF/CNumber empty. Long holder
names wrap across two extracted lines; the parser carries the previous line
forward until the row completes. Duplicates under the owner are skipped and
reported. Verified against a real 160-row PDF: 160/160 parsed, no
unparsed lines. Probe: `cargo run --example pdf_probe -- /path/to.pdf`.

## Cross-platform builds (done)

`npm run build` branches on OS: macOS keeps the .app + verified-DMG path;
Windows produces NSIS/MSI and Linux AppImage/deb straight from Tauri
(`--bundles app` is macOS-only and was applied everywhere — fixed). The
sidecar must still be built per OS (`build:sidecar`; PyInstaller cannot
cross-compile). `.github/workflows/release.yml` builds all three on their
own runners and attaches the installers to the GitHub release on a `v*`
tag — the shipping path for Windows/Linux users, who also need Chrome.

## Open work

1. **Local multi-account** — mentioned as a possibility, not started. Would need
   an owner column on `accounts` and `lists`. Cheaper now, while lists are empty.
2. **Backup/restore via a real file dialog** — implemented with typed paths; a
   native picker would need `tauri-plugin-dialog`.
3. **Verify the upgrade on the shipped app** — the 0.4.0 DMG built from
   `d34651d` (no-Atlas) replaced the one from `e2202a3`; the login/credential
   path changed only by losing a fallback, and tests cover the rest.

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

## Portable JSON backup (done)

The same section also does a **portable JSON export/import** (`portable_export` /
`portable_read` / `portable_import` in `backup.rs`, commands
`export_portable_backup` / `import_portable_backup`). The file is
human-readable JSON: accounts and lists in plain JSON, the DOP password as the
exact Fernet token the store holds, plus the `kdf_salt` and the `login_hash`
that open it. The **app login password is the key**: import on another machine
means set the login there to the same password, sign in, then import with that
password — it is verified against the carried `login_hash`, the token is
decrypted, and the DOP password is re-encrypted under the new machine's login
key. Export needs no password at all (the token travels as stored). Import
replaces accounts/lists/credential wholesale and keeps a
`pre-restore-<stamp>` SQLite safety copy of what was replaced. Probe:
`cargo run --example backup_probe -- json ~/Backups/autodop-export.json`. A
real export exists at `~/Backups/autodop-export-2026-10-02.json` (148 accounts,
26 lists, credential).

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
- Probes: `backup_probe` (backup/inspect/roundtrip) and `build_info`. The
  Atlas-only probes (`db_schema`, `db_probe`, `import_probe`, `fernet_probe`)
  were removed with Atlas.