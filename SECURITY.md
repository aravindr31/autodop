# Security Policy

AutoDOP handles real financial actions — it pays India Post RD installments
through the DOP agent portal — and stores the credentials that make that
possible. This page explains the trust model and how to report problems.

## Supported versions

Only the latest release is supported. Releases are cut automatically from
`main` and carry a build stamp (`Manage → This build` shows `<sha> <time>`);
please confirm you are on the newest one before reporting.

## The trust model

**Everything is local.** There is no server, no backend service, no telemetry.
The app's only network activity is the Selenium session you trigger against
`dopagent.indiapost.gov.in` and ChromeDriver's one-time download on first run.

- All data lives in one SQLite file in your OS app-config directory
  (`~/Library/Application Support/in.aravind.autodop/autodop.db` on macOS,
  `%APPDATA%\in.aravind.autodop\` on Windows, `~/.config/in.aravind.autodop\`
  on Linux).
- The **DOP portal password** is stored only as a Fernet token, encrypted with
  a key derived from your local login password via **Argon2id**. The derived
  key exists solely in memory while you are signed in — it is never written to
  disk or to the webview.
- The password travels only from the Rust backend into the runner's argv —
  never through the frontend, and it is **redacted from run logs**.
- Restore and import flows keep a safety copy of the database they replace.

### What is *not* defended

- **Someone with your local login password** can decrypt everything; that is
  the design. The login password is the root of trust.
- **Malware on your machine** can keylog the portal login. A local password
  manager cannot prevent that; neither can this app.
- **The portal itself.** The app automates the real payment flow; it has no
  control over what the DOP portal does with a session.

## Reporting a vulnerability

**Do not open a public issue** for anything you believe is exploitable — that
includes credential handling, the runner invocation, the backup/restore
format, and the IPC bridge between the webview and Rust.

Use [GitHub's private vulnerability reporting](../../security/advisories/new),
or contact the maintainer directly if you prefer email.

Please include:

- the version and build stamp (Manage → This build)
- your OS
- the steps to reproduce, and impact as you understand it
- any log output — the app redacts the DOP password from logs, but review
  anything you paste before sending it

You will get an acknowledgement within a few days and a fix timeline once the
issue is confirmed. Fixes ship as a regular release through the normal
pipeline; credit is given if you want it.

## Scope

In scope: the desktop application in this repository — the Rust backend, the
frontend, the Selenium runner, the packaging scripts, and the CI/CD pipeline
that produces releases.

Out of scope: the India Post DOP agent portal itself (report those to India
Post), Chrome/ChromeDriver, and Tauri/Chromium platform bugs — report those
upstream, but you are welcome to open an issue here linking them if they
affect this app.

## Data handling for contributors

Never commit real account numbers, credentials, `FERNET_KEY` values, database
files or `.env` files. `.gitignore` already excludes `.venv-scraper/`,
`src-tauri/.env` and `src-tauri/binaries/*`; tests use synthetic accounts and
throwaway in-memory databases. If you accidentally paste secrets into a PR,
rotate the DOP password on the portal first, then clean the history — treat it
as compromised.