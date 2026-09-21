/**
 * List Viewing section (spec §3.3 + §4.2 + §4.3): summaries of all non-empty
 * lists with live total denomination + item count, expandable items with
 * Remove, Copy numbers, Clear, and backend submission to a configurable
 * endpoint.
 */
import { useState } from 'react';
import { useStore } from '../lib/store';
import { accountById } from '../lib/accounts';
import { submitList } from '../lib/api';
import { formatINR, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import { Copy, Send, Trash, ChevronDown } from 'lucide-react';
import { Button, EmptyState, Pill, IconButton } from './ui';

function copyText(text: string): boolean {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    void navigator.clipboard.writeText(text);
    return true;
  }
  return false;
}

function ItemRow({ listId, accountId }: { listId: string; accountId: string }): React.ReactElement {
  const store = useStore.getState();
  const account = accountById(accountId);
  if (!account) return <tr className="hidden" />;
  return (
    <tr className="border-b border-slate-100 last:border-0">
      <td className="px-3 py-2 text-sm font-medium text-slate-800">{account.Name}</td>
      <td className="px-3 py-2 font-mono text-xs text-slate-500">{account.Number}</td>
      <td className="px-3 py-2 text-right text-sm tabular-nums text-slate-600">{denominationLabel(account.Denomination)}</td>
      <td className="px-3 py-2 text-right">
        <IconButton
          label={`Remove ${account.Name}`}
          danger
          onClick={() => {
            store.removeFromList(listId, accountId);
            notify(`${account.Name} removed`, 'info');
          }}
        >
          <Trash className="h-4 w-4" />
        </IconButton>
      </td>
    </tr>
  );
}

function ListCard({ listId }: { listId: string }): React.ReactElement {
  const store = useStore.getState();
  const list = store.lists.find((l) => l.id === listId);
  if (!list || list.accountIds.length === 0) return <span className="hidden" />;

  const totals = store.totalsOf(listId);
  const [expanded, setExpanded] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const onCopy = () => {
    const numbers = list.accountIds.map((id) => accountById(id)?.Number).filter(Boolean).join(', ');
    copyText(numbers);
    notify(`Copied ${list.accountIds.length} number(s) to clipboard`, 'success');
  };

  const onClear = () => {
    store.clearList(listId);
    notify(`Cleared ${list.name}`, 'info');
  };

  const onSubmit = async () => {
    setSubmitting(true);
    const res = await submitList(list, store.submitEndpoint);
    notify(res.message, res.ok ? 'success' : 'error');
    setSubmitting(false);
  };

  return (
    <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
      <button
        type="button"
        onClick={() => setExpanded(!expanded)}
        className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left transition-colors hover:bg-slate-50"
        aria-expanded={expanded}
      >
        <span className="flex min-w-0 items-center gap-2.5">
          <ChevronDown className={`h-4 w-4 text-slate-400 transition-transform ${expanded ? 'rotate-180' : ''}`} />
          <span className="truncate text-sm font-semibold text-slate-800">{list.name}</span>
          <Pill tone="neutral">{list.accountIds.length} item{list.accountIds.length === 1 ? '' : 's'}</Pill>
        </span>
        <span className="inline-flex items-center gap-1 text-sm font-semibold text-slate-800 tabular-nums">
          <span className="text-[11px] uppercase tracking-wide text-slate-400">₹</span>
          {formatINR(totals.amount)}
        </span>
      </button>

      {expanded && (
        <div className="border-t border-slate-100 px-0">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-[11px] uppercase tracking-wide text-slate-400">
                <th className="py-2 pl-3 pr-3 font-medium">Name</th>
                <th className="py-2 pl-3 pr-3 font-medium">Number</th>
                <th className="py-2 pl-3 pr-3 text-right font-medium">Denom</th>
                <th className="py-2 pl-3 pr-3" aria-hidden />
              </tr>
            </thead>
            <tbody>
              {list.accountIds.map((id) => <ItemRow key={id} listId={listId} accountId={id} />)}
            </tbody>
          </table>

          <div className="flex flex-wrap items-center gap-2 border-t border-slate-100 bg-slate-50/50 px-4 py-2.5">
            <Button variant="secondary" size="sm" onClick={onCopy}><Copy className="h-4 w-4" />Copy numbers</Button>
            <Button variant="danger" size="sm" onClick={onClear}><Trash className="h-4 w-4" />Clear list</Button>
            <div className="min-w-[1px] flex-1" />
            <Button
              variant="primary"
              size="sm"
              disabled={submitting || !store.submitEndpoint.trim()}
              onClick={() => void onSubmit()}
            >
              {submitting ? (
                'Submitting…'
              ) : (
                <><Send className="h-4 w-4" />Submit {list.accountIds.length} to backend</>
              )}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

export default function ListsView(): React.ReactElement {
  const lists = useStore((s) => s.lists);
  const endpoint = useStore((s) => s.submitEndpoint);
  const store = useStore.getState();
  const [endpointInput, setEndpointInput] = useState(endpoint);

  // Lists that have at least one account (spec §3.3: "lists that are not empty").
  const populated = lists
    .filter((l) => l.accountIds.length > 0)
    .sort((x, y) => x.name.localeCompare(y.name, undefined, { sensitivity: 'base' }));
  const emptyCount = lists.length - populated.length;

  const saveEndpoint = () => {
    store.setSubmitEndpoint(endpointInput);
    notify('Endpoint saved', 'success');
  };

  return (
    <section className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2.5 rounded-xl border border-slate-200 bg-white px-4 py-2.5">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-slate-400">Backend</span>
        <div className="relative min-w-0 flex-1">
          <input
            className="w-full rounded-lg border border-slate-200 bg-white py-1.5 pl-3 pr-2 text-sm font-mono text-slate-600 placeholder:text-slate-400 focus:border-indigo-400 focus:outline-2 focus:outline-offset-0 focus:outline-indigo-200"
            placeholder="https://api.example.com/submit (optional)"
            value={endpointInput}
            onChange={(e) => setEndpointInput(e.currentTarget.value)}
          />
        </div>
        <Button variant="secondary" size="sm" onClick={saveEndpoint}>Save</Button>
      </div>

      {populated.length === 0 ? (
        <EmptyState
          title={emptyCount === 0 ? 'No lists yet' : 'No populated lists'}
          hint="Open the Accounts tab, pick or create a list, then tap Add on any account."
        />
      ) : (
        <div className="flex flex-col gap-3">
          {populated.map((l) => <ListCard key={l.id} listId={l.id} />)}
        </div>
      )}
    </section>
  );
}