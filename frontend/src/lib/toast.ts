/**
 * Minimal decoupled toast notifications. Components fire-and-forget a message;
 * the App shell subscribes and renders the stack. Avoids threading a callback
 * through every component or coupling toasts to the store.
 */

export type ToastTone = 'success' | 'error' | 'info';

export interface ToastMessage {
  id: number;
  text: string;
  tone: ToastTone;
}

type Listener = (message: ToastMessage) => void;

const listeners = new Set<Listener>();
let nextId = 1;

export function notify(text: string, tone: ToastTone = 'info'): void {
  const message: ToastMessage = { id: nextId++, text, tone };
  for (const listener of listeners) listener(message);
}

/** Subscribe; returns an unsubscribe function. */
export function onNotify(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}