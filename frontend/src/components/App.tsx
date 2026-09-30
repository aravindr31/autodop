/**
 * Application shell (spec): header with active-list chip, Accounts / Lists
 * tabs, an inline sidebar on desktop that becomes a slide-over drawer on
 * mobile, a Manage drawer (add/delete account, change password, sign out),
 * and a global toast stack. All interactive state is reactive via the Zustand
 * store and persists to localStorage. The shell is gated behind a client-side
 * login (AuthScreen) until the separately-built backend provides real auth.
 */
import { useEffect, useState } from 'react';
import { useStore } from '../lib/store';
import { formatINR } from '../lib/format';
import { onNotify, notify } from '../lib/toast';
import { dbStatus, isDesktop, loadAccountsFromDb } from '../lib/bridge';
import { SidebarPanel } from './Sidebar';
import Sidebar from './Sidebar';
import Browser from './Browser';
import ListsView from './ListsView';
import AuthScreen from './AuthScreen';
import ManagePanel from './ManagePanel';
import { VersionLine } from './VersionLine';
import { ToastViewItem } from './ui';
import type { ToastView } from './ui';
import { Menu, Check, Settings } from 'lucide-react';

type Tab = 'accounts' | 'lists';

const LOGO = (
  <span className="inline-flex h-8 w-8 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-600 to-violet-600 text-sm font-black text-white shadow-sm">
    AD
  </span>
);

export default function App(): React.ReactElement {
  const loggedIn = useStore((s) => s.loggedIn);
  const lists = useStore((s) => s.lists);
  const activeListId = useStore((s) => s.activeListId);
  const activeList = lists.find((l) => l.id === activeListId);
  // totalsOf returns a fresh object each call — selecting it would be a new
  // snapshot identity every render (React "../#infinite-loop" warning). Select
  // primitives instead so the snapshot is cacheable/stable.
  const activeCount = useStore((s) => s.totalsOf(activeListId).count);
  const activeAmount = useStore((s) => s.totalsOf(activeListId).amount);

  const [tab, setTab] = useState<Tab>('accounts');
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const [toasts, setToasts] = useState<ToastView[]>([]);

  useEffect(
    () =>
      onNotify((message) => {
        setToasts((current) => [...current, message]);
        const id = message.id;
        setTimeout(() => {
          setToasts((current) => current.filter((t) => t.id !== id));
        }, 2400);
      }),
    [],
  );

  // In the desktop app, source accounts from Atlas when it is reachable;
  // otherwise the persisted/seeded accounts stay in place.
  useEffect(() => {
    if (!isDesktop()) return;
    let cancelled = false;
    void (async () => {
      const status = await dbStatus();
      if (cancelled || !status?.connected) return;
      const res = await loadAccountsFromDb();
      if (cancelled || !res.ok || !res.accounts) {
        if (!cancelled && res.error) notify(`Database load failed: ${res.error}`, 'error');
        return;
      }
      useStore.getState().setAccounts(res.accounts);
      notify(`Loaded ${res.accounts.length} accounts from ${status.db}.${status.collection}`, 'success');
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // All hooks above are unconditional; the gate below is a pure render branch.
  if (!loggedIn) {
    return (
      <div className="min-h-screen">
        <AuthScreen />
        <ToastStack toasts={toasts} onDismiss={(id) => setToasts((cur) => cur.filter((t) => t.id !== id))} />
      </div>
    );
  }

  return (
    <div className="flex min-h-screen flex-col bg-slate-50 text-slate-900 antialiased">
      {/* ---------------- Top bar ---------------- */}
      <header className="sticky top-0 z-30 border-b border-slate-200 bg-white/95 backdrop-blur">
        <div className="flex items-center gap-3 px-4 py-3">
          <button
            type="button"
            aria-label="Open lists"
            onClick={() => setDrawerOpen(true)}
            className="inline-flex h-9 w-9 items-center justify-center rounded-lg text-slate-600 hover:bg-slate-100 lg:hidden"
          >
            <Menu className="h-5 w-5" />
          </button>

          {LOGO}
          <div className="leading-tight">
            <h1 className="text-[15px] font-bold tracking-tight text-slate-900">AutoDOP</h1>
            <p className="text-[11px] text-slate-400">Account list manager</p>
          </div>

          <div className="min-w-[1px] flex-1" />

          {/* Active list chip */}
          <div className="hidden items-center gap-2 rounded-lg border border-slate-200 bg-white px-2.5 py-1.5 sm:flex">
            <span className="text-[11px] font-semibold uppercase tracking-wide text-slate-400">Active</span>
            <span className="text-sm font-medium text-slate-800">{activeList?.name ?? '—'}</span>
            <PillHost count={activeCount} amount={activeAmount} />
          </div>

          {/* Manage */}
          <button
            type="button"
            aria-label="Manage accounts"
            title="Manage: add / delete account, change password, sign out"
            onClick={() => setManageOpen(true)}
            className="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-slate-200 bg-white text-slate-600 shadow-sm hover:bg-slate-100 hover:text-slate-800"
          >
            <Settings className="h-5 w-5" />
          </button>

          {/* Tabs */}
          <nav className="flex rounded-lg bg-slate-100 p-0.5" aria-label="Sections">
            {([
              ['accounts', 'Accounts'],
              ['lists', 'Lists'],
            ] as [Tab, string][]).map(([id, label]) => (
              <button
                key={id}
                type="button"
                onClick={() => setTab(id)}
                aria-current={tab === id ? 'page' : undefined}
                className={tab === id
                  ? 'rounded-md bg-white px-3 py-1.5 text-sm font-medium text-slate-900 shadow-sm'
                  : 'rounded-md px-3 py-1.5 text-sm font-medium text-slate-500 hover:text-slate-800'}
              >
                {label}
              </button>
            ))}
          </nav>
        </div>
      </header>

      {/* ---------------- Body ---------------- */}
      <div className="mx-auto flex w-full max-w-7xl flex-col gap-4 px-4 py-4 lg:grid lg:grid-cols-[16rem_1fr] lg:items-start lg:gap-6">
        {/* Desktop sidebar */}
        <aside className="hidden w-full rounded-xl border border-slate-200 bg-white p-3 shadow-sm lg:block">
          <SidebarPanel onNavigate={() => {}} />
          <div className="mt-3 border-t border-slate-100 pt-2">
            <VersionLine />
          </div>
        </aside>

        {/* Main tab content */}
        <main className="min-w-0">{tab === 'accounts' ? <Browser /> : <ListsView />}</main>
      </div>

      {/* Mobile drawer */}
      <Sidebar open={drawerOpen} onClose={() => setDrawerOpen(false)} />

      {/* Manage drawer */}
      {manageOpen && <ManagePanel onClose={() => setManageOpen(false)} />}

      {/* Toasts */}
      <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
        {toasts.map((t) => (
          <ToastViewItem
            key={t.id}
            toast={t}
            onDismiss={() => setToasts((cur) => cur.filter((x) => x.id !== t.id))}
          />
        ))}
      </div>
    </div>
  );
}

function ToastStack({ toasts, onDismiss }: { toasts: ToastView[]; onDismiss: (id: number) => void }): React.ReactElement {
  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
      {toasts.map((t) => <ToastViewItem key={t.id} toast={t} onDismiss={() => onDismiss(t.id)} />)}
    </div>
  );
}

function PillHost({ count, amount }: { count: number; amount: number }): React.ReactElement {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-indigo-50 px-2 py-0.5 text-[11px] font-semibold text-indigo-700">
      <Check className="h-3 w-3" />
      {count} · ₹{formatINR(amount)}
    </span>
  );
}