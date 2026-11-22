// The one "restart needed" notice (D30): every setting that needs a restart
// adds an entry here, and the shell shows them in one consistent place.
import { signal } from '@preact/signals';

export interface RestartNotice {
  /** Same id = same notice; adding it again replaces it. */
  id: string;
  /** What needs restarting: the hub, one app, or the second core. */
  kind: 'hub' | 'app' | 'esphome';
  /** App name for kind "app". */
  target?: string;
  /** Translated text: what changed. */
  reason: string;
}

export const restartNotices = signal<RestartNotice[]>([]);

export function addRestartNotice(n: RestartNotice) {
  restartNotices.value = [...restartNotices.value.filter((x) => x.id !== n.id), n];
}

export function removeRestartNotice(id: string) {
  restartNotices.value = restartNotices.value.filter((x) => x.id !== id);
}

export function removeRestartNotices(kind: RestartNotice['kind'], target?: string) {
  restartNotices.value = restartNotices.value.filter((x) => !(x.kind === kind && (target === undefined || x.target === target)));
}
