// UI tests in a real browser against the real daemon (D46): no mock backend.
//
//   SMHUB_URL      the daemon (API and sockets): default http://127.0.0.1:8000,
//                  smhubd on this machine; for a hub, http://<hub> (its nginx).
//                  The tests build the UI and serve it with `vite preview`,
//                  which forwards /api and /ws there.
//   SMHUB_UI_URL   optional: a hub whose nginx already serves this UI; the
//                  tests then open it directly and build nothing.
//   SMHUB_USER, SMHUB_PASSWORD   an account, if the hub asks for sign-in.
import { defineConfig, devices } from '@playwright/test';

const hub = process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const ui = process.env.SMHUB_UI_URL;
const PORT = 4173;

export default defineConfig({
  testDir: 'tests',
  timeout: 60_000,
  expect: { timeout: 15_000 },
  // One hub, one set of preferences: the tests change them, so they run in order.
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list'], ['html', { open: 'never' }]],
  globalSetup: './tests/global-setup.ts',
  use: {
    baseURL: ui ?? `http://localhost:${PORT}`,
    storageState: 'test-results/.auth.json',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  webServer: ui
    ? undefined
    : {
        command: `npm run build && npx vite preview --port ${PORT} --strictPort`,
        url: `http://localhost:${PORT}`,
        reuseExistingServer: true,
        timeout: 240_000,
        env: { SMHUB_URL: hub },
      },
  projects: [
    { name: 'phone', use: { ...devices['Pixel 7'] } },
    { name: 'desktop', use: { ...devices['Desktop Chrome'], viewport: { width: 1366, height: 860 } } },
  ],
});
