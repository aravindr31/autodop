# AutoDOP

A self-contained desktop app for India Post DOP **agents**: keep your RD
account holders, group them into lists, and pay monthly installments on the DOP
agent portal without touching a browser. Everything — your data, your passwords,
the automation — lives on your own machine.

Available for **macOS (Apple Silicon)**, **Windows (x64)** and **Linux (x64)**.

---

## Download

Grab the installer for your platform from the
[**Releases**](../../releases) page:

| Platform | File |
| --- | --- |
| macOS | `AutoDOP_<version>_aarch64.dmg` |
| Windows | `AutoDOP_<version>_x64-setup.exe` (or `.msi`) |
| Linux | `AutoDOP_<version>_amd64.AppImage` (or `.deb`) |

Every release is built and attached automatically by CI from a tested merge to
`main` — never hand-built.

### First run

1. **Create your workspace** — your DOP portal id (mobile number) and a local
   password. The password protects your data on this machine *and* unlocks the
   DOP password you save later.
2. **Import your accounts** — **Manage → Import from PDF**: print the agent
   portal's *Deposit Accounts* list to PDF, point the app at the file, and every
   row (number, name, denomination) becomes an account. Already have data?
   **Manage → Backup & restore → Import JSON** with a portable backup instead.
3. **Save your DOP password** — **Manage → DOP portal password** (it rotates on
   the portal every 180 days; save the new one here after changing it there).
4. **Build a list** and hit **Generate**. That last step drives the real portal
   and pays real installments — try a one-account list first.

Switching machines: export a portable JSON on the old one, set the **same local
password** on the new one, import the JSON. The DOP password travels encrypted
and only that password can open it.

> **Unsigned builds.** macOS Gatekeeper: right-click → **Open** once (or System
> Settings → Privacy & Security → *Open Anyway*). Windows SmartScreen: *More
> info → Run anyway*. Linux does not care.

### What a machine needs

Only **Google Chrome** — the app carries its own runner (Python + Selenium are
frozen inside the installer). The first Generate run downloads a matching
ChromeDriver, so it needs internet once. Install Chrome in the normal place for
your OS and you are done.

---

## Features

- **Account browser** — search across name / number / ref, switch between 4, 3
  and 2-column grids or a table, paginate through hundreds of holders.
- **Lists with rebates** — the A-to-Z workflow from the old scripts: one rebate
  per account (`1` means *skip the rebate step*), editable per list.
- **Generate** — pays every list (or one list) in a single Selenium session,
  streaming live progress and writing a timestamped log per run.
- **Local workspaces** — several people can share one install; each workspace
  has its own accounts, lists, DOP password and run history, opened by its own
  password.
- **Backups** — a whole-database `.db` backup for this machine, and a portable
  JSON export that moves everything (including the DOP password, encrypted) to
  any machine on any OS.
- **Password rotation** — India Post expires the DOP password every 180 days;
  change it on the portal, save the new one in Manage, done.

---

## Security

**Your data never leaves your machine.** There is no server, no telemetry, and
no account in this app — the only network traffic comes from the DOP portal
login you trigger and ChromeDriver's one-time download.

- Everything lives in **one SQLite file** in your OS app-config directory
  (macOS: `~/Library/Application Support/in.aravind.autodop/`,
  Windows: `%APPDATA%\in.aravind.autodop\`,
  Linux: `~/.config/in.aravind.autodop/`).
- The **DOP portal password** is stored as a Fernet token encrypted with a key
  derived from your local login password (Argon2id). The key exists only in
  memory while you are signed in — a copied database file is useless without
  your password.
- Run logs **redact the password**, and the password is never sent to the
  webview: it goes from Rust straight into the runner's argv.
- A restore/import always keeps a safety copy of the replaced database.

**Reporting a vulnerability.** Please do not open a public issue. Use
[GitHub's private vulnerability reporting](../../security/advisories/new) on
this repository, or contact the maintainer directly. Include the version
(shown in-app as *This build* under Manage) and, if relevant, the run log with
the password redacted.

---

## Contributing

PRs are welcome — the pipeline is built around them.

**Rules of the road:**

1. **Branch**: PRs must come from `feature/*`, `fix/*` or `hotfix/*`. CI
   rejects PRs to `main` from any other branch.
2. **Conventional commits** — every commit message starts with a type, because
   the release pipeline reads them to decide the next version:

   | Type | Version effect |
   | --- | --- |
   | `fix: …` | patch (0.6.1 → 0.6.2) |
   | `feat: …` | minor (0.6.1 → 0.7.0) |
   | `…!: …` or a `BREAKING CHANGE` footer | major (0.6.1 → 1.0.0) |
   | `chore:` `docs:` `ci:` `refactor:` … | none on its own |

   Merges containing none of `feat`/`fix`/breaking still release a patch, so
   every merge to `main` ships a newer build.
3. **No manual versions.** On merge to `main` CI runs the test suite, derives
   the next version from the commit messages, writes `VERSION`, `CHANGELOG.md`
   and all four version fields, tags `vX.Y.Z`, builds macOS/Windows/Linux from
   that exact commit and publishes the release with the changelog section as
   the notes. Merges touching only docs/workflows skip the release entirely.

### Development setup

```bash
npm install && npm --prefix frontend install
npm run dev          # hot-reloading dev window (needs Rust: rustup)
```

Building a distributable locally:

```bash
npm run build:sidecar   # freeze scraper.py for THIS os (PyInstaller)
npm run build           # .dmg (macOS, mount-verified) / .exe+.msi / .AppImage+.deb
```

The sidecar cannot cross-compile — build on the OS you ship. The CI release
workflow does all three automatically.

### Checks before you push

```bash
cd src-tauri && cargo test --lib     # Rust suite, incl. the schema migration
cd frontend && npx astro check       # types
cd frontend && npm run smoke         # store integration test
```

CI runs all of these on every push.

---

## How it works, briefly

- `frontend/` — Astro + React + Tailwind UI.
- `src-tauri/` — the Tauri/Rust backend: SQLite storage, Argon2id/Fernet
  credential handling, and the process that drives the runner.
- `scraper.py` — the Selenium automation, frozen into each installer by
  PyInstaller and launched as a local subprocess (which is the one thing a
  browser page cannot do).
- `Generate` spawns the runner with the list payload; it logs into the portal
  once and works through the lists in order. A run can take minutes — the
  backend allows a full hour before killing it, and every run leaves a log at
  `~/Library/Logs/in.aravind.autodop/` (`%LOCALAPPDATA%\...` on Windows).

Rebate semantics: `scraper.py` *skips* the rebate step only when the value is
`1`; every other value is typed into `RD_INSTALLMENT_NO` and saved. `0` is the
default.

Advanced overrides (custom runner path, custom Python, `AUTODOP_*` variables)
still exist for development — see `scripts/build-sidecar.mjs` and
`src-tauri/src/db.rs`.

---

## Changelog

Every release's changes are in [`CHANGELOG.md`](CHANGELOG.md), generated from
the commit history at release time.

## License

AutoDOP is source-available under the
[PolyForm Noncommercial License 1.0.0](LICENSE).

You may use, study, modify and redistribute it freely **for personal,
non-commercial purposes** — but you may not sell it, offer it as a paid
service, or use it commercially to gain revenue of any kind without the
copyright holder's written agreement. If you want a commercial license, open an
issue or contact the maintainer.