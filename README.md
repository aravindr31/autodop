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
| `pip install selenium webdriver-manager` | what `scraper.py` imports |
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

## Generating a list

**Lists** tab → expand a list → **Generate (DOP)**.

The button calls the Rust command `generate_lists`, which spawns
`scraper.py <user> <pass> <lists_json>` with this list's account numbers and
rebate values, streams the script's output back into the UI as it runs, and
returns the parsed result array.

## Credentials

Set them once in **Manage (gear) → DOP Credentials**. They are written by the
Rust backend to the app config folder — never to the browser:

- macOS: `~/Library/Application Support/in.aravind.autodop/credentials.json`
- Windows: `%APPDATA%\in.aravind.autodop\credentials.json`

`DOP_USERNAME` / `DOP_PASSWORD` in the environment take precedence.

## Overrides

| Variable | Default | Purpose |
| --- | --- | --- |
| `AUTODOP_SCRAPER` | `<repo>/scraper.py` | path to the Selenium script |
| `AUTODOP_PYTHON` | `python3` (mac) / `python` (win) | interpreter to launch it with |
| `DOP_USERNAME` / `DOP_PASSWORD` | config file | portal credentials |

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
  instead of assuming `1`.

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

## Checks

```bash
npm --prefix frontend run build     # frontend build
npm --prefix frontend run check     # astro check (types)
npm --prefix frontend run smoke     # store integration test
cd src-tauri && cargo test          # Rust payload/parsing tests
```