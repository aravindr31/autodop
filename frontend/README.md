# AutoDOP — Frontend

Modern Astro.js rewrite of the AutoDOP account-list manager (per `spec.md`),
built on **Astro 7 + React 19 + Zustand + Tailwind CSS v4**.

Organize India Post DOP agent accounts into persistent, configurable lists —
with live totals, search, and backend submission — directly in the browser.

## Stack

| Concern       | Choice                                                    |
| ------------- | --------------------------------------------------------- |
| Framework     | Astro 7 (static shell + React island, spec §7)            |
| State         | Zustand, persisted to `localStorage` (spec §5)            |
| Styling       | Tailwind CSS v4 (`@tailwindcss/vite`)                     |
| Icons         | `lucide-react`                                            |
| Data (dev)    | Static JSON in `src/data/accounts.json` (spec §7)         |

> Note: `zustand/middleware/persist` is **not** used — the current npm mirror
> ships its type stubs but no implementation. Persistence is a small hand-rolled
> layer on Zustand's `create` + `subscribe` (see `src/lib/store.ts`), which is
> also SSR-safe (no `localStorage` touched at import on the server).

## Scripts

```bash
npm install          # install deps
npm run dev          # dev server (http://localhost:4321)
npm run build        # production build -> dist/
npm run preview      # serve the built site
npm run smoke        # store + persistence integration test (Node/esbuild)
npx astro check      # typecheck
```

## Running

```bash
npm install
npm run dev
```

The app boots with a demo dataset (160 accounts) matching the document shape in
spec §2 (`_id`, `Number`, `Name`, `Denomination`, `CNumber`, `Ref_Number`,
`addedIn`). Replace `src/data/accounts.json` with your real data — the shape is
left as-is (BSON `_id: { $oid }` included).

## Usage

- **Accounts** tab — search by name/number/CNumber/ref, then **Add** to the
  active list. Already-added accounts show which list they're in.
- **Lists** (left panel) — **Create List** adds the next alphabetical list
  (default list is **A**, then B, C … Z, AA, …); lists render alphabetically.
  Select (active) / rename / delete. All state persists across sessions via
  `localStorage`.
- **Lists** tab — summary cards (name, item count, total ₹ denomination) that
  expand to show items with **Remove**, plus **Copy numbers**, **Clear list**,
  and **Submit to backend**.
- **Sign in** — the app gates behind a login (first run asks you to set a
  password). This is a *client-side* gate only (see `src/lib/auth.ts`); the
  real credential check ships with the backend.
- **Manage** (gear, top-right) — **Add New Account**, **Delete Account**
  (search + confirm), **Change Password**, and **Sign out**. Account
  additions/deletions hit the live store, reflect immediately in the Accounts
  tab, and persist.

## Backend submission

The **Lists → Backend** field stores a POST endpoint (persisted to
`localStorage`). Submitting posts the array of that list's account objects
(exact §2 shape, with `addedIn` set) as JSON:

```http
POST <endpoint>
Content-Type: application/json

[{ "Number": "…", "Name": "…", "Denomination": "…", "CNumber": "…",
   "Ref_Number": "…", "addedIn": "<list name>", "_id": { "$oid": "…" } }, …]
```

You can also set the default via env (copy `.env.example` → `.env`):

```
PUBLIC_BACKEND_API_URL=https://api.example.com/submit
```

The backend itself is out of scope (`spec.md` §8 — handled separately).

## Project layout

```
frontend/
├─ astro.config.mjs        # [react] integration + Tailwind vite plugin
├─ scripts/
│  ├─ run-smoke.mjs        # bundles store + runs npm smoke
│  └─ store-smoke.mjs      # two-stage store/persistence assertions
└─ src/
   ├─ data/accounts.json   # dev dataset (replace with real data)
   ├─ lib/                 # types, store, api client, format, toast
   ├─ components/          # App shell, Sidebar, Browser, ListsView, ui
   ├─ styles/global.css    # Tailwind entry (thin base layer)
   └─ pages/index.astro    # static shell hosting the React island
```