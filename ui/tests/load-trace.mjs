// Timeline of a cold page load: when each request finishes and each event frame arrives.
//   node tests/load-trace.mjs [hub URL]
import { chromium } from '@playwright/test';

const hub = process.argv[2] ?? process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const browser = await chromium.launch();
const page = await browser.newPage();
const t0 = Date.now();
const log = (s) => console.log(String(Date.now() - t0).padStart(6), s);
page.on('request', (r) => log(`-> ${r.method()} ${r.url().replace(hub, '')}`));
page.on('requestfinished', (r) => log(`<- ${r.url().replace(hub, '')}`));
page.on('websocket', (ws) => {
  log(`ws open ${ws.url()}`);
  ws.on('framesent', (f) => log(`ws -> ${String(f.payload).slice(0, 120)}`));
  ws.on('framereceived', (f) => {
    const p = String(f.payload);
    const m = p.match(/"type":"(\w+)"(?:,"resource":"(\w+)")?/);
    log(`ws <- ${m ? m[1] + ' ' + (m[2] ?? '') : p.slice(0, 60)} (${p.length} B)`);
  });
});
await page.goto(hub + '/');
await page.getByRole('region', { name: 'Apps' }).getByRole('listitem').first().waitFor({ timeout: 30000 });
log('apps card filled');
await page.waitForTimeout(1500);
await browser.close();
