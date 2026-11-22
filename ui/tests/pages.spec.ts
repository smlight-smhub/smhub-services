// Every page at both UI levels, on the phone and desktop projects, with an
// accessibility check on each. Nothing here restarts the hub, changes its
// network or logs Tailscale out; preferences changed by the tests are put back.
//
// A hub in its first-run state (System.setup_needed) shows the setup instead
// of pages, so the page tests are skipped then. The setup test runs only with
// SMHUB_TEST_SETUP=1, because finishing the setup clears the hub's flag.
import { expect, test } from '@playwright/test';
import { checkA11y, getJson, open, setPrefs, type Apps, type Preferences, type System } from './helpers';

interface Route {
  path: string;
  advanced?: boolean;
  when?: (s: System) => boolean;
}

const ROUTES: Route[] = [
  { path: '/' },
  { path: '/apps' },
  { path: '/network' },
  { path: '/network/ethernet' },
  { path: '/network/wifi', when: (s) => s.features.wifi },
  { path: '/network/vpn' },
  { path: '/network/lte', when: (s) => s.features.lte },
  { path: '/radios', when: (s) => s.radios.length > 0 },
  { path: '/updates' },
  { path: '/settings' },
  { path: '/settings/general' },
  { path: '/settings/accounts' },
  { path: '/settings/appearance' },
  { path: '/settings/regional' },
  { path: '/settings/mqtt' },
  { path: '/settings/esphome', when: (s) => s.features.esphome },
  { path: '/peripherals' },
  { path: '/peripherals/buzzer', when: (s) => s.features.buzzer },
  { path: '/peripherals/lights' },
  { path: '/peripherals/infrared', when: (s) => s.features.ir },
  { path: '/peripherals/buttons' },
  { path: '/peripherals/ups', when: (s) => s.family === 'smhub' },
  { path: '/peripherals/usb', advanced: true },
  { path: '/settings/backups' },
  { path: '/settings/system' },
  { path: '/settings/ssh', advanced: true },
  { path: '/settings/serial-bridge', advanced: true },
  { path: '/tools/terminal', advanced: true },
  { path: '/tools/files', advanced: true },
  { path: '/tools/logs', advanced: true },
  { path: '/tools/services', advanced: true },
];

let original: Preferences;
let system: System;
let apps: Apps;

test.beforeAll(async ({ request }) => {
  original = await getJson<Preferences>(request, '/preferences');
  system = await getJson<System>(request, '/system');
  apps = await getJson<Apps>(request, '/apps');
});

/** Pages are only reachable once the hub's first-run setup is done. */
const needsPages = () => test.skip(system.setup_needed, 'the hub shows its first-run setup (System.setup_needed)');

test.afterAll(async ({ request }) => {
  if (original) await setPrefs(request, original);
});

for (const level of ['simple', 'advanced'] as const) {
  test.describe(`${level} level`, () => {
    test.beforeAll(async ({ request }) => {
      await setPrefs(request, { level, language: 'en', theme: 'light' });
    });
    test.beforeEach(needsPages);

    for (const r of ROUTES) {
      test(`${r.path}`, async ({ page }) => {
        test.skip(!!r.when && !r.when(system), 'this hub does not have the feature');
        await open(page, r.path);
        if (r.advanced && level === 'simple') await expect(page.getByText('This is in advanced mode')).toBeVisible();
        await checkA11y(page, `${r.path} (${level})`);
      });
    }

    test('app pages', async ({ page }) => {
      const installed = apps.apps.filter((a) => a.installed && !a.system);
      test.skip(!installed.length, 'no apps installed');
      const app = installed.find((a) => a.is_service) ?? installed[0]!;
      for (const tab of ['', '?tab=settings', '?tab=log', '?tab=changelog']) {
        await open(page, `/apps/${app.name}${tab}`);
        await checkA11y(page, `/apps/${app.name}${tab} (${level})`);
      }
    });

    test('an app inside the hub', async ({ page }) => {
      const app = apps.apps.find((a) => a.installed && a.web);
      test.skip(!app, 'no installed app has a web page');
      await open(page, `/open/${app!.name}`);
      await checkA11y(page, `/open/${app!.name} (${level})`);
    });

    test('dialogs', async ({ page }) => {
      await open(page, '/settings/accounts');
      await checkA11y(page, `accounts (${level})`);
      await open(page, '/settings/general');
      await page.getByRole('button', { name: /^Time zone/ }).click();
      await expect(page.getByRole('dialog')).toBeVisible();
      await checkA11y(page, `time zone dialog (${level})`);
    });
  });
}

test.describe('shell', () => {
  test.beforeAll(async ({ request }) => {
    await setPrefs(request, { level: 'advanced', language: 'en', theme: 'light' });
  });
  test.beforeEach(({}, info) => {
    if (info.title !== 'first-run setup') needsPages();
  });

  test('old /#/ addresses are redirected', async ({ page }) => {
    await page.goto('/#/settings/ethernet');
    await expect(page).toHaveURL(/\/network\/ethernet$/);
    await page.goto('/#/console');
    await expect(page).toHaveURL(/\/tools\/terminal$/);
  });

  test('command palette jumps to a page', async ({ page, isMobile }) => {
    test.skip(isMobile, 'keyboard shortcut');
    await open(page, '/');
    await page.keyboard.press('Control+k');
    const box = page.getByRole('combobox');
    await expect(box).toBeVisible();
    await checkA11y(page, 'command palette');
    await box.fill('ethernet');
    await page.keyboard.press('Enter');
    await expect(page).toHaveURL(/\/network\/ethernet/);
  });

  test('phone navigation and the More sheet', async ({ page, isMobile }) => {
    test.skip(!isMobile, 'phone layout');
    await open(page, '/');
    await page.getByRole('button', { name: 'More' }).click();
    await expect(page.getByRole('dialog')).toBeVisible();
    await checkA11y(page, 'more sheet');
  });

  test('a setting saved from the page, and undone', async ({ page, request }) => {
    await setPrefs(request, { theme: 'light' });
    await open(page, '/settings/appearance');
    const saved = page.waitForResponse((r) => r.url().endsWith('/api/v2/preferences') && r.request().method() === 'PATCH');
    await page.locator('#theme').getByText('Dark', { exact: true }).click();
    expect((await saved).status()).toBe(200);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('button', { name: 'Undo' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await expect.poll(async () => (await getJson<Preferences>(request, '/preferences')).theme).toBe('light');
  });

  test('dark theme', async ({ page, request }) => {
    await setPrefs(request, { theme: 'dark' });
    for (const p of ['/', '/apps', '/settings/appearance']) {
      await open(page, p);
      await checkA11y(page, `${p} (dark)`);
    }
  });

  test('Ukrainian', async ({ page, request }) => {
    await setPrefs(request, { language: 'uk', theme: 'light' });
    for (const p of ['/', '/settings', '/network']) {
      await open(page, p);
      await expect(page.locator('html')).toHaveAttribute('lang', 'uk');
      await checkA11y(page, `${p} (uk)`);
    }
  });

  test('first-run setup', async ({ page, request }) => {
    test.skip(!process.env.SMHUB_TEST_SETUP, 'set SMHUB_TEST_SETUP=1 to test the setup; finishing it clears the hub\'s flag');
    test.skip(!system.setup_needed, 'the hub is not in its first-run state (System.setup_needed is false)');
    await setPrefs(request, { language: 'en' });
    await page.goto('/');
    await expect(page.getByRole('heading', { level: 1 })).toContainText('Welcome');
    await checkA11y(page, 'setup: welcome');
    await page.getByRole('button', { name: 'Let’s start' }).click();
    await checkA11y(page, 'setup: language');
    await page.getByRole('button', { name: 'Skip setup' }).click();
    await expect(page.locator('#hero-title')).toBeVisible();
    // Skipping tells the hub (POST /system/setup-done), so the setup does not come back.
    await expect.poll(async () => (await getJson<System>(request, '/system')).setup_needed).toBe(false);
  });
});
