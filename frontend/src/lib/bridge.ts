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

export interface GenList {
  name: string;
  numbers: string[];
  /** Per-account rebate (installment no.). `1` means "no rebate, just pay". */
  rebate: number[];
}

export interface GenResult {
  ok: boolean;
  results?: Array<{ list_name?: string; status?: string; details?: unknown }>;
  error?: string;
  returncode?: number;
  log?: string;
}

export interface AppInfo {
  desktop: boolean;
  scraper: string;
  scraper_present: boolean;
  credentials: boolean;
  python: string;
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

export async function saveCredentials(
  username: string,
  password: string,
): Promise<{ ok: boolean; error?: string }> {
  if (!isDesktop()) return { ok: false, error: 'Desktop app not available.' };
  try {
    await invoke('set_credentials', { username, password });
    return { ok: true };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
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