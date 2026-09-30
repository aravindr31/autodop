/**
 * Login / first-run setup screen (spec-aligned with the Python app, which
 * required a login before any account view). Because the backend is separate,
 * this is a client-side session gate: on first run the user sets a password,
 * afterwards they log in with it. See `src/lib/auth.ts` for the (client-side
 * only, pending-backend) credential note.
 */
import { useState } from 'react';
import { useStore } from '../lib/store';
import { Lock, UserPlus, LogIn } from 'lucide-react';
import { Button } from './ui';
import { VersionLine } from './VersionLine';

const FIELD =
  'w-full rounded-xl border border-slate-200 bg-white py-2.5 pl-3 pr-3 text-sm placeholder:text-slate-400 focus:border-indigo-400 focus:outline-2 focus:outline-offset-0 focus:outline-indigo-200';

export default function AuthScreen(): React.ReactElement {
  const store = useStore.getState();
  const needsSetup = useStore((s) => s.auth === null);
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    setErr('');
    if (busy) return;
    setBusy(true);
    try {
      if (needsSetup) {
        if (pw.length < 1) setErr('Choose a password to continue.');
        else if (pw !== pw2) setErr('Passwords do not match.');
        else await store.setupPassword(pw);
      } else {
        if (await store.login(pw)) setPw('');
        else setErr('Incorrect password.');
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-2 bg-slate-50 px-4">
      <div className="flex h-12 w-12 items-center justify-center rounded-2xl bg-indigo-600 text-white shadow-sm">
        <Lock className="h-6 w-6" />
      </div>
      <h1 className="text-xl font-semibold text-slate-800">AutoDOP</h1>
      <p className="text-sm text-slate-500">
        {needsSetup ? 'Set a password to protect this workspace.' : 'Sign in to continue.'}
      </p>
      <form
        className="mt-4 w-full max-w-sm rounded-2xl border border-slate-200 bg-white p-5 shadow-sm"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <label className="block text-xs font-medium text-slate-500">Password</label>
        <input type="password" autoFocus className={FIELD} value={pw} onChange={(e) => setPw(e.currentTarget.value)} placeholder="Password" />
        {needsSetup && (
          <>
            <label className="mt-3 block text-xs font-medium text-slate-500">Confirm password</label>
            <input type="password" className={FIELD} value={pw2} onChange={(e) => setPw2(e.currentTarget.value)} placeholder="Repeat password" />
          </>
        )}
        {err ? <p className="mt-3 text-xs font-medium text-rose-600">{err}</p> : null}
        <Button
          type="submit"
          variant="primary"
          className="!mt-4 !w-full"
          disabled={busy}
        >
          {needsSetup ? <><UserPlus className="mx-1 h-4 w-4" />Set password</> : <><LogIn className="mx-1 h-4 w-4" />Sign in</>}
        </Button>
        <p className="mt-4 text-center text-[11px] leading-snug text-slate-400">
          This protects the local workspace only. Real authentication comes from the separately-deployed backend.
        </p>
        <VersionLine className="mt-3 text-center" />
      </form>
    </div>
  );
}