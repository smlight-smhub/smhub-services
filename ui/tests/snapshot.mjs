// Prints the accessibility tree of every page of a hub's UI, at the advanced level.
//   node tests/snapshot.mjs [hub URL] > snapshot.txt
// The level is put back to what it was.
import { chromium } from '@playwright/test';

const hub = process.argv[2] ?? process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const routes = ['/', '/apps', '/apps/zigbee2mqtt', '/apps/tailscale', '/open/zeesm', '/network', '/network/ethernet', '/network/wifi', '/network/vpn', '/network/lte', '/radios', '/updates', '/settings', '/settings/general', '/settings/accounts', '/settings/appearance', '/settings/regional','/settings/mqtt', '/settings/esphome', '/settings/backups', '/peripherals', '/peripherals/usb', '/settings/ssh', '/settings/serial-bridge', '/settings/system', '/tools/terminal', '/tools/files', '/tools/logs', '/tools/services'];

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1366, height: 860 } });
const prefs = await (await page.request.get(`${hub}/api/v2/preferences`)).json();
await page.request.patch(`${hub}/api/v2/preferences`, { data: { level: 'advanced' } });
try {
  for (const r of routes) {
    await page.goto(hub + r);
    await page.locator('h1').first().waitFor({ timeout: 15000 }).catch(() => {});
    await page.waitForTimeout(1500);
    console.log(`\n===== ${r}`);
    console.log(await page.locator('main').ariaSnapshot().catch((e) => `(no main: ${e.message})`));
  }
} finally {
  await page.request.patch(`${hub}/api/v2/preferences`, { data: { level: prefs.level } });
  await browser.close();
}
