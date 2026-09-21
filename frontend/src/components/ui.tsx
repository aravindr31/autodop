/**
 * Shared UI primitives: buttons, badges, empty states, tooltips, toast stack.
 * Styling is utility-class based (Tailwind v4); no @apply, so builds stay safe.
 */
import type { ReactNode } from 'react';
import { Check, X } from 'lucide-react';

/* ------------------------------------------------------------------ */
/* Button                                                              */
/* ------------------------------------------------------------------ */

interface ButtonProps {
  onClick?: () => void;
  disabled?: boolean;
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  size?: 'sm' | 'md';
  className?: string;
  children: ReactNode;
  title?: string;
  type?: 'button' | 'submit';
}

const BUTTON_VARIANTS: Record<NonNullable<ButtonProps['variant']>, string> = {
  primary:
    'inline-flex items-center justify-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400 disabled:opacity-50',
  secondary:
    'inline-flex items-center justify-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 shadow-sm hover:bg-slate-50 hover:text-slate-900 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400 disabled:opacity-50',
  ghost:
    'inline-flex items-center justify-center gap-1.5 rounded-lg px-2.5 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-100 hover:text-slate-900 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400 disabled:opacity-50',
  danger:
    'inline-flex items-center justify-center gap-1.5 rounded-lg border border-rose-200 bg-white px-3 py-1.5 text-sm font-medium text-rose-600 hover:border-rose-300 hover:bg-rose-50 hover:text-rose-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-rose-400 disabled:opacity-50',
};

export function Button({ onClick, disabled, variant = 'secondary', size = 'md', className, children, title, type = 'button' }: ButtonProps): React.ReactElement {
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      title={title}
      className={[BUTTON_VARIANTS[variant], size === 'sm' ? '!text-xs !px-2 !py-1' : '', className ?? ''].join(' ')}
    >
      {children}
    </button>
  );
}

/** Small 1:1 icon button used for row actions (remove, expand). */
export function IconButton({
  onClick,
  disabled,
  danger,
  label,
  title,
  children,
}: {
  onClick?: () => void;
  disabled?: boolean;
  danger?: boolean;
  label: string;
  title?: string;
  children: ReactNode;
}): React.ReactElement {
  return (
    <button
      type="button"
      aria-label={label}
      title={title ?? label}
      onClick={onClick}
      disabled={disabled}
      className={danger
        ? 'inline-flex h-7 w-7 items-center justify-center rounded-md border border-rose-200 bg-white text-rose-500 hover:bg-rose-50 hover:text-rose-600 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-rose-400 disabled:opacity-40'
        : 'inline-flex h-7 w-7 items-center justify-center rounded-md border border-slate-200 bg-white text-slate-500 hover:bg-slate-100 hover:text-slate-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-slate-400 disabled:opacity-40'}
    >
      {children}
    </button>
  );
}

/* ------------------------------------------------------------------ */
/* Badge / pill                                                          */
/* ------------------------------------------------------------------ */

export function Pill({ children, tone = 'neutral' }: { children: ReactNode; tone?: 'neutral' | 'accent' | 'positive' }): React.ReactElement {
  const styles: Record<string, string> = {
    neutral: 'bg-slate-100 text-slate-700',
    accent: 'bg-indigo-50 text-indigo-700',
    positive: 'bg-emerald-50 text-emerald-700',
  };
  return (
    <span className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium ${styles[tone]}`}>
      {children}
    </span>
  );
}

/* ------------------------------------------------------------------ */
/* Empty state                                                           */
/* ------------------------------------------------------------------ */

export function EmptyState({ icon, title, hint }: { icon?: ReactNode; title: string; hint?: string }): React.ReactElement {
  return (
    <div className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-slate-300 bg-white px-6 py-10 text-center">
      {icon ? <div className="text-slate-300">{icon}</div> : null}
      <p className="text-sm font-medium text-slate-600">{title}</p>
      {hint ? <p className="text-xs text-slate-400">{hint}</p> : null}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Toast stack (rendered by the App shell)                               */
/* ------------------------------------------------------------------ */

export interface ToastView {
  id: number;
  text: string;
  tone: 'success' | 'error' | 'info';
}

const TONE_STYLES: Record<ToastView['tone'], string> = {
  success: 'border-emerald-200 bg-emerald-50 text-emerald-800',
  error: 'border-rose-200 bg-rose-50 text-rose-800',
  info: 'border-slate-200 bg-white text-slate-700',
};

export function ToastViewItem({ toast, onDismiss }: { toast: ToastView; onDismiss: () => void }): React.ReactElement {
  return (
    <div className={`pointer-events-auto flex w-80 items-start gap-2.5 rounded-lg border p-3 text-sm shadow-lg ${TONE_STYLES[toast.tone]}`} role="status">
      {toast.tone === 'success' ? <Check className="mt-0.5 h-4 w-4 shrink-0" /> : null}
      <span className="min-w-0 flex-1 leading-snug">{toast.text}</span>
      <button type="button" aria-label="Dismiss" onClick={onDismiss} className="text-xs opacity-70 hover:opacity-100">
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}