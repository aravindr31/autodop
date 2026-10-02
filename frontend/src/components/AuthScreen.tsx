import { useState } from 'react';
import { useStore } from '../lib/store';
import { ChevronLeft, Lock, UserPlus, LogIn, Users } from 'lucide-react';
import { Button } from './ui';
import { VersionLine } from './VersionLine';

const FIELD =
  'w-full rounded-xl border border-slate-200 bg-white py-2.5 pl-3 pr-3 text-sm placeholder:text-slate-400 focus:border-indigo-400 focus:outline-2 focus:outline-offset-0 focus:outline-indigo-200';

export default function AuthScreen(): React.ReactElement {
  const store = useStore.getState();
  const needsSetup = useStore((s) => s.authConfigured === false);
  const authReady = useStore((s) => s.authReady);
  const selected = useStore((s) => s.selectedOwner);
  const owners = useStore((s) => s.owners);
  const [choosing, setChoosing] = useState(false);

  const [adding, setAdding] = useState(false);
  const [username, setUsername] = useState('');
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setErr('');
    if (busy) return;
    setBusy(true);
    try {
      if (needsSetup || adding) {
        if (!username.trim()) setErr('Enter your DOP portal id (your mobile number).');
        else if (pw.length < 1) setErr('Choose a password to continue.');
        else if (pw !== pw2) setErr('Passwords do not match.');
        else {
          await store.setupPassword(username, pw);
          setAdding(false);
        }
      } else {
        const target = selected ?? owners[0];
        if (!target) {
          setErr('No workspace found — add one below.');
          setChoosing(true);
          return;
        }
        if (await store.login(pw, target)) setPw('');
        else setErr('Incorrect password.');
      }
    } catch (error) {
      setErr(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  if (!authReady) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-slate-50 text-sm text-slate-400">
        Checking…
      </div>
    );
  }

  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-2 bg-slate-50 px-4">
      <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-indigo-600 text-white shadow-sm">
        <Lock className="h-6 w-6" />
      </div>
      <h1 className="text-xl font-semibold text-slate-800">AutoDOP</h1>
      <p className="text-sm text-slate-500">
        {needsSetup || adding
          ? 'Create your workspace — DOP portal id and a password.'
          : selected
            ? `Welcome back, ${selected.username}.`
            : 'Sign in to continue.'}
      </p>
      <form
        className="mt-4 w-full max-w-sm rounded-2xl border border-slate-200 bg-white p-5 shadow-sm"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        {(needsSetup || adding) && (
          <>
            <label className="block text-xs font-medium text-slate-500">
              DOP portal id (mobile number)
            </label>
            <input
              autoFocus
              className={FIELD}
              value={username}
              onChange={(e) => setUsername(e.currentTarget.value)}
              placeholder="e.g. 9440000000"
            />
          </>
        )}
        {!needsSetup && selected && (
          <div className="mb-1 flex items-center justify-between rounded-lg bg-slate-50 px-2.5 py-1.5">
            <span className="truncate font-mono text-xs text-slate-700">{selected.username}</span>
            <button
              type="button"
              className="text-[11px] font-medium text-indigo-600 hover:text-indigo-800"
              onClick={() => setChoosing((v) => !v)}
            >
              Not you?
            </button>
          </div>
        )}
        <label className="block text-xs font-medium text-slate-500">Password</label>
        <input
          type="password"
          autoFocus={!needsSetup}
          className={FIELD}
          value={pw}
          onChange={(e) => setPw(e.currentTarget.value)}
          placeholder="Password"
        />
        {(needsSetup || adding) && (
          <>
            <label className="mt-3 block text-xs font-medium text-slate-500">Confirm password</label>
            <input
              type="password"
              className={FIELD}
              value={pw2}
              onChange={(e) => setPw2(e.currentTarget.value)}
              placeholder="Repeat password"
            />
          </>
        )}
        {err ? <p className="mt-3 text-xs font-medium text-rose-600">{err}</p> : null}
        <Button type="submit" variant="primary" className="!mt-4 !w-full" disabled={busy}>
          {needsSetup || adding ? (
            <>
              <UserPlus className="mx-1 h-4 w-4" />Create workspace
            </>
          ) : (
            <>
              <LogIn className="mx-1 h-4 w-4" />Sign in
            </>
          )}
        </Button>

        {adding && (
          <button
            type="button"
            className="mt-2 text-[11px] text-slate-400 hover:text-slate-600"
            onClick={() => {
              setAdding(false);
              setErr('');
            }}
          >
            Back to sign in
          </button>
        )}

        {!needsSetup && !adding && choosing && (
          <div className="mt-3 rounded-xl border border-slate-100 bg-slate-50 p-3">
            <p className="mb-2 flex items-center gap-1 text-[11px] font-semibold text-slate-600">
              <Users className="h-3.5 w-3.5" />Workspaces on this machine
            </p>
            <ul className="flex flex-col gap-1">
              {owners
                .filter((o) => o.id !== selected?.id)
                .map((o) => (
                  <li key={o.id}>
                    <button
                      type="button"
                      className="w-full rounded-lg bg-white px-2.5 py-1.5 text-left text-xs text-slate-700 shadow-sm hover:bg-indigo-50"
                      onClick={() => {
                        store.selectOwner(o);
                        setPw('');
                        setErr('');
                        setChoosing(false);
                      }}
                    >
                      {o.username}
                    </button>
                  </li>
                ))}
              {owners.filter((o) => o.id !== selected?.id).length === 0 && (
                <li className="text-[11px] text-slate-400">No other workspace yet.</li>
              )}
            </ul>
            <button
              type="button"
              className="mt-1 w-full rounded-lg border border-dashed border-slate-300 px-2.5 py-1.5 text-left text-xs font-medium text-indigo-600 hover:bg-indigo-50"
              onClick={() => {
                setAdding(true);
                setUsername('');
                setPw('');
                setPw2('');
                setErr('');
                setChoosing(false);
              }}
            >
              + Add a new workspace
            </button>
            <p className="mt-2 text-[11px] leading-snug text-slate-400">
              Each workspace is separate: sign in with your own id, import your
              backup, and your data never mixes with anyone else's.
            </p>
          </div>
        )}

        <p className="mt-4 text-center text-[11px] leading-snug text-slate-400">
          The password protects this workspace on this machine only — the same
          password opens your portable backup imports too.
        </p>
        <VersionLine className="mt-3 text-center" />
      </form>
      {choosing && (
        <button
          type="button"
          className="mt-1 flex items-center gap-1 text-[11px] text-slate-400 hover:text-slate-600"
          onClick={() => setChoosing(false)}
        >
          <ChevronLeft className="h-3 w-3" />Back
        </button>
      )}
    </div>
  );
}
