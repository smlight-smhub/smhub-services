// Screenshots of a hub's UI for the README (docs/screenshots/), in a real Chromium.
//   node tests/screenshots.mjs [hub URL]
//   SCREENSHOT_HIDE_APPS=name1,name2   apps left out of the pictures
// It switches the level, language and theme it needs and puts them back. The
// pictures are public: before each one, the hub's own addresses on the page
// (IPv4, IPv6, MAC) are replaced with neutral example values.
import { mkdirSync } from 'node:fs';
import { chromium } from '@playwright/test';

const hub = process.argv[2] ?? process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const hideApps = (process.env.SCREENSHOT_HIDE_APPS ?? '').split(',').map((a) => a.trim().toLowerCase()).filter(Boolean);
const out = new URL('../../docs/screenshots/', import.meta.url);
mkdirSync(out, { recursive: true });

// Runs in the page: neutral addresses, and the listed apps' rows removed.
function anonymize(hide) {
  const swaps = [
    [/\b100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.\d{1,3}\.\d{1,3}\b/g, '100.64.0.10'],
    [/\b(?:192\.168|10\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01]))\.\d{1,3}\.\d{1,3}\b/g, '192.168.1.20'],
    [/\b(?:[0-9a-f]{2}:){5}[0-9a-f]{2}\b/gi, '02:00:00:00:00:01'],
    [/\bfe80::[0-9a-f:]+/gi, 'fe80::1'],
  ];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  const texts = [];
  for (let n = walker.nextNode(); n; n = walker.nextNode()) texts.push(n);
  const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  for (const t of texts) {
    let v = t.nodeValue;
    for (const [rx, to] of swaps) v = v.replace(rx, to);
    // a radio held by a left-out app
    for (const name of hide) v = v.replace(new RegExp(`Used by ${esc(name)}\\b`, 'gi'), 'Used by another program');
    if (v !== t.nodeValue) t.nodeValue = v;
    if (hide.includes(t.nodeValue.trim().toLowerCase())) {
      const el = t.parentElement;
      (el?.closest('li') ?? el?.closest('a'))?.remove();
    }
  }
}

const SHOTS = [
  { file: 'home.jpg', path: '/', theme: 'light' },
  { file: 'apps.jpg', path: '/apps', theme: 'light' },
  { file: 'peripherals.jpg', path: '/peripherals', theme: 'light' },
  { file: 'lights-dark.jpg', path: '/peripherals/lights', theme: 'dark' },
  { file: 'buttons.jpg', path: '/peripherals/buttons', theme: 'light' },
  { file: 'infrared.jpg', path: '/peripherals/infrared', theme: 'light' },
  { file: 'phone-home-dark.jpg', path: '/', theme: 'dark', phone: true },
];

const browser = await chromium.launch();
const api = await browser.newContext();
const prefs = await (await api.request.get(`${hub}/api/v2/preferences`)).json();
try {
  for (const s of SHOTS) {
    await api.request.patch(`${hub}/api/v2/preferences`, { data: { level: 'advanced', language: 'en', theme: s.theme } });
    const ctx = await browser.newContext(
      s.phone ? { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true } : { viewport: { width: 1366, height: 860 } },
    );
    // the "anyone on your network can change this hub" notice, as if the reader had closed it
    await ctx.addInitScript(() => sessionStorage.setItem('smhub.openNoticeDismissed', '1'));
    const page = await ctx.newPage();
    await page.goto(hub + s.path);
    await page.locator('h1').first().waitFor({ timeout: 15000 });
    // live data and images settle
    await page.waitForTimeout(3000);
    await page.evaluate(anonymize, hideApps);
    await page.screenshot({ path: new URL(s.file, out).pathname.replace(/^\/([A-Za-z]:)/, '$1'), type: 'jpeg', quality: 82 });
    console.log(`${s.file}  ${s.path}`);
    await ctx.close();
  }
} finally {
  await api.request.patch(`${hub}/api/v2/preferences`, { data: { level: prefs.level, language: prefs.language, theme: prefs.theme } });
  await browser.close();
}
