// Toasts report the result of an action. Persistent conditions belong in inline notices.
export type Toast = { id: number; tone: 'success' | 'error' | 'info'; title: string; body?: string };
export const toasts = $state<Toast[]>([]);
let seq = 0;

export function toast(title: string, opts: { tone?: Toast['tone']; body?: string } = {}) {
  const existing = toasts.find((t) => t.title === title);
  if (existing) dismiss(existing.id);
  const t: Toast = { id: ++seq, tone: opts.tone ?? 'success', title, body: opts.body };
  toasts.push(t);
  if (toasts.length > 4) toasts.shift();
  setTimeout(() => dismiss(t.id), t.tone === 'error' ? 9000 : 4500);
}

export function dismiss(id: number) {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}
