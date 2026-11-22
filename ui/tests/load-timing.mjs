// Measures the web UI as a hub serves it: requests and bytes before the home page is usable,
// time to usable with an empty and a warm cache, and requests while a page sits open.
//   node tests/load-timing.mjs [hub URL] [seconds to hold the page open]
import { chromium } from '@playwright/test';

const hub = process.argv[2] ?? process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const hold = Number(process.argv[3] ?? 0);
const browser = await chromium.launch();

async function load(context, label) {
  const page = await context.newPage();
  const reqs = [];
  page.on('requestfinished', async (r) => {
    const s = await r.sizes().catch(() => null);
    reqs.push({ url: r.url().replace(hub, ''), bytes: s ? s.responseBodySize + s.responseHeadersSize : 0, t: Date.now() });
  });
  const ws = [];
  page.on('websocket', (w) => ws.push({ url: w.url().replace(hub.replace('http', 'ws'), ''), t: Date.now() }));
  const t0 = Date.now();
  await page.goto(hub + '/');
  // usable: the overall status and the apps card show real content
  await page.getByRole('heading', { name: /Everything is running|needs attention|is stopped/i }).waitFor({ timeout: 30000 });
  await page.getByRole('region', { name: 'Apps' }).getByRole('listitem').first().waitFor({ timeout: 30000 });
  const t1 = Date.now();
  const before = reqs.filter((r) => r.t <= t1);
  console.log(`${label}: usable after ${t1 - t0} ms; ${before.length} requests, ${(before.reduce((a, r) => a + r.bytes, 0) / 1024).toFixed(1)} KB transferred, ${ws.filter((w) => w.t <= t1).length} WebSocket`);
  for (const r of before) console.log(`   ${String(r.bytes).padStart(8)}  ${r.url}`);
  return { page, reqs, t1 };
}

const cold = await browser.newContext();
const first = await load(cold, 'empty cache');
await first.page.close();
const warm = await load(cold, 'warm cache');
if (hold > 0) {
  const n0 = warm.reqs.length;
  await warm.page.waitForTimeout(hold * 1000);
  const extra = warm.reqs.slice(n0).map((r) => r.url);
  console.log(`page open ${hold} s without changes: ${extra.length} more HTTP requests${extra.length ? ': ' + extra.join(', ') : ''}`);
  const perf = await warm.page.evaluate(() => ({ heap: performance.memory ? Math.round(performance.memory.usedJSHeapSize / 1024) : null }));
  console.log(`page JS heap: ${perf.heap} KB`);
}
await browser.close();
