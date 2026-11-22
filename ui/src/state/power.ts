// Restarting and powering off the hub, and noticing when it is back.
import { effect, signal } from '@preact/signals';
import { api } from '../api/client';
import { t } from '../text/i18n';
import { confirm } from './confirm';
import { removeRestartNotices } from './notices';
import { errorText } from './save';
import { connectedAt } from './store';
import { toast } from './toasts';

export type Power = 'on' | 'restarting' | 'off';
export const power = signal<Power>('on');
let requestedAt = 0;

export async function restartHub(ask = true): Promise<boolean> {
  if (ask && !(await confirm({ title: t('power.restartTitle'), body: t('power.restartBody'), confirm: t('power.restart') }))) return false;
  try {
    await api.post('/system/reboot');
    requestedAt = Date.now();
    power.value = 'restarting';
    removeRestartNotices('hub');
    return true;
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
    return false;
  }
}

export async function powerOffHub(): Promise<boolean> {
  if (!(await confirm({ title: t('power.offTitle'), body: t('power.offBody'), confirm: t('power.off'), tone: 'danger' }))) return false;
  try {
    await api.post('/system/shutdown');
    requestedAt = Date.now();
    power.value = 'off';
    return true;
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
    return false;
  }
}

// A new event connection some seconds after the restart means the hub is back.
effect(() => {
  const at = connectedAt.value;
  if (power.peek() === 'restarting' && at > requestedAt + 3000) {
    power.value = 'on';
    toast({ text: t('power.back'), tone: 'ok' });
  }
});
