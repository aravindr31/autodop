import { create } from 'zustand';
import type { Account, AccountList, AuthCredential, NewAccountInput } from './types';
import type { OwnerInfo } from './bridge';
import { SEED_ACCOUNTS } from './accounts';
import { makeCredential, verifyCredential } from './auth';
import { newId, nextListLabel } from './format';
import {
  authStatus,
  changeLoginPassword,
  dbListToLocal,
  deleteAccount as deleteAccountRemote,
  isDesktop,
  loadAccountsFromDb,
  loadLists,
  loginPassword,
  logoutDesktop,
  saveAccount,
  setupLogin,
} from './bridge';

export interface ListSummary {
  count: number;

  amount: number;
}

export interface AppState {

  accounts: Account[];
  lists: AccountList[];
  activeListId: string;
  submitEndpoint: string;
  auth: AuthCredential | null;


  loggedIn: boolean;

  owners: OwnerInfo[];

  currentOwner: OwnerInfo | null;

  selectedOwner: OwnerInfo | null;




  authConfigured: boolean;

  authReady: boolean;


  accountByIdNow: (id: string) => Account | undefined;
  listNameOf: (accountId: string) => string;
  isAdded: (accountId: string) => boolean;
  totalsOf: (listId: string) => ListSummary;


  createList: (name?: string) => { id: string; name: string };
  renameList: (id: string, name: string) => void;
  deleteList: (id: string) => void;
  setActiveList: (id: string) => void;
  addToActive: (accountId: string) => void;
  removeFromList: (listId: string, accountId: string) => void;

  setRebate: (listId: string, accountId: string, rebate: number) => void;
  clearList: (listId: string) => void;

  clearAllLists: () => number;
  setSubmitEndpoint: (endpoint: string) => void;


  addAccount: (input: NewAccountInput) => Account;
  deleteAccount: (id: string) => boolean;

  setAccounts: (accounts: Account[]) => void;

  setLists: (lists: AccountList[], activeListId?: string) => void;


  setupPassword: (username: string, password: string) => Promise<void>;

  login: (password: string, owner?: OwnerInfo) => Promise<boolean>;

  selectOwner: (owner: OwnerInfo) => void;
  logout: () => void;
  changePassword: (oldPassword: string, newPassword: string) => Promise<boolean>;

  refreshAuth: () => Promise<void>;
}

const STORAGE_KEY = 'autodop-state-v1';
const OWNER_KEY = 'autodop-last-owner';
const PERSIST_VERSION = 1;


function rememberOwner(owner: OwnerInfo | null): void {
  try {
    if (owner) localStorage.setItem(OWNER_KEY, JSON.stringify({ id: owner.id, username: owner.username }));
    else localStorage.removeItem(OWNER_KEY);
  } catch {   }
}

function rememberedOwner(): OwnerInfo | null {
  try {
    const raw = localStorage.getItem(OWNER_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (typeof parsed?.id === 'string' && typeof parsed?.username === 'string') {
      return { id: parsed.id, username: parsed.username, has_credentials: false };
    }
  } catch {   }
  return null;
}








function clearWorkspaceCache(): { accounts: Account[]; lists: AccountList[]; activeListId: string } {
  try { localStorage.removeItem(STORAGE_KEY); } catch {   }
  if (isDesktop()) {
    return { accounts: [], lists: structuredClone(DEFAULT_LISTS), activeListId: DEFAULT_LISTS[0].id };
  }
  return { accounts: SEED_ACCOUNTS, lists: structuredClone(DEFAULT_LISTS), activeListId: DEFAULT_LISTS[0].id };
}


async function loadWorkspaceFromDb(): Promise<{
  accounts: Account[];
  lists?: AccountList[];
  activeListId?: string;
} | null> {
  const accounts = await loadAccountsFromDb();
  if (!accounts.ok || !accounts.accounts) return null;
  const lists = await loadLists();
  if (!lists.ok || !lists.lists) return { accounts: accounts.accounts };
  return {
    accounts: accounts.accounts,
    lists: lists.lists,
    activeListId: lists.activeId,
  };
}

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

  accounts: INITIAL_ACCOUNTS,
  lists: INITIAL_LISTS,
  activeListId: INITIAL_ACTIVE,
  submitEndpoint: INITIAL_ENDPOINT,
  auth: INITIAL_AUTH,


  loggedIn: false,


  authConfigured: true,
  authReady: !isDesktop(),
  owners: [],
  currentOwner: null,
  selectedOwner: rememberedOwner(),


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
      const n = Number(acc.Denomination.replace(/[^\d.]/g, ''));
      if (!Number.isNaN(n)) amount += n;
      count += 1;
    }
    return { count, amount };
  },


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
    if (get().isAdded(accountId)) return;
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
      lists: get().lists.map((l) =>
        l.id === listId ? { ...l, accountIds: [], rebates: {} } : l
      ),
    });
  },

  clearAllLists: () => {
    const lists = get().lists;
    const removed = lists.reduce((total, list) => total + list.accountIds.length, 0);
    set({
      lists: lists.map((list) => ({ ...list, accountIds: [], rebates: {} })),
    });
    return removed;
  },

  setRebate: (listId, accountId, rebate) => {


    set({
      lists: get().lists.map((l) =>
        l.id === listId ? { ...l, rebates: { ...l.rebates, [accountId]: rebate } } : l,
      ),
    });
  },

  setSubmitEndpoint: (endpoint) => set({ submitEndpoint: endpoint.trim() }),


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
    if (isDesktop()) {


      void saveAccount(account);
    }
    return account;
  },

  deleteAccount: (id) => {
    if (!get().accounts.some((a) => a._id === id)) return false;
    if (isDesktop()) void deleteAccountRemote(id);
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

  setLists: (lists, activeListId) =>
    set((state) => ({
      lists,
      activeListId:
        activeListId ??
        (lists.some((l) => l.id === state.activeListId) ? state.activeListId : (lists[0]?.id ?? '')),
    })),





  setupPassword: async (username, password) => {
    if (isDesktop()) {
      const res = await setupLogin(username.trim(), password);
      if (!res.ok || !res.owner) throw new Error(res.error ?? 'Could not create the workspace.');
      rememberOwner(res.owner);
      set({
        authConfigured: true,
        loggedIn: true,
        currentOwner: res.owner,
        selectedOwner: res.owner,
        owners: [...get().owners, res.owner],
        ...clearWorkspaceCache(),
      });
      return;
    }
    set({ auth: await makeCredential(password), loggedIn: true, authConfigured: true });
  },

  login: async (password, owner) => {
    if (isDesktop()) {
      const target = owner ?? get().selectedOwner ?? get().owners[0] ?? null;
      if (!target) return false;
      const res = await loginPassword(target.id, password);
      if (res.owner) {
        rememberOwner(res.owner);
        set({
          loggedIn: true,
          currentOwner: res.owner,
          selectedOwner: res.owner,
          owners: get().owners.some((o) => o.id === res.owner!.id)
            ? get().owners
            : [...get().owners, res.owner],
          ...clearWorkspaceCache(),
        });

        const fresh = await loadWorkspaceFromDb();
        if (fresh) {
          set({
            accounts: fresh.accounts,
            ...(fresh.lists ? { lists: fresh.lists, activeListId: fresh.activeListId ?? get().activeListId } : {}),
          });
        }
      } else if (res.error) {
        set({ owners: [], selectedOwner: null, loggedIn: false, ...clearWorkspaceCache() });
        throw new Error(res.error);
      } else {
        set({ loggedIn: false, ...clearWorkspaceCache() });
      }
      return res.ok;
    }
    if (!(await verifyCredential(get().auth, password))) return false;
    set({ loggedIn: true });
    return true;
  },

  selectOwner: (owner) => {
    rememberOwner(owner);
    set({ selectedOwner: owner });
  },

  logout: () => {
    if (isDesktop()) void logoutDesktop();
    set({ loggedIn: false, currentOwner: null });
  },

  changePassword: async (oldPassword, newPassword) => {
    if (newPassword.trim().length < 1) return false;
    if (isDesktop()) {
      const res = await changeLoginPassword(oldPassword, newPassword);
      return res.ok;
    }
    if (!(await verifyCredential(get().auth, oldPassword))) return false;
    set({ auth: await makeCredential(newPassword), loggedIn: true });
    return true;
  },

  refreshAuth: async () => {
    if (!isDesktop()) {
      set({ authConfigured: get().auth !== null, authReady: true });
      return;
    }
    const status = await authStatus();
    const owners = status?.owners ?? [];
    const current = owners.find((o) => o.id === status?.current) ?? null;

    const selected = current ?? rememberedOwner() ?? owners[0] ?? null;
    if (selected) rememberOwner(selected);
    set({
      authConfigured: status?.configured ?? false,

      loggedIn: status?.unlocked ?? false,
      owners,
      currentOwner: current,
      selectedOwner: selected,
      authReady: true,
    });
  },
}));


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

    }
  });
}
