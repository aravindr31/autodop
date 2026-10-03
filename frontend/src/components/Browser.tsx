import { useState } from 'react';
import type { Account } from '../lib/types';
import { useStore } from '../lib/store';
import { matchesQuery, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import { importAccountsPdfBytes, fileToBase64, loadAccountsFromDb } from '../lib/bridge';
import { Search, Check, Plus, LayoutGrid, Columns2, Rows3, Table2, FileText, Download } from 'lucide-react';
import { Button, EmptyState, Pill } from './ui';

const PAGE_SIZES = [50, 100, 250];
const VIEW_KEY = 'autodop-account-view';
type View = '4' | '3' | '2' | 'table';

const VIEW_CLASS: Record<Exclude<View, 'table'>, string> = {
  '4': 'grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 xl:grid-cols-4',
  '3': 'grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3',
  '2': 'grid grid-cols-1 gap-3 md:grid-cols-2',
};

function rememberedView(): View {
  try {
    const raw = localStorage.getItem(VIEW_KEY);
    if (raw === '4' || raw === '3' || raw === '2' || raw === 'table') return raw;
  } catch {   }
  return '4';
}

function ViewButton({
  active,
  label,
  children,
  onClick,
}: {
  active: boolean;
  label: string;
  children: React.ReactNode;
  onClick: () => void;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active}
      onClick={onClick}
      className={
        'inline-flex h-8 w-8 items-center justify-center rounded-lg border transition-colors ' +
        (active
          ? 'border-indigo-300 bg-indigo-50 text-indigo-700'
          : 'border-slate-200 bg-white text-slate-400 hover:text-slate-700')
      }
    >
      {children}
    </button>
  );
}

function AccountCard({ account }: { account: Account }): React.ReactElement {
  const store = useStore.getState();
  const id = account._id;
  const activeListId = useStore((s) => s.activeListId);
  const inListName = useStore((s) => s.listNameOf(id));
  const inActiveList = useStore((s) => s.lists.find((l) => l.id === s.activeListId)?.accountIds.includes(id) ?? false);

  const add = () => {
    store.addToActive(id);
    notify(`${account.Name} added to active list`, 'success');
  };
  const remove = () => {
    store.removeFromList(activeListId, id);
    notify(`${account.Name} removed from active list`, 'info');
  };

  const action =
    inActiveList ? (
      <Button variant="ghost" onClick={remove}><Check className="h-4 w-4" />Remove</Button>
    ) : inListName ? (
      <Pill tone="neutral"><Check className="h-3 w-3" />In {inListName}</Pill>
    ) : (
      <Button variant="primary" onClick={add}><Plus className="h-4 w-4" />Add</Button>
    );

  return (
    <div className="flex flex-col rounded-xl border border-slate-200 bg-white p-3 shadow-sm transition-shadow hover:shadow-md">
      { }
      <div className="flex items-start justify-between gap-2">
        <p className="truncate font-mono text-[15px] font-semibold tracking-tight text-slate-900" title={account.Number}>
          {account.Number}
        </p>
        <Pill tone="accent">{denominationLabel(account.Denomination)}</Pill>
      </div>
      <h3 className="mt-0.5 truncate text-sm text-slate-600" title={account.Name}>{account.Name}</h3>
      <div className="mt-1.5 flex items-center gap-2 text-[11px] text-slate-400">
        <span>Ref {account.Ref_Number || '—'}</span>
      </div>
      <div className="mt-2.5 flex items-center justify-end">{action}</div>
    </div>
  );
}

/**
 * What a first-time user sees: no accounts yet, so point them at the agent
 * portal's Deposit-Accounts printout and let them import it right here.
 */
function ImportOnboarding(): React.ReactElement {
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    if (!file) return;
    setBusy(true);
    const res = await importAccountsPdfBytes(await fileToBase64(file));
    setBusy(false);
    if (!res.ok || !res.report) {
      notify(res.error ?? 'Import failed', 'error');
      return;
    }
    const { imported, skipped_duplicates } = res.report;
    notify(
      `Imported ${imported} account(s)` +
        (skipped_duplicates > 0 ? `, skipped ${skipped_duplicates} already present` : ''),
      'success',
    );
    setFile(null);
    const accounts = await loadAccountsFromDb();
    if (accounts.ok && accounts.accounts) useStore.getState().setAccounts(accounts.accounts);
  };

  return (
    <div className="mx-auto max-w-xl rounded-2xl border border-indigo-200 bg-white p-5 shadow-sm">
      <div className="mb-3 flex items-center gap-2">
        <span className="flex h-9 w-9 items-center justify-center rounded-xl bg-indigo-600 text-white">
          <Download className="h-4 w-4" />
        </span>
        <h2 className="text-sm font-semibold text-slate-800">
          Bring in your account data
        </h2>
      </div>
      <ol className="mb-3 list-decimal space-y-1.5 pl-5 text-xs leading-relaxed text-slate-600">
        <li>
          Sign in at{' '}
          <a
            href="https://dopagent.indiapost.gov.in/"
            target="_blank"
            rel="noreferrer"
            className="font-medium text-indigo-600 underline-offset-2 hover:underline"
          >
            dopagent.indiapost.gov.in
          </a>{' '}
          with your agent credentials.
        </li>
        <li>
          Open <strong>Deposits → Account</strong> (the Deposit Accounts list) and use
          the print/save option to download it as a <strong>PDF</strong>.
        </li>
        <li>Pick that PDF below — every row becomes an account in AutoDOP.</li>
      </ol>
      <label className="flex w-full cursor-pointer items-center gap-2 rounded-xl border border-dashed border-slate-300 bg-white py-2.5 pl-3 pr-3 text-sm text-slate-600 hover:border-indigo-300">
        <FileText className="h-4 w-4 shrink-0 text-slate-400" />
        <span className="min-w-0 flex-1 truncate">
          {file ? file.name : 'Choose the Deposit Accounts PDF…'}
        </span>
        <input
          type="file"
          accept=".pdf,application/pdf"
          className="hidden"
          onChange={(e) => setFile(e.currentTarget.files?.[0] ?? null)}
        />
      </label>
      <Button
        variant="primary"
        className="!mt-3 !w-full"
        disabled={busy || !file}
        onClick={() => void run()}
      >
        <Plus className="h-4 w-4" />Import accounts
      </Button>
      <p className="mt-3 text-[11px] leading-snug text-slate-400">
        Prefer moving an existing setup over? Manage → Backup &amp; restore imports a
        portable JSON backup from another machine.
      </p>
    </div>
  );
}

function AccountTable({ accounts }: { accounts: Account[] }): React.ReactElement {
  const store = useStore.getState();
  const activeListId = useStore((s) => s.activeListId);

  return (
    <div className="overflow-x-auto rounded-xl border border-slate-200 bg-white shadow-sm">
      <table className="w-full text-left text-sm">
        <thead>
          <tr className="border-b border-slate-200 text-[11px] uppercase tracking-wide text-slate-400">
            <th className="px-3 py-2 font-medium">Account No</th>
            <th className="px-3 py-2 font-medium">Name</th>
            <th className="px-3 py-2 font-medium">Denomination</th>
            <th className="px-3 py-2 font-medium">CNumber</th>
            <th className="px-3 py-2 font-medium">Ref</th>
            <th className="px-3 py-2 font-medium text-right">In list</th>
          </tr>
        </thead>
        <tbody>
          {accounts.map((account) => {
            const inListName = store.listNameOf(account._id);
            const inActiveList =
              store.lists.find((l) => l.id === activeListId)?.accountIds.includes(account._id) ?? false;
            return (
              <tr key={account._id} className="border-b border-slate-100 last:border-0 hover:bg-slate-50">
                <td className="px-3 py-2 font-mono font-semibold text-slate-900">{account.Number}</td>
                <td className="max-w-48 truncate px-3 py-2 text-slate-600" title={account.Name}>{account.Name}</td>
                <td className="px-3 py-2"><Pill tone="accent">{denominationLabel(account.Denomination)}</Pill></td>
                <td className="px-3 py-2 font-mono text-xs text-slate-400">{account.CNumber || '—'}</td>
                <td className="px-3 py-2 text-slate-500">{account.Ref_Number || '—'}</td>
                <td className="px-3 py-2 text-right">
                  {inActiveList ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        store.removeFromList(activeListId, account._id);
                        notify(`${account.Name} removed from active list`, 'info');
                      }}
                    >
                      <Check className="h-4 w-4" />Remove
                    </Button>
                  ) : inListName ? (
                    <Pill tone="neutral"><Check className="h-3 w-3" />{inListName}</Pill>
                  ) : (
                    <Button
                      variant="primary"
                      size="sm"
                      onClick={() => {
                        store.addToActive(account._id);
                        notify(`${account.Name} added to active list`, 'success');
                      }}
                    >
                      <Plus className="h-4 w-4" />Add
                    </Button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export default function Browser(): React.ReactElement {
  const accounts = useStore((s) => s.accounts);
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(100);
  const [view, setView] = useState<View>(rememberedView);

  const changeView = (next: View) => {
    setView(next);
    try { localStorage.setItem(VIEW_KEY, next); } catch {   }
  };

  const matched = accounts.filter((a) => matchesQuery(a, query));
  const pageCount = Math.max(1, Math.ceil(matched.length / pageSize));
  const current = Math.min(page, pageCount - 1);
  const slice = matched.slice(current * pageSize, (current + 1) * pageSize);


  const changeQuery = (value: string) => {
    setQuery(value);
    setPage(0);
  };

  return (
    <section className="flex min-h-0 flex-1 flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-0 flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <input
            className="w-full rounded-xl border border-slate-200 bg-white py-2.5 pl-9 pr-3 text-sm placeholder:text-slate-400 focus:border-indigo-400 focus:outline-2 focus:outline-offset-0 focus:outline-indigo-200"
            placeholder="Search name, number, CNumber or ref…"
            value={query}
            onChange={(e) => changeQuery(e.currentTarget.value)}
          />
        </div>
        <Pill tone="neutral">{matched.length.toLocaleString('en-IN')} of {accounts.length.toLocaleString('en-IN')}</Pill>
        <div className="flex items-center gap-1.5" role="group" aria-label="View density">
          <ViewButton active={view === '4'} label="4-column grid" onClick={() => changeView('4')}>
            <LayoutGrid className="h-4 w-4" />
          </ViewButton>
          <ViewButton active={view === '3'} label="3-column grid" onClick={() => changeView('3')}>
            <Columns2 className="h-4 w-4" />
          </ViewButton>
          <ViewButton active={view === '2'} label="2-column grid" onClick={() => changeView('2')}>
            <Rows3 className="h-4 w-4" />
          </ViewButton>
          <ViewButton active={view === 'table'} label="Table view" onClick={() => changeView('table')}>
            <Table2 className="h-4 w-4" />
          </ViewButton>
        </div>
      </div>

      {accounts.length === 0 ? (
        <ImportOnboarding />
      ) : matched.length === 0 ? (
        <EmptyState
          title="No accounts match your search"
          hint="Try a name, account number, CNumber or reference number, or add a new account."
        />
      ) : view === 'table' ? (
        <AccountTable accounts={slice} />
      ) : (
        <div className={VIEW_CLASS[view]}>
          {slice.map((a) => <AccountCard key={a._id} account={a} />)}
        </div>
      )}

      {pageCount > 1 && (
        <div className="flex items-center justify-center gap-3 border-t border-slate-200 bg-white py-3">
          <Pill tone="neutral">Page {current + 1} / {pageCount}</Pill>
          <div className="flex items-center gap-1.5">
            <Button variant="secondary" size="sm" disabled={current === 0} onClick={() => setPage(Math.max(0, current - 1))}>
              Prev
            </Button>
            <select
              aria-label="Items per page"
              value={pageSize}
              onChange={(e) => {
                setPageSize(Number(e.currentTarget.value));
                setPage(0);
              }}
              className="rounded-md border border-slate-200 bg-white py-1 text-xs text-slate-600"
            >
              {PAGE_SIZES.map((n) => <option key={n} value={n}>{n} / page</option>)}
            </select>
            <Button variant="secondary" size="sm" disabled={current >= pageCount - 1} onClick={() => setPage(Math.min(pageCount - 1, current + 1))}>
              Next
            </Button>
          </div>
        </div>
      )}
    </section>
  );
}
