// Ad-hoc probe: open a page, run a few steps, print the accessibility tree and failing requests.
//   node tests/probe.mjs <hub> <path> '<js body using page>'
import { chromium } from '@playwright/test';

const [hub, path, body] = process.argv.slice(2);
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1366, height: 860 } });
page.on('response', async (r) => {
  if (r.request().method() !== 'GET' || r.status() >= 400) console.log('>>', r.status(), r.request().method(), r.url().replace(hub, ''), (r.request().postData() ?? '').slice(0, 200));
});
await page.goto(hub + path);
await page.locator('h1').first().waitFor({ timeout: 20000 });
await page.waitForTimeout(1000);
await new Function('page', `return (async () => { ${body} })()`)(page);
await page.screenshot({ path: 'clickthrough-results/probe.png' });
console.log((await page.locator('main').ariaSnapshot()).slice(0, 3000));
await browser.close();
