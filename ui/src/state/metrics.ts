// CPU and memory history for the sparklines, kept while metrics arrive.
// The hub pushes metrics only while they are subscribed; nothing is polled.
import { effect, signal } from '@preact/signals';
import type { Metrics } from '../api/types';
import { live } from './store';

const HISTORY = 60;
export const cpuHistory = signal<number[]>([]);
export const memoryHistory = signal<number[]>([]);

let last: Metrics | undefined;
effect(() => {
  const m = live('metrics').value;
  if (!m || m === last) return;
  last = m;
  cpuHistory.value = [...cpuHistory.peek(), m.cpu_percent].slice(-HISTORY);
  memoryHistory.value = [...memoryHistory.peek(), m.memory.total ? (m.memory.used / m.memory.total) * 100 : 0].slice(-HISTORY);
});
