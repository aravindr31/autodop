/**
 * List management side panel (spec §3.2). Lists are the unit of organization:
 * create, select (persists as the active list), rename and delete. On mobile
 * this panel is shown inside a slide-over drawer (see App.tsx).
 */
import { useState } from 'react';
import { useStore } from '../lib/store';
import { Plus, Pencil, X, Check } from 'lucide-react';

function ListRow({
  list,
  active,
  onNavigate,
}: {
  list: { id: string; name: string };
  active: boolean;
  onNavigate: () => void;
}): React.ReactElement {
  const store = useStore.getState();
  const count = useStore((s) => s.totalsOf(list.id).count);
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(list.name);
  const [confirming, setConfirming] = useState(false);

  const commitRename = () => {
    store.renameList(list.id, value);
    setEditing(false);
  };

  const commitDelete = () => {
    store.deleteList(list.id);
    setConfirming(false);
  };

  const select = () => {
    store.setActiveList(list.id);
    onNavigate();
  };

  return (
    <li className={active
      ? 'group relative rounded-lg bg-indigo-50 text-indigo-800 shadow-sm'
      : 'group relative rounded-lg bg-white text-slate-700 hover:bg-slate-50'}>
      <button
        type="button"
        onClick={select}
        className="flex w-full items-center gap-2.5 px-2.5 py-2 text-left text-sm font-medium"
      >
        {!editing && (
          <>
            <span className={`h-1.5 w-1.5 rounded-full ${active ? 'bg-indigo-500' : 'bg-transparent'}`} />
            <span className="min-w-0 flex-1 truncate">{list.name}</span>
            <span className="rounded-full bg-slate-200 px-1.5 text-[11px] font-semibold leading-none text-slate-600">{count}</span>
          </>
        )}
      </button>

      {editing ? (
        <form className="flex items-center gap-1.5 px-2.5 py-2" onSubmit={commitRename}>
          <input
            className="w-full min-w-0 flex-1 rounded-md border border-slate-300 px-2 py-1 text-sm"
            value={value}
            onChange={(e) => setValue(e.currentTarget.value)}
            onBlur={commitRename}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setValue(list.name);
                setEditing(false);
              }
            }}
            autoFocus
          />
          <button type="button" aria-label="Save rename" onClick={commitRename}
            className="inline-flex h-6 w-6 items-center justify-center rounded-md bg-slate-100 text-slate-600">
            <Check className="h-4 w-4" />
          </button>
        </form>
      ) : (
        <div className="absolute right-1.5 top-1/2 flex -translate-y-1/2 gap-1 rounded-lg bg-white opacity-60 shadow-sm transition-opacity hover:opacity-100 group-hover:opacity-100">
          {confirming ? (
            <>
              <button type="button" aria-label="Confirm delete" onClick={commitDelete}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md bg-rose-100 text-rose-700">
                <Check className="h-4 w-4" />
              </button>
              <button type="button" aria-label="Cancel" onClick={() => setConfirming(false)}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md bg-slate-100 text-slate-600">
                <X className="h-4 w-4" />
              </button>
            </>
          ) : (
            <>
              <button type="button" aria-label={`Rename ${list.name}`} onClick={() => { setValue(list.name); setEditing(true); }}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md text-slate-600 hover:bg-slate-100">
                <Pencil className="h-4 w-4" />
              </button>
              <button type="button" aria-label={`Delete ${list.name}`} onClick={() => setConfirming(true)}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md text-rose-500 hover:bg-rose-50">
                <X className="h-4 w-4" />
              </button>
            </>
          )}
        </div>
      )}
    </li>
  );
}

/** Pure list UI — reused on desktop (inline aside) and mobile (drawer). */
export function SidebarPanel({ onNavigate }: { onNavigate: () => void }): React.ReactElement {
  const lists = useStore((s) => s.lists);
  const activeId = useStore((s) => s.activeListId);
  const store = useStore.getState();

  // New lists get the next free letter (A, B, C …) and lists render alphabetically.
  const sorted = [...lists].sort((x, y) => x.name.localeCompare(y.name, undefined, { sensitivity: 'base' }));

  const create = () => {
    const created = store.createList();
    store.setActiveList(created.id);
    onNavigate();
  };

  return (
    <div className="flex flex-col gap-1.5">
      <div className="px-2.5 py-1 text-[11px] font-semibold uppercase tracking-wide text-slate-400">
        Lists {lists.length > 0 ? `· ${lists.length}` : ''}
      </div>

      {sorted.length === 0 ? (
        <p className="px-2.5 py-2 text-xs text-slate-400">No lists yet. Create one below.</p>
      ) : (
        <ul className="flex flex-col gap-0.5">
          {sorted.map((list) => (
            <ListRow
              key={list.id}
              list={list}
              active={list.id === activeId}
              onNavigate={onNavigate}
            />
          ))}
        </ul>
      )}

      <button
        type="button"
        onClick={create}
        className="mt-2 inline-flex w-full items-center justify-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
      >
        <Plus className="h-4 w-4" />
        Create List
      </button>
    </div>
  );
}

/** Wraps SidebarPanel for the mobile slide-over drawer. */
export default function Sidebar({ open, onClose }: { open: boolean; onClose: () => void }): React.ReactElement {
  if (!open) return <span className="hidden" />;
  return (
    <div className="fixed inset-0 z-40 lg:hidden">
      <div className="absolute inset-0 bg-slate-900/25 backdrop-blur-sm" onClick={onClose} aria-hidden />
      <div className="absolute inset-y-0 left-0 w-64 max-w-[85%] overflow-y-auto border-r border-slate-200 bg-white p-4 shadow-2xl" role="dialog" aria-label="Lists">
        <button type="button" onClick={onClose} aria-label="Close lists"
          className="absolute right-2.5 top-2.5 inline-flex h-7 w-7 items-center justify-center rounded-md text-slate-500 hover:bg-slate-100">
          <X className="h-5 w-5" />
        </button>
        <div className="mt-2">
          <SidebarPanel onNavigate={onClose} />
        </div>
      </div>
    </div>
  );
}