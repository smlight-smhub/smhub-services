import { expect, type APIRequestContext, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';
import type { components } from '../src/api/schema';

export type Preferences = components['schemas']['Preferences'];
export type System = components['schemas']['System'];
export type Apps = components['schemas']['Apps'];

export async function getJson<T>(request: APIRequestContext, path: string): Promise<T> {
  const res = await request.get(`/api/v2${path}`);
  expect(res.ok(), `GET ${path} answered ${res.status()}`).toBeTruthy();
  return (await res.json()) as T;
}

export async function setPrefs(request: APIRequestContext, patch: Partial<Preferences>): Promise<Preferences> {
  const res = await request.patch('/api/v2/preferences', { data: patch });
  expect(res.ok(), `PATCH /preferences answered ${res.status()}: ${await res.text()}`).toBeTruthy();
  return (await res.json()) as Preferences;
}

/** Opens a page and waits until it has drawn its heading and settled. */
export async function open(page: Page, path: string) {
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(path);
  await expect(page.locator('h1').first()).toBeVisible();
  // Live resources arrive over the event connection: give them a moment.
  await page.waitForTimeout(1200);
  await expect(page.getByText('This page ran into a problem')).toHaveCount(0);
  expect(errors, `JavaScript errors on ${path}`).toEqual([]);
}

/** Runs axe for WCAG 2.2 A and AA. App web pages in iframes are not ours and are left out. */
export async function checkA11y(page: Page, label: string) {
  // Let entry animations finish: contrast is measured on the final colours.
  await page.waitForTimeout(500);
  const result = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
    .exclude('iframe')
    .analyze();
  const report = result.violations.map((v) => `${v.id} (${v.impact}): ${v.help}\n    ${v.nodes.slice(0, 5).map((n) => n.target.join(' ')).join('\n    ')}`);
  expect(report, `accessibility problems on ${label}`).toEqual([]);
}
