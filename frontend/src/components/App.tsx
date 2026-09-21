/**
 * Application shell (spec): header with active-list chip, Accounts / Lists
 * tabs, an inline sidebar on desktop that becomes a slide-over drawer on
 * mobile, and a global toast stack. All interactive state is reactive via
 * the Zustand store and persists to localStorage.
 */
import { useEffect, useState } from 'react';
import { useStore } from '../lib/store';
import { formatINR } from '../lib/format';
import { onNotify } from '../lib/toast';
import { SidebarPanel } from './Sidebar';
import Sidebar from './Sidebar';
import Browser from './Browser';
import ListsView from './ListsView';
import { ToastViewItem } from './ui';
import type { ToastView } from './ui';
import { Menu, Check } from 'lucide-react';

type Tab = 'accounts' | 'lists';

const LOGO = (
  <span className="inline-flex h-8 w-8 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-600 to-violet-600 text-sm font-black text-white shadow-sm">
    AD
  </span>
);

export default function App(): React.ReactElement {
  const lists = useStore((s) => s.lists);
  const activeListId = useStore((s) => s.activeListId);
  const activeList = lists.find((l) => l.id === activeListId);
  // totalsOf is safe for a missing/unset list id — unconditional so the hook
  // count never changes between renders (React rules).
  const activeTotals = useStore((s) => s.totalsOf(activeListId));

  const [tab, setTab] = useState<Tab>('accounts');
  const [drawerOpen, setDrawerOpen] = useState(false);
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
            <PillHost count={activeTotals.count} amount={activeTotals.amount} />
          </div>

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
        </aside>

        {/* Main tab content */}
        <main className="min-w-0">{tab === 'accounts' ? <Browser /> : <ListsView />}</main>
      </div>

      {/* Mobile drawer */}
      <Sidebar open={drawerOpen} onClose={() => setDrawerOpen(false)} />

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

function PillHost({ count, amount }: { count: number; amount: number }): React.ReactElement {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-indigo-50 px-2 py-0.5 text-[11px] font-semibold text-indigo-700">
      <Check className="h-3 w-3" />
      {count} · ₹{formatINR(amount)}
    </span>
  );
}