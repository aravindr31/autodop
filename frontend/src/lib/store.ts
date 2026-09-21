/**
 * Reactive application store (Zustand) with localStorage persistence
 * (spec §5). Every mutation is a pure action on the store — the UI layer
 * derives "addedIn" from list membership rather than duplicating it.
 *
 * Persistence is a small hand-rolled layer on zustand's `create` + `subscribe`
 * (the upstream `zustand/middleware/persist` is absent from this npm mirror,
 * and rolling our own keeps the app dependency-light and SSR-safe).
 */
import { create } from 'zustand';
import type { AccountList, PersistedState } from './types';
import { accountById } from './accounts';
import { newId, nextListLabel } from './format';

export interface ListSummary {
  count: number;
  /** Sum of account Denomination values (numeric). */
  amount: number;
}

export interface AppState extends PersistedState {
  // ---- derived selectors ----
  /** Name of the list (if any) that currently contains the given account. */
  listNameOf: (accountId: string) => string;
  /** Whether an account belongs to any list (drives the "Add" button state). */
  isAdded: (accountId: string) => boolean;
  /** Live count + total denomination for a list. */
  totalsOf: (listId: string) => ListSummary;

  // ---- actions ----
  createList: (name?: string) => { id: string; name: string };
  renameList: (id: string, name: string) => void;
  deleteList: (id: string) => void;
  setActiveList: (id: string) => void;
  addToActive: (accountId: string) => void;
  removeFromList: (listId: string, accountId: string) => void;
  clearList: (listId: string) => void;
  setSubmitEndpoint: (endpoint: string) => void;
}

const STORAGE_KEY = 'autodop-state-v1';
const PERSIST_VERSION = 1;

const DEFAULT_LISTS: AccountList[] = [{ id: 'main', name: 'A', accountIds: [] }];

function makeList(raw: unknown): AccountList | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as { id?: unknown; name?: unknown; accountIds?: unknown };
  if (typeof r.id !== 'string' || typeof r.name !== 'string') return null;
  const accountIds = Array.isArray(r.accountIds)
    ? r.accountIds.map(String).filter((s) => s.length > 0)
    : [];
  return { id: r.id, name: r.name, accountIds };
}

/** Reads + validates persisted state. Returns null when absent/invalid/SSR. */
function loadPersisted(): { lists: AccountList[]; activeListId: string; submitEndpoint: string } | null {
  if (typeof localStorage === 'undefined') return null;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as {
      version?: unknown;
      lists?: unknown;
      activeListId?: unknown;
      submitEndpoint?: unknown;
    };
    if (parsed.version !== PERSIST_VERSION || !Array.isArray(parsed.lists)) return null;
    const lists = parsed.lists.map(makeList).filter((l): l is AccountList => l !== null);
    if (lists.length === 0) return null;
    // Migrate the legacy default name to the current "A" convention.
    if (!lists.some((l) => l.name === 'A')) {
      const legacy = lists.find((l) => l.name === 'Main List');
      if (legacy) legacy.name = 'A';
    }
    const activeId =
      typeof parsed.activeListId === 'string' && lists.some((l) => l.id === parsed.activeListId)
        ? parsed.activeListId
        : lists[0].id;
    return {
      lists,
      activeListId: activeId,
      submitEndpoint: typeof parsed.submitEndpoint === 'string' ? parsed.submitEndpoint : '',
    };
  } catch {
    return null;
  }
}

/** Sanitized initial state — default when nothing (valid) is persisted. */
const initial = loadPersisted();
const INITIAL_LISTS = initial?.lists ?? DEFAULT_LISTS;
const INITIAL_ACTIVE = initial?.activeListId ?? DEFAULT_LISTS[0].id;
let INITIAL_ENDPOINT = initial?.submitEndpoint ?? '';
if (!INITIAL_ENDPOINT) {
  INITIAL_ENDPOINT = (import.meta.env?.PUBLIC_BACKEND_API_URL as string | undefined) ?? '';
}

export const useStore = create<AppState>()((set, get) => ({
  // ---- persisted data ----
  lists: INITIAL_LISTS,
  activeListId: INITIAL_ACTIVE,
  submitEndpoint: INITIAL_ENDPOINT,

  // ---- derived selectors ----
  listNameOf: (accountId) =>
    get().lists.find((l) => l.accountIds.includes(accountId))?.name ?? '',
  isAdded: (accountId) => get().lists.some((l) => l.accountIds.includes(accountId)),
  totalsOf: (listId) => {
    const list = get().lists.find((l) => l.id === listId);
    if (!list) return { count: 0, amount: 0 };
    let count = 0;
    let amount = 0;
    for (const id of list.accountIds) {
      const acc = accountById(id);
      if (!acc) continue;
      const n = Number(acc.Denomination.replace(/[^\d]/g, ''));
      if (!Number.isNaN(n)) amount += n;
      count += 1;
    }
    return { count, amount };
  },

  // ---- actions ----
  createList: (name) => {
    const trimmed = (name ?? '').trim();
    const id = newId();
    const list: AccountList = {
      id,
      name: trimmed || nextListLabel(get().lists.map((l) => l.name)),
      accountIds: [],
    };
    set({ lists: [...get().lists, list], activeListId: id });
    return { id, name: list.name };
  },

  renameList: (id, name) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    set({
      lists: get().lists.map((l) => (l.id === id ? { ...l, name: trimmed } : l)),
    });
  },

  deleteList: (id) => {
    const remaining = get().lists.filter((l) => l.id !== id);
    const active =
      get().activeListId === id ? (remaining[0]?.id ?? '') : get().activeListId;
    set({ lists: remaining, activeListId: active });
  },

  setActiveList: (id) => {
    if (get().lists.some((l) => l.id === id)) set({ activeListId: id });
  },

  addToActive: (accountId) => {
    const list = get().lists.find((l) => l.id === get().activeListId);
    if (!list) return;
    if (get().isAdded(accountId)) return; // no-op: already belongs to a list
    set({
      lists: get().lists.map((l) =>
        l.id === list.id ? { ...l, accountIds: [...l.accountIds, accountId] } : l
      ),
    });
  },

  removeFromList: (listId, accountId) => {
    set({
      lists: get().lists.map((l) =>
        l.id === listId
          ? { ...l, accountIds: l.accountIds.filter((id) => id !== accountId) }
          : l
      ),
    });
  },

  clearList: (listId) => {
    set({
      lists: get().lists.map((l) => (l.id === listId ? { ...l, accountIds: [] } : l)),
    });
  },

  setSubmitEndpoint: (endpoint) => set({ submitEndpoint: endpoint.trim() }),
}));

// Persist every change back to localStorage (browser only).
if (typeof localStorage !== 'undefined') {
  useStore.subscribe((state) => {
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          version: PERSIST_VERSION,
          lists: state.lists,
          activeListId: state.activeListId,
          submitEndpoint: state.submitEndpoint,
        }),
      );
    } catch {
      /* storage full / private mode — non-fatal */
    }
  });
}