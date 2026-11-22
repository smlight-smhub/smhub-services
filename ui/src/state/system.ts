// What the hub is (D10) and its clock. Clocks tick locally from the hub's
// `time`; nothing is polled.
import { computed, effect, signal } from '@preact/signals';
import { useEffect } from 'preact/hooks';
import type { Metrics } from '../api/types';
import { live } from './store';
import { publicSystem } from './session';

export const system = computed(() => live('system').value ?? publicSystem.value);
export const features = computed(() => system.value?.features);

/** Hub time minus browser time, in ms. */
const offset = signal(0);
/** Uptime in seconds at a known browser time. */
const uptimeBase = signal<{ s: number; at: number } | null>(null);

effect(() => {
  const s = live('system').value;
  if (!s) return;
  const t = Date.parse(s.time);
  if (!Number.isNaN(t)) offset.value = t - Date.now();
  uptimeBase.value = { s: s.uptime_s, at: Date.now() };
});
effect(() => {
  const m: Metrics | undefined = live('metrics').value;
  if (m) uptimeBase.value = { s: m.uptime_s, at: Date.now() };
});

const now = signal(Date.now());
let perSecond = 0;
let perMinute = 0;
let timer: ReturnType<typeof setTimeout> | undefined;

function schedule() {
  clearTimeout(timer);
  if (!perSecond && !perMinute) return;
  const t = Date.now();
  const wait = perSecond ? 1000 - (t % 1000) : 60_000 - (t % 60_000);
  timer = setTimeout(() => {
    now.value = Date.now();
    schedule();
  }, wait + 15);
}

/** The current browser time, re-rendering each second or each minute while shown. */
export function useNow(every: 'second' | 'minute' = 'minute'): number {
  useEffect(() => {
    if (every === 'second') perSecond++;
    else perMinute++;
    now.value = Date.now();
    schedule();
    return () => {
      if (every === 'second') perSecond--;
      else perMinute--;
      schedule();
    };
  }, [every]);
  return now.value;
}

/** The hub's time now (ms), from its last report. */
export function hubTime(browserNow: number): number {
  return browserNow + offset.value;
}

/** The hub's uptime now, in seconds. */
export function uptimeAt(browserNow: number): number | undefined {
  const b = uptimeBase.value;
  return b ? b.s + Math.max(0, browserNow - b.at) / 1000 : undefined;
}
