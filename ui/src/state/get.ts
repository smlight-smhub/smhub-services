// Resources that are not on the event connection are read with one GET per
// page view and kept here (D30). Widgets of the same page share the request;
// a page opened again shows the kept value at once while it is read again.
import { signal, type Signal } from '@preact/signals';
import { useEffect } from 'preact/hooks';
import { api, buildUrl, type Answer, type CallOptions, type PathsWith, ApiError } from '../api/client';
import type { Paths } from '../api/types';

/** Increased by the shell when the user opens another page. */
export const pageView = signal(0);

interface Cached<T> {
  data: Signal<T | undefined>;
  error: Signal<ApiError | undefined>;
  loading: Signal<boolean>;
  view: number;
  inflight?: Promise<void>;
}

const cache = new Map<string, Cached<unknown>>();
const MAX_ENTRIES = 64;

function cached<T>(key: string): Cached<T> {
  let c = cache.get(key) as Cached<T> | undefined;
  if (!c) {
    if (cache.size >= MAX_ENTRIES) {
      // Drop the oldest entry that is not loading; the cache stays bounded.
      for (const [k, v] of cache) {
        if (!v.inflight) {
          cache.delete(k);
          break;
        }
      }
    }
    c = { data: signal<T | undefined>(undefined), error: signal(undefined), loading: signal(false), view: -1 };
    cache.set(key, c as Cached<unknown>);
  }
  return c;
}

function load<T>(c: Cached<T>, fetcher: () => Promise<T>): Promise<void> {
  c.view = pageView.peek();
  c.loading.value = true;
  const p = fetcher()
    .then((d) => {
      c.data.value = d;
      c.error.value = undefined;
    })
    .catch((e: unknown) => {
      c.error.value = e instanceof ApiError ? e : new ApiError(0, 'internal', String(e));
    })
    .finally(() => {
      c.loading.value = false;
      if (c.inflight === p) c.inflight = undefined;
    });
  c.inflight = p;
  return p;
}

export interface GetResult<T> {
  data: T | undefined;
  error: ApiError | undefined;
  loading: boolean;
  /** Read it again now. */
  reload: () => Promise<void>;
  /** The shared signal, for the save path. */
  sig: Signal<T | undefined>;
}

type GetOpts<P extends PathsWith<'get'>> = CallOptions<NonNullable<Paths[P]['get']>> & { skip?: boolean };

/** Reads a GET resource once per page view. */
export function useGet<P extends PathsWith<'get'>>(path: P, opts?: GetOpts<P>): GetResult<Answer<P, 'get'>> {
  type T = Answer<P, 'get'>;
  const o = (opts ?? {}) as { path?: Record<string, unknown>; query?: Record<string, unknown>; skip?: boolean };
  const key = buildUrl(path, o.path, o.query);
  const c = cached<T>(key);
  const fetcher = () => api.get(path, opts as CallOptions<NonNullable<Paths[P]['get']>>) as Promise<T>;
  useEffect(() => {
    if (o.skip) return;
    const fresh = c.view === pageView.peek() && (c.inflight || c.data.peek() !== undefined);
    if (!fresh) void load(c, fetcher);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, o.skip]);
  return {
    data: c.data.value,
    error: c.error.value,
    loading: c.loading.value,
    reload: () => load(c, fetcher),
    sig: c.data,
  };
}

/** The kept value of a GET, outside components (e.g. to update it after a command). */
export function getCached<P extends PathsWith<'get'>>(path: P, params?: Record<string, unknown>): Signal<Answer<P, 'get'> | undefined> {
  return cached<Answer<P, 'get'>>(buildUrl(path, params)).data;
}

/** Forgets every kept value (after signing out). */
export function clearGets() {
  cache.clear();
}
