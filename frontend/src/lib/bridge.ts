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
  /** App version, from tauri.conf.json — also names the installer. */
  version: string;
  /** `<short sha> <commit date>` for the running build. */
  build: string;
  scraper: string;
  scraper_present: boolean;
  /** `chosen` | `env` | `sidecar` | `bundled` | `repo` | `cwd`. */
  scraper_source: string;
  /** `sidecar` (self-contained) | `script` (needs Python). */
  scraper_kind: string;
  credentials: boolean;
  python: string;
}

/** Where the runner was found, and where it came from. */
export interface ScraperLocation {
  path: string;
  source: string;
  /** `sidecar` (self-contained) | `script` (needs Python). */
  kind: string;
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

/** The local database this app owns. */
export interface LocalStatus {
  /** Absolute path to the SQLite file. */
  path: string;
  accounts: number;
  lists: number;
  entries: number;
  has_credentials: boolean;
  error?: string;
}

/** A list as stored in the local database. */
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
  /** Encrypted before it was stored. Always true. */
  stored_encrypted: boolean;
  /** The local database file it now lives in. */
  location: string;
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

/** One workspace known on this machine. */
export interface OwnerInfo {
  id: string;
  username: string;
  has_credentials: boolean;
}

/** Which workspaces exist, and which one this session belongs to. */
export interface AuthStatus {
  configured: boolean;
  unlocked: boolean;
  owners: OwnerInfo[];
  current: string | null;
}

export async function authStatus(): Promise<AuthStatus | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<AuthStatus>('auth_status');
  } catch {
    return null;
  }
}

/**
 * Create a workspace on first run: username (the DOP portal id, a mobile
 * number) plus a login password.
 *
 * In the desktop app this also derives the key that protects the DOP password,
 * which is why the password is sent to the backend rather than hashed here.
 */
export async function setupLogin(
  username: string,
  password: string,
): Promise<{ ok: boolean; error?: string; owner?: OwnerInfo }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    const owner = await invoke<OwnerInfo>('setup_login', { username, password });
    return { ok: true, owner };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Verify the login password for `ownerId` and unlock this session.
 *
 * `false` means the password was wrong; an error string means the workspace
 * itself is gone.
 */
export async function loginPassword(
  ownerId: string,
  password: string,
): Promise<{ ok: boolean; error?: string; owner?: OwnerInfo }> {
  if (!isDesktop()) return { ok: false };
  try {
    const owner = await invoke<OwnerInfo>('login', { ownerId, password });
    return { ok: true, owner };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    // A wrong password is reported as an error string by the backend; only a
    // mismatch is "just wrong", anything else is worth surfacing.
    if (message.includes('Incorrect password')) return { ok: false };
    return { ok: false, error: message };
  }
}

/** Insert or update one account under the signed-in owner. */
export async function saveAccount(
  account: Account,
): Promise<{ ok: boolean; error?: string; id?: string }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return { ok: true, id: await invoke<string>('save_account', { account }) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Remove one account under the signed-in owner; list entries go with it. */
export async function deleteAccount(
  id: string,
): Promise<{ ok: boolean; error?: string }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    await invoke('delete_account', { id });
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Forget the derived key. */
export async function logoutDesktop(): Promise<void> {
  if (!isDesktop()) return;
  try {
    await invoke('logout');
  } catch {
    /* nothing useful to do */
  }
}

/** Change the login password, re-wrapping the stored DOP password with it. */
export async function changeLoginPassword(
  oldPassword: string,
  newPassword: string,
): Promise<{ ok: boolean; error?: string }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    await invoke('change_login_password', { oldPassword, newPassword });
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** What the local database currently holds. */
export async function localStatus(): Promise<LocalStatus | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<LocalStatus>('local_status');
  } catch {
    return null;
  }
}

/** What a backup or restore did, as reported by the backend. */
export interface BackupOutcome {
  accounts: number;
  lists: number;
  entries: number;
  has_credentials: boolean;
  /** The backup file written (export), or the safety copy of the replaced database (restore). */
  previous?: string;
}

/**
 * Write a full backup of the local database to `path`.
 *
 * The file is a complete SQLite copy — accounts, lists, the encrypted DOP
 * password, the login hash and salt. It only opens with the login password
 * that key was derived from, so a backup is the file *plus* that password.
 */
export async function exportBackup(
  path: string,
): Promise<{ ok: boolean; error?: string; outcome?: BackupOutcome }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return { ok: true, outcome: await invoke<BackupOutcome>('export_backup', { path }) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Replace the local database with the backup at `path`.
 *
 * The replaced database is kept as `autodop.db.pre-restore-<stamp>` next to
 * the live file, and you are signed out — the restored file may expect a
 * different login password.
 */
export async function importBackup(
  path: string,
): Promise<{ ok: boolean; error?: string; outcome?: BackupOutcome }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return { ok: true, outcome: await invoke<BackupOutcome>('import_backup', { path }) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Write a portable JSON backup (accounts + lists as plain JSON, the DOP
 * password as the token encrypted with the current app login password) to
 * `path`. Importable on any machine.
 */
export async function exportPortableBackup(
  path: string,
): Promise<{ ok: boolean; error?: string; outcome?: BackupOutcome }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return { ok: true, outcome: await invoke<BackupOutcome>('export_portable_backup', { path }) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Replace the local database with the portable JSON backup at `path`.
 *
 * `loginPassword` is the app login password the backup was made with — it
 * opens the carried DOP password, which is then re-encrypted under this
 * machine's login.
 */
export async function importPortableBackup(
  path: string,
  loginPassword: string,
): Promise<{ ok: boolean; error?: string; outcome?: BackupOutcome }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return {
      ok: true,
      outcome: await invoke<BackupOutcome>('import_portable_backup', {
        path,
        loginPassword,
      }),
    };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Load every account from the local database. */
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

/** Convert a stored list into the local list shape. */
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

/** Convert a local list into the stored shape. */
export function localListToDb(list: AccountList, active: boolean): DbList {
  return {
    id: list.id,
    name: list.name,
    active,
    // 0 matches the Streamlit UI's `acc.get("Rebate", 0)` default.
    entries: list.accountIds.map((id) => ({ id, rebate: list.rebates?.[id] ?? 0 })),
  };
}

/** Load the saved lists from the local database. */
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

/** Upsert the local lists; resolves with what is now stored. */
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
  source: 'env' | 'local' | 'config';
  has_password: boolean;
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