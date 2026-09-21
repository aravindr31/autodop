/**
 * Manage panel (right drawer): the account-maintenance features from the
 * Python app — Add New Account, Delete Account, Change Password — plus Sign
 * out. Account mutations hit the live store and persist; the credential gate
 * is client-side (see `src/lib/auth.ts`).
 */
import { useState } from 'react';
import { useStore } from '../lib/store';
import { matchesQuery, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import { X, Plus, Trash, LogOut, Search, KeyRound } from 'lucide-react';
import { Button, Pill } from './ui';

const FIELD =
  'w-full rounded-xl border border-slate-200 bg-white py-2 px-3 text-sm placeholder:text-slate-400 focus:border-indigo-400 focus:outline-2 focus:outline-offset-0 focus:outline-indigo-200';

function Section({ title, children }: { title: string; children: React.ReactNode }): React.ReactElement {
  return (
    <section className="rounded-xl border border-slate-200 bg-white p-4">
      <h2 className="mb-3 text-sm font-semibold text-slate-700">{title}</h2>
      {children}
    </section>
  );
}

function AddAccountSection(): React.ReactElement {
  const store = useStore.getState();
  const [n, setN] = useState('');
  const [name, setName] = useState('');
  const [denom, setDenom] = useState('');
  const [cnum, setCnum] = useState('');
  const [ref, setRef] = useState('');

  const addAccount = () => {
    if (!n.trim() || !name.trim()) {
      notify('Account number and name are required', 'error');
      return;
    }
    const account = store.addAccount({
      Number: n,
      Name: name,
      Denomination: denom || '0',
      CNumber: cnum,
      Ref_Number: ref,
    });
    notify(`${account.Name} added`, 'success');
    setN(''); setName(''); setDenom(''); setCnum(''); setRef('');
  };

  return (
    <Section title="Add New Account">
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          addAccount();
        }}
      >
        <input aria-label="Account number" required value={n} onChange={(e) => setN(e.currentTarget.value)} className={FIELD} placeholder="Account number * (e.g. 4999087654321)" />
        <input aria-label="Account name" required value={name} onChange={(e) => setName(e.currentTarget.value)} className={FIELD} placeholder="Account name * (e.g. SBI OPC)" />
        <div className="flex gap-2">
          <input aria-label="Denomination" value={denom} onChange={(e) => setDenom(e.currentTarget.value)} className={FIELD} placeholder="Denom (e.g. 100)" />
          <input aria-label="CNumber" value={cnum} onChange={(e) => setCnum(e.currentTarget.value)} className={FIELD} placeholder="CNumber" />
        </div>
        <input aria-label="Reference number" value={ref} onChange={(e) => setRef(e.currentTarget.value)} className={FIELD} placeholder="Reference number (optional)" />
        <Button type="submit" variant="primary" className="!mt-1 !w-full"><Plus className="h-4 w-4" />Add account</Button>
      </form>
    </Section>
  );
}

function DeleteAccountSection(): React.ReactElement {
  const store = useStore.getState();
  const accounts = useStore((s) => s.accounts);
  const [q, setQ] = useState('');
  const [confirmId, setConfirmId] = useState('');

  const results = accounts.filter((a) => matchesQuery(a, q)).slice(0, 8);
  const doDelete = (id: string) => {
    store.deleteAccount(id);
    notify('Account deleted', 'info');
    setConfirmId('');
  };

  return (
    <Section title="Delete Account">
      <div className="relative">
        <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
        <input
          aria-label="Search accounts to delete"
          value={q}
          onChange={(e) => { setQ(e.currentTarget.value); setConfirmId(''); }}
          className={FIELD + ' pl-9'}
          placeholder="Search to find an account…"
        />
      </div>
      <ul className="mt-2 flex max-h-56 flex-col gap-1 overflow-y-auto">
        {results.map((a) => (
          <li key={a._id} className="flex items-center justify-between gap-2 rounded-lg border border-slate-100 bg-slate-50 px-2 py-1.5">
            <span className="min-w-0 flex-1 truncate text-sm text-slate-700">
              <span className="font-medium">{a.Name}</span>
              <span className="block truncate font-mono text-[11px] text-slate-400">{a.Number} · {denominationLabel(a.Denomination)}</span>
            </span>
            {confirmId === a._id ? (
              <div className="flex items-center gap-1">
                <Button variant="danger" size="sm" onClick={() => doDelete(a._id)}>Sure?</Button>
                <button type="button" aria-label="Cancel" onClick={() => setConfirmId('')} className="px-1 text-slate-400 hover:text-slate-700">✕</button>
              </div>
            ) : (
              <Button variant="danger" size="sm" onClick={() => setConfirmId(a._id)}><Trash className="h-4 w-4" /></Button>
            )}
          </li>
        ))}
      </ul>
      {q && results.length === 0 ? <p className="mt-1 text-xs text-slate-400">No accounts match.</p> : null}
    </Section>
  );
}

function ChangePasswordSection(): React.ReactElement {
  const store = useStore.getState();
  const [oldPw, setOldPw] = useState('');
  const [newPw, setNewPw] = useState('');
  const [newPw2, setNewPw2] = useState('');
  const [busy, setBusy] = useState(false);

  const changePassword = async () => {
    if (newPw !== newPw2) {
      notify('New passwords do not match', 'error');
      return;
    }
    setBusy(true);
    const ok = await store.changePassword(oldPw, newPw);
    setBusy(false);
    if (ok) {
      notify('Password updated', 'success');
      setOldPw(''); setNewPw(''); setNewPw2('');
    } else {
      notify('Current password is incorrect', 'error');
    }
  };

  return (
    <Section title="Change Password">
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void changePassword();
        }}
      >
        <input type="password" aria-label="Current password" value={oldPw} onChange={(e) => setOldPw(e.currentTarget.value)} className={FIELD} placeholder="Current password" />
        <input type="password" aria-label="New password" value={newPw} onChange={(e) => setNewPw(e.currentTarget.value)} className={FIELD} placeholder="New password" />
        <input type="password" aria-label="Confirm new password" value={newPw2} onChange={(e) => setNewPw2(e.currentTarget.value)} className={FIELD} placeholder="Confirm new password" />
        <Button type="submit" variant="secondary" className="!mt-1 !w-full" disabled={busy}><KeyRound className="h-4 w-4" />Update password</Button>
      </form>
    </Section>
  );
}

export default function ManagePanel({ onClose }: { onClose: () => void }): React.ReactElement {
  const store = useStore.getState();
  const accounts = useStore((s) => s.accounts);
  const lists = useStore((s) => s.lists);

  return (
    <div className="fixed inset-0 z-40" role="dialog" aria-label="Manage accounts">
      <div className="absolute inset-0 bg-slate-900/40" onClick={onClose} />
      <aside className="absolute right-0 top-0 h-full w-full max-w-md overflow-y-auto overscroll-contain border-l border-slate-200 bg-white p-5 shadow-2xl">
        <div className="flex items-center justify-between">
          <span className="flex items-center gap-1.5 text-sm font-semibold text-slate-800">
            Manage
            <Pill tone="neutral">{accounts.length.toLocaleString('en-IN')} accounts</Pill>
            <Pill tone="neutral">{lists.length} lists</Pill>
          </span>
          <button type="button" aria-label="Close" onClick={onClose} className="inline-flex h-8 w-8 items-center justify-center rounded-lg text-slate-500 hover:bg-slate-100 hover:text-slate-700">
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="mt-4 flex flex-col gap-3">
          <AddAccountSection />
          <DeleteAccountSection />
          <ChangePasswordSection />
        </div>

        <div className="mt-4 border-t border-slate-200 pt-3">
          <Button variant="danger" className="!w-full" onClick={() => store.logout()}>
            <LogOut className="h-4 w-4" />Sign out
          </Button>
        </div>
      </aside>
    </div>
  );
}