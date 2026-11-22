// Ad-hoc probe of the phone layout: tap "More" and show the sheet.
import { chromium } from '@playwright/test';

const hub = process.argv[2];
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
const p = await ctx.newPage();
await p.goto(hub + '/apps');
await p.locator('h1').first().waitFor();
await p.waitForTimeout(1000);
try {
  await p.getByRole('button', { name: 'More' }).tap({ timeout: 5000 });
  console.log('tapped');
  await p.waitForTimeout(800);
  console.log(await p.getByRole('dialog').ariaSnapshot());
} catch (e) {
  console.log('ERR', e.message.split('\n').slice(0, 14).join(' | '));
}
await p.screenshot({ path: 'clickthrough-results/probe-phone.png' });
await browser.close();
