// The resource store (D30, architecture "How a page gets its data").
// Widgets name the live resources they show; the store subscribes once per
// resource over the one event connection, keeps one signal per resource that
// every widget shares, and unsubscribes a while after the last widget leaves.
import { signal, type Signal } from '@preact/signals';
import { useEffect } from 'preact/hooks';
import { EventConnection, type ConnectionState, type ServerMessage } from '../api/events';
import type { LiveName, LiveResources, Operation } from '../api/types';

/** Unsubscribe this long after the last user leaves, so quick navigation does not resubscribe. */
const UNSUB_AFTER = 10_000;
/** A hidden tab stops live updates after this long, to spare the hub's CPU. */
const PAUSE_HIDDEN_AFTER = 30_000;
/** Kept while the tab is hidden: they are small and change rarely. */
const KEEP_WHEN_HIDDEN = new Set<LiveName>(['operations', 'system']);

interface Entry<N extends LiveName = LiveName> {
  data: Signal<LiveResources[N] | undefined>;
  users: number;
  subscribed: boolean;
  releaseTimer?: ReturnType<typeof setTimeout>;
}

const entries = new Map<LiveName, Entry>();

/** State of the event connection, for the "reconnecting" notice. */
export const connection = signal<ConnectionState>('idle');
/** When the connection last opened (ms), to tell a reconnect from the first open. */
export const connectedAt = signal(0);

function entry<N extends LiveName>(name: N): Entry<N> {
  let e = entries.get(name) as Entry<N> | undefined;
  if (!e) {
    e = { data: signal<LiveResources[N] | undefined>(undefined), users: 0, subscribed: false };
    entries.set(name, e as unknown as Entry);
  }
  return e;
}

/** The shared signal of a live resource (does not subscribe by itself). */
export function live<N extends LiveName>(name: N): Signal<LiveResources[N] | undefined> {
  return entry(name).data;
}

/** Puts an answer from a command (e.g. POST /wifi/connect) into the store at once. */
export function setLive<N extends LiveName>(name: N, data: LiveResources[N]) {
  entry(name).data.value = data;
}

let paused = false;
let failedHandler: (() => void) | undefined;
const pending = new Set<LiveName>();
let flushQueued = false;

const conn = new EventConnection({
  open() {
    connectedAt.value = Date.now();
    subscribeAllActive();
  },
  message: onMessage,
  state(s) {
    connection.value = s;
    if (s !== 'open') for (const e of entries.values()) e.subscribed = false;
  },
  failed() {
    failedHandler?.();
  },
});

/** Called when the socket cannot open, e.g. to check whether the session ended. */
export function onConnectFailed(handler: () => void) {
  failedHandler = handler;
}

export function startEvents() {
  conn.start();
}

export function stopEvents() {
  conn.stop();
}

function queueSub(name: LiveName) {
  pending.add(name);
  if (flushQueued) return;
  flushQueued = true;
  // Every widget rendered in the same pass asks in one message.
  queueMicrotask(() => {
    flushQueued = false;
    if (paused || !conn.isOpen) return pending.clear(); // sent when the socket opens
    const names = [...pending].filter((n) => {
      const e = entry(n);
      return e.users > 0 && !e.subscribed;
    });
    pending.clear();
    if (!names.length) return;
    for (const n of names) entry(n).subscribed = true;
    conn.send({ op: 'sub', resources: names });
  });
}

function activeNames(): LiveName[] {
  return [...entries].filter(([, e]) => e.users > 0).map(([n]) => n);
}

function subscribeAllActive() {
  const names = activeNames().filter((n) => !paused || KEEP_WHEN_HIDDEN.has(n));
  for (const n of names) entry(n).subscribed = true;
  if (names.length) conn.send({ op: 'sub', resources: names });
}

function onMessage(m: ServerMessage) {
  if (m.type === 'state') {
    entry(m.resource).data.value = m.data as LiveResources[typeof m.resource];
  } else if (m.type === 'operation') {
    upsertOperation(m.data);
  } else if (m.type === 'reload') {
    // We fell behind: ask for every shown resource again.
    subscribeAllActive();
  }
}

/** Keeps the `operations` resource current from single operation events. */
export function upsertOperation(op: Operation) {
  const ops = entry('operations').data;
  const list = ops.peek()?.operations ?? [];
  const i = list.findIndex((o) => o.id === op.id);
  const next = i >= 0 ? list.map((o, j) => (j === i ? op : o)) : [op, ...list];
  ops.value = { operations: next };
}

/** Starts showing a resource; answers the function that stops. */
export function acquire(name: LiveName): () => void {
  const e = entry(name);
  e.users++;
  clearTimeout(e.releaseTimer);
  if (!e.subscribed) queueSub(name);
  let released = false;
  return () => {
    if (released) return;
    released = true;
    e.users = Math.max(0, e.users - 1);
    if (e.users > 0) return;
    e.releaseTimer = setTimeout(() => {
      if (e.users > 0 || !e.subscribed) return;
      e.subscribed = false;
      conn.send({ op: 'unsub', resources: [name] });
    }, UNSUB_AFTER);
  };
}

/** A live resource for a component: subscribes while it is shown. Undefined until the first state arrives. */
export function useLive<N extends LiveName>(name: N): LiveResources[N] | undefined {
  useEffect(() => acquire(name), [name]);
  return entry(name).data.value;
}

// A hidden tab stops live updates (except small ones) after a while and
// resumes with fresh state when shown again.
if (typeof document !== 'undefined') {
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) {
      hideTimer = setTimeout(() => {
        paused = true;
        const names = activeNames().filter((n) => !KEEP_WHEN_HIDDEN.has(n) && entry(n).subscribed);
        for (const n of names) entry(n).subscribed = false;
        if (names.length) conn.send({ op: 'unsub', resources: names });
      }, PAUSE_HIDDEN_AFTER);
    } else {
      clearTimeout(hideTimer);
      if (paused) {
        paused = false;
        const names = activeNames().filter((n) => !entry(n).subscribed);
        for (const n of names) entry(n).subscribed = true;
        if (names.length) conn.send({ op: 'sub', resources: names });
      }
      conn.kick();
    }
  });
  window.addEventListener('online', () => conn.kick());
}
