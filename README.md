# AutoDOP

Desktop app (macOS / Windows) for managing India Post DOP agent account lists
and driving the DOP portal with Selenium.

- `frontend/` — Astro + React + Tailwind UI (account browser, lists, manage panel)
- `src-tauri/` — Tauri (Rust) desktop backend; runs `scraper.py` locally
- `scraper.py` — unchanged Selenium script (the DOP portal automation)
- `spec.md` — the original feature spec

## Prerequisites

| Need | Why |
| --- | --- |
| Node 20+ | builds the frontend |
| Rust + Cargo | builds the desktop shell (`rustup` if missing) |
| Python 3 | `scraper.py` runs as a local process |
| selenium + webdriver-manager | what `scraper.py` imports — see [Python & Chrome](#python--chrome) |
| Google Chrome | Selenium drives it |

## Run

```bash
npm install                 # Tauri CLI (repo root)
npm --prefix frontend install
npm run dev                 # dev window; hot-reloads the UI
```

Build a distributable:

```bash
npm run build               # macOS: .app/.dmg   Windows: .msi/.exe
```

## Generating lists

Two ways in, both on the **Lists** tab:

- **Generate All Lists** — every non-empty list in a single run (the Streamlit
  UI's headline action, `main.py:543`).
- **Generate (DOP)** inside an expanded list — just that one.

Either way it is a **single invocation**: the Rust command `generate_lists`
spawns `scraper.py <user> <pass> <lists_json>`, where `lists_json` is an array of
`{name, numbers, rebate}` — the same key names and argv order the Streamlit UI
used (`main.py:557` → `run_scraper_script` → `subprocess.run`). `scraper.py`
logs in once and then works through the lists in order; each list's result comes
back in the parsed array.

**Rebate.** `scraper.py:165` only *skips* the rebate step when the value is `1`
(`if rebate_val != 1`); any other value is typed into `RD_INSTALLMENT_NO` and
saved. The Streamlit UI read `acc.get("Rebate", 0)` and those documents carry no
`Rebate` field, so **`0` is the default**; a list's stored rebate overrides it.

Expand a list on the **Lists** tab to edit the rebate per account. It is stored
against the account on that list, rides along with Generate, and survives a
**Load lists** / **Save lists to Atlas** round trip — including `1`, which is a
real value rather than "unset".

## Credentials

### The DOP portal password expires every 180 days

Change it on the India Post portal first, then save the new one here:
**Manage (gear) → DOP portal password** — enter the DOP id, the new password
twice, and press **Save DOP password**. (Typing it twice exists because a typo
would otherwise break every run until the next rotation.)

It is stored **encrypted** — a Fernet token under the same `FERNET_KEY` that
protects the Atlas copy — at:

- macOS: `~/Library/Application Support/in.aravind.autodop/credentials.json`
- Windows: `%APPDATA%\in.aravind.autodop\credentials.json`

…mode 600, never in the browser. The app also tries to write that same ciphertext
back to `users.UserInfo.DOP_password`. While the Atlas role is read-only that
fails harmlessly and the UI says so; the local file outranks Atlas, so either way
the new password takes effect immediately.

`FERNET_KEY` is what makes that file readable, so keep it: change it and the saved
password can no longer be decrypted, and the app falls back to the Atlas copy
(save the password again after restoring the key). Note the local file outranking
Atlas also means rotating the password *outside* the app — editing the document in
the Atlas UI, say — has no effect on this Mac until you either save it here again
or delete `credentials.json`.

### Resolution order

1. `DOP_USERNAME` / `DOP_PASSWORD` in the environment or a `.env`
2. the app-config `credentials.json` — what **Save DOP password** writes
3. **the `users` collection in Atlas** (`_id: 5fbf919c87da8228f87bd62f`):
   `UserInfo.DOP_ID` is the portal username and `UserInfo.DOP_password` is a
   Fernet token that gets decrypted in Rust.

**Credentials already exist in your database**, so a fresh install needs nothing
typed. Manage → DOP portal password shows which source is in use. **The password
is never sent to the webview** — it goes from Rust straight into `scraper.py`'s
argv.

## Scraper script

Every build carries its own copy of `scraper.py`, so there is normally nothing to
configure. Resolution order:

1. a path chosen in **Manage → Scraper script** (`settings.json`, same folder)
2. `AUTODOP_SCRAPER`
3. the copy inside the app bundle
4. `<repo>/scraper.py`, which only exists when running from source
5. `scraper.py` in the working directory

**Built-in copy** in that section clears the override and goes back to the bundled
one. Bundling settles the *path* question only: `selenium` and Chrome are still
needed, because the script is Python rather than a compiled binary — shipping
that too means a PyInstaller sidecar, which is not wired up.

## Overrides

| Variable | Default | Purpose |
| --- | --- | --- |
| `AUTODOP_SCRAPER` | the bundled copy | path to the Selenium script |
| `AUTODOP_PYTHON` | `python3` (mac) / `python` (win) | interpreter to launch it with |
| `DOP_USERNAME` / `DOP_PASSWORD` | config file | portal credentials |

## Python & Chrome

`scraper.py` drives Chrome through Selenium, so the interpreter the app launches
needs both `selenium` and `webdriver_manager` — and it is easy for that to be a
*different* Python from the one in your shell. The app shows the one it will use
in **Manage → DOP Credentials**. If it reports the bare `python3`, it came from
`PATH` and probably has neither package (the symptom is Generate failing with
*"Scraper produced no parseable result"* and no Chrome window).

A venv wired up through `.env` keeps it deterministic:

```bash
python3 -m venv .venv-scraper
.venv-scraper/bin/pip install selenium webdriver-manager
```

```bash
# src-tauri/.env   (gitignored)
AUTODOP_PYTHON=/absolute/path/to/.venv-scraper/bin/python3
```

`AUTODOP_PYTHON` and `AUTODOP_SCRAPER` are declared in `.env` too — at startup
the app copies every `.env` entry into its process environment, so settings that
are read straight from `std::env` work as well as the ones read through the
config lookup.

Chrome needs no manual driver: `webdriver_manager` fetches the matching
ChromeDriver on first run and caches it under `~/.wdm`.

## Logs

Every Generate run writes a log file, because the progress area is cleared as
soon as the window moves on.

- **In the app** — a failed run shows the backend's log tail inline, with a
  *Copy log* button and the full path. The scraper's `stderr` is streamed live as
  well, so a Python traceback appears while it happens rather than only at the
  end.
- **On disk** — `~/Library/Logs/in.aravind.autodop/scraper-<timestamp>.log`
  (`%LOCALAPPDATA%\in.aravind.autodop\logs` on Windows). One file per run,
  timestamped, with the password redacted.

## Database (MongoDB Atlas)

Accounts are read from Atlas by the Rust backend — the webview never sees the
connection string.

```bash
cp src-tauri/.env.example src-tauri/.env   # then paste your URI
```

**Verified against your cluster** (`cargo run --example db_probe`):

| Database | Collection | Docs | Contents |
| --- | --- | --- | --- |
| `accounts` | `accountHolders` | 149 | the account documents ← accounts come from here |
| `accounts` | `savedList` | 26 | lists (`A`…`Z`), each with `{id, rebate}` per account |
| `accounts` | `users` | 1 | login record |
| `accounts` | `admin` | 1 | — |
| `accounts` | `list` | 0 | empty |

`accounts.accounts` is **empty** — the collection is `accountHolders`, which is
the default. Document fields confirmed as `_id, Number, Name, Denomination,
CNumber, Ref_Number, addedIn`.

**Manage (gear) → Database** shows connection status plus **Reload accounts**,
**Load lists** and **Save lists to Atlas**. On launch the app loads accounts from
Atlas when reachable, otherwise it keeps the persisted/seeded set.

### Lists

List documents are `{_id, listName, active, accounts: [{id: ObjectId, rebate}]}`.

- **Load lists** replaces the local lists with the stored ones and adopts the
  list marked `active`.
- **Save lists to Atlas** upserts the lists you have and adopts the stored ids,
  so a later save updates in place instead of duplicating. **Nothing is ever
  deleted** — lists you removed locally stay in Atlas.
- Documents are matched by `_id` when it is a real ObjectId, otherwise by
  `listName` (local ids are UUIDs, so this is what updates the existing A–Z
  rows rather than inserting new ones).
- `rebate` rides along per account, so **Generate (DOP)** sends the stored value
  when there is one and falls back to `0` — the Streamlit UI's default.

Your 26 lists are currently **empty shells** — every one has `accounts: []`, so
there is no membership or rebate data to import yet. `A` is the one flagged
`active`.

### Your Atlas user is read-only

The probe reports the authenticated roles, and this connection is
**`readAnyDatabase`** — reads work, writes do not. Atlas refuses:

```
user is not allowed to do action [update] on [accounts.savedList]
```

So **Load lists** and **Reload accounts** work today, and **Save lists to Atlas**
is live in the UI and ready to use — it will simply be refused by Atlas until you
grant write access. Read-only is a deliberate starting point here; switch the
role when you're ready.

The refusal is handled honestly: the toast says the user is read-only and names
the database to grant `readWrite` on, rather than showing the raw Atlas error.
Nothing is half-applied — a rejected write changes nothing.

`db_status` returns `writable` and `roles`, so the panel can state this up front
instead of leaving you to guess why a save failed.

### Overrides

| Variable | Default | Purpose |
| --- | --- | --- |
| `MONGO_URI` | — | Atlas connection string (required) |
| `MONGO_DB` | `accounts` | database name |
| `MONGO_COLLECTION` | `accountHolders` | account documents collection |
| `MONGO_LISTS_COLLECTION` | `savedList` | lists collection (probe/test override) |
| `FERNET_KEY` | — | decrypts `users.UserInfo.DOP_password` (required for Atlas credentials) |
| `AUTODOP_ENV_FILE` | — | explicit path to a `.env` to read |

Environment variables win over the `.env` file. `src-tauri/.env` is gitignored;
`src-tauri/.env.example` is the committed template.

### Probing the cluster

```bash
cd src-tauri && cargo run --example db_probe     # read-only; prints field names, never values
MONGO_LISTS_COLLECTION=savedList_probe cargo run --example db_probe   # adds a write round-trip
```

The write probe only ever touches the collection named by
`MONGO_LISTS_COLLECTION`, and drops it again afterwards — `savedList` is never
modified by it.

### How the DOP password is decrypted

The old Streamlit app encrypted it with Python's `cryptography.fernet.Fernet`
(`main.py` → `settings.decrypt_dop_passwd`), so only a Fernet-compatible
decryptor can read that ciphertext — no other cipher opens it.

`src-tauri/src/crypt.rs` implements the Fernet framing on the standard RustCrypto
primitives (`aes`, `cbc`, `hmac`, `sha2`). The `fernet` crate would do the same
job but links **OpenSSL**, which this project avoids: it already uses rustls and
is meant to build on both macOS and Windows.

It is verified against the reference implementation rather than assumed.
`src-tauri/src/crypt_vectors.rs` holds tokens generated by Python's
`cryptography` under a throwaway key; the tests decrypt them and also assert that
a wrong key, a tampered token, a truncated token and non-base64 garbage are all
rejected. Regenerate with `python3 src-tauri/scripts/gen_fernet_vectors.py`.

No TTL is enforced, matching `Fernet.decrypt`, whose default is `ttl=None` — a
credential stored long ago must keep working.

## Notes and limits

- **Rebate** — `scraper.py` needs one rebate value per account (`1` = "no
  rebate, just pay"). Generate sends each account's stored rebate, falling back
  to `1` when a list has none.
- **Runtime** — a run logs in once and can take minutes (Selenium waits up to
  360 s on the DOP login alone). The backend allows 3600 s, then kills it.
- **Distribution** — the app currently launches the machine's own Python +
  Selenium install. Shipping it to someone else means bundling those (e.g. a
  PyInstaller sidecar in `src-tauri/binaries/`) so the target machine needs no
  Python; that is not wired up yet.
- **Reality check** — this clicks the real *pay* flow on the real DOP portal.
  Try a one-account list first.

## Troubleshooting

**Generate fails with *"Scraper produced no parseable result"* and Chrome never
opens.**

That message means the Python process died before printing its result. The
reason is in the log shown under the button. The usual cause is a missing
import — `ModuleNotFoundError: No module named 'selenium'` — from the
interpreter the app picked; see [Python & Chrome](#python--chrome).

**`tauri build` fails at `bundle_dmg.sh`.**

First check `df -h /` — a full disk is the commonest cause, and the error
(`hdiutil: create failed - No space left on device`) is easy to mistake for a
script problem.

If there is space, look inside `src-tauri/target/release/bundle/macos/`. A
failed run leaves a ~31 MB `rw.*.dmg` scratch image in there, and Tauri packages
that folder as the DMG's **source**, so the next attempt copies the junk into
its own image, fails again, and leaves a bigger one — the folder grows on every
failure. `npm run build` now clears them first (`npm run clean:dmg`); the
`AutoDOP.app` built alongside is unaffected either way and stays usable.

## Checks

```bash
npm --prefix frontend run build     # frontend build
npm --prefix frontend run check     # astro check (types)
npm --prefix frontend run smoke     # store integration test
cd src-tauri && cargo test          # Rust payload/parsing tests
```