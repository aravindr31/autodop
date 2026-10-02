/**
 * Manage panel (right drawer): the account-maintenance features from the
 * Python app — Add New Account, Delete Account, Change Password — plus Sign
 * out. Account mutations hit the live store and persist; the credential gate
 * is client-side (see `src/lib/auth.ts`).
 */
import { useState, useEffect, useCallback } from 'react';
import { useStore } from '../lib/store';
import { matchesQuery, denominationLabel } from '../lib/format';
import { notify } from '../lib/toast';
import {
  saveCredentials,
  useDesktop,
  localStatus,
  importFromAtlas,
  loadAccountsFromDb,
  loadLists,
  saveLists,
  dopCredentialsStatus,
  scraperLocation,
  setScraperPath,
  clearScraperPath,
  exportBackup,
  importBackup,
} from '../lib/bridge';
import type { LocalStatus, DopCredentialStatus, SavedCredentials, ScraperLocation } from '../lib/bridge';
import { X, Plus, Trash, LogOut, Search, KeyRound, Terminal, Database, Save, Upload } from 'lucide-react';
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
      notify('Password updated — the DOP password was re-encrypted', 'success');
      setOldPw(''); setNewPw(''); setNewPw2('');
    } else {
      notify('Current password is incorrect', 'error');
    }
  };

  return (
    <Section title="Change Password">
      <p className="mb-2 text-[11px] leading-relaxed text-slate-500">
        This is the password that unlocks the app, and the one the stored DOP
        password is encrypted with. Changing it re-encrypts that for you.
        <strong> If you forget it, the saved DOP password cannot be recovered</strong> —
        you would re-enter it from Manage → DOP portal password.
      </p>
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
  const [confirm, setConfirm] = useState('');
  const [busy, setBusy] = useState(false);
  const [configured, setConfigured] = useState(false);
  const [outcome, setOutcome] = useState<SavedCredentials | null>(null);

  useEffect(() => {
    if (info) setConfigured(info.credentials);
  }, [info]);

  const save = async () => {
    if (!username.trim() || !password) {
      notify('Username and password are both required', 'error');
      return;
    }
    // A mistyped password would silently break every run for 180 days.
    if (password !== confirm) {
      notify('The two passwords do not match', 'error');
      return;
    }
    setBusy(true);
    const res = await saveCredentials(username.trim(), password);
    setBusy(false);
    if (!res.ok) {
      notify(res.error ?? 'Could not save the DOP password', 'error');
      return;
    }
    setConfigured(true);
    setOutcome(res.result ?? null);
    setUsername('');
    setPassword('');
    setConfirm('');
    notify('DOP password saved, encrypted', 'success');
  };

  return (
    <Section title="DOP portal password">
      {ready ? (
        <>
          <p className="mb-2 text-xs leading-relaxed text-slate-500">
            The password <code className="font-mono">scraper.py</code> signs into the DOP portal
            with. India Post expires it every 180 days — change it on the portal first, then save it
            here.
          </p>
          <p className="mb-2 flex items-center gap-1.5 text-xs text-slate-500">
            <Terminal className="h-3.5 w-3.5 shrink-0" />
            <span>
              {configured ? 'A password is saved.' : 'No DOP password saved yet.'} Held by the Rust
              backend, never in the browser.
            </span>
          </p>
          <div className="flex flex-col gap-2">
            <input aria-label="DOP username" value={username} onChange={(e) => setUsername(e.currentTarget.value)} className={FIELD} placeholder="DOP username (DOP.MI…)" />
            <input type="password" aria-label="New DOP password" value={password} onChange={(e) => setPassword(e.currentTarget.value)} className={FIELD} placeholder="New DOP password" />
            <input type="password" aria-label="Confirm new DOP password" value={confirm} onChange={(e) => setConfirm(e.currentTarget.value)} className={FIELD} placeholder="Repeat the new password" />
            <Button variant="secondary" className="!w-full" disabled={busy} onClick={() => void save()}>
              <KeyRound className="h-4 w-4" />Save DOP password
            </Button>
          </div>
          <p className="mt-2 text-[11px] leading-relaxed text-slate-500">
            Stored encrypted with <code className="font-mono">FERNET_KEY</code> in this app&rsquo;s own
            database on this machine, readable only by your user. Nothing is sent anywhere.
          </p>
          {outcome ? (
            <p className="mt-2 text-[11px] leading-relaxed text-emerald-700">
              Stored encrypted — saved in{' '}
              <code className="font-mono">{outcome.location || 'autodop.db'}</code>.
            </p>
          ) : null}
          {info && !info.scraper_present ? (
            <p className="mt-2 text-[11px] text-rose-600">
              scraper.py was not found — pick one under <strong>Scraper script</strong> below.
            </p>
          ) : null}
          {info ? (
            <p className="mt-2 text-[11px] leading-relaxed text-slate-500">
              {info.scraper_kind === 'sidecar' ? (
                <>
                  Runner: <code className="font-mono">built into the app</code> — it carries its own
                  Python and selenium, so no interpreter is needed here.
                </>
              ) : (
                <>
                  Python: <code className="font-mono">{info.python}</code>
                  {info.python.includes('/') ? null : (
                    <>
                      {' '}
                      — the bare <code className="font-mono">python3</code> from PATH. Install selenium
                      into it, or point <code className="font-mono">AUTODOP_PYTHON</code> at an
                      interpreter that has it.
                    </>
                  )}
                </>
              )}
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

/**
 * Which build you are looking at — the answer to "did the install actually
 * replace the last one?".
 */
function BuildSection(): React.ReactElement {
  const { ready, info } = useDesktop();

  if (!ready || !info) {
    return (
      <Section title="This build">
        <p className="text-xs text-slate-500">Available in the desktop app only.</p>
      </Section>
    );
  }

  return (
    <Section title="This build">
      <dl className="flex flex-col gap-1 text-[11px] leading-relaxed">
        <div className="flex gap-2">
          <dt className="w-20 shrink-0 text-slate-400">Version</dt>
          <dd className="font-mono text-slate-700">{info.version}</dd>
        </div>
        <div className="flex gap-2">
          <dt className="w-20 shrink-0 text-slate-400">Built from</dt>
          <dd className="font-mono text-slate-700">{info.build}</dd>
        </div>
        <div className="flex gap-2">
          <dt className="w-20 shrink-0 text-slate-400">Runner</dt>
          <dd className="font-mono text-slate-700">
            {info.scraper_kind === 'sidecar' ? 'self-contained' : 'script + Python'}
          </dd>
        </div>
      </dl>
      <p className="mt-2 text-[11px] leading-relaxed text-slate-400">
        The installer is named after the version, so two builds never look alike.
      </p>
    </Section>
  );
}

/** How the resolved `scraper.py` is described in the UI. */
const SCRAPER_SOURCE_LABEL: Record<string, string> = {
  chosen: 'chosen here',
  env: 'AUTODOP_SCRAPER',
  sidecar: 'built in — no Python needed',
  bundled: 'bundled script (needs Python)',
  repo: 'repo checkout (dev)',
  cwd: 'working directory',
};

/**
 * Where `scraper.py` comes from.
 *
 * Every build carries its own copy, so this section only matters when you want
 * to point the app at a different script — a modified one, say.
 */
function ScraperSection(): React.ReactElement {
  const { ready } = useDesktop();
  const [location, setLocation] = useState<ScraperLocation | null>(null);
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    setLocation(await scraperLocation());
  }, []);

  useEffect(() => {
    if (ready) void refresh();
  }, [ready, refresh]);

  const apply = async () => {
    if (!draft.trim()) {
      notify('Enter the full path to a scraper.py', 'error');
      return;
    }
    setBusy(true);
    const res = await setScraperPath(draft.trim());
    setBusy(false);
    if (!res.ok) {
      notify(res.error ?? 'Could not use that path', 'error');
      return;
    }
    setLocation(res.location ?? null);
    setDraft('');
    notify('scraper.py path updated', 'success');
  };

  const useBundled = async () => {
    setBusy(true);
    const res = await clearScraperPath();
    setBusy(false);
    if (!res.ok) {
      notify(res.error ?? 'Could not reset the path', 'error');
      return;
    }
    setLocation(res.location ?? null);
    notify('Back to the copy that ships with the app', 'success');
  };

  return (
    <Section title="Scraper script">
      {ready ? (
        <>
          <p className="mb-2 text-xs leading-relaxed text-slate-500">
            The runner that drives the DOP portal. Builds carry a self-contained copy, so there is
            normally nothing to set here.
          </p>
          {location ? (
            <div className="mb-2 flex flex-col gap-1 rounded-lg bg-slate-50 px-2.5 py-2">
              <Pill tone={location.present ? 'positive' : 'neutral'}>
                {location.present
                  ? (SCRAPER_SOURCE_LABEL[location.source] ?? location.source)
                  : 'not found'}
              </Pill>
              <code className="break-all font-mono text-[11px] text-slate-700">{location.path}</code>
            </div>
          ) : null}
          <div className="flex flex-col gap-2">
            <input
              aria-label="Path to scraper.py"
              value={draft}
              onChange={(e) => setDraft(e.currentTarget.value)}
              className={FIELD}
              placeholder="/path/to/scraper.py"
            />
            <div className="flex gap-2">
              <Button variant="secondary" className="!flex-1" disabled={busy} onClick={() => void apply()}>
                Use this path
              </Button>
              <Button variant="ghost" className="!flex-1" disabled={busy} onClick={() => void useBundled()}>
                Built-in copy
              </Button>
            </div>
          </div>
          <p className="mt-2 text-[11px] leading-relaxed text-slate-500">
            {location?.kind === 'sidecar' ? (
              <>This copy carries its own Python and selenium, so only Chrome has to be installed.</>
            ) : (
              <>
                This only picks the script. <code className="font-mono">selenium</code> and Chrome
                still have to be installed — see Python above.
              </>
            )}
          </p>
        </>
      ) : (
        <p className="text-xs text-slate-500">Available in the desktop app only.</p>
      )}
    </Section>
  );
}

/**
 * Backup / restore — a copy of the SQLite file, made with SQLite's own
 * backup API so WAL pages are not missed.
 *
 * The backup carries the encrypted DOP password and the login hash, and
 * neither is readable without the login password the key was derived from.
 * That is the caveat, said plainly: the backup is the file plus you
 * remembering the password.
 */
function BackupSection(): React.ReactElement {
  const { ready } = useDesktop();
  const [exportPath, setExportPath] = useState('');
  const [importPath, setImportPath] = useState('');
  const [busy, setBusy] = useState(false);
  const [confirmingRestore, setConfirmingRestore] = useState(false);

  const runExport = async () => {
    if (!exportPath.trim()) {
      notify('Enter a full path for the backup file', 'error');
      return;
    }
    setBusy(true);
    const res = await exportBackup(exportPath.trim());
    setBusy(false);
    if (!res.ok || !res.outcome) {
      notify(res.error ?? 'Backup failed', 'error');
      return;
    }
    notify(
      `Backed up ${res.outcome.accounts} accounts, ${res.outcome.lists} list(s)` +
        (res.outcome.has_credentials ? ' and the DOP password' : '') +
        ` to ${res.outcome.previous}`,
      'success',
    );
  };

  const runRestore = async () => {
    setBusy(true);
    const res = await importBackup(importPath.trim());
    setBusy(false);
    setConfirmingRestore(false);
    if (!res.ok || !res.outcome) {
      notify(res.error ?? 'Restore failed', 'error');
      return;
    }
    setImportPath('');
    notify(
      `Restored ${res.outcome.accounts} accounts, ${res.outcome.lists} list(s)` +
        (res.outcome.has_credentials ? ' and the DOP password' : ''),
      'success',
    );
    if (res.outcome.previous) {
      notify(`The replaced database was kept at ${res.outcome.previous}`, 'info');
    }
    const accounts = await loadAccountsFromDb();
    if (accounts.ok && accounts.accounts) useStore.getState().setAccounts(accounts.accounts);
    const lists = await loadLists();
    if (lists.ok && lists.lists) useStore.getState().setLists(lists.lists, lists.activeId);
  };

  return (
    <Section title="Backup & restore">
      {ready ? (
        <>
          <p className="mb-2 text-[11px] leading-relaxed text-slate-500">
            One SQLite file holds everything on this machine. The backup carries
            the DOP password encrypted with your login password —
            <strong> it only opens together with that password</strong>. On another
            machine, sign in with the same password first.
          </p>
          <input
            aria-label="Backup destination path"
            value={exportPath}
            onChange={(e) => setExportPath(e.currentTarget.value)}
            className={FIELD}
            placeholder="~/Backups/autodop-backup.db"
          />
          <Button
            variant="secondary"
            className="!mt-2 !w-full"
            disabled={busy}
            onClick={() => void runExport()}
          >
            <Save className="h-4 w-4" />Back up now
          </Button>
          <div className="mt-3 border-t border-slate-100 pt-2">
            <input
              aria-label="Backup path to restore from"
              value={importPath}
              onChange={(e) => { setImportPath(e.currentTarget.value); setConfirmingRestore(false); }}
              className={FIELD}
              placeholder="Path to an autodop backup .db"
            />
            {confirmingRestore ? (
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <span className="text-[11px] text-rose-700">
                  Replace the live database with this backup?
                </span>
                <Button variant="danger" size="sm" disabled={busy} onClick={() => void runRestore()}>
                  Yes, restore
                </Button>
                <Button variant="secondary" size="sm" onClick={() => setConfirmingRestore(false)}>
                  Cancel
                </Button>
              </div>
            ) : (
              <Button
                variant="secondary"
                className="!mt-2 !w-full"
                disabled={busy || !importPath.trim()}
                onClick={() => setConfirmingRestore(true)}
              >
                <Upload className="h-4 w-4" />Restore from backup
              </Button>
            )}
            <p className="mt-2 text-[11px] leading-relaxed text-slate-500">
              A restore keeps the replaced database as
              <code className="font-mono"> autodop.db.pre-restore-…</code> next to the live
              file, and signs you out — sign back in with the password the backup expects.
            </p>
          </div>
        </>
      ) : (
        <p className="text-xs text-slate-500">Available in the desktop app only.</p>
      )}
    </Section>
  );
}

/**
 * The local database — one SQLite file on this machine.
 *
 * Nothing is shared with anyone else, so there is no connection to check, no
 * role to grant, and no read-only caveat. Atlas appears here only as the
 * one-time import source for the data this app started out with.
 */
function DatabaseSection(): React.ReactElement {
  const { ready } = useDesktop();
  const shown = useStore((s) => s.accounts.length);
  const [status, setStatus] = useState<LocalStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmingImport, setConfirmingImport] = useState(false);

  const check = useCallback(async () => {
    setStatus(await localStatus());
  }, []);

  useEffect(() => {
    if (ready) void check();
  }, [ready, check]);

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
    notify(`Loaded ${res.lists.length} list(s)`, 'success');
  };

  const pushLists = async () => {
    const store = useStore.getState();
    setBusy(true);
    const res = await saveLists(store.lists, store.activeListId);
    setBusy(false);
    if (!res.ok || !res.lists) {
      notify(res.error ?? 'Saving lists failed', 'error');
      return;
    }
    // Adopt the stored ids so the next save updates instead of duplicating.
    // Ids can change (a list is matched by name when it has no id yet), so
    // follow the active list across by name rather than by id.
    const activeName = store.lists.find((list) => list.id === store.activeListId)?.name;
    const activeId = res.lists.find((list) => list.name === activeName)?.id;
    useStore.getState().setLists(res.lists, activeId);
    notify(`Saved ${res.lists.length} list(s)`, 'success');
    void check();
  };

  const runImport = async (force: boolean) => {
    setBusy(true);
    const res = await importFromAtlas(force);
    setBusy(false);
    setConfirmingImport(false);
    if (!res.ok) {
      notify(res.error ?? 'Import failed', 'error');
      return;
    }
    const report = res.report;
    notify(
      `Imported ${report?.accounts ?? 0} accounts, ${report?.lists ?? 0} lists` +
        (report?.credentials ? ', and the DOP password' : ''),
      'success',
    );
    for (const warning of report?.warnings ?? []) notify(warning, 'error');
    // Show it immediately rather than waiting for a restart.
    const accounts = await loadAccountsFromDb();
    if (accounts.ok && accounts.accounts) useStore.getState().setAccounts(accounts.accounts);
    const lists = await loadLists();
    if (lists.ok && lists.lists) useStore.getState().setLists(lists.lists, lists.activeId);
    void check();
  };

  return (
    <Section title="Local database">
      {!ready ? (
        <p className="text-xs text-slate-500">Available in the desktop app only.</p>
      ) : (
        <>
          <div className="mb-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-slate-500">
            <Pill tone={status?.error ? 'neutral' : 'positive'}>
              <Database className="h-3 w-3" />
              {(status?.accounts ?? 0).toLocaleString('en-IN')} accounts
            </Pill>
            <span>
              · {status?.lists ?? 0} lists · {status?.entries ?? 0} in lists · showing{' '}
              {shown.toLocaleString('en-IN')}
            </span>
          </div>
          {status?.path ? (
            <p className="mb-2 break-all font-mono text-[11px] text-slate-400">{status.path}</p>
          ) : null}
          {status?.error ? (
            <p className="mb-2 text-[11px] text-rose-600">{status.error}</p>
          ) : null}
          <p className="mb-2 text-[11px] leading-relaxed text-slate-500">
            {status?.has_credentials ? (
              <><Pill tone="positive">DOP password stored</Pill> encrypted in this file.</>
            ) : (
              <><Pill tone="neutral">no DOP password yet</Pill> add one below.</>
            )}
          </p>
          <div className="flex gap-2">
            <Button variant="secondary" className="!w-full" disabled={busy} onClick={() => void reload()}>
              Reload accounts
            </Button>
            <Button variant="secondary" className="!w-full" disabled={busy} onClick={() => void pullLists()}>
              Load lists
            </Button>
          </div>
          <div className="mt-2">
            <Button variant="primary" className="!w-full" disabled={busy} onClick={() => void pushLists()}>
              Save lists
            </Button>
          </div>
          {status?.atlas_available ? (
            <div className="mt-3 border-t border-slate-100 pt-2">
              <p className="mb-2 text-[11px] leading-relaxed text-slate-500">
                {status.atlas_imported_at ? (
                  <>
                    Imported from Atlas on{' '}
                    <span className="font-mono">{status.atlas_imported_at}</span>. Running it again
                    replaces the local accounts and lists with the Atlas copy.
                  </>
                ) : (
                  <>
                    Atlas still holds the data this app started with. Import it once — accounts,
                    lists, and the DOP password re-encrypted with this machine&rsquo;s own key — and
                    nothing here depends on Atlas afterwards.
                  </>
                )}
              </p>
              {confirmingImport ? (
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[11px] text-rose-700">
                    Replace the local accounts and lists with the Atlas copy?
                  </span>
                  <Button variant="danger" size="sm" disabled={busy} onClick={() => void runImport(true)}>
                    Yes, import
                  </Button>
                  <Button variant="secondary" size="sm" onClick={() => setConfirmingImport(false)}>
                    Cancel
                  </Button>
                </div>
              ) : (
                <Button
                  variant="secondary"
                  className="!w-full"
                  disabled={busy}
                  onClick={() =>
                    (status.accounts > 0 ? setConfirmingImport(true) : void runImport(false))
                  }
                >
                  <Database className="h-4 w-4" />Import from Atlas
                </Button>
              )}
            </div>
          ) : null}
          <p className="mt-2 text-[11px] leading-relaxed text-slate-400">
            One SQLite file on this machine, owned by this app. Nothing is shared with anyone else, so
            there is no connection to check and no role to grant.
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
    status.source === 'local'
      ? "this app's database"
      : status.source === 'atlas'
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
            'Save them below — they go into this app’s own database, encrypted.'}
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
          <BackupSection />
          <DesktopSection />
          <ScraperSection />
          <BuildSection />
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