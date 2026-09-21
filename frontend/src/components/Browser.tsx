/**
 * Main account display (spec §3.1): a responsive card grid of all accounts
 * with live search (Name / Number / CNumber / Ref_Number) and client-side
 * pagination so "hundreds of objects" stay snappy without virtualization.
 * Reads the live account list from the store (accounts can be added/deleted).
 */
import { useState } from 'react';
import type { Account } from '../lib/types';
import { useStore } from '../lib/store';
import { matchesQuery, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import { Search, Check, Plus } from 'lucide-react';
import { Button, EmptyState, Pill } from './ui';

const PAGE_SIZES = [50, 100, 250];

function AccountCard({ account }: { account: Account }): React.ReactElement {
  const store = useStore.getState();
  const id = account._id;
  const activeListId = useStore((s) => s.activeListId);
  const inListName = useStore((s) => s.listNameOf(id)); // "" when not in any list
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
      <Button variant="ghost" size="sm" onClick={remove}><Check className="h-4 w-4" />Remove</Button>
    ) : inListName ? (
      <Pill tone="neutral"><Check className="h-3 w-3" />In {inListName}</Pill>
    ) : (
      <Button variant="primary" size="sm" onClick={add}><Plus className="h-4 w-4" />Add</Button>
    );

  return (
    <div className="flex flex-col rounded-xl border border-slate-200 bg-white p-3 shadow-sm transition-shadow hover:shadow-md">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-sm font-semibold text-slate-800" title={account.Name}>{account.Name}</h3>
          <p className="mt-0.5 font-mono text-xs text-slate-500">{account.Number}</p>
        </div>
        <Pill tone="accent">{denominationLabel(account.Denomination)}</Pill>
      </div>
      <div className="mt-1.5 flex items-center gap-2 text-[11px] text-slate-400">
        <span className="truncate font-mono">{account.CNumber}</span>
        <span aria-hidden>·</span>
        <span>Ref {account.Ref_Number || '—'}</span>
      </div>
      <div className="mt-2.5 flex items-center justify-end">{action}</div>
    </div>
  );
}

export default function Browser(): React.ReactElement {
  const accounts = useStore((s) => s.accounts);
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(100);

  const matched = accounts.filter((a) => matchesQuery(a, query));
  const pageCount = Math.max(1, Math.ceil(matched.length / pageSize));
  const current = Math.min(page, pageCount - 1);
  const slice = matched.slice(current * pageSize, (current + 1) * pageSize);

  // Return to the first page whenever the filter narrows the result set.
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
      </div>

      {matched.length === 0 ? (
        <EmptyState
          title="No accounts match your search"
          hint="Try a name, account number, CNumber or reference number, or add a new account."
        />
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 xl:grid-cols-4">
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