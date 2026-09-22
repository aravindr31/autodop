/**
 * Desktop bridge (Tauri).
 *
 * When the page runs inside the Tauri shell, `invoke()` calls straight into the
 * Rust backend, which can spawn `scraper.py` as a local process — exactly what
 * a plain browser page is forbidden from doing. Outside the shell (e.g. running
 * `npm run dev` in `frontend/` alone) `isDesktop()` is false and callers show a
 * hint instead of failing silently.
 */
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Account, AccountList } from './types';

export interface GenList {
  name: string;
  numbers: string[];
  /** Per-account rebate (RD installment no.). `1` makes scraper.py skip the step. */
  rebate: number[];
}

export interface GenResult {
  ok: boolean;
  results?: Array<{ list_name?: string; status?: string; details?: unknown }>;
  error?: string;
  returncode?: number;
  log?: string;
  /** Path to the full run log, written by the backend. */
  log_path?: string;
}

export interface AppInfo {
  desktop: boolean;
  scraper: string;
  scraper_present: boolean;
  /** `chosen` | `env` | `bundled` | `repo` | `cwd`. */
  scraper_source: string;
  credentials: boolean;
  python: string;
}

/** Where `scraper.py` was found, and where it came from. */
export interface ScraperLocation {
  path: string;
  source: string;
  present: boolean;
}

export async function scraperLocation(): Promise<ScraperLocation | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<ScraperLocation>('scraper_location');
  } catch {
    return null;
  }
}

/** Point the app at a different `scraper.py`. */
export async function setScraperPath(
  path: string,
): Promise<{ ok: boolean; error?: string; location?: ScraperLocation }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return { ok: true, location: await invoke<ScraperLocation>('set_scraper_path', { path }) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Drop the override and fall back to the bundled copy. */
export async function clearScraperPath(): Promise<{
  ok: boolean;
  error?: string;
  location?: ScraperLocation;
}> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return { ok: true, location: await invoke<ScraperLocation>('clear_scraper_path') };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Atlas connection state reported by the Rust backend. */
export interface DbStatus {
  configured: boolean;
  connected: boolean;
  db: string;
  collection: string;
  /** False when the Atlas user can only read — saving lists would be refused. */
  writable?: boolean;
  /** Authenticated roles, e.g. `['readAnyDatabase']`. */
  roles?: string[];
  count?: number;
  error?: string;
}

/** A list as stored in Atlas (`savedList`). */
export interface DbListEntry {
  id: string;
  rebate: number;
}

export interface DbList {
  id: string;
  name: string;
  active: boolean;
  entries: DbListEntry[];
}

/** Event name the Rust side streams scraper output on. */
const PROGRESS_EVENT = 'scraper-progress';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

/** True when running inside the Tauri webview. */
export function isDesktop(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Tauri injects its API before page scripts run, so this resolves at once. */
export function waitForBridge(): Promise<boolean> {
  return Promise.resolve(isDesktop());
}

export async function desktopInfo(): Promise<AppInfo | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<AppInfo>('app_info');
  } catch {
    return null;
  }
}

/** Subscribe to live scraper output. Returns an unsubscribe function. */
export function onProgress(cb: (message: string) => void): () => void {
  if (!isDesktop()) return () => {};
  const pending = listen<string>(PROGRESS_EVENT, (event) => cb(event.payload));
  return () => {
    void pending.then((unlisten) => unlisten()).catch(() => {});
  };
}

export async function generateLists(lists: GenList[]): Promise<GenResult> {
  if (!isDesktop()) {
    return { ok: false, error: 'Generate needs the desktop app — launch it with `npm run dev` (Tauri).' };
  }
  try {
    return await invoke<GenResult>('generate_lists', { lists });
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Where a saved DOP password ended up. */
export interface SavedCredentials {
  /** Written to the app-config file, encrypted. */
  stored_encrypted: boolean;
  /** The `users` document in Atlas was updated too. */
  atlas_updated: boolean;
  /** Why the Atlas copy was left alone (read-only role, offline, ...). */
  atlas_note?: string | null;
}

export async function saveCredentials(
  username: string,
  password: string,
): Promise<{ ok: boolean; error?: string; result?: SavedCredentials }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    const result = await invoke<SavedCredentials>('set_credentials', { username, password });
    return { ok: true, result };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Ping Atlas: is a URI configured, and is the cluster reachable? */
export async function dbStatus(): Promise<DbStatus | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<DbStatus>('db_status');
  } catch {
    return null;
  }
}

/** Fetch all accounts from Atlas (db `accounts`, collection `accountHolders`). */
export async function loadAccountsFromDb(): Promise<{
  ok: boolean;
  accounts?: Account[];
  error?: string;
}> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    const accounts = await invoke<Account[]>('load_accounts');
    return { ok: true, accounts };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Convert an Atlas `savedList` document into the local list shape. */
export function dbListToLocal(list: DbList): AccountList {
  // Keep every rebate, including 1: it is a meaningful value ("skip the rebate
  // step" in scraper.py). Filtering it out here made an explicit 1 silently
  // revert to the default on the next load.
  const rebates: Record<string, number> = {};
  for (const entry of list.entries) {
    rebates[entry.id] = entry.rebate;
  }
  const local: AccountList = {
    id: list.id,
    name: list.name,
    accountIds: list.entries.map((entry) => entry.id),
  };
  if (Object.keys(rebates).length > 0) local.rebates = rebates;
  return local;
}

/** Convert a local list into the document shape Atlas expects. */
export function localListToDb(list: AccountList, active: boolean): DbList {
  return {
    id: list.id,
    name: list.name,
    active,
    // 0 matches the Streamlit UI's `acc.get("Rebate", 0)` default.
    entries: list.accountIds.map((id) => ({ id, rebate: list.rebates?.[id] ?? 0 })),
  };
}

/** Load the saved lists from Atlas. */
export async function loadLists(): Promise<{
  ok: boolean;
  lists?: AccountList[];
  activeId?: string;
  error?: string;
}> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    const raw = await invoke<DbList[]>('load_lists');
    return {
      ok: true,
      lists: raw.map(dbListToLocal),
      activeId: raw.find((list) => list.active)?.id,
    };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Upsert the local lists to Atlas; resolves with what is now stored. */
export async function saveLists(
  lists: AccountList[],
  activeListId: string,
): Promise<{ ok: boolean; lists?: AccountList[]; error?: string }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    const payload = lists.map((list) => localListToDb(list, list.id === activeListId));
    const saved = await invoke<DbList[]>('save_lists', { lists: payload });
    return { ok: true, lists: saved.map(dbListToLocal) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Which DOP credentials the app would use, and where they come from. */
export interface DopCredentialStatus {
  username: string;
  source: 'env' | 'config' | 'atlas';
  has_password: boolean;
  atlas_available: boolean;
  detail?: string;
}

/** Credential source + portal id. The password is never part of this. */
export async function dopCredentialsStatus(): Promise<DopCredentialStatus | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<DopCredentialStatus>('dop_credentials_status');
  } catch {
    return null;
  }
}

/** Reactive bridge state for components. */
export function useDesktop(): { ready: boolean; info: AppInfo | null } {
  const [ready, setReady] = useState(() => isDesktop());
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    let cancelled = false;
    void waitForBridge().then(async (ok) => {
      if (cancelled) return;
      setReady(ok);
      if (ok) setInfo(await desktopInfo());
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return { ready, info };
}