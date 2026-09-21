# AutoDOP desktop shell

Runs the Astro frontend in a **native window** and lets a button in the UI call
`scraper.py` directly — **no API server to run, no port to configure**.

## Why a shell?

A page in a normal browser cannot start a local process (deliberate browser
security). pywebview embeds the built frontend in a native window and injects a
Python API into it, so `Generate` in the UI calls Python *in-process*.

The bundled assets are handed to the webview by pywebview's own loopback-only
file server, created and destroyed with the process — nothing listens after the
window closes.

## Setup

```bash
# 1. build the frontend once (and after any frontend change)
cd frontend && npm install && npm run build && cd ..

# 2. Python deps (pywebview + the Selenium stack scraper.py needs)
python3 -m pip install -r desktop/requirements.txt

# 3. credentials — server-side only, never sent to the browser
cp desktop/.env.example desktop/.env   # then fill in DOP_USERNAME / DOP_PASSWORD
```

## Run

```bash
python3 desktop/main.py              # launch the window
python3 desktop/main.py --selftest   # headless check of the bridge logic
```

Then in the UI: **Lists** tab → expand a list → **Generate (DOP)**.

## How it works

| Piece | Role |
| --- | --- |
| `desktop/main.py` | pywebview host; `Api.generate_lists()` runs `scraper.py` |
| `frontend/src/lib/bridge.ts` | Detects the shell, calls the Python API, receives progress |
| `scraper.py` | Unchanged — still `python3 scraper.py <user> <pass> <lists_json>` |

`Api.generate_lists(lists)` receives `[{name, numbers[], rebate[]}]`, launches
`scraper.py` as a subprocess in the host process, streams progress lines into
the page via `window.__autodopProgress`, and returns the parsed result array.

## Notes / gotchas

- **Credentials** live in `desktop/.env` (chmod 600) or `DOP_USERNAME` /
  `DOP_PASSWORD` in the environment. `desktop/.gitignore` excludes `.env`.
- **Rebate** — `scraper.py` expects one rebate value per account number
  (`1` = "no rebate, just pay"). The UI currently sends `1` for every account;
  the account model has no rebate field yet.
- **Runtime** — a run logs in once and can take minutes (Selenium waits up to
  360 s on the DOP login alone); the host allows 3600 s.
- **Reality check** — this drives the real India Post DOP portal and clicks the
  real *pay* flow. Test on a small list first.
- Rebuilding the frontend (`npm run build`) is required after frontend changes;
  the shell always loads `frontend/dist/`.