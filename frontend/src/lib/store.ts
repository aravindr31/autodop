/**
 * Reactive application store (Zustand) with localStorage persistence
 * (spec §5). Every mutation is a pure action on the store — the UI layer
 * derives "addedIn" from list membership rather than duplicating it.
 *
 * The store also owns the live `accounts` array (seeded from
 * `src/data/accounts.json`) so accounts can be added/deleted, and a client-side
 * session credential (see `./auth.ts`) for the login gate.
 *
 * Persistence is a small hand-rolled layer on zustand's `create` + `subscribe`
 * (the upstream `zustand/middleware/persist` is absent from this npm mirror,
 * and rolling our own keeps the app dependency-light and SSR-safe).
 */
import { create } from 'zustand';
import type { Account, AccountList, AuthCredential, NewAccountInput } from './types';
import { SEED_ACCOUNTS } from './accounts';
import { makeCredential, verifyCredential } from './auth';
import { newId, nextListLabel } from './format';

export interface ListSummary {
  count: number;
  /** Sum of account Denomination values (numeric). */
  amount: number;
}

export interface AppState {
  // ---- persisted data ----
  accounts: Account[];
  lists: AccountList[];
  activeListId: string;
  submitEndpoint: string;
  auth: AuthCredential | null;

  // ---- session (not persisted) ----
  loggedIn: boolean;

  // ---- derived selectors ----
  accountByIdNow: (id: string) => Account | undefined;
  listNameOf: (accountId: string) => string;
  isAdded: (accountId: string) => boolean;
  totalsOf: (listId: string) => ListSummary;

  // ---- list actions ----
  createList: (name?: string) => { id: string; name: string };
  renameList: (id: string, name: string) => void;
  deleteList: (id: string) => void;
  setActiveList: (id: string) => void;
  addToActive: (accountId: string) => void;
  removeFromList: (listId: string, accountId: string) => void;
  clearList: (listId: string) => void;
  setSubmitEndpoint: (endpoint: string) => void;

  // ---- account actions ----
  addAccount: (input: NewAccountInput) => Account;
  deleteAccount: (id: string) => boolean;
  /** Replace the whole account list (e.g. after loading from Atlas). */
  setAccounts: (accounts: Account[]) => void;

  // ---- auth actions (async: WebCrypto) ----
  setupPassword: (password: string) => Promise<void>;
  login: (password: string) => Promise<boolean>;
  logout: () => void;
  changePassword: (oldPassword: string, newPassword: string) => Promise<boolean>;
}

const STORAGE_KEY = 'autodop-state-v1';
const PERSIST_VERSION = 1;

const DEFAULT_LISTS: AccountList[] = [{ id: 'main', name: 'A', accountIds: [] }];

function makeAccount(raw: unknown): Account | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  if (typeof r._id !== 'string') return null;
  return {
    _id: r._id,
    Number: String(r.Number ?? ''),
    Name: String(r.Name ?? ''),
    Denomination: String(r.Denomination ?? '0'),
    CNumber: String(r.CNumber ?? ''),
    Ref_Number: String(r.Ref_Number ?? ''),
    addedIn: String(r.addedIn ?? ''),
  };
}

function makeList(raw: unknown): AccountList | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as { id?: unknown; name?: unknown; accountIds?: unknown };
  if (typeof r.id !== 'string' || typeof r.name !== 'string') return null;
  const accountIds = Array.isArray(r.accountIds)
    ? r.accountIds.map(String).filter((s) => s.length > 0)
    : [];
  return { id: r.id, name: r.name, accountIds };
}

interface Persisted {
  accounts: Account[];
  lists: AccountList[];
  activeListId: string;
  submitEndpoint: string;
  auth: AuthCredential | null;
}

/** Reads + validates persisted state. Returns null when absent/invalid/SSR. */
function loadPersisted(): Persisted | null {
  if (typeof localStorage === 'undefined') return null;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as {
      version?: unknown;
      accounts?: unknown;
      lists?: unknown;
      activeListId?: unknown;
      submitEndpoint?: unknown;
      auth?: unknown;
    };
    if (parsed.version !== PERSIST_VERSION || !Array.isArray(parsed.lists)) return null;

    const lists = parsed.lists.map(makeList).filter((
      l,
    ): l is AccountList => l !== null);
    if (lists.length === 0) return null;
    // Migrate the legacy default name to the current "A" convention.
    if (!lists.some((l) => l.name === 'A')) {
      const legacy = lists.find((l) => l.name === 'Main List');
      if (legacy) legacy.name = 'A';
    }

    const accounts = Array.isArray(parsed.accounts)
      ? parsed.accounts.map(makeAccount).filter((a): a is Account => a !== null)
      : SEED_ACCOUNTS;

    const authRaw = parsed.auth as { salt?: unknown; hash?: unknown } | null | undefined;
    const auth =
      authRaw && typeof authRaw.salt === 'string' && typeof authRaw.hash === 'string'
        ? { salt: authRaw.salt, hash: authRaw.hash }
        : null;

    const activeId =
      typeof parsed.activeListId === 'string' && lists.some((l) => l.id === parsed.activeListId)
        ? parsed.activeListId
        : lists[0].id;

    return {
      accounts,
      lists,
      activeListId: activeId,
      submitEndpoint: typeof parsed.submitEndpoint === 'string' ? parsed.submitEndpoint : '',
      auth,
    };
  } catch {
    return null;
  }
}

/** Sanitized initial state — defaults when nothing (valid) is persisted. */
const initial = loadPersisted();
const INITIAL_ACCOUNTS = initial?.accounts ?? SEED_ACCOUNTS;
const INITIAL_LISTS = initial?.lists ?? DEFAULT_LISTS;
const INITIAL_ACTIVE = initial?.activeListId ?? DEFAULT_LISTS[0].id;
let INITIAL_ENDPOINT = initial?.submitEndpoint ?? '';
if (!INITIAL_ENDPOINT) {
  INITIAL_ENDPOINT = (import.meta.env?.PUBLIC_BACKEND_API_URL as string | undefined) ?? '';
}
const INITIAL_AUTH = initial?.auth ?? null;

export const useStore = create<AppState>()((set, get) => ({
  // ---- persisted data ----
  accounts: INITIAL_ACCOUNTS,
  lists: INITIAL_LISTS,
  activeListId: INITIAL_ACTIVE,
  submitEndpoint: INITIAL_ENDPOINT,
  auth: INITIAL_AUTH,

  // ---- session ----
  loggedIn: false,

  // ---- derived selectors ----
  accountByIdNow: (id) => get().accounts.find((a) => a._id === id),
  listNameOf: (accountId) =>
    get().lists.find((l) => l.accountIds.includes(accountId))?.name ?? '',
  isAdded: (accountId) => get().lists.some((l) => l.accountIds.includes(accountId)),
  totalsOf: (listId) => {
    const list = get().lists.find((l) => l.id === listId);
    if (!list) return { count: 0, amount: 0 };
    let count = 0;
    let amount = 0;
    for (const id of list.accountIds) {
      const acc = get().accounts.find((a) => a._id === id);
      if (!acc) continue;
      const n = Number(acc.Denomination.replace(/[^\d]/g, ''));
      if (!Number.isNaN(n)) amount += n;
      count += 1;
    }
    return { count, amount };
  },

  // ---- list actions ----
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
    const active = get().activeListId === id ? (remaining[0]?.id ?? '') : get().activeListId;
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

  // ---- account actions ----
  addAccount: (input) => {
    const account: Account = {
      _id: newId(),
      Number: input.Number.trim(),
      Name: input.Name.trim(),
      Denomination: input.Denomination.trim() || '0',
      CNumber: input.CNumber.trim(),
      Ref_Number: input.Ref_Number.trim(),
      addedIn: '',
    };
    set({ accounts: [...get().accounts, account] });
    return account;
  },

  deleteAccount: (id) => {
    if (!get().accounts.some((a) => a._id === id)) return false;
    set({
      accounts: get().accounts.filter((a) => a._id !== id),
      lists: get().lists.map((l) =>
        l.accountIds.includes(id)
          ? { ...l, accountIds: l.accountIds.filter((x) => x !== id) }
          : l
      ),
    });
    return true;
  },

  setAccounts: (accounts) => set({ accounts }),

  // ---- auth actions (async: WebCrypto) ----
  setupPassword: async (password) => {
    set({ auth: await makeCredential(password), loggedIn: true });
  },

  login: async (password) => {
    if (!(await verifyCredential(get().auth, password))) return false;
    set({ loggedIn: true });
    return true;
  },

  logout: () => set({ loggedIn: false }),

  changePassword: async (oldPassword, newPassword) => {
    if (newPassword.trim().length < 1) return false;
    if (!(await verifyCredential(get().auth, oldPassword))) return false;
    set({ auth: await makeCredential(newPassword), loggedIn: true });
    return true;
  },
}));

// Persist every change back to localStorage (browser only).
if (typeof localStorage !== 'undefined') {
  useStore.subscribe((state) => {
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          version: PERSIST_VERSION,
          accounts: state.accounts,
          lists: state.lists,
          activeListId: state.activeListId,
          submitEndpoint: state.submitEndpoint,
          auth: state.auth,
        }),
      );
    } catch {
      /* storage full / private mode — non-fatal */
    }
  });
}