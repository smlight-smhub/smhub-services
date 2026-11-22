// Old addresses (`/#/settings/wifi` and friends) keep working (D34): they are
// rewritten to the new paths before the router starts.

const MAP: Record<string, string> = {
  '': '/',
  apps: '/apps',
  console: '/tools/terminal',
  'file-manager': '/tools/files',
  settings: '/settings',
  'settings/general': '/settings/general',
  'settings/authorization': '/settings/accounts',
  'settings/backups': '/settings/backups',
  'settings/ethernet': '/network/ethernet',
  'settings/wifi': '/network/wifi',
  'settings/lte': '/network/lte',
  'settings/usb': '/peripherals/usb',
  'settings/mqtt': '/settings/mqtt',
  'settings/radios': '/radios',
  'settings/radio': '/radios',
  'settings/updates': '/updates',
  'settings/Updates': '/updates',
  'settings/esphome': '/settings/esphome',
  'settings/ser2net': '/settings/serial-bridge',
  'settings/wireguard': '/network/vpn',
  'settings/apps': '/apps',
};

const APP_NAME = /^[a-z0-9][a-z0-9+._-]*$/;

/**
 * The new address for an old one ("settings/wifi", "apps/zigbee2mqtt?tab=logs").
 * A single unknown segment was an app's embedded page in the old UI.
 */
export function legacyTarget(old: string, allowAppFrame = true): string | undefined {
  const [rawPath = '', query = ''] = old.replace(/^\/+/, '').split('?');
  const path = rawPath.replace(/\/+$/, '');
  if (path in MAP) return MAP[path];
  const app = /^apps\/([^/]+)$/.exec(path);
  if (app) {
    const tab = new URLSearchParams(query).get('tab');
    return `/apps/${app[1]}${tab === 'logs' ? '?tab=log' : ''}`;
  }
  if (allowAppFrame && APP_NAME.test(path)) return `/open/${path}`;
  return undefined;
}

/** Rewrites a `#/…` address in place, before anything renders. */
export function redirectLegacyHash() {
  const h = location.hash;
  if (!h.startsWith('#/') && h !== '#') return;
  const target = legacyTarget(h.slice(1)) ?? '/';
  history.replaceState(null, '', target);
}
