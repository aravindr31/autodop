import { useState, useEffect } from 'react';
import { useStore } from '../lib/store';
import type { Account, AccountList } from '../lib/types';
import { submitList } from '../lib/api';
import { generateLists, onProgress, useDesktop } from '../lib/bridge';
import { formatINR, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import { Copy, Send, Trash, ChevronDown, Play } from 'lucide-react';
import { Button, EmptyState, Pill, IconButton } from './ui';

function copyText(text: string): boolean {
  if (navigator.clipboard && navigator.clipboard.writeText) {
    void navigator.clipboard.writeText(text);
    return true;
  }
  return false;
}

function ItemRow({ listId, account }: { listId: string; account: Account }): React.ReactElement {
  const store = useStore.getState();

  const rebate = useStore(
    (s) => s.lists.find((l) => l.id === listId)?.rebates?.[account._id] ?? 0,
  );
  return (
    <tr className="border-b border-slate-100 last:border-0">
      <td className="px-3 py-2 text-sm font-medium text-slate-800">{account.Name}</td>
      <td className="px-3 py-2 font-mono text-xs text-slate-500">{account.Number}</td>
      <td className="px-3 py-2 text-right text-sm tabular-nums text-slate-600">{denominationLabel(account.Denomination)}</td>
      <td className="px-3 py-2 text-right">
        <input
          type="number"
          min={0}
          value={rebate}
          aria-label={`Rebate for ${account.Name}`}
          title="RD installment number sent for this account. 1 = skip the rebate step in scraper.py."
          onChange={(event) => {
            const next = Number.parseInt(event.currentTarget.value, 10);
            store.setRebate(listId, account._id, Number.isFinite(next) ? next : 0);
          }}
          className="w-16 rounded-md border border-slate-200 bg-white px-2 py-1 text-right text-xs tabular-nums text-slate-700 focus:border-indigo-400 focus:outline-2 focus:outline-offset-0 focus:outline-indigo-200"
        />
      </td>
      <td className="px-3 py-2 text-right">
        <IconButton
          label={`Remove ${account.Name}`}
          danger
          onClick={() => {
            store.removeFromList(listId, account._id);
            notify(`${account.Name} removed`, 'info');
          }}
        >
          <Trash className="h-4 w-4" />
        </IconButton>
      </td>
    </tr>
  );
}










function listPayload(
  list: AccountList,
  accounts: Account[],
): { name: string; numbers: string[]; rebate: number[] } {
  const entries = list.accountIds.flatMap((id) => {
    const account = accounts.find((a) => a._id === id);
    return account && account.Number
      ? [{ number: account.Number, rebate: list.rebates?.[id] ?? 0 }]
      : [];
  });
  return {
    name: list.name,
    numbers: entries.map((entry) => entry.number),
    rebate: entries.map((entry) => entry.rebate),
  };
}


function useGenerate() {
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState('');


  const [failure, setFailure] = useState<{
    message: string;
    log?: string;
    logPath?: string;
  } | null>(null);

  useEffect(() => onProgress((message) => setProgress(message)), []);

  const run = async (payload: Array<{ name: string; numbers: string[]; rebate: number[] }>) => {
    const total = payload.reduce((sum, item) => sum + item.numbers.length, 0);
    if (total === 0) {
      notify('No account numbers to generate yet.', 'error');
      return;
    }
    setBusy(true);
    setFailure(null);
    setProgress('Starting scraper…');


    const res = await generateLists(payload);
    setBusy(false);
    setProgress('');
    if (!res.ok) {


      setFailure({ message: res.error ?? 'Generate failed', log: res.log, logPath: res.log_path });
      notify(res.error ?? 'Generate failed', 'error');
      return;
    }
    const rows = res.results ?? [];
    const succeeded = rows.filter((r) => r.status === 'success').length;
    const allOk = rows.length > 0 && succeeded === rows.length;
    if (!allOk) {
      setFailure({
        message: `Generated ${succeeded}/${rows.length} list(s)`,
        log: res.log,
        logPath: res.log_path,
      });
    }
    notify(
      allOk
        ? `Generated ${succeeded} list(s) successfully`
        : `Generated ${succeeded}/${rows.length} list(s) — check logs`,
      allOk ? 'success' : 'error',
    );
  };

  return { busy, progress, failure, run };
}


function FailureNote({
  failure,
}: {
  failure: { message: string; log?: string; logPath?: string };
}): React.ReactElement {
  return (
    <div className="mt-2 basis-full rounded-md border border-rose-200 bg-rose-50/70 p-2 text-[11px] leading-relaxed text-rose-900">
      <div className="flex items-center justify-between gap-2">
        <span className="font-medium">{failure.message}</span>
        {failure.log ? (
          <button
            type="button"
            className="shrink-0 rounded px-1.5 py-0.5 font-medium text-rose-700 hover:bg-rose-100"
            onClick={() => {
              if (copyText(failure.log ?? '')) notify('Log copied', 'success');
            }}
          >
            Copy log
          </button>
        ) : null}
      </div>
      {failure.log ? (
        <pre className="mt-1 max-h-40 overflow-auto font-mono whitespace-pre-wrap break-all">
          {failure.log}
        </pre>
      ) : null}
      {failure.logPath ? (
        <p className="mt-1 break-all text-rose-700/80">Full log: {failure.logPath}</p>
      ) : null}
    </div>
  );
}






function GenerateButton({ list, numbers }: { list: AccountList; numbers: string[] }): React.ReactElement {
  const { ready, info } = useDesktop();
  const accounts = useStore((s) => s.accounts);
  const { busy, progress, failure, run } = useGenerate();

  const start = () => {
    if (!ready) {
      notify('Generate needs the desktop app — launch it with `npm run dev`', 'error');
      return;
    }
    if (numbers.length === 0) {
      notify(`No account numbers in ${list.name} yet.`, 'error');
      return;
    }
    void run([listPayload(list, accounts)]);
  };

  const hint = !ready
    ? 'Available in the desktop app (npm run dev)'
    : info && !info.credentials
      ? 'Add DOP credentials in Manage → DOP Credentials first'
      : `Run scraper.py for ${numbers.length} account(s)`;

  return (
    <>
      <Button variant="secondary" size="sm" disabled={busy} onClick={start} title={hint}>
        <Play className="h-4 w-4" />{busy ? 'Generating…' : 'Generate (DOP)'}
      </Button>
      {progress ? <span className="text-[11px] text-slate-500">{progress}</span> : null}
      {failure ? <FailureNote failure={failure} /> : null}
    </>
  );
}





function ClearAllButton({ lists }: { lists: AccountList[] }): React.ReactElement | null {
  const store = useStore.getState();
  const [confirming, setConfirming] = useState(false);

  const total = lists.reduce((sum, list) => sum + list.accountIds.length, 0);
  if (total === 0) return null;

  const clear = () => {
    const removed = store.clearAllLists();
    setConfirming(false);
    notify(`Cleared all lists — ${removed} account(s) removed`, 'info');
  };

  if (confirming) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm text-slate-600">
          Remove all {total} account(s) from every list?
        </span>
        <Button variant="danger" size="sm" onClick={clear}>
          <Trash className="h-4 w-4" />Yes, clear all
        </Button>
        <Button variant="secondary" size="sm" onClick={() => setConfirming(false)}>Cancel</Button>
      </div>
    );
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button variant="danger" size="sm" onClick={() => setConfirming(true)}>
        <Trash className="h-4 w-4" />Clear all lists
      </Button>
      <span className="text-[11px] text-slate-500">
        Empties every list — the lists and the accounts stay.
      </span>
    </div>
  );
}






function GenerateAllButton({ lists }: { lists: AccountList[] }): React.ReactElement {
  const { ready, info } = useDesktop();
  const accounts = useStore((s) => s.accounts);
  const { busy, progress, failure, run } = useGenerate();

  const accountCount = lists.reduce((sum, l) => sum + l.accountIds.length, 0);
  const hint = !ready
    ? 'Available in the desktop app (npm run dev)'
    : info && !info.credentials
      ? 'Add DOP credentials in Manage → DOP Credentials first'
      : `Run scraper.py once for all ${lists.length} list(s)`;

  const start = () => {
    if (!ready) {
      notify('Generate needs the desktop app — launch it with `npm run dev`', 'error');
      return;
    }
    void run(lists.map((l) => listPayload(l, accounts)));
  };

  return (
    <div className="flex w-full flex-wrap items-center gap-2.5 rounded-xl border border-slate-200 bg-white px-4 py-2.5">
      <Button variant="primary" size="sm" disabled={busy} onClick={start} title={hint}>
        <Play className="h-4 w-4" />
        {busy ? 'Generating all…' : 'Generate All Lists'}
      </Button>
      <span className="text-[11px] text-slate-500">
        {lists.length} list(s) · {accountCount} account(s) · one run
      </span>
      {progress ? <span className="text-[11px] text-slate-500">{progress}</span> : null}
      {failure ? <FailureNote failure={failure} /> : null}
    </div>
  );
}

function ListCard({ listId }: { listId: string }): React.ReactElement {
  const store = useStore.getState();
  const accounts = useStore((s) => s.accounts);
  const list = useStore((s) => s.lists.find((l) => l.id === listId));
  if (!list || list.accountIds.length === 0) return <span className="hidden" />;

  const totals = store.totalsOf(listId);
  const numbers = list.accountIds.flatMap((id) => {
    const acc = accounts.find((a) => a._id === id);
    return acc && acc.Number ? [acc.Number] : [];
  });
  const [expanded, setExpanded] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const onCopy = () => {
    const numbers = list.accountIds
      .map((id) => accounts.find((a) => a._id === id)?.Number)
      .filter(Boolean)
      .join(', ');
    copyText(numbers);
    notify(`Copied ${list.accountIds.length} number(s) to clipboard`, 'success');
  };

  const onClear = () => {
    store.clearList(listId);
    notify(`Cleared ${list.name}`, 'info');
  };

  const onSubmit = async () => {
    setSubmitting(true);

    const accountOf = (id: string) => useStore.getState().accounts.find((a) => a._id === id);
    const res = await submitList(list, store.submitEndpoint, accountOf);
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
                <th className="py-2 pl-3 pr-3 text-right font-medium">Rebate</th>
                <th className="py-2 pl-3 pr-3" aria-hidden />
              </tr>
            </thead>
            <tbody>
              {list.accountIds.flatMap((id) => {
                const acc = accounts.find((a) => a._id === id);
                return acc ? [<ItemRow key={id} listId={listId} account={acc} />] : [];
              })}
            </tbody>
          </table>

          <div className="flex flex-wrap items-center gap-2 border-t border-slate-100 bg-slate-50/50 px-4 py-2.5">
            <Button variant="secondary" size="sm" onClick={onCopy}><Copy className="h-4 w-4" />Copy numbers</Button>
            <Button variant="danger" size="sm" onClick={onClear}><Trash className="h-4 w-4" />Clear list</Button>
            <GenerateButton list={list} numbers={numbers} />
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
      {/* <div className="flex flex-wrap items-center gap-2.5 rounded-xl border border-slate-200 bg-white px-4 py-2.5">
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
      </div> */}

      { }
      {populated.length > 0 ? <GenerateAllButton lists={populated} /> : null}
      {populated.length > 0 ? <ClearAllButton lists={populated} /> : null}

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
