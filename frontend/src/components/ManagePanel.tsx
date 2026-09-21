/**
 * Manage panel (right drawer): the account-maintenance features from the
 * Python app — Add New Account, Delete Account, Change Password — plus Sign
 * out. Account mutations hit the live store and persist; the credential gate
 * is client-side (see `src/lib/auth.ts`).
 */
import { useState, useEffect } from 'react';
import { useStore } from '../lib/store';
import { matchesQuery, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import {
  saveCredentials,
  useDesktop,
  dbStatus,
  loadAccountsFromDb,
  loadLists,
  saveLists,
  dopCredentialsStatus,
} from '../lib/bridge';
import type { DbStatus, DopCredentialStatus } from '../lib/bridge';
import { X, Plus, Trash, LogOut, Search, KeyRound, Terminal, Database } from 'lucide-react';
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

function DesktopSection(): React.ReactElement {
  const { ready, info } = useDesktop();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [configured, setConfigured] = useState(false);

  useEffect(() => {
    if (info) setConfigured(info.credentials);
  }, [info]);

  const save = async () => {
    if (!username.trim() || !password) {
      notify('Username and password are both required', 'error');
      return;
    }
    setBusy(true);
    const res = await saveCredentials(username.trim(), password);
    setBusy(false);
    if (res.ok) {
      notify('DOP credentials saved to desktop/.env', 'success');
      setConfigured(true);
      setUsername('');
      setPassword('');
    } else {
      notify(res.error ?? 'Could not save credentials', 'error');
    }
  };

  return (
    <Section title="DOP Credentials">
      {ready ? (
        <>
          <p className="mb-2 flex items-center gap-1.5 text-xs text-slate-500">
            <Terminal className="h-3.5 w-3.5 shrink-0" />
            <span>
              {configured ? 'Configured.' : 'Not configured yet.'} Stored by the Rust backend in this
              app&rsquo;s config folder — never in the browser.
            </span>
          </p>
          <div className="flex flex-col gap-2">
            <input aria-label="DOP username" value={username} onChange={(e) => setUsername(e.currentTarget.value)} className={FIELD} placeholder="DOP username" />
            <input type="password" aria-label="DOP password" value={password} onChange={(e) => setPassword(e.currentTarget.value)} className={FIELD} placeholder="DOP password" />
            <Button variant="secondary" className="!w-full" disabled={busy} onClick={() => void save()}>
              <KeyRound className="h-4 w-4" />Save credentials
            </Button>
          </div>
          {info && !info.scraper_present ? (
            <p className="mt-2 text-[11px] text-rose-600">
              scraper.py was not found — set <code className="font-mono">AUTODOP_SCRAPER</code> to its path.
            </p>
          ) : null}
          <CredentialSourceNote />
        </>
      ) : (
        <p className="text-xs leading-relaxed text-slate-500">
          Available in the desktop app only — a browser cannot run Selenium. Launch it with{' '}
          <code className="rounded bg-slate-100 px-1 font-mono">npm run dev</code>.
        </p>
      )}
    </Section>
  );
}

function DatabaseSection(): React.ReactElement {
  const { ready } = useDesktop();
  const shown = useStore((s) => s.accounts.length);
  const [status, setStatus] = useState<DbStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const check = async () => {
    setBusy(true);
    setStatus(await dbStatus());
    setBusy(false);
  };

  useEffect(() => {
    if (ready) void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready]);

  const reload = async () => {
    setBusy(true);
    const res = await loadAccountsFromDb();
    setBusy(false);
    if (!res.ok || !res.accounts) {
      notify(res.error ?? 'Reload failed', 'error');
      return;
    }
    useStore.getState().setAccounts(res.accounts);
    notify(`Reloaded ${res.accounts.length} accounts`, 'success');
    void check();
  };

  const pullLists = async () => {
    setBusy(true);
    const res = await loadLists();
    setBusy(false);
    if (!res.ok || !res.lists) {
      notify(res.error ?? 'Loading lists failed', 'error');
      return;
    }
    useStore.getState().setLists(res.lists, res.activeId);
    notify(`Loaded ${res.lists.length} list(s) from Atlas`, 'success');
  };

  const pushLists = async () => {
    const store = useStore.getState();
    setBusy(true);
    const res = await saveLists(store.lists, store.activeListId);
    setBusy(false);
    if (!res.ok || !res.lists) {
      const message = res.error ?? 'Saving lists failed';
      // Atlas phrasing when the user lacks write privileges.
      notify(
        /not allowed to do action/i.test(message)
          ? `Atlas refused the write — this user is read-only. Grant readWrite on the ${status?.db ?? 'accounts'} database.`
          : message,
        'error',
      );
      return;
    }
    // Adopt the stored ids so the next save updates instead of duplicating.
    // Ids can change (a list is matched by name when it has no ObjectId yet),
    // so follow the active list across by name rather than by id.
    const activeName = store.lists.find((list) => list.id === store.activeListId)?.name;
    const activeId = res.lists.find((list) => list.name === activeName)?.id;
    useStore.getState().setLists(res.lists, activeId);
    notify(`Saved ${res.lists.length} list(s) to Atlas`, 'success');
    void check();
  };

  return (
    <Section title="Database (MongoDB Atlas)">
      {!ready ? (
        <p className="text-xs text-slate-500">Available in the desktop app only.</p>
      ) : (
        <>
          <div className="mb-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-slate-500">
            {status?.connected ? (
              <>
                <Pill tone="positive"><Database className="h-3 w-3" />connected</Pill>
                <span className="font-mono">{status.db}.{status.collection}</span>
                {typeof status.count === 'number' ? (
                  <span>· {status.count.toLocaleString('en-IN')} docs</span>
                ) : null}
                <span>· showing {shown.toLocaleString('en-IN')}</span>
              </>
            ) : (
              <>
                <Pill tone="neutral">{status?.configured ? 'configured' : 'not configured'}</Pill>
                <span>{status?.error ?? 'Checking…'}</span>
              </>
            )}
          </div>
          <div className="flex gap-2">
            <Button variant="secondary" className="!w-full" disabled={busy} onClick={() => void check()}>
              Check connection
            </Button>
            <Button variant="primary" className="!w-full" disabled={busy || !status?.connected} onClick={() => void reload()}>
              Reload accounts
            </Button>
          </div>
          <div className="mt-2 flex gap-2">
            <Button variant="secondary" className="!w-full" disabled={busy || !status?.connected} onClick={() => void pullLists()}>
              Load lists
            </Button>
            <Button
              variant="secondary"
              className="!w-full"
              disabled={busy || !status?.connected}
              onClick={() => void pushLists()}
              title={
                status && status.writable === false
                  ? `Ready to use — Atlas will refuse it until this user gets readWrite (now: ${(status.roles ?? []).join(', ') || 'no roles'})`
                  : undefined
              }
            >
              Save lists to Atlas
            </Button>
          </div>
          {status?.connected && status.writable === false ? (
            <p className="mt-2 text-[11px] leading-relaxed text-slate-400">
              Saving is wired up and ready to use. This Atlas user is currently{' '}
              <strong>read-only</strong> ({(status.roles ?? []).join(', ') || 'no roles'}), so Atlas will
              refuse the write until you grant <code className="font-mono">readWrite</code> on the{' '}
              <code className="font-mono">{status.db}</code> database.
            </p>
          ) : (
            <p className="mt-2 text-[11px] leading-relaxed text-slate-400">
              Lists are read from and written to <code className="font-mono">savedList</code>. Saving only
              upserts the lists you have — nothing in Atlas is ever deleted.
            </p>
          )}
          <p className="mt-2 text-[11px] leading-relaxed text-slate-400">
            URI comes from <code className="font-mono">src-tauri/.env</code> (gitignored) or the{' '}
            <code className="font-mono">MONGO_URI</code> env var. Override the target with{' '}
            <code className="font-mono">MONGO_DB</code> / <code className="font-mono">MONGO_COLLECTION</code>.
          </p>
        </>
      )}
    </Section>
  );
}

/**
 * Where Generate will get its DOP credentials from.
 *
 * Reports the source and the portal id only — the password lives in the Rust
 * backend and is handed straight to `scraper.py`.
 */
function CredentialSourceNote(): React.ReactElement {
  const [status, setStatus] = useState<DopCredentialStatus | null>(null);

  useEffect(() => {
    void dopCredentialsStatus().then(setStatus);
  }, []);

  if (!status) return <span className="hidden" />;

  const source =
    status.source === 'atlas'
      ? 'Atlas (users → UserInfo)'
      : status.source === 'env'
        ? 'DOP_USERNAME / DOP_PASSWORD'
        : "this app's config file";

  return (
    <p className="mt-2 text-[11px] leading-relaxed text-slate-500">
      {status.has_password ? (
        <>
          <Pill tone="positive">credentials found</Pill> using{' '}
          <code className="font-mono">{status.username}</code> from {source}. The password stays in the
          Rust backend and is passed straight to <code className="font-mono">scraper.py</code>.
        </>
      ) : (
        <>
          <Pill tone="neutral">no credentials yet</Pill>{' '}
          {status.detail ??
            'Save them below, or add FERNET_KEY to src-tauri/.env so they can be read from Atlas.'}
        </>
      )}
    </p>
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
          <DatabaseSection />
          <DesktopSection />
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