// nginx sends the UI's files without cache headers, so a browser may run an
// older UI from its cache after an update. The hub says which smhub-ui is
// installed (`System.ui_version`); a page built for another version reloads
// once to get it. A session flag makes sure this can never loop.
import { effect } from '@preact/signals';
import type { System } from '../api/types';
import { t } from '../text/i18n';
import { live } from './store';
import { toast } from './toasts';

/** The package version this build belongs to; absent in development, where nothing is checked. */
export const UI_VERSION = import.meta.env.VITE_UI_VERSION || undefined;
const KEY = 'smhub-reloaded-for';

function outdated(sys: System | null | undefined): string | undefined {
  const installed = sys?.ui_version;
  return UI_VERSION && installed && installed !== UI_VERSION ? installed : undefined;
}

/**
 * At start: reloads the page when the hub has another UI version installed.
 * Answers true when the page is reloading (render nothing more).
 */
export function reloadIfOutdated(sys: System | null | undefined): boolean {
  const installed = outdated(sys);
  if (!installed) return false;
  try {
    if (sessionStorage.getItem(KEY) === installed) return false; // already tried once for this version
    sessionStorage.setItem(KEY, installed);
  } catch {
    return false; // without the flag a reload could loop; carry on with this version
  }
  location.reload();
  return true;
}

// While the page is open: the UI package was updated (e.g. from the app
// store). Reloading under the user's hands could lose their input, so they
// are asked instead.
let offered: string | undefined;
effect(() => {
  const installed = outdated(live('system').value);
  if (!installed || installed === offered) return;
  offered = installed;
  toast({ text: t('version.updated'), tone: 'info', timeout: 0, action: { label: t('common.reload'), run: () => location.reload() } });
});
