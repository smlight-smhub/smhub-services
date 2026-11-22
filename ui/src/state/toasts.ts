import { signal } from '@preact/signals';

export type Tone = 'ok' | 'info' | 'warn' | 'danger';

export interface Toast {
  id: number;
  text: string;
  tone: Tone;
  /** Shown as "Undo"; runs once. */
  undo?: () => unknown;
  action?: { label: string; run: () => unknown };
  /** Milliseconds; 0 keeps it until dismissed. */
  timeout: number;
}

export const toasts = signal<Toast[]>([]);
let next = 1;
const MAX = 4;

export function toast(t: Omit<Toast, 'id' | 'timeout' | 'tone'> & { tone?: Tone; timeout?: number }): number {
  const id = next++;
  const item: Toast = {
    tone: 'info',
    timeout: t.undo || t.action ? 8000 : t.tone === 'danger' ? 8000 : 4500,
    ...t,
    id,
  };
  toasts.value = [...toasts.value, item].slice(-MAX);
  return id;
}

export function dismissToast(id: number) {
  toasts.value = toasts.value.filter((t) => t.id !== id);
}
