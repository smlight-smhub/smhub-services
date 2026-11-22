// The one save path for settings (D30, architecture "How a setting is saved"):
// show the change at once, send it, take the hub's answer as the truth, or put
// the old value back and show the hub's message next to the field it names.
import { signal, type Signal } from '@preact/signals';
import { useMemo, useState, useEffect } from 'preact/hooks';
import { ApiError } from '../api/client';
import { t, tx } from '../text/i18n';
import { addRestartNotice, type RestartNotice } from './notices';
import { toast } from './toasts';
import { advanced } from './prefs';

export type FieldErrors = Record<string, string>;

/** The translated text of an error; experts also see the hub's own detail. */
export function errorText(e: unknown): string {
  const err = e instanceof ApiError ? e : new ApiError(0, 'internal', String((e as Error)?.message ?? e));
  const text = tx(`err.${err.code}`, t('err.internal'));
  return advanced.peek() && err.message && err.code !== 'network' ? `${text} (${err.message})` : text;
}

export interface SaveOptions<T, A> {
  /** The value shown on the page: changed at once, then set to the hub's answer. */
  target?: Signal<T | undefined> | Signal<T | null>;
  /** The value to show while the change is sent. */
  optimistic?: (current: T) => T;
  /** Sends the change. */
  send: () => Promise<A>;
  /** Turns the answer into the new value; by default the answer itself is the value. */
  accept?: (answer: A, current: T) => T;
  /** Where field messages go. */
  errors?: Signal<FieldErrors>;
  /** A short message after success, e.g. "Saved". */
  success?: string;
  /** Offered as "Undo" with the success message. */
  undo?: () => unknown;
  /** A restart this change needs, from the answer. */
  restart?: (answer: A) => RestartNotice | null | undefined | false;
}

/** Saves a change. Answers the hub's answer, or undefined if it was refused. */
export async function save<T, A = T>(o: SaveOptions<T, A>): Promise<A | undefined> {
  const target = o.target as Signal<T | undefined> | undefined;
  const before = target?.peek();
  if (target && o.optimistic && before !== undefined && before !== null) target.value = o.optimistic(before);
  if (o.errors) o.errors.value = {};
  try {
    const answer = await o.send();
    if (target) {
      const cur = target.peek() as T;
      target.value = o.accept ? o.accept(answer, cur) : (answer as unknown as T);
    }
    const r = o.restart?.(answer);
    if (r) addRestartNotice(r);
    // A change that can be reversed always offers "Undo" (D38).
    if (o.success || o.undo) toast({ text: o.success ?? t('common.saved'), tone: 'ok', undo: o.undo });
    return answer;
  } catch (e) {
    if (target) target.value = before;
    const err = e instanceof ApiError ? e : undefined;
    const text = errorText(e);
    if (err?.field && o.errors) {
      o.errors.value = { [err.field]: text };
      toast({ text: t('save.failedField'), tone: 'danger' });
    } else {
      toast({ text: t('save.failed', { reason: text }), tone: 'danger' });
    }
    return undefined;
  }
}

/** Field messages for one form. */
export function useFieldErrors() {
  return useMemo(() => signal<FieldErrors>({}), []);
}

/**
 * An editable copy of a resource for a form with a Save button. While the
 * user has not changed anything, it follows the resource (live changes show).
 */
export function useDraft<T extends object>(source: T | undefined | null) {
  const [draft, setDraft] = useState<T | undefined>(source ?? undefined);
  const [dirty, setDirty] = useState(false);
  // Follow the source by its content, not its identity: pages pass a new object on every
  // render, and following identity re-rendered without end and threw away what was typed.
  const key = source == null ? '' : JSON.stringify(source);
  useEffect(() => {
    if (!dirty) setDraft(source ?? undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` stands for `source`
  }, [key, dirty]);
  return {
    draft,
    dirty,
    set<K extends keyof T>(key: K, value: T[K]) {
      setDraft((d) => ({ ...(d as T), [key]: value }));
      setDirty(true);
    },
    replace(next: T) {
      setDraft(next);
      setDirty(true);
    },
    reset() {
      setDraft(source ?? undefined);
      setDirty(false);
    },
    /** Saved: follow the resource again. */
    done() {
      setDirty(false);
    },
    /** Only the fields that differ from the resource. */
    changes(): Partial<T> {
      const out: Partial<T> = {};
      if (!draft) return out;
      for (const k of Object.keys(draft) as (keyof T)[]) {
        if (JSON.stringify(draft[k]) !== JSON.stringify(source?.[k])) out[k] = draft[k];
      }
      return out;
    },
  };
}
