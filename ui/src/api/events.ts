// The one event connection to the hub (D9). Protocol: see the description in
// api/openapi.yaml. This module only keeps the socket alive; the store
// (state/store.ts) decides what to subscribe to.
import { wsUrl } from './client';
import type { LiveName, Operation } from './types';

export type ServerMessage =
  | { type: 'state'; resource: LiveName; data: unknown }
  | { type: 'operation'; data: Operation }
  | { type: 'reload' };

export type ClientMessage =
  | { op: 'sub'; resources: LiveName[] }
  | { op: 'unsub'; resources: LiveName[] }
  | { op: 'ping' };

export type ConnectionState = 'idle' | 'connecting' | 'open' | 'closed';

export interface EventHandlers {
  /** The socket is open: (re)subscribe what is shown. */
  open(): void;
  message(m: ServerMessage): void;
  state(s: ConnectionState): void;
  /** The socket closed before it opened (hub down, or not signed in). */
  failed(): void;
}

const PING_EVERY = 25_000;
const PONG_WITHIN = 10_000;
const BACKOFF_MIN = 1_000;
const BACKOFF_MAX = 15_000;

export class EventConnection {
  private ws: WebSocket | undefined;
  private attempt = 0;
  private retryTimer: ReturnType<typeof setTimeout> | undefined;
  private pingTimer: ReturnType<typeof setInterval> | undefined;
  private pongTimer: ReturnType<typeof setTimeout> | undefined;
  private running = false;

  constructor(private readonly h: EventHandlers) {}

  start() {
    if (this.running) return;
    this.running = true;
    this.connect();
  }

  stop() {
    this.running = false;
    clearTimeout(this.retryTimer);
    this.clearPing();
    const ws = this.ws;
    this.ws = undefined;
    ws?.close();
    this.h.state('idle');
  }

  /** Try again at once, e.g. when the browser is back online or the tab is shown. */
  kick() {
    if (!this.running || this.ws) return;
    clearTimeout(this.retryTimer);
    this.attempt = 0;
    this.connect();
  }

  get isOpen() {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  send(msg: ClientMessage): boolean {
    if (!this.isOpen) return false;
    this.ws!.send(JSON.stringify(msg));
    return true;
  }

  private connect() {
    this.h.state('connecting');
    let opened = false;
    let ws: WebSocket;
    try {
      ws = new WebSocket(wsUrl('/ws/v2/events'));
    } catch {
      this.scheduleRetry();
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      opened = true;
      this.attempt = 0;
      this.h.state('open');
      this.startPing();
      this.h.open();
    };
    ws.onmessage = (e) => {
      if (typeof e.data !== 'string') return;
      let m: { type?: string };
      try {
        m = JSON.parse(e.data) as { type?: string };
      } catch {
        return;
      }
      if (m.type === 'pong') {
        clearTimeout(this.pongTimer);
        return;
      }
      if (m.type === 'state' || m.type === 'operation' || m.type === 'reload') this.h.message(m as ServerMessage);
    };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = undefined;
      this.clearPing();
      if (!this.running) return;
      this.h.state('closed');
      if (!opened) this.h.failed();
      this.scheduleRetry();
    };
  }

  private scheduleRetry() {
    const delay = Math.min(BACKOFF_MAX, BACKOFF_MIN * 2 ** this.attempt++);
    this.retryTimer = setTimeout(() => this.connect(), delay);
  }

  private startPing() {
    this.clearPing();
    this.pingTimer = setInterval(() => {
      if (!this.send({ op: 'ping' })) return;
      clearTimeout(this.pongTimer);
      // No answer: the connection is dead even if the browser has not noticed.
      this.pongTimer = setTimeout(() => this.ws?.close(), PONG_WITHIN);
    }, PING_EVERY);
  }

  private clearPing() {
    clearInterval(this.pingTimer);
    clearTimeout(this.pongTimer);
  }
}
