"""AutoDOP desktop shell (pywebview).

Runs the prebuilt Astro app in a native window and injects a Python API into
the page, so a button in the UI calls `scraper.py` *in-process* — there is no
API server for you to run and no port to configure.

(The bundled web assets are handed to the webview by pywebview's own internal,
loopback-only file server, which is created and torn down with this process.
Nothing listens after the window closes.)

Credentials are read from the environment or `desktop/.env` — they never reach
the browser.

Usage
-----
    python3 desktop/main.py              # launch the window
    python3 desktop/main.py --selftest   # headless check of the bridge logic
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / "frontend" / "dist"
INDEX = DIST / "index.html"
SCRAPER = ROOT / "scraper.py"
ENV_FILE = Path(__file__).resolve().parent / ".env"

# Selenium waits up to 360s for the DOP login alone; allow a long ceiling.
SCRAPER_TIMEOUT = 3600


# --------------------------------------------------------------------------- #
# credentials                                                                  #
# --------------------------------------------------------------------------- #

def _read_env_file(path: Path) -> Dict[str, str]:
    values: Dict[str, str] = {}
    if not path.exists():
        return values
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        values[key.strip()] = value.strip().strip('"').strip("'")
    return values


def load_credentials() -> Tuple[str, str]:
    """DOP username/password from the environment, falling back to desktop/.env.

    Kept strictly server-side: the Astro page only ever learns whether they are
    configured (a boolean), never the values.
    """
    user = (os.environ.get("DOP_USERNAME") or "").strip()
    password = (os.environ.get("DOP_PASSWORD") or "").strip()
    if not user or not password:
        file_values = _read_env_file(ENV_FILE)
        user = user or file_values.get("DOP_USERNAME", "")
        password = password or file_values.get("DOP_PASSWORD", "")
    return user, password


def write_credentials(username: str, password: str) -> None:
    ENV_FILE.write_text(
        "# AutoDOP desktop credentials — keep this file private.\n"
        f"DOP_USERNAME={username}\n"
        f"DOP_PASSWORD={password}\n",
        encoding="utf-8",
    )
    try:
        ENV_FILE.chmod(0o600)
    except OSError:
        pass


# --------------------------------------------------------------------------- #
# payload + result handling                                                    #
# --------------------------------------------------------------------------- #

def build_payload(lists: Any) -> List[Dict[str, Any]]:
    """Normalize the UI payload into the shape scraper.py expects.

    `process_lists` zips `numbers` with `rebate`, so the two arrays must be the
    same length — pad missing rebates with 1 ("no rebate, just pay").
    """
    if not isinstance(lists, list):
        raise ValueError("lists must be an array")
    payload: List[Dict[str, Any]] = []
    for item in lists:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name") or "Unnamed")
        numbers = [str(n).strip() for n in (item.get("numbers") or []) if str(n).strip()]
        raw_rebate = item.get("rebate") or []
        rebates: List[int] = []
        for value in raw_rebate:
            try:
                rebates.append(int(value))
            except (TypeError, ValueError):
                rebates.append(1)
        if len(rebates) < len(numbers):
            rebates.extend([1] * (len(numbers) - len(rebates)))
        rebates = rebates[: len(numbers)]
        if numbers:
            payload.append({"name": name, "numbers": numbers, "rebate": rebates})
    return payload


def parse_results(stdout: str) -> Optional[List[Dict[str, Any]]]:
    """Extract the JSON array scraper.py prints after its log lines."""
    decoder = json.JSONDecoder()
    candidates = [i for i, ch in enumerate(stdout) if ch == "["]
    for index in reversed(candidates):  # the result block is printed last
        try:
            value, _ = decoder.raw_decode(stdout[index:])
        except ValueError:
            continue
        if isinstance(value, list):
            return value
    return None


# --------------------------------------------------------------------------- #
# JS API exposed to the page                                                   #
# --------------------------------------------------------------------------- #

class Api:
    def __init__(self) -> None:
        self.window: Any = None

    # -- info ------------------------------------------------------------- #
    def app_info(self) -> Dict[str, Any]:
        user, password = load_credentials()
        return {
            "desktop": True,
            "scraper": str(SCRAPER),
            "scraper_present": SCRAPER.exists(),
            "credentials": bool(user and password),
            "python": sys.version.split()[0],
        }

    def set_credentials(self, username: str, password: str) -> Dict[str, Any]:
        username = (username or "").strip()
        password = password or ""
        if not username or not password:
            return {"ok": False, "error": "Username and password are both required."}
        try:
            write_credentials(username, password)
        except OSError as exc:
            return {"ok": False, "error": f"Could not save credentials: {exc}"}
        return {"ok": True}

    # -- the button ------------------------------------------------------- #
    def generate_lists(self, lists: Any) -> Dict[str, Any]:
        """Run scraper.py for the supplied lists and return its results."""
        user, password = load_credentials()
        if not (user and password):
            return {
                "ok": False,
                "error": "No DOP credentials configured. Add DOP_USERNAME / DOP_PASSWORD to desktop/.env.",
            }
        if not SCRAPER.exists():
            return {"ok": False, "error": f"scraper.py not found at {SCRAPER}"}

        try:
            payload = build_payload(lists)
        except ValueError as exc:
            return {"ok": False, "error": str(exc)}
        if not payload:
            return {"ok": False, "error": "No account numbers to generate — add accounts to the list first."}

        names = ", ".join(entry["name"] for entry in payload)
        self._push(f"Starting scraper for: {names}")

        try:
            proc = subprocess.run(
                [sys.executable, str(SCRAPER), user, password, json.dumps(payload)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=SCRAPER_TIMEOUT,
            )
        except subprocess.TimeoutExpired:
            return {"ok": False, "error": f"Scraper timed out after {SCRAPER_TIMEOUT}s."}
        except OSError as exc:
            return {"ok": False, "error": f"Could not start scraper: {exc}"}

        stdout = proc.stdout or ""
        stderr = proc.stderr or ""
        results = parse_results(stdout)

        if results is None:
            tail = (stderr.strip() or stdout.strip())[-1500:]
            return {
                "ok": False,
                "returncode": proc.returncode,
                "error": "Scraper produced no parseable result.",
                "log": tail,
            }

        self._push("Scraper finished.")
        return {
            "ok": proc.returncode == 0,
            "results": results,
            "returncode": proc.returncode,
            "log": stderr.strip()[-1500:],
        }

    # -- helpers ---------------------------------------------------------- #
    def _push(self, message: str) -> None:
        if self.window is None:
            return
        try:
            self.window.evaluate_js(
                "window.__autodopProgress && window.__autodopProgress(%s)" % json.dumps(message)
            )
        except Exception:
            pass


# --------------------------------------------------------------------------- #
# entrypoints                                                                  #
# --------------------------------------------------------------------------- #

def run_selftest() -> int:
    """Headless check of credential loading, payload shaping and result parsing."""
    failures = 0

    def check(label: str, condition: bool, detail: str = "") -> None:
        nonlocal failures
        if not condition:
            failures += 1
        print("%s  %s%s" % ("PASS" if condition else "FAIL", label, "  [%s]" % detail if detail else ""))

    # credentials
    os.environ["DOP_USERNAME"] = "agent42"
    os.environ["DOP_PASSWORD"] = "s3cret"
    check("creds: from environment", load_credentials() == ("agent42", "s3cret"))
    del os.environ["DOP_USERNAME"]
    del os.environ["DOP_PASSWORD"]
    check("creds: absent when unset", load_credentials() == ("", ""))

    backup = ENV_FILE.read_text(encoding="utf-8") if ENV_FILE.exists() else None
    try:
        write_credentials("fileuser", "filepass")
        check("creds: round-trip via .env", load_credentials() == ("fileuser", "filepass"))
        check("creds: .env is private", oct(ENV_FILE.stat().st_mode)[-3:] == "600")
    finally:
        if backup is None:
            ENV_FILE.unlink(missing_ok=True)
        else:
            ENV_FILE.write_text(backup, encoding="utf-8")

    # payload shaping
    payload = build_payload([{"name": "A", "numbers": ["111", "222", "333"], "rebate": [4]}])
    check("payload: pads rebates to match numbers", payload[0]["rebate"] == [4, 1, 1])
    check("payload: keeps the name", payload[0]["name"] == "A")
    check("payload: drops empty lists", build_payload([{"name": "B", "numbers": []}]) == [])
    check(
        "payload: coerces junk rebate to 1",
        build_payload([{"name": "C", "numbers": ["9"], "rebate": ["x"]}])[0]["rebate"] == [1],
    )
    check("payload: rejects non-array", _expect_raises(build_payload, "nope") == "ValueError")

    # result parsing (scraper prints logs, then the JSON array)
    sample = (
        "Inside Login Page\n"
        "Processing list: A\n"
        "Account numbers: ['111']\n"
        "Generated number: 1234567890\n"
        '[\n  {\n    "list_name": "A",\n    "status": "success",\n'
        '    "details": {"gen_number": "1234567890"}\n  }\n]\n'
    )
    parsed = parse_results(sample)
    check("parse: finds trailing JSON array", isinstance(parsed, list) and len(parsed) == 1)
    check("parse: keeps list_name", bool(parsed) and parsed[0]["list_name"] == "A")
    check("parse: none when absent", parse_results("no json here") is None)

    print("\nSELFTEST: %s" % ("ALL GOOD" if failures == 0 else "%d FAILURE(S)" % failures))
    return 0 if failures == 0 else 1


def _expect_raises(fn, arg) -> str:
    """Return the exception type name when fn(arg) raises, else 'no-raise'."""
    try:
        fn(arg)
    except Exception as exc:  # noqa: BLE001 - selftest helper
        return type(exc).__name__
    return "no-raise"


def main() -> int:
    parser = argparse.ArgumentParser(description="AutoDOP desktop shell")
    parser.add_argument("--selftest", action="store_true", help="run headless bridge checks and exit")
    args = parser.parse_args()

    if args.selftest:
        return run_selftest()

    if not INDEX.exists():
        print(
            "The frontend build is missing (%s).\nRun:  cd frontend && npm install && npm run build" % INDEX,
            file=sys.stderr,
        )
        return 1

    try:
        import webview  # noqa: PLC0415 - imported late so --selftest needs no GUI deps
    except ImportError:
        print(
            "pywebview is not installed.\nRun:  python3 -m pip install -r desktop/requirements.txt",
            file=sys.stderr,
        )
        return 1

    api = Api()
    window = webview.create_window(
        "AutoDOP",
        url=str(INDEX),
        js_api=api,
        width=1280,
        height=860,
        min_size=(960, 600),
    )
    api.window = window
    webview.start()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())