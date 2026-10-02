import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Account, AccountList } from './types';

export interface GenList {
  name: string;
  numbers: string[];

  rebate: number[];
}

export interface GenResult {
  ok: boolean;
  results?: Array<{ list_name?: string; status?: string; details?: unknown }>;
  error?: string;
  returncode?: number;
  log?: string;

  log_path?: string;
}

export interface AppInfo {
  desktop: boolean;

  version: string;

  build: string;
  scraper: string;
  scraper_present: boolean;

  scraper_source: string;

  scraper_kind: string;
  credentials: boolean;
  python: string;
}


export interface ScraperLocation {
  path: string;
  source: string;

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


export interface LocalStatus {

  path: string;
  accounts: number;
  lists: number;
  entries: number;
  has_credentials: boolean;
  error?: string;
}


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


const PROGRESS_EVENT = 'scraper-progress';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}


export function isDesktop(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}


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


export interface SavedCredentials {

  stored_encrypted: boolean;

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


export interface OwnerInfo {
  id: string;
  username: string;
  has_credentials: boolean;
}


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


    if (message.includes('Incorrect password')) return { ok: false };
    return { ok: false, error: message };
  }
}


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


export interface PdfImportReport {
  imported: number;
  skipped_duplicates: number;
  unparsed: number;
}





export async function importAccountsPdf(
  path: string,
): Promise<{ ok: boolean; error?: string; report?: PdfImportReport }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    return {
      ok: true,
      report: await invoke<PdfImportReport>('import_accounts_pdf', { path }),
    };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}


export async function logoutDesktop(): Promise<void> {
  if (!isDesktop()) return;
  try {
    await invoke('logout');
  } catch {

  }
}


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


export async function localStatus(): Promise<LocalStatus | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<LocalStatus>('local_status');
  } catch {
    return null;
  }
}


export interface BackupOutcome {
  accounts: number;
  lists: number;
  entries: number;
  has_credentials: boolean;

  previous?: string;
}








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


export function dbListToLocal(list: DbList): AccountList {



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


export function localListToDb(list: AccountList, active: boolean): DbList {
  return {
    id: list.id,
    name: list.name,
    active,

    entries: list.accountIds.map((id) => ({ id, rebate: list.rebates?.[id] ?? 0 })),
  };
}


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


export interface DopCredentialStatus {
  username: string;
  source: 'env' | 'local' | 'config';
  has_password: boolean;
  detail?: string;
}


export async function dopCredentialsStatus(): Promise<DopCredentialStatus | null> {
  if (!isDesktop()) return null;
  try {
    return await invoke<DopCredentialStatus>('dop_credentials_status');
  } catch {
    return null;
  }
}


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
