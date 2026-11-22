// Commands to the hub's own hardware (docs/peripherals.md). Each answers the
// whole `hardware` resource, which replaces the live copy at once; the event
// connection then keeps it current.
import { api } from '../api/client';
import type { Hardware, LightPatch, PlayResult } from '../api/types';
import { t, tx } from '../text/i18n';
import { errorText } from './save';
import { setLive } from './store';
import { toast } from './toasts';

async function run(fn: () => Promise<Hardware>): Promise<Hardware | undefined> {
  try {
    const r = await fn();
    setLive('hardware', r);
    return r;
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
    return undefined;
  }
}

/** What happened to a sound request, for a short message. */
export function playResultText(r: PlayResult): string | undefined {
  if (r.result === 'playing') return undefined;
  if (r.result === 'queued') return t('hw.queued', { position: r.position ?? 1 });
  return tx(`hw.dropped.${r.reason ?? 'full'}`, t('hw.dropped.full'));
}

export async function play(body: { sound?: string; rtttl?: string }): Promise<boolean> {
  try {
    const r = await api.post('/hardware/buzzer/play', { body });
    const text = playResultText(r);
    if (text) toast({ text, tone: r.result === 'dropped' ? 'warn' : 'info' });
    return r.result !== 'dropped';
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
    return false;
  }
}

/** Send a saved infrared code. */
export async function sendCode(code: { id: string; name: string }) {
  try {
    await api.post('/ir/codes/{id}/send', { path: { id: code.id } });
    toast({ text: t('ir.sent', { name: code.name }), tone: 'ok' });
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
  }
}

export const stopSound = () => run(() => api.post('/hardware/buzzer/stop', { body: { clear: true } }));
export const setLight = (body: LightPatch) => run(() => api.patch('/hardware/light', { body }));
export const setLeds = (body: { power?: boolean; blue?: boolean }) => run(() => api.patch('/hardware/leds', { body }));
export const findHub = (on: boolean) => run(() => api.post('/hardware/find', { body: { on } }));
