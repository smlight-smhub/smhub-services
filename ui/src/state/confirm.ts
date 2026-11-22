import { signal } from '@preact/signals';
import type { ComponentChildren } from 'preact';

export interface ConfirmOptions {
  title: string;
  body?: ComponentChildren;
  confirm: string;
  cancel?: string;
  tone?: 'danger' | 'primary';
}

export interface ConfirmRequest extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}

/** The question the shell's dialog is showing. */
export const confirmRequest = signal<ConfirmRequest | null>(null);

/** Asks the user before something that cannot be undone. */
export function confirm(o: ConfirmOptions): Promise<boolean> {
  confirmRequest.peek()?.resolve(false);
  return new Promise((resolve) => {
    confirmRequest.value = {
      ...o,
      resolve: (ok) => {
        confirmRequest.value = null;
        resolve(ok);
      },
    };
  });
}
