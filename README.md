# AutoDOP

Desktop app (macOS / Windows / Linux) for managing India Post DOP agent account
lists and driving the DOP portal with Selenium.

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

## Versioning, commits & releases

The pipeline builds itself from how you commit — no manual version bumps.

**Conventional commits.** Every commit message starts with a type:

| Type | Version effect |
| --- | --- |
| `fix: …` | patch (0.6.1 → 0.6.2) |
| `feat: …` | minor (0.6.1 → 0.7.0) |
| `…!: …` or a `BREAKING CHANGE` footer | major (0.6.1 → 1.0.0) |
| `chore:` `docs:` `ci:` `refactor:` … | no effect on its own |

```bash
git commit -m "feat: add a notes field per account"
git commit -m "fix: keep the rebate when a list is renamed"
```

**Branches & PRs.** Work happens on `feature/*` / `fix/*` / `hotfix/*` branches —
CI rejects PRs to `main` from any other branch. A merge to `main` is what ships.

**Release flow (automatic on merge to `main`):**

1. `test` runs the full suite on the merge commit — nothing ships failing.
2. The next version is derived from the commit messages since the last release
   (`scripts/release-version.mjs`) and written into `VERSION`, `CHANGELOG.md`
   and all four version fields. That lands as a
   `chore(release): vX.Y.Z [skip ci]` commit and is tagged `vX.Y.Z`.
3. Three runners build macOS (verified DMG), Windows (NSIS/MSI) and Linux
   (AppImage/deb) from exactly that tagged commit, and attach the installers
   to the GitHub release with the changelog section as the notes.

You never edit the version by hand; `npm run version:bump -- X.Y.Z` still
exists for local builds. Merges that only touch `README.md`, `CHANGELOG.md`,
`VERSION`, `.github/**`, `docs/**` or other markdown skip the release
workflow entirely.

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

Every build carries its own copy of the runner, so there is normally nothing to
configure. Resolution order:

1. a path chosen in **Manage → Scraper script** (`settings.json`, same folder)
2. `AUTODOP_SCRAPER`
3. `binaries/scraper-<os>-<arch>` — the frozen helper, which needs no Python
4. `scraper.py` inside the app, which does need a Python with selenium
5. `<repo>/scraper.py`, which only exists when running from source
6. `scraper.py` in the working directory

**Built-in copy** in that section clears the override and goes back to the
bundled one. A `.py` needs an interpreter; anything else is treated as a frozen
helper and gets the arguments directly. See
[Shipping it to someone else](#shipping-it-to-someone-else) for how the helper is
built.

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

## Data

Everything this app owns lives in **one SQLite file on this machine** — accounts,
lists, the DOP password, and a run history. Nothing is shared, so there is no
connection string to distribute, no server to run, and no database role to grant.

- macOS: `~/Library/Application Support/in.aravind.autodop/autodop.db`
- Windows: `%APPDATA%\in.aravind.autodop\autodop.db`
- Linux: `~/.config/in.aravind.autodop/autodop.db`

The exact path is shown in **Manage → Local database**.

| Table | Holds |
| --- | --- |
| `accounts` | one row per account holder; `number` is unique |
| `lists` | the lists (`A`…`Z`), one of them flagged `active` |
| `list_entries` | which accounts are in which list, in order, with the rebate |
| `credentials` | the DOP id and a Fernet-encrypted password |
| `runs` | one row per Generate run — what was attempted and how it ended |
| `meta` | the schema version, and whether the Atlas import has run |

`list_entries` references `accounts` and `lists` with `ON DELETE CASCADE`, so
deleting an account takes it out of every list, and deleting a list takes its
entries with it. `rebate` defaults to `0`; `1` is a real value meaning "skip the
rebate step" in `scraper.py`, and it survives every round trip.

**Manage → Local database** shows the counts and the file path, with **Reload
accounts**, **Load lists** and **Save lists**. A save upserts by id and falls back
to matching on the name, so saving twice updates instead of duplicating.

### Importing the data this app started with

The accounts, lists and DOP password began life in MongoDB Atlas. Move them
across once, from **Manage → Local database → Import from Atlas**. It:

- reads Atlas read-only — the role never needed to be writable for this,
- replaces the local accounts and lists, so it asks for confirmation when the
  local database already has accounts,
- re-encrypts the DOP password under **this machine's** own key,
- records when it ran, which the panel then shows.

Nothing depends on Atlas afterwards. You can run and check it without the app:

```bash
cd src-tauri && cargo run --example import_probe               # throwaway database
cd src-tauri && cargo run --example import_probe -- ~/t.db     # into a real file
```

Last run of that probe against the real cluster:

```
imported: 148 accounts, 26 lists, 0 entries, credentials=true
lists: 26 (active=Some("A"))
stored credential: DOP.MI6855840100003 (token 100 chars)
```

All 26 lists are empty shells in Atlas (`accounts: []`), so there is no membership
or rebate data to bring across — only the account master and the password.

### What is still read from Atlas

Only that import. `MONGO_URI` and `FERNET_KEY` are needed for it and nothing
else; with no `MONGO_URI` the app never touches the network at all. Credential
resolution, most explicit first:

1. `DOP_USERNAME` / `DOP_PASSWORD` (environment or a `.env`)
2. the local database
3. the older `credentials.json`, still honoured so an existing install keeps working
4. the Atlas copy — a read-only fallback that exists only until the import has run

### Overrides

| Variable | Default | Purpose |
| --- | --- | --- |
| `MONGO_URI` | — | Atlas connection string — **import source only** |
| `MONGO_DB` | `accounts` | database name for the import |
| `MONGO_COLLECTION` | `accountHolders` | account documents collection |
| `MONGO_LISTS_COLLECTION` | `savedList` | lists collection (probe override) |
| `FERNET_KEY` | generated on first use | encrypts the stored DOP password, and opens the Atlas copy |
| `AUTODOP_ENV_FILE` | — | explicit path to a `.env` to read |

Environment variables win over the `.env` file. `src-tauri/.env` is gitignored;
`src-tauri/.env.example` is the committed template.

### Probing

```bash
cd src-tauri && cargo run --example db_schema      # Atlas collections, indexes, field names
cd src-tauri && cargo run --example db_probe       # Atlas connectivity + credential check
cd src-tauri && cargo run --example import_probe   # the import, into a throwaway database
cd src-tauri && cargo run --example build_info     # version + build stamp
```

None of them print a value — field names, counts and indexes only.

### How the DOP password is encrypted

The stored password is a Fernet token, so the old Streamlit app could read it and
so can this one. `src-tauri/src/crypt.rs` implements the Fernet framing on the
standard RustCrypto primitives (`aes`, `cbc`, `hmac`, `sha2`) — in both
directions. The `fernet` crate would do the same job but links **OpenSSL**, which
this project avoids: it already uses rustls and is meant to build on macOS and
Windows alike.

It is verified against the reference implementation rather than assumed.
`src-tauri/src/crypt_vectors.rs` holds tokens generated by Python's
`cryptography` under a throwaway key; the tests decrypt them, round-trip their own,
and assert that a wrong key, a tampered token, a truncated token and non-base64
garbage are all rejected. A token this code writes also decrypts under Python's
`Fernet` — `cargo run --example fernet_probe` prints one to check with.
Regenerate the vectors with `python3 src-tauri/scripts/gen_fernet_vectors.py`.

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

## Shipping it to someone else

Build once per OS — `npm run build` gives a `.dmg` on macOS, `.msi`/`.exe` on
Windows, `.deb`/`.AppImage` on Linux. PyInstaller cannot cross-compile, so the
frozen runner has to be produced on each platform you ship:

```bash
npm run build:sidecar   # freezes scraper.py for THIS os/arch
npm run build           # bundles that into the installer
```

`build:sidecar` writes `src-tauri/binaries/scraper-<os>-<arch>[.exe]`, and Tauri
ships it inside the app. The app prefers it over the bundled `.py` because it
carries its own Python and selenium, so the target machine needs **no Python, no
pip, no selenium**. It does still need **Google Chrome** — Selenium drives the
real browser — and the first run downloads a matching chromedriver, so it needs
internet once.

Nothing about this is required for your own Mac: with no sidecar present the app
falls back to the bundled `scraper.py` exactly as before, so a plain
`git clone && npm run build` still works without PyInstaller.

### First run on a new machine

The app generates its own encryption key (`key`, mode 600) in the app config
folder the first time it needs to store a DOP password, so nobody has to create a
`.env`. Enter the DOP id and password once in **Manage → DOP portal password**:

- macOS: `~/Library/Application Support/in.aravind.autodop/`
- Windows: `%APPDATA%\in.aravind.autodop\`
- Linux: `~/.config/in.aravind.autodop/`

A configured `FERNET_KEY` still takes precedence — that is the key that reads the
encrypted copy in Atlas, which a freshly generated one deliberately cannot.

### Not signed — what that costs

These builds are not code-signed or notarized. On macOS Gatekeeper will refuse a
downloaded `.dmg`; the recipient has to right-click the app → **Open** once (or
System Settings → Privacy & Security → **Open Anyway**). Unsigned Windows builds
show a SmartScreen prompt behind *More info → Run anyway*. Linux does not care.

Signing is what removes those prompts — a Developer ID certificate plus
`notarytool` on macOS, an EV/OV certificate on Windows. Without it, whoever you
hand the app to needs that one extra step explained to them.

The macOS bundle is **arm64-only**; an Intel Mac needs a build produced on, or
targeted at, `x86_64`.

## Troubleshooting

**Generate fails with *"Scraper produced no parseable result"* and Chrome never
opens.**

That message means the Python process died before printing its result. The
reason is in the log shown under the button. The usual cause is a missing
import — `ModuleNotFoundError: No module named 'selenium'` — from the
interpreter the app picked; see [Python & Chrome](#python--chrome).

**`tauri build` fails at `bundle_dmg.sh`.**

`npm run build` no longer takes that path. Tauri's DMG step (create-dmg) writes
its scratch `rw.*.dmg` image into the very folder it is about to copy, so the
image contains a copy of itself; a failed run leaves tens of MB behind and each
retry nests deeper, ending in a misleading `hdiutil: create failed - No space
left on device` from the resize step. It also leaves a mounted volume behind.

`scripts/build.mjs` avoids it: it builds the `.app` with Tauri, stages a pristine
copy in the system temp dir, makes the image with a single `hdiutil create`, then
mounts the result read-only and checks the app, the `/Applications` symlink, the
bundled `.py` and the frozen runner are all really inside. A `.dmg` is only
reported once those pass.

If you need the old behaviour, `npm run build:tauri` is still there — check
`df -h /` first, and clear leftovers with `npm run clean:dmg`.

## Versions

The version lives in four files that must agree, so bump them together:

```bash
npm run version:bump -- 0.3.0
```

That updates `tauri.conf.json` (which names the installer,
`AutoDOP_<version>_<arch>.dmg`), `Cargo.toml`, and both `package.json`s.

Every build also carries the commit it came from, stamped by `build.rs` as
`<short sha> <commit date>`. The running build is shown in the app: at the bottom
of the left-hand list panel, on the sign-in screen, and as **This build** in
Manage. So "did the install replace the last one?" is answered by looking, not
guessing.

## Checks

```bash
npm --prefix frontend run build     # frontend build
npm --prefix frontend run check     # astro check (types)
npm --prefix frontend run smoke     # store integration test
cd src-tauri && cargo test          # Rust payload/parsing tests
```