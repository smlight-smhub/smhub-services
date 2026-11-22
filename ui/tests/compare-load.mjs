// Loads a hub's web UI the same way whatever version it serves, for side-by-side comparisons.
//
//   node tests/compare-load.mjs URL LABEL [READY_TEXT] [--runs N] [--cpu X] [--hold S] [--film DIR]
//
// READY_TEXT: a regular expression for text that shows the dashboard has real data.
// --cpu X slows the browser's CPU X times (4 is about a mid-range phone).
// --hold S keeps the last page open S seconds and counts the requests it makes meanwhile.
// --film DIR saves screenshots 0.5, 1, 2, 3 and 5 s after navigation, each from an empty cache.
// Prints one JSON line per run.
import { chromium } from '@playwright/test';
import { mkdirSync } from 'node:fs';

const [url, label, readyText] = process.argv.slice(2).filter((a, i, all) => !a.startsWith('--') && !all[i - 1]?.startsWith('--'));
const opt = (name, dflt) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : dflt;
};
const runs = Number(opt('runs', 3));
const cpu = Number(opt('cpu', 1));
const hold = Number(opt('hold', 0));
const film = opt('film', '');
const origin = new URL(url).origin;
const browser = await chromium.launch();

async function fresh() {
  const context = await browser.newContext({ viewport: { width: 1366, height: 860 } });
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send('Performance.enable');
  if (cpu > 1) await cdp.send('Emulation.setCPUThrottlingRate', { rate: cpu });
  return { context, page, cdp };
}

async function run(n, keep) {
  const { context, page, cdp } = await fresh();
  const events = [];
  const reqs = [];
  page.on('request', (r) => events.push({ t: Date.now(), kind: 'start', url: r.url() }));
  page.on('requestfinished', async (r) => {
    const s = await r.sizes().catch(() => null);
    const tm = r.timing();
    reqs.push({ url: r.url(), bytes: s ? s.responseBodySize + s.responseHeadersSize : 0, t: Date.now(), ms: tm.responseEnd > 0 ? Math.round(tm.responseEnd - tm.requestStart) : null });
    events.push({ t: Date.now(), kind: 'end' });
  });
  page.on('requestfailed', (r) => {
    reqs.push({ url: r.url(), bytes: 0, t: Date.now(), failed: true });
    events.push({ t: Date.now(), kind: 'end' });
  });
  page.on('websocket', (w) => {
    events.push({ t: Date.now(), kind: 'ws' });
    w.on('framereceived', () => events.push({ t: Date.now(), kind: 'frame' }));
  });
  const t0 = Date.now();
  await page.goto(url, { waitUntil: 'load', timeout: 60000 });
  const tLoad = Date.now() - t0;
  let tReady = null;
  if (readyText) {
    await page.getByText(new RegExp(readyText, 'i')).first().waitFor({ timeout: 60000 });
    tReady = Date.now() - t0;
  }
  // quiet: the first moment after which nothing (request, response, socket frame) happens for 1.5 s
  let quiet = null;
  for (let waited = 0; waited < 30000 && quiet === null; waited += 250) {
    await page.waitForTimeout(250);
    const last = Math.max(...events.map((e) => e.t));
    if (Date.now() - last >= 1500) quiet = last - t0;
  }
  const m = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
  const firstRequests = reqs.filter((r) => r.t - t0 <= (quiet ?? 30000));
  const hosts = [...new Set(reqs.map((r) => new URL(r.url).host))];
  const out = {
    label, run: n, cpu,
    load_ms: tLoad, ready_ms: tReady, quiet_ms: quiet,
    requests: firstRequests.length,
    kb: Math.round(firstRequests.reduce((a, r) => a + r.bytes, 0) / 1024),
    failed: reqs.filter((r) => r.failed).length,
    other_hosts: hosts.filter((h) => h !== new URL(origin).host),
    script_ms: Math.round((m.ScriptDuration ?? 0) * 1000),
    task_ms: Math.round((m.TaskDuration ?? 0) * 1000),
    heap_kb: Math.round((m.JSHeapUsedSize ?? 0) / 1024),
    dom_nodes: m.Nodes,
    // the API requests of the first load, each with the time from sending it to the last byte
    api: firstRequests.filter((r) => /\/api\//.test(r.url)).map((r) => `${new URL(r.url).pathname} ${r.ms}`),
  };
  if (keep && hold > 0) {
    const n0 = reqs.length;
    const f0 = events.filter((e) => e.kind === 'frame').length;
    await page.waitForTimeout(hold * 1000);
    const extra = reqs.slice(n0);
    out.hold_s = hold;
    out.hold_requests = extra.length;
    out.hold_kb = Math.round(extra.reduce((a, r) => a + r.bytes, 0) / 1024);
    out.hold_ws_frames = events.filter((e) => e.kind === 'frame').length - f0;
    out.hold_paths = [...new Set(extra.map((r) => new URL(r.url).pathname))].slice(0, 12);
    const m2 = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((x) => [x.name, x.value]));
    out.hold_heap_kb = Math.round((m2.JSHeapUsedSize ?? 0) / 1024);
  }
  console.log(JSON.stringify(out));
  await context.close();
}

if (film) {
  mkdirSync(film, { recursive: true });
  for (const at of [500, 1000, 2000, 3000, 5000]) {
    const { context, page } = await fresh();
    const t0 = Date.now();
    page.goto(url, { timeout: 60000 }).catch(() => {});
    await page.waitForTimeout(at);
    await page.screenshot({ path: `${film}/${label}-${String(at).padStart(4, '0')}ms.jpg`, type: 'jpeg', quality: 70 });
    console.log(`frame ${at} ms (taken at ${Date.now() - t0} ms)`);
    await context.close();
  }
} else {
  for (let i = 1; i <= runs; i++) await run(i, i === runs);
}
await browser.close();
