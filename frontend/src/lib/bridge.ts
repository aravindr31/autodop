/**
 * Desktop-shell bridge (pywebview).
 *
 * When the app is loaded by the pywebview desktop host (desktop/main.py) a
 * `window.pywebview.api` object is injected, letting a click in the UI call
 * Python *in-process* — no HTTP API server, no port to manage. In a normal
 * browser this module reports `ready: false` and callers fall back to a hint.
 */
import { useEffect, useState } from 'react';

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

interface PywebviewApi {
  app_info(): Promise<AppInfo>;
  generate_lists(lists: GenList[]): Promise<GenResult>;
  set_credentials(username: string, password: string): Promise<{ ok: boolean; error?: string }>;
}

declare global {
  interface Window {
    pywebview?: { api: PywebviewApi };
    __autodopProgress?: (message: string) => void;
  }
}

/** True once pywebview has injected its API into the page. */
export function isDesktop(): boolean {
  return typeof window !== 'undefined' && Boolean(window.pywebview?.api);
}

/**
 * Resolve when the bridge is available (or after `timeoutMs`). pywebview
 * injects the API asynchronously and fires a `pywebviewready` event.
 */
export function waitForBridge(timeoutMs = 3000): Promise<boolean> {
  if (isDesktop()) return Promise.resolve(true);
  if (typeof window === 'undefined') return Promise.resolve(false);
  return new Promise((resolve) => {
    const done = (value: boolean) => {
      window.removeEventListener('pywebviewready', onReady);
      resolve(value);
    };
    const onReady = () => done(isDesktop());
    window.addEventListener('pywebviewready', onReady);
    setTimeout(() => done(isDesktop()), timeoutMs);
  });
}

export async function desktopInfo(): Promise<AppInfo | null> {
  const api = window.pywebview?.api;
  if (!api) return null;
  try {
    return await api.app_info();
  } catch {
    return null;
  }
}

/** Register the progress callback the Python host calls back into. */
export function onProgress(cb: (message: string) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  window.__autodopProgress = cb;
  return () => {
    if (window.__autodopProgress === cb) delete window.__autodopProgress;
  };
}

export async function generateLists(lists: GenList[]): Promise<GenResult> {
  const api = window.pywebview?.api;
  if (!api) return { ok: false, error: 'Desktop shell not available — open the app via desktop/main.py.' };
  try {
    return await api.generate_lists(lists);
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
}

export async function saveCredentials(username: string, password: string): Promise<{ ok: boolean; error?: string }> {
  const api = window.pywebview?.api;
  if (!api) return { ok: false, error: 'Desktop shell not available.' };
  try {
    return await api.set_credentials(username, password);
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