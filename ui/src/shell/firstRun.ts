// First-run setup is shown while the hub says it needs it
// (`System.setup_needed`: a new hub or a factory reset). Finishing or
// skipping tells the hub (POST /system/setup-done), so it does not come back
// on any browser.
import { computed, signal } from '@preact/signals';
import { api } from '../api/client';
import { errorText } from '../state/save';
import { auth } from '../state/session';
import { live, setLive } from '../state/store';
import { toast } from '../state/toasts';

/** Set when the user finishes or skips the setup in this tab. */
export const setupClosed = signal(false);

export const needsSetup = computed(() => auth.value === 'in' && !!live('system').value?.setup_needed && !setupClosed.value);

/** Ends the setup for this hub. The page moves on even if the hub cannot be told. */
export async function setupDone(): Promise<void> {
  setupClosed.value = true;
  try {
    await api.post('/system/setup-done');
    const sys = live('system').peek();
    if (sys) setLive('system', { ...sys, setup_needed: false });
  } catch (e) {
    toast({ text: errorText(e), tone: 'danger' });
  }
}
