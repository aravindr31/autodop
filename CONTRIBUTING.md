# Contributing to AutoDOP

Thanks for helping. This document covers what you need to develop locally and
the conventions the project's automation depends on — the release pipeline
reads your commit messages and builds releases from them, so a few rules are
load-bearing rather than stylistic.

## Getting set up

Prerequisites:

| Need | Why |
| --- | --- |
| Node 20+ | frontend build and the Tauri CLI |
| Rust + Cargo (`rustup`) | the desktop backend |
| Python 3 + `pip install selenium webdriver-manager pyinstaller` | only to rebuild the scraper sidecar |
| Google Chrome | what the Selenium runner drives |

```bash
git clone <repo> && cd AutoDOP
npm install && npm --prefix frontend install

npm run dev        # hot-reloading dev window
```

To test the full packaging locally:

```bash
npm run build:sidecar   # freeze scraper.py for your OS (PyInstaller)
npm run build           # macOS .dmg / Windows .exe+.msi / Linux .AppImage+.deb
```

The sidecar cannot cross-compile — build it on the OS you target.

## Branches and pull requests

- PRs must come from a **`feature/*`**, **`fix/*`** or **`hotfix/*`** branch.
  CI rejects PRs to `main` from any other branch — including `main` itself and
  `develop`.
- Keep PRs focused: one feature or one fix. A PR that also carries the next
  version bump is fine when it is the release PR.
- CI runs on every push: `cargo fmt --check`, the Rust test suite, the frontend
  type check and the store smoke test. All green is required before merge.

## Conventional commits (required)

Every commit message starts with a type. The release pipeline parses these to
decide the next version — this is why the convention is enforced by review:

| Type | Version effect on merge to `main` |
| --- | --- |
| `fix: …` | patch — 0.7.0 → 0.7.1 |
| `feat: …` | minor — 0.7.0 → 0.8.0 |
| `anything!: …` or a `BREAKING CHANGE:` footer | major — 0.7.0 → 1.0.0 |
| `chore:` `docs:` `ci:` `refactor:` `perf:` `test:` | none on its own |

Optional scope: `feat(pdf): accept multi-page printouts`. A merge containing
none of `feat`/`fix`/breaking still releases a patch — every merge to `main`
ships a newer build, so write the type that honestly reflects the change.

Good messages say *why*, briefly:

```text
fix: keep the rebate when a list is renamed

save_lists matched on name first, so renaming dropped the stored rebate.
Match on id first, fall back to name only for client-created ids.
```

## What happens when a PR merges to `main`

1. `test` runs the full suite on the merge commit. Failing code does not ship.
2. The next version is derived from the commit messages since the last release
   and written into `VERSION`, `CHANGELOG.md` and all four version fields by
   `scripts/release-version.mjs`. That lands as a
   `chore(release): vX.Y.Z [skip ci]` commit, tagged `vX.Y.Z`.
3. Three runners build macOS (DMG), Windows (NSIS/MSI) and Linux
   (AppImage/deb) from exactly that tagged commit and attach the installers to
   the GitHub release, with the new changelog section as the release notes.

Merges touching only docs, workflows or the changelog skip the release
workflow entirely — they are pipeline noise by design.

You never hand-edit versions. `VERSION` and `CHANGELOG.md` are owned by the
pipeline; local builds that need a specific number can use
`npm run version:bump -- X.Y.Z`, which writes the same fields.

## Code conventions

- **Rust** (`src-tauri/`): `cargo fmt` clean; no new clippy warnings. Prefer
  comments that explain *why*; keep the SQLite schema changes accompanied by a
  migration in `Store::migrate` and tests that open a database built by the
  previous schema version.
- **Frontend** (`frontend/src/`): TypeScript strict via `astro check`; state
  lives in the zustand store (`src/lib/store.ts`) rather than component-local
  duplicates; the desktop bridge (`src/lib/bridge.ts`) is the only place that
  calls `invoke`.
- **Tests**: behaviour changes come with tests. The Rust suite covers storage,
  crypto and parsing; the smoke test covers the store's persisted state
  machine. If you fix a bug, add the test that would have caught it.

## Reporting bugs

Open a GitHub issue with:

- the version (`Manage → This build` shows `<sha> <time>`)
- your OS
- what you did, what you expected, what happened
- for Generate failures: the run log — **the password is already redacted**,
  but double-check before pasting anything

## License

By contributing you agree that your contributions are licensed under the
[PolyForm Noncommercial License 1.0.0](LICENSE) — free for personal,
non-commercial use; commercial use requires the maintainer's written agreement.