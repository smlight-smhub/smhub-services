// Click-through of the actions the page tests leave out, on a hub that serves this UI itself.
// Real Chromium, real clicks and typing; every change is put back.
//
//   node tests/clickthrough.mjs [hub URL] [step,step,...]
//
// Screenshots go to test-results/clickthrough/. Prints PASS/FAIL per check.
import { chromium } from '@playwright/test';
import { mkdirSync, writeFileSync, statSync } from 'node:fs';
import { createHash, randomBytes } from 'node:crypto';

const hub = process.argv[2] ?? process.env.SMHUB_URL ?? 'http://127.0.0.1:8000';
const only = process.argv[3]?.split(',');
const shots = 'clickthrough-results';
mkdirSync(shots, { recursive: true });

const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${detail ? `  (${detail})` : ''}`);
}

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1366, height: 860 }, acceptDownloads: true });
const page = await context.newPage();
const consoleErrors = [];
page.on('pageerror', (e) => consoleErrors.push(e.message));
page.on('console', (m) => { if (m.type() === 'error') consoleErrors.push(m.text()); });
const badResponses = [];
page.on('response', (r) => { if (r.status() >= 400) badResponses.push(`${r.status()} ${r.request().method()} ${r.url().replace(hub, '')}`); });

const api = {
  get: async (p) => (await page.request.get(`${hub}/api/v2${p}`)).json(),
  patch: async (p, data) => (await page.request.patch(`${hub}/api/v2${p}`, { data, headers: { Origin: hub } })).json(),
};

async function open(path) {
  await page.goto(hub + path);
  await page.locator('h1').first().waitFor({ timeout: 20000 });
  await page.waitForTimeout(800);
}
async function shot(name) {
  await page.screenshot({ path: `${shots}/${name}.png`, fullPage: false });
}
/** Waits for an operation of a kind that was not in `before` (a set of ids) to finish. */
async function waitOp(kind, before, limitS = 600) {
  for (let i = 0; i < limitS / 2; i++) {
    const o = (await api.get('/operations')).operations.find((x) => x.kind === kind && !before.has(x.id));
    if (o && o.state !== 'running') return o;
    await page.waitForTimeout(2000);
  }
  return null;
}
/** The ids of the operations the hub knows now. */
const now = async () => new Set((await api.get('/operations')).operations.map((o) => o.id));
/** Clicks the confirm dialog's button with this name, if a dialog is open. */
async function confirmDialog(name) {
  const dlg = page.getByRole('dialog');
  await dlg.waitFor({ timeout: 5000 });
  await dlg.getByRole('button', { name, exact: true }).click();
}
async function toastSeen(re) {
  return page.getByText(re).first().waitFor({ timeout: 10000 }).then(() => true, () => false);
}

const steps = {
  async appsRefresh() {
    await open('/apps');
    const t0 = await now();
    await page.getByRole('button', { name: 'Check for updates' }).click();
    const op = await waitOp('apps_refresh', t0, 300);
    check('apps: check for updates runs opkg update', op?.state === 'succeeded', op ? `${op.state} ${op.error ?? ''}` : 'no operation');
    await shot('apps-refreshed');
  },

  async appInstallRemove() {
    await open('/apps');
    await page.getByRole('tab', { name: /^Available/ }).click();
    await page.getByRole('link', { name: /^Matterbridge Matterbridge|^Matterbridge [A-Z]/ }).first().click();
    await page.locator('h1').filter({ hasText: 'Matterbridge' }).waitFor();
    await shot('app-matterbridge-before');
    const t0 = await now();
    await page.getByRole('button', { name: 'Install', exact: true }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await confirmDialog('Install');
    const op = await waitOp('app_install', t0, 900);
    check('app: install from its page', op?.state === 'succeeded', op ? `${op.state} ${op.error ?? ''}` : 'no operation');
    const t = Date.now();
    await page.getByRole('button', { name: 'Remove', exact: true }).waitFor({ timeout: 60000 });
    check('app: page shows the installed app', true, `${Date.now() - t} ms after the install finished`);
    const stopBtn = page.getByRole('button', { name: 'Stop', exact: true });
    const startBtn = page.getByRole('button', { name: 'Start', exact: true });
    await shot('app-matterbridge-installed');
    if (await stopBtn.isVisible().catch(() => false)) {
      await stopBtn.click();
      await startBtn.waitFor({ timeout: 60000 });
      check('app: stop', true);
      await startBtn.click();
      await stopBtn.waitFor({ timeout: 60000 });
      check('app: start', true);
    } else if (await startBtn.isVisible().catch(() => false)) {
      await startBtn.click();
      await stopBtn.waitFor({ timeout: 60000 });
      check('app: start', true);
      await stopBtn.click();
      await startBtn.waitFor({ timeout: 60000 });
      check('app: stop', true);
    } else check('app: start/stop buttons shown', false);
    const t1 = await now();
    await page.getByRole('button', { name: 'Remove', exact: true }).click();
    await confirmDialog('Remove');
    const op2 = await waitOp('app_remove', t1, 600);
    check('app: remove from its page', op2?.state === 'succeeded', op2 ? `${op2.state} ${op2.error ?? ''}` : 'no operation');
    await shot('app-matterbridge-removed');
  },

  async appOptionsAndSettings() {
    await open('/apps/zigbee2mqtt');
    const wd = page.getByRole('switch', { name: /^Restart if it stops/ });
    await wd.click();
    await page.waitForTimeout(1500);
    let d = await api.get('/apps/zigbee2mqtt');
    check('app: watchdog switch on', d.watchdog === true);
    await wd.click();
    await page.waitForTimeout(1500);
    d = await api.get('/apps/zigbee2mqtt');
    check('app: watchdog switch off', d.watchdog === false);
    const auto = page.getByRole('switch', { name: /^Start with the hub/ });
    const before = d.autostart;
    await auto.click();
    await page.waitForTimeout(2000);
    d = await api.get('/apps/zigbee2mqtt');
    check('app: start-with-hub switch', d.autostart === !before);
    await auto.click();
    await page.waitForTimeout(2000);
    d = await api.get('/apps/zigbee2mqtt');
    check('app: start-with-hub switch back', d.autostart === before);
    // settings tab
    await page.getByRole('tab', { name: 'Settings' }).click();
    await page.waitForTimeout(800);
    writeFileSync(`${shots}/z2m-settings.aria.txt`, await page.locator('main').ariaSnapshot());
    await shot('app-z2m-settings');
    const ch = page.getByLabel(/channel/i).first();
    if (await ch.count()) {
      const old = String(d.settings.zigbee_channel);
      await ch.fill(old === '20' ? '25' : '20');
      await page.getByRole('button', { name: /^Save/ }).first().click();
      await page.waitForTimeout(2500);
      d = await api.get('/apps/zigbee2mqtt');
      check('app: setting saved from the form', String(d.settings.zigbee_channel) !== old, `channel ${d.settings.zigbee_channel}`);
      await ch.fill(old);
      await page.getByRole('button', { name: /^Save/ }).first().click();
      await page.waitForTimeout(2500);
      d = await api.get('/apps/zigbee2mqtt');
      check('app: setting put back', String(d.settings.zigbee_channel) === old);
    } else check('app: channel field found', false);
    await page.getByRole('tab', { name: 'Log' }).click();
    await page.waitForTimeout(1500);
    await shot('app-z2m-log');
    check('app: log tab', await page.getByRole('log').count() > 0 || (await page.locator('main').innerText()).includes('log'));
    await page.getByRole('tab', { name: /What.s new/ }).click();
    await page.waitForTimeout(1500);
    check('app: changelog tab', (await page.locator('[role=tabpanel]').innerText()).length > 20);
    await shot('app-z2m-changelog');
  },

  async wifi() {
    await open('/network/wifi');
    await page.getByRole('button', { name: 'Scan', exact: true }).click();
    await page.waitForTimeout(6000);
    const nets = await page.getByRole('button', { name: 'Connect', exact: true }).count();
    check('wifi: scan lists networks', nets > 0, `${nets} networks`);
    await shot('wifi-scanned');
    // a hidden network that does not exist: nothing is sent to anybody's access point
    await page.getByRole('button', { name: 'Hidden network' }).click();
    const dlg = page.getByRole('dialog');
    await dlg.getByLabel('Network name').fill('smhub-test-does-not-exist');
    await dlg.getByLabel('Password').fill('not-a-real-password');
    const answer = page.waitForResponse((r) => r.url().endsWith('/api/v2/wifi/connect'), { timeout: 120000 });
    await dlg.getByRole('button', { name: 'Connect', exact: true }).click();
    const res = await answer;
    await page.waitForTimeout(1000);
    await shot('wifi-hidden-failed');
    const shown = await page.getByText('The hub cannot find this network').first().isVisible().catch(() => false);
    const w = await api.get('/wifi');
    check('wifi: joining a network that is not there says so', res.status() === 422 && shown && !(w.current && w.current.ssid), `${res.status()}, message shown: ${shown}`);
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.keyboard.press('Escape');
    const saved = (w.saved ?? []).map((s) => s.name ?? s).filter((n) => String(n).includes('smhub-test'));
    check('wifi: no profile left behind', saved.length === 0, JSON.stringify(saved));
  },

  async vpn() {
    const key = () => Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString('base64');
    await open('/network/vpn');
    await page.getByLabel('Private key').fill(key());
    await page.getByLabel('Addresses in the tunnel 1').fill('10.99.0.2/32');
    await page.getByLabel('Server’s public key').fill(key());
    await page.getByLabel('Server address').fill('vpn.example.com:51820');
    // only a test network goes through the tunnel: never the default route
    await page.getByLabel('Send through the tunnel 1').fill('10.99.0.0/24');
    await shot('vpn-filled');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(4000);
    let v = await api.get('/vpn');
    check('vpn: WireGuard saved', JSON.stringify(v).includes('10.99.0.2'), JSON.stringify(v).slice(0, 160));
    await shot('vpn-saved');
    const routes = await api.get('/network');
    check('vpn: default route untouched', JSON.stringify(routes).includes('192.168.1.1'));
    await page.getByRole('button', { name: 'Remove VPN' }).click();
    await confirmDialog('Remove VPN');
    await page.waitForTimeout(3000);
    v = await api.get('/vpn');
    check('vpn: removed', !JSON.stringify(v).includes('10.99.0.2'), JSON.stringify(v).slice(0, 120));
    await shot('vpn-removed');
  },

  async lte() {
    await open('/network/lte');
    await page.getByRole('button', { name: 'Refresh', exact: true }).click();
    await page.waitForTimeout(3000);
    check('lte: refresh', true);
    await page.getByRole('button', { name: 'Power on', exact: true }).click();
    await page.waitForTimeout(6000);
    const l = await api.get('/lte');
    await shot('lte-power-on');
    check('lte: power on pulse sent', true, JSON.stringify(l).slice(0, 160));
  },

  async radioFlash() {
    await open('/radios');
    await page.getByRole('region', { name: 'CC2652P' }).getByRole('button', { name: 'Change firmware' }).click();
    const dlg = page.getByRole('dialog');
    await dlg.waitFor();
    await page.waitForTimeout(3000);
    writeFileSync(`${shots}/radio-dialog.aria.txt`, await dlg.ariaSnapshot());
    await shot('radio-dialog');
    // pick the coordinator firmware 20250325 if it is offered as a choice
    const pick = dlg.getByText('20250325').first();
    if (await pick.count()) await pick.click();
    const before = await now();
    const go = dlg.getByRole('button', { name: /^(Flash|Install|Write)/ }).first();
    await go.click();
    if (await page.getByRole('dialog').getByRole('button', { name: /^(Flash|Yes|Continue)/ }).count()) {
      await page.getByRole('dialog').getByRole('button', { name: /^(Flash|Yes|Continue)/ }).first().click();
    }
    const op = await waitOp('radio_flash', before, 600);
    await shot('radio-flashed');
    check('radios: flash from the page', op?.state === 'succeeded', op ? `${op.state} ${op.error ?? ''}` : 'no operation');
  },

  async updates() {
    await open('/updates');
    await page.getByRole('button', { name: 'Check now' }).click();
    await page.waitForTimeout(5000);
    const u = await api.get('/updates');
    check('updates: check now', !!u.os.checked_at, u.os.checked_at ?? '');
    await page.locator('label').filter({ hasText: /^Test$/ }).click();
    await confirmDialog('Use test releases');
    await page.waitForTimeout(2500);
    let u2 = await api.get('/updates');
    check('updates: test channel', u2.os.channel === 'dev' || u2.channel === 'dev', JSON.stringify(u2.os.channel ?? u2.channel));
    await page.locator('label').filter({ hasText: /^Stable$/ }).click();
    await page.waitForTimeout(2500);
    u2 = await api.get('/updates');
    check('updates: back to stable', (u2.os.channel ?? u2.channel) === 'stable');
    const sw = page.getByRole('switch', { name: /^Update automatically/ });
    await sw.click();
    await page.waitForTimeout(1500);
    const a = (await api.get('/updates')).auto_update;
    await sw.click();
    await page.waitForTimeout(1500);
    const b = (await api.get('/updates')).auto_update;
    check('updates: automatic updates switch', a !== b, `${a} -> ${b}`);
    await shot('updates');
  },

  async general() {
    await open('/settings/general');
    const name = page.getByLabel('Name', { exact: true });
    const old = await name.inputValue();
    await name.fill('SMHUB-click');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(3000);
    let g = await api.get('/settings/general');
    check('general: hub name saved', g.hostname === 'SMHUB-click', g.hostname);
    await name.fill(old);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(3000);
    g = await api.get('/settings/general');
    check('general: hub name put back', g.hostname === old, g.hostname);
    // time zone through the search select
    await page.getByRole('button', { name: /^Time zone/ }).click();
    await page.keyboard.type('Kyiv');
    await page.waitForTimeout(800);
    await shot('general-tz-search');
    await page.getByRole('dialog').getByRole('button', { name: /Kyiv/ }).first().click();
    await page.waitForTimeout(3000);
    g = await api.get('/settings/general');
    check('general: time zone chosen', /Kyiv|Kiev/.test(g.timezone), g.timezone);
    await page.getByRole('button', { name: /^Time zone/ }).click();
    await page.keyboard.type('Etc/UTC');
    await page.waitForTimeout(800);
    await page.getByRole('dialog').getByRole('button', { name: 'Etc/UTC', exact: true }).first().click();
    await page.waitForTimeout(3000);
    g = await api.get('/settings/general');
    check('general: time zone put back', g.timezone === 'Etc/UTC', g.timezone);
  },

  async appearance() {
    await open('/settings/regional');
    const p0 = await api.get('/preferences'); // put back at the end: the owner's own choices
    // the language list is searched: in the language's own name or in the current UI language
    const chooseLanguage = async (button, search) => {
      await page.getByRole('button', { name: button }).first().click();
      await page.getByRole('dialog').getByRole('searchbox').fill(search);
      await page.keyboard.press('Enter');
      await page.waitForTimeout(1500);
    };
    await chooseLanguage(/^Language/, 'Ukrain');
    const h = await page.locator('h1').first().innerText();
    check('appearance: Ukrainian', /[а-яіїєґ]/i.test(h), h);
    await shot('appearance-uk');
    await chooseLanguage(/^Мова/, 'English');
    check('appearance: back to English', (await page.locator('h1').first().innerText()) === 'Regional settings');
    await page.locator('label').filter({ hasText: /^Imperial$/ }).click();
    await page.waitForTimeout(1000);
    check('appearance: imperial units', (await page.locator('main').innerText()).includes('mi'));
    await page.locator('label').filter({ hasText: /^Metric$/ }).click();
    await page.waitForTimeout(1000);
    // color tone, text size and spacing change the page at once and are saved
    await open('/settings/appearance');
    const look = () => page.evaluate(() => ({ ...document.documentElement.dataset, accent: getComputedStyle(document.documentElement).getPropertyValue('--accent').trim(), font: getComputedStyle(document.documentElement).fontSize }));
    await page.locator('label').filter({ hasText: /^Blue$/ }).click();
    await page.locator('label').filter({ hasText: /^Large$/ }).click();
    await page.locator('label').filter({ hasText: /^Compact/ }).click();
    await page.waitForTimeout(1500);
    let l = await look();
    let pr = await api.get('/preferences');
    // blue's accent in the light or the dark theme (scripts/tones.mjs)
    check('appearance: blue tone, large text, compact spacing', l.tone === 'blue' && l.text === 'large' && l.density === 'compact' && ['#3565a1', '#6bacfd'].includes(l.accent) && l.font === '18px', JSON.stringify(l));
    check('appearance: look saved', pr.tone === 'blue' && pr.text_size === 'large' && pr.density === 'compact', JSON.stringify(pr).slice(0, 160));
    await shot('appearance-blue-large-compact');
    await page.locator('label').filter({ hasText: /^Green$/ }).click();
    await page.locator('label').filter({ hasText: /^Standard$/ }).click();
    await page.locator('label').filter({ hasText: /^Comfortable/ }).click();
    await page.waitForTimeout(1500);
    l = await look();
    pr = await api.get('/preferences');
    check('appearance: look back to the default', !l.tone && !l.text && !l.density && l.font === '16px' && pr.tone === 'green' && pr.text_size === 'normal' && pr.density === 'comfortable', JSON.stringify(l));
    await page.locator('label').filter({ hasText: /^Dark$/ }).click();
    await page.waitForTimeout(1000);
    const bg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    await shot('appearance-dark');
    check('appearance: dark theme', /rgb\((\d{1,2}), (\d{1,2}), (\d{1,2})\)/.test(bg), bg);
    await page.locator('label').filter({ hasText: /^Automatic/ }).first().click();
    await page.waitForTimeout(1000);
    const p = await api.get('/preferences');
    check('appearance: preferences back', p.language === 'en' && p.theme === 'auto' && p.units === 'metric', JSON.stringify(p).slice(0, 120));
    // the theme button beside the account: Automatic → Light → Dark, without opening settings
    const seen = [];
    for (let i = 0; i < 3; i++) {
      await page.getByRole('button', { name: /^Theme: / }).first().click();
      await page.waitForTimeout(1200);
      seen.push((await api.get('/preferences')).theme);
    }
    check('theme button: Light, Dark, then Automatic again', seen.join(' ') === 'light dark auto', seen.join(' → '));
    await api.patch('/preferences', { language: p0.language, theme: p0.theme, units: p0.units, text_size: p0.text_size, density: p0.density, tone: p0.tone });
  },

  async mqtt() {
    await open('/settings/mqtt');
    await page.getByRole('button', { name: 'Add account' }).click();
    const dlg = page.getByRole('dialog');
    await dlg.getByLabel(/^User name/).fill('clicktest');
    await dlg.getByLabel(/^Password/).fill('Click-test-1');
    await dlg.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(2500);
    let m = await api.get('/settings/mqtt');
    check('mqtt: account added', JSON.stringify(m).includes('clicktest'));
    await shot('mqtt-user');
    await page.getByRole('button', { name: 'Remove clicktest' }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button', { name: /^Remove/ }).last().click();
    await page.waitForTimeout(2500);
    m = await api.get('/settings/mqtt');
    check('mqtt: account removed', !JSON.stringify(m).includes('clicktest'));
    const port = page.getByLabel('Port', { exact: true });
    await port.fill('1884');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(4000);
    m = await api.get('/settings/mqtt');
    check('mqtt: port saved', m.port === 1884, String(m.port));
    await port.fill('1883');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(4000);
    m = await api.get('/settings/mqtt');
    check('mqtt: port put back', m.port === 1883, String(m.port));
  },

  async esphome() {
    await open('/settings/esphome');
    const bz = page.getByRole('switch', { name: /^Buzzer/ });
    await bz.click();
    await page.waitForTimeout(2500);
    let e = await api.get('/settings/esphome');
    check('esphome: buzzer off', e.buzzer === false);
    await bz.click();
    await page.waitForTimeout(2500);
    e = await api.get('/settings/esphome');
    check('esphome: buzzer on again', e.buzzer === true);
    await page.getByRole('button', { name: 'Show' }).first().click();
    check('esphome: key shown', (await page.locator('main').innerText()).includes('VGVzdGluZ1RST'));
    await page.getByRole('button', { name: 'Restart ESPHome' }).first().click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button', { name: /^Restart/ }).last().click();
    await page.waitForTimeout(3000);
    await shot('esphome');
    check('esphome: restart accepted', true);
  },

  async usb() {
    await open('/peripherals/usb');
    // USB, SSH, the serial bridge and the tools are on the advanced level
    if (await page.getByRole('button', { name: 'Switch to advanced mode' }).isVisible().catch(() => false)) {
      await page.getByRole('button', { name: 'Switch to advanced mode' }).click();
      await page.waitForTimeout(1500);
      check('level: switch to advanced from a page', (await api.get('/preferences')).level === 'advanced');
    }
    await page.locator('label').filter({ hasText: /^Host/ }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button').filter({ hasText: /Switch|Host|Save|Yes/ }).first().click();
    await page.waitForTimeout(2500);
    let u = await api.get('/settings/usb');
    check('usb: host mode chosen', u.mode === 'host' || u.pending_mode === 'host', JSON.stringify(u).slice(0, 100));
    await shot('usb-host');
    await page.locator('label').filter({ hasText: /^Device/ }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button').filter({ hasText: /Switch|Device|Save|Yes/ }).first().click();
    await page.waitForTimeout(2500);
    u = await api.get('/settings/usb');
    check('usb: device mode back', (u.pending_mode ?? u.mode) === 'device', JSON.stringify(u).slice(0, 100));
  },

  async ssh() {
    await open('/settings/ssh');
    const root = page.getByRole('switch', { name: /^Allow root/ });
    await root.click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button').last().click();
    await page.waitForTimeout(4000);
    let s = await api.get('/settings/ssh');
    check('ssh: root sign-in off', s.root_login === false);
    await root.click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button').last().click();
    await page.waitForTimeout(4000);
    s = await api.get('/settings/ssh');
    check('ssh: root sign-in back on', s.root_login === true);
    // a key whose fingerprint has "/" in it (base64; about every other key): its removal must
    // still reach the hub, which a fingerprint in the address path would not (nginx decodes %2F)
    let blob, fp;
    do {
      const len = (n) => { const b = Buffer.alloc(4); b.writeUInt32BE(n); return b; };
      blob = Buffer.concat([len(11), Buffer.from('ssh-ed25519'), len(32), randomBytes(32)]);
      fp = 'SHA256:' + createHash('sha256').update(blob).digest('base64').replace(/=+$/, '');
    } while (!fp.includes('/'));
    await page.getByLabel('Add a key').fill(`ssh-ed25519 ${blob.toString('base64')} clicktest@example`);
    await page.getByRole('button', { name: 'Add key' }).click();
    await page.waitForTimeout(2500);
    s = await api.get('/settings/ssh');
    check('ssh: key added', s.keys.some((k) => k.fingerprint === fp), fp);
    await shot('ssh-key');
    await page.getByRole('button', { name: /^Remove key/ }).first().click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button', { name: /^Remove/ }).last().click();
    await page.waitForTimeout(2500);
    s = await api.get('/settings/ssh');
    check('ssh: key removed', !JSON.stringify(s.keys).includes('clicktest'));
  },

  async serialBridge() {
    await open('/settings/serial-bridge');
    await page.getByRole('button', { name: 'Add a port' }).click();
    await page.waitForTimeout(800);
    writeFileSync(`${shots}/serial.aria.txt`, await page.locator('main').ariaSnapshot());
    await shot('serial-add');
    const dev = page.getByLabel('Serial port').last();
    const tag = await dev.evaluate((e) => e.tagName);
    if (tag === 'SELECT') await dev.selectOption('/dev/ttyS2');
    else await dev.fill('/dev/ttyS2');
    await page.getByLabel('TCP port').last().fill('6638');
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(3000);
    let b = await api.get('/settings/serial-bridge');
    check('serial: port shared', JSON.stringify(b.ports).includes('6638'), JSON.stringify(b.ports));
    // a save restarts ser2net and answers once its ports are open: "started" alone does not
    // show that ser2net took its file
    check('serial: port accepts connections', b.ports.some((p) => p.tcp_port === 6638 && p.listening), JSON.stringify(b.ports));
    check('serial: no warning', !(await page.getByText('nothing accepts connections').isVisible().catch(() => false)));
    // saving starts the bridge when it is set to run; then stop, start and stop it by hand
    if (await page.getByRole('button', { name: 'Stop', exact: true }).isVisible().catch(() => false)) {
      await page.getByRole('button', { name: 'Stop', exact: true }).click();
      await page.getByRole('button', { name: 'Start', exact: true }).waitFor({ timeout: 30000 });
    }
    await page.getByRole('button', { name: 'Start', exact: true }).click();
    await page.getByRole('button', { name: 'Stop', exact: true }).waitFor({ timeout: 30000 });
    b = await api.get('/settings/serial-bridge');
    check('serial: bridge started', b.state === 'started', b.state);
    await shot('serial-started');
    await page.getByRole('button', { name: 'Stop', exact: true }).click();
    await page.getByRole('button', { name: 'Start', exact: true }).waitFor({ timeout: 30000 });
    b = await api.get('/settings/serial-bridge');
    check('serial: bridge stopped', b.state !== 'started', b.state);
    await page.getByRole('button', { name: /^Remove port/ }).last().click();
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(3000);
    b = await api.get('/settings/serial-bridge');
    check('serial: port removed', !JSON.stringify(b.ports).includes('6638'), JSON.stringify(b.ports));
  },

  async terminal() {
    await open('/tools/terminal');
    await page.getByText('Connected').first().waitFor({ timeout: 15000 });
    await page.getByLabel('Terminal input').click();
    await page.keyboard.type('echo click-$((40+2)) && whoami\n');
    const ok = await page.getByText('click-42').first().waitFor({ timeout: 10000 }).then(() => true, () => false);
    await shot('terminal');
    check('terminal: runs a command typed on the page', ok);
    const user = (await page.locator('.xterm-rows').innerText().catch(() => '')).includes('smlight');
    check('terminal: as the device user', user);
  },

  async files() {
    await open('/tools/files');
    await page.getByRole('button', { name: 'New folder' }).click();
    const dlg = page.getByRole('dialog');
    await dlg.getByRole('textbox').first().fill('click-test');
    await dlg.getByRole('button', { name: 'Create' }).click();
    await page.getByRole('button', { name: 'click-test', exact: true }).waitFor({ timeout: 10000 });
    check('files: folder created', true);
    await page.getByRole('button', { name: 'click-test', exact: true }).click();
    await page.waitForTimeout(1000);
    // upload through the file chooser the Upload button opens
    const content = Buffer.from('hello from the click-through\n'.repeat(40000)); // about 1.2 MB: two pieces
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Upload' }).click();
    await (await chooser).setFiles({ name: 'upload.txt', mimeType: 'text/plain', buffer: content });
    const up = await page.getByRole('button', { name: 'upload.txt', exact: true }).waitFor({ timeout: 60000 }).then(() => true, () => false);
    await shot('files-uploaded');
    const listing = await api.get('/files?path=' + encodeURIComponent('/home/smlight/click-test'));
    const entry = JSON.stringify(listing).match(/"name":"upload.txt"[^}]*"size":(\d+)/);
    check('files: upload of 1.2 MB in pieces', up && entry && Number(entry[1]) === content.length, entry ? `${entry[1]} bytes` : JSON.stringify(listing).slice(0, 120));
    // rename
    await page.getByRole('button', { name: 'Actions for upload.txt' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Rename', exact: true }).click();
    await page.getByRole('dialog', { name: /^Rename/ }).getByLabel('Name').fill('renamed.txt');
    await page.getByRole('dialog', { name: /^Rename/ }).getByRole('button', { name: 'Rename', exact: true }).click();
    const renamed = await page.getByRole('button', { name: 'renamed.txt', exact: true }).waitFor({ timeout: 10000 }).then(() => true, () => false);
    check('files: rename', renamed);
    // download
    const dl = page.waitForEvent('download', { timeout: 20000 }).catch(() => null);
    await page.getByRole('button', { name: 'Actions for renamed.txt' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Download', exact: true }).click();
    const d = await dl.catch(() => null);
    const p = d ? await d.path() : null;
    check('files: download', !!p && statSync(p).size === content.length, p ? `${statSync(p).size} bytes` : 'no download');
    // back up and delete the folder
    await page.getByRole('button', { name: 'Up one folder' }).click();
    await page.getByRole('button', { name: 'Actions for click-test' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Delete', exact: true }).click();
    await confirmDialog('Delete');
    const gone = await page.getByRole('button', { name: 'click-test', exact: true }).waitFor({ state: 'detached', timeout: 10000 }).then(() => true, () => false);
    check('files: delete a folder', gone);
  },

  async logs() {
    await open('/tools/logs');
    await page.getByRole('button', { name: /^SMHUB Services/ }).click();
    await page.waitForTimeout(2500);
    const lines = await page.getByRole('log').innerText();
    check('logs: a log opens with its last lines', lines.includes('smhubd'), lines.slice(-80));
    await page.getByPlaceholder(/Show only lines with/).fill('ready');
    await page.waitForTimeout(2500);
    const filtered = await page.getByRole('log').innerText();
    check('logs: filter', filtered.split('\n').filter(Boolean).every((l) => /ready/i.test(l)), filtered.slice(0, 80));
    const dl = page.waitForEvent('download', { timeout: 20000 }).catch(() => null);
    await page.getByRole('link', { name: 'Download the whole log' }).click();
    const d = await dl.catch(() => null);
    check('logs: download', !!d);
    await shot('logs');
  },

  async services() {
    await open('/tools/services');
    await page.getByRole('button', { name: 'Restart stunnel' }).click();
    await confirmDialog('Restart');
    await page.waitForTimeout(4000);
    const s = (await api.get('/services')).services.find((x) => x.name === 'stunnel');
    check('services: restart stunnel', s?.state === 'started', s?.state);
    check('services: no Stop for sshd', (await page.getByRole('button', { name: 'Stop sshd' }).count()) === 0);
    check('services: no actions for the hub itself', (await page.getByRole('button', { name: /smhub-services$/ }).count()) === 0);
    await page.getByRole('button', { name: 'Start zigbee2mqtt' }).click();
    await page.getByRole('button', { name: 'Stop zigbee2mqtt' }).waitFor({ timeout: 60000 });
    check('services: start an app service', true);
    await page.getByRole('button', { name: 'Stop zigbee2mqtt' }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button', { name: /^Stop/ }).last().click();
    await page.getByRole('button', { name: 'Start zigbee2mqtt' }).waitFor({ timeout: 60000 });
    check('services: stop it again', true);
    await shot('services');
  },

  async palette() {
    await open('/');
    await page.keyboard.press('Control+K');
    const dlg = page.getByRole('dialog');
    await dlg.waitFor({ timeout: 5000 });
    await page.keyboard.type('wi-fi');
    await page.waitForTimeout(500);
    await shot('palette');
    await page.keyboard.press('Enter');
    await page.waitForURL(/\/network\/wifi/, { timeout: 5000 }).catch(() => {});
    check('palette: Ctrl+K, type, Enter opens the page', page.url().endsWith('/network/wifi'), page.url());
  },

  async phone() {
    const ph = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
    const p2 = await ph.newPage();
    await p2.goto(hub + '/');
    await p2.locator('h1').first().waitFor({ timeout: 20000 });
    await p2.waitForTimeout(1000);
    await p2.screenshot({ path: `${shots}/phone-home.png` });
    const nav = p2.getByRole('navigation').last();
    await nav.getByRole('link', { name: 'Apps' }).tap();
    await p2.waitForURL(/\/apps$/, { timeout: 5000 }).catch(() => {});
    check('phone: bottom bar opens Apps', p2.url().endsWith('/apps'), p2.url());
    await p2.getByRole('button', { name: 'More' }).tap();
    await p2.waitForTimeout(600);
    await p2.screenshot({ path: `${shots}/phone-more.png` });
    await p2.getByRole('dialog').getByRole('link', { name: 'Radios' }).tap();
    await p2.waitForURL(/\/settings$/, { timeout: 5000 }).catch(() => {});
    check('phone: More sheet opens Radios', p2.url().endsWith('/radios'), p2.url());
    await p2.screenshot({ path: `${shots}/phone-radios.png` });
    const wide = await p2.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);
    check('phone: no sideways scrolling', wide);
    await ph.close();
  },

  async accounts() {
    // a separate test account: the factory account keeps its password (the 1.x UI signs in
    // with it when sign-in is off, so changing it would matter after going back)
    await open('/settings/accounts');
    await page.getByLabel(/^User name/).fill('owner@example.com');
    await page.getByLabel(/^Password( \*)?$/).fill('Click-owner-1');
    await page.getByLabel(/^Password again/).fill('Click-owner-1');
    await page.getByRole('button', { name: 'Ask for this password' }).click();
    await page.waitForTimeout(3000);
    let sec = await api.get('/security');
    check('accounts: sign-in turned on', sec.required === true, JSON.stringify(sec));
    await shot('accounts-on');
    // a new browser without the cookie is refused
    const anon = await browser.newContext();
    const r = await anon.request.get(`${hub}/api/v2/apps`);
    check('accounts: other browsers must sign in', r.status() === 401, String(r.status()));
    const ap = await anon.newPage();
    await ap.goto(hub + '/');
    const signInShown = await ap.getByRole('button', { name: 'Sign in' }).waitFor({ timeout: 15000 }).then(() => true, () => false);
    await ap.screenshot({ path: `${shots}/signin-page.png` });
    check('accounts: sign-in page for a new browser', signInShown);
    await ap.getByLabel(/^User name/).fill('owner@example.com');
    await ap.getByLabel(/^Password/).fill('wrong-password-1');
    await ap.getByRole('button', { name: 'Sign in' }).click();
    check('accounts: wrong password refused', await ap.getByText('That user name or password is not right.').waitFor({ timeout: 10000 }).then(() => true, () => false));
    await ap.getByLabel(/^Password/).fill('Click-owner-1');
    await ap.getByRole('button', { name: 'Sign in' }).click();
    check('accounts: sign in', await ap.locator('h1').filter({ hasText: /Everything|running|Home/ }).first().waitFor({ timeout: 15000 }).then(() => true, () => false));
    await ap.getByRole('button', { name: 'Sign out' }).click();
    check('accounts: sign out', await ap.getByRole('button', { name: 'Sign in' }).waitFor({ timeout: 10000 }).then(() => true, () => false));
    await anon.close();
    // add, change and remove another account in the first browser
    await page.reload();
    await page.locator('h1').first().waitFor();
    await page.getByRole('button', { name: 'Add account' }).click();
    let dlg = page.getByRole('dialog');
    await dlg.getByLabel(/^User name/).fill('second@example.com');
    await dlg.getByLabel(/^New password/).fill('Click-second-1');
    if (await dlg.getByLabel(/^Password again/).count()) await dlg.getByLabel(/^Password again/).fill('Click-second-1');
    await dlg.getByRole('button', { name: /^(Add|Save|Add account)$/ }).last().click();
    await page.waitForTimeout(2500);
    let users = await api.get('/users');
    check('accounts: account added', JSON.stringify(users).includes('second@example.com'));
    await page.getByRole('button', { name: 'Change the password of second@example.com' }).click();
    dlg = page.getByRole('dialog');
    await dlg.getByLabel(/^New password/).fill('Click-second-2');
    if (await dlg.getByLabel(/^Password again/).count()) await dlg.getByLabel(/^Password again/).fill('Click-second-2');
    await dlg.getByRole('button', { name: /^(Change password|Save)$/ }).last().click();
    check('accounts: password changed', await page.getByText('Password changed').first().waitFor({ timeout: 10000 }).then(() => true, () => false));
    await page.getByRole('button', { name: 'Remove second@example.com' }).click();
    await confirmDialog('Remove');
    await page.waitForTimeout(2500);
    users = await api.get('/users');
    check('accounts: account removed', !JSON.stringify(users).includes('second@example.com'));
    await shot('accounts-list');
    // open again
    await page.getByRole('switch', { name: /^Ask for a password/ }).click();
    await confirmDialog('Stop asking');
    await page.waitForTimeout(2500);
    sec = await api.get('/security');
    check('accounts: sign-in turned off', sec.required === false);
    users = await api.get('/users');
    const owner = (Array.isArray(users) ? users : users.users ?? []).find((u) => u.username === 'owner@example.com');
    if (owner) {
      // with sign-in off the page shows no account list (by design); the test account goes by API
      const fresh = await browser.newContext(); // not signed in as the owner: nobody may remove themselves
      await fresh.request.delete(`${hub}/api/v2/users/${owner.id}`, { headers: { Origin: hub } });
      await fresh.close();
      users = await api.get('/users');
      check('accounts: test owner removed', !JSON.stringify(users).includes('owner@example.com'));
    }
  },

  async backupRestore() {
    await open('/settings/backups');
    const dl = page.waitForEvent('download', { timeout: 180000 }).catch(() => null);
    await page.getByRole('button', { name: 'Make and download' }).click();
    const d = await dl;
    const file = d ? await d.path() : null;
    const size = file ? statSync(file).size : 0;
    check('backups: make and download', size > 1000, `${size} bytes, ${d?.suggestedFilename()}`);
    if (!file) return;
    const rowsBefore = await api.get('/apps');
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Choose a file' }).click();
    await (await chooser).setFiles(file);
    const dlg = page.getByRole('dialog', { name: 'Restore this backup?' });
    const shown = await dlg.waitFor({ timeout: 60000 }).then(() => true, () => false);
    await shot('backup-restore-confirm');
    check('backups: the file is checked and described before restoring', shown && (await dlg.innerText()).includes('Made'));
    await dlg.getByRole('button', { name: 'Restore', exact: true }).click();
    await page.waitForTimeout(5000);
    await shot('backup-restoring');
    // apps stop (Tailscale too, so this connection may drop), then the hub's service restarts
    let back = false;
    for (let i = 0; i < 90 && !back; i++) {
      await page.waitForTimeout(4000);
      back = await page.request.get(`${hub}/api/v2/health`, { timeout: 5000 }).then((r) => r.ok(), () => false);
      if (back) {
        const svc = await api.get('/services').catch(() => null);
        back = !!svc && ['zeesm', 'tailscale'].every((n) => svc.services.find((s) => s.name === n)?.state === 'started');
      }
    }
    check('backups: restore finishes and apps run again', back);
    const rowsAfter = await api.get('/apps');
    check('backups: app list unchanged after restore', rowsAfter.apps.length === rowsBefore.apps.length, `${rowsBefore.apps.length} -> ${rowsAfter.apps.length}`);
    await page.goto(hub + '/');
    await page.locator('h1').first().waitFor({ timeout: 30000 });
    await shot('after-restore');
  },

  async systemPage() {
    await open('/settings/system');
    await page.getByRole('button', { name: 'Power off' }).click();
    const dlg = page.getByRole('dialog');
    await dlg.waitFor();
    await shot('poweroff-confirm');
    check('system: power off asks first', (await dlg.innerText()).length > 20);
    await dlg.getByRole('button', { name: 'Cancel' }).click();
    await page.waitForTimeout(1000);
    const up = await page.request.get(`${hub}/api/v2/health`).then((r) => r.ok(), () => false);
    check('system: cancel keeps the hub on', up);
  },

  async restartHub() {
    await open('/settings/system');
    await page.getByRole('button', { name: 'Restart the hub' }).click();
    const dlg = page.getByRole('dialog');
    await dlg.waitFor();
    await dlg.getByRole('button', { name: /^Restart/ }).last().click();
    const t0 = Date.now();
    await page.waitForTimeout(15000);
    await shot('restarting');
    let back = false;
    for (let i = 0; i < 60 && !back; i++) {
      await page.waitForTimeout(5000);
      back = await page.request.get(`${hub}/api/v2/system`, { timeout: 5000 }).then(async (r) => r.ok() && (await r.json()).uptime_s < 600, () => false);
    }
    check('system: the hub restarts and answers again', back, `${Math.round((Date.now() - t0) / 1000)} s`);
    const reconnected = await page.locator('h1').first().waitFor({ timeout: 60000 }).then(() => true, () => false);
    await page.waitForTimeout(5000);
    await shot('after-restart');
    check('system: the open page reconnects by itself', reconnected && (await page.getByText('Live').first().isVisible().catch(() => false)));
  },

  /** Needs the hub's setup marker cleared first (a new hub). Skips the password step. */
  async setup() {
    const sys = await api.get('/system');
    if (!sys.setup_needed) {
      check('setup: hub reports setup needed (clear the marker first)', false);
      return;
    }
    await page.goto(hub + '/');
    await page.getByRole('button', { name: /Let.s start/ }).waitFor({ timeout: 20000 });
    await shot('setup-0');
    await page.getByRole('button', { name: /Let.s start/ }).click();
    const order = [/^Finish|^Done|^Go to (the|my) hub|^Open the hub/, /^Next|^Continue|^Save and continue/, /^Continue without|^Skip this step|^Not now|^Skip$/];
    for (let i = 1; i <= 8; i++) {
      await page.waitForTimeout(1200);
      await shot(`setup-${i}`);
      writeFileSync(`${shots}/setup-${i}.aria.txt`, await page.locator('body').ariaSnapshot());
      if (!(await api.get('/system')).setup_needed) break;
      const body = await page.locator('body').innerText();
      // never set a password here: the step that asks for one is skipped
      const pw = /password/i.test(body) && (await page.getByLabel(/password/i).count()) > 0;
      let clicked = false;
      for (const re of pw ? [order[2], order[0]] : order) {
        const b = page.getByRole('button', { name: re }).first();
        if (await b.isVisible().catch(() => false)) {
          await b.click();
          clicked = true;
          break;
        }
      }
      if (!clicked) break;
    }
    await page.waitForTimeout(2000);
    const after = await api.get('/system');
    check('setup: finishing the setup records it', after.setup_needed === false);
    await page.goto(hub + '/');
    const dash = await page.locator('h1').first().waitFor({ timeout: 20000 }).then(async () => !(await page.getByRole('button', { name: /Let.s start/ }).isVisible().catch(() => false)), () => false);
    check('setup: not offered again', dash);
  },

  async smallControls() {
    await open('/');
    await page.getByRole('button', { name: 'Start Zigbee2mqtt' }).click();
    await page.getByRole('button', { name: 'Stop Zigbee2mqtt' }).waitFor({ timeout: 60000 });
    check('dashboard: start an app from its card', true);
    await page.getByRole('button', { name: 'Stop Zigbee2mqtt' }).click();
    await page.getByRole('button', { name: 'Start Zigbee2mqtt' }).waitFor({ timeout: 60000 });
    check('dashboard: stop it again', true);
    await page.getByRole('button', { name: 'Dismiss' }).first().click();
    await page.waitForTimeout(500);
    check('dashboard: open-hub notice can be dismissed', !(await page.getByText('Anyone on your network can change this hub').first().isVisible().catch(() => false)));
    await page.locator('label').filter({ hasText: /^Simple$/ }).first().click();
    await page.waitForTimeout(1200);
    check('level: back to simple from the sidebar', (await api.get('/preferences')).level === 'simple');
    await page.locator('label').filter({ hasText: /^Advanced$/ }).first().click();
    await page.waitForTimeout(1200);
    check('level: advanced from the sidebar', (await api.get('/preferences')).level === 'advanced');
    await page.getByRole('button', { name: 'Collapse the sidebar' }).click();
    await page.waitForTimeout(600);
    await shot('sidebar-collapsed');
    await page.getByRole('button', { name: /Expand the sidebar/ }).click();
    check('shell: sidebar collapses and expands', true);
    await open('/network/lte');
    await page.getByRole('button', { name: 'Reset modem' }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button', { name: /^Reset/ }).last().click();
    await page.waitForTimeout(5000);
    check('lte: reset modem', true);
    await open('/settings/appearance');
    await page.getByRole('button', { name: 'Reset the layout' }).click();
    await page.waitForTimeout(1000);
    const dash = (await api.get('/preferences')).dashboard;
    check('appearance: reset the home layout', dash.length === 0 || (dash.length === 6 && !dash.some((c) => c.startsWith('hidden:'))), JSON.stringify(dash));
  },

  async extras() {
    // Wi-Fi radio off and on (the hub is on the cable)
    await open('/network/wifi');
    const sw = page.getByRole('switch', { name: /^Wi-Fi/ });
    await sw.click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button').last().click();
    await page.waitForTimeout(4000);
    let w = await api.get('/wifi');
    check('wifi: radio off', w.enabled === false, JSON.stringify(w.enabled));
    await sw.click();
    await page.waitForTimeout(4000);
    w = await api.get('/wifi');
    check('wifi: radio on again', w.enabled === true, JSON.stringify(w.enabled));
    // VPN from a .conf file, with only a test network routed
    const key = () => Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString('base64');
    await open('/network/vpn');
    const conf = `[Interface]\nPrivateKey = ${key()}\nAddress = 10.98.0.2/32\n\n[Peer]\nPublicKey = ${key()}\nEndpoint = vpn.example.org:51820\nAllowedIPs = 10.98.0.0/24\nPersistentKeepalive = 25\n`;
    const chooser = page.waitForEvent('filechooser');
    await page.getByRole('button', { name: 'Import .conf' }).click();
    await (await chooser).setFiles({ name: 'test.conf', mimeType: 'text/plain', buffer: Buffer.from(conf) });
    await page.waitForTimeout(1000);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await page.waitForTimeout(4000);
    let v = await api.get('/vpn');
    check('vpn: import a .conf file', JSON.stringify(v).includes('10.98.0.2') && JSON.stringify(v).includes('vpn.example.org'), JSON.stringify(v.settings ?? v).slice(0, 120));
    await page.getByRole('button', { name: 'Remove VPN' }).click();
    await confirmDialog('Remove VPN');
    await page.waitForTimeout(3000);
    v = await api.get('/vpn');
    check('vpn: removed again', v.configured === false);
    // a new ESPHome key, then the factory key back
    await open('/settings/esphome');
    await page.getByRole('button', { name: 'Make a new key' }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await page.getByRole('dialog').getByRole('button').last().click();
    await page.waitForTimeout(5000);
    let e = await api.get('/settings/esphome');
    check('esphome: new key made', e.encryption_key && !e.encryption_key.startsWith('VGVzdGluZ1RST'));
    await api.patch('/settings/esphome', { encryption_key: 'VGVzdGluZ1RSTkdFbmNyeXB0aW9uS2V5MTIzNDU2Nzg=' });
    await page.waitForTimeout(3000);
    e = await api.get('/settings/esphome');
    check('esphome: factory key put back', e.encryption_key === 'VGVzdGluZ1RSTkdFbmNyeXB0aW9uS2V5MTIzNDU2Nzg=');
  },

  async appReset() {
    await open('/apps/matterbridge');
    const before = await now();
    await page.getByRole('button', { name: 'Install', exact: true }).click();
    if (await page.getByRole('dialog').isVisible().catch(() => false)) await confirmDialog('Install');
    const op = await waitOp('app_install', before, 900);
    check('app reset: test app installed', op?.state === 'succeeded');
    await page.getByRole('button', { name: 'Reset settings' }).waitFor({ timeout: 60000 });
    await page.getByRole('button', { name: 'Reset settings' }).click();
    await confirmDialog('Reset settings');
    const reset = await page.getByText(/reset|put back/i).first().waitFor({ timeout: 60000 }).then(() => true, () => false);
    await shot('app-reset');
    check('app reset: settings reset', reset);
    const b2 = await now();
    await page.getByRole('button', { name: 'Remove', exact: true }).click();
    await confirmDialog('Remove');
    const op2 = await waitOp('app_remove', b2, 600);
    check('app reset: test app removed', op2?.state === 'succeeded');
  },

  /** Saving the broker's settings must not lock out Zigbee2MQTT, which connects without a password. */
  async mqttKeepsZ2m() {
    const z2mLog = async () => (await api.get('/logs/zigbee2mqtt?lines=5000')).lines ?? [];
    const count = async (re) => (await z2mLog()).filter((l) => re.test(l)).length;
    const running = async () => (await api.get('/services')).services.find((s) => s.name === 'zigbee2mqtt')?.state;
    const OK = /Connected to MQTT server/;
    const REFUSED = /Not authorized/;
    /** Waits until Zigbee2MQTT logs a new MQTT connection (it is restarted with the broker). */
    const connected = async (okBefore, refusedBefore, limitS = 240) => {
      for (let i = 0; i < limitS / 3; i++) {
        if ((await count(REFUSED)) > refusedBefore) return false;
        if ((await count(OK)) > okBefore && (await running()) === 'started') return true;
        await page.waitForTimeout(3000);
      }
      return false;
    };
    const wasRunning = (await running()) === 'started';
    let ok0 = await count(OK);
    let ref0 = await count(REFUSED);
    if (!wasRunning) await page.request.post(`${hub}/api/v2/apps/zigbee2mqtt/start`, { headers: { Origin: hub } });
    check('mqtt/z2m: Zigbee2MQTT starts and connects without a password', wasRunning || (await connected(ok0, ref0)));
    await page.waitForTimeout(5000); // let it finish starting before the broker restarts
    await open('/settings/mqtt');
    const steps = [
      ['other devices on', async () => page.getByRole('switch', { name: /^Allow other devices/ }).click()],
      ['other devices off', async () => page.getByRole('switch', { name: /^Allow other devices/ }).click()],
      ['port 1884', async () => { await page.getByLabel('Port', { exact: true }).fill('1884'); await page.getByRole('button', { name: 'Save', exact: true }).click(); }],
      ['port back to 1883', async () => { await page.getByLabel('Port', { exact: true }).fill('1883'); await page.getByRole('button', { name: 'Save', exact: true }).click(); }],
    ];
    for (const [name, act] of steps) {
      ok0 = await count(OK);
      ref0 = await count(REFUSED);
      await act();
      await page.waitForTimeout(3000);
      if (name === 'port 1884') continue; // Zigbee2MQTT still points at 1883: it reconnects once the port is back
      check(`mqtt/z2m: connected again after "${name}"`, await connected(ok0, ref0));
      await page.waitForTimeout(5000);
    }
    const m = await api.get('/settings/mqtt');
    check('mqtt/z2m: broker back to hub-only, no password', m.port === 1883 && !m.allow_external && m.allow_anonymous, JSON.stringify(m).slice(0, 80));
    // turning the password requirement on asks first; cancelled here
    await page.getByRole('switch', { name: /^Allow without a password/ }).click();
    const asked = await page.getByRole('dialog', { name: 'Ask every client for a password?' }).waitFor({ timeout: 5000 }).then(() => true, () => false);
    await shot('mqtt-password-confirm');
    if (asked) await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();
    check('mqtt/z2m: asking for a password warns about apps first', asked && (await api.get('/settings/mqtt')).allow_anonymous === true);
    if (!wasRunning) await page.request.post(`${hub}/api/v2/apps/zigbee2mqtt/stop`, { headers: { Origin: hub } });
  },

  /** Every app in the store: install, run, web page, settings, stop, start, remove. APPS=a,b limits it. */
  async allApps() {
    const svcState = async (name) => (await api.get('/services')).services.find((s) => s.app === name || s.name === name)?.state ?? 'none';
    const list = (await api.get('/apps')).apps;
    const wanted = process.env.APPS ? process.env.APPS.split(',') : list.filter((a) => !a.installed && a.compatible !== false).map((a) => a.name);
    const report = [];
    for (const name of wanted) {
      const r = { app: name, install: '', runs: '', web: '', settings: '', stopStart: '', remove: '', left: '' };
      report.push(r);
      try {
        await open(`/apps/${name}`);
        let ids = await now();
        await page.getByRole('button', { name: 'Install', exact: true }).click();
        if (await page.getByRole('dialog').isVisible().catch(() => false)) await confirmDialog('Install');
        const t0 = Date.now();
        const op = await waitOp('app_install', ids, 1200);
        r.install = op?.state === 'succeeded' ? `ok ${Math.round((Date.now() - t0) / 1000)} s` : `FAILED ${op?.error?.split('\n')[0] ?? 'no operation'}`;
        if (op?.state !== 'succeeded') continue;
        await page.getByRole('button', { name: 'Remove', exact: true }).waitFor({ timeout: 60000 });
        const d = await api.get(`/apps/${name}`);
        // run it
        if (d.is_service) {
          if (await page.getByRole('button', { name: 'Start', exact: true }).isVisible().catch(() => false)) {
            await page.getByRole('button', { name: 'Start', exact: true }).click();
          }
          let st = 'none';
          for (let i = 0; i < 40 && st !== 'started'; i++) {
            await page.waitForTimeout(3000);
            st = await svcState(name);
          }
          // still running after 30 s: not a crash loop
          await page.waitForTimeout(30000);
          const st2 = await svcState(name);
          r.runs = st === 'started' && st2 === 'started' ? 'ok' : `FAILED (${st} then ${st2})`;
        } else r.runs = 'not a service';
        // its web page
        const det = await api.get(`/apps/${name}`);
        if (det.web?.port) {
          let code = 0;
          for (let i = 0; i < 30 && !(code > 0 && code < 500); i++) {
            code = await page.request.get(`http://${new URL(hub).hostname}:${det.web.port}/`, { timeout: 5000, maxRedirects: 0 }).then((x) => x.status(), () => 0);
            if (!(code > 0 && code < 500)) await page.waitForTimeout(4000);
          }
          await open(`/open/${name}`);
          const framed = await page.locator('iframe').waitFor({ timeout: 20000 }).then(() => 1, () => 0); // drawn once the app list has arrived
          await shot(`app-${name}-web`);
          r.web = code > 0 && code < 500 && framed ? `ok (port ${det.web.port}, HTTP ${code}; https ${det.web.https_port ?? '-'})` : `FAILED (HTTP ${code}, frame ${framed})`;
          await open(`/apps/${name}`);
        } else r.web = 'none';
        // settings form
        if (det.form?.length) {
          await page.getByRole('tab', { name: 'Settings' }).click();
          await page.waitForTimeout(1500);
          const fields = await page.locator('[role=tabpanel] input, [role=tabpanel] select, [role=tabpanel] textarea').count();
          r.settings = fields > 0 ? `ok (${det.form.length} fields)` : 'FAILED (form empty)';
          await page.getByRole('tab', { name: 'Overview' }).click();
        } else r.settings = 'none';
        // stop and start again
        if (d.is_service) {
          await page.getByRole('button', { name: 'Stop', exact: true }).click();
          await page.getByRole('button', { name: 'Start', exact: true }).waitFor({ timeout: 90000 });
          const s1 = await svcState(name);
          await page.getByRole('button', { name: 'Start', exact: true }).click();
          await page.getByRole('button', { name: 'Stop', exact: true }).waitFor({ timeout: 90000 });
          const s2 = await svcState(name);
          r.stopStart = s1 !== 'started' && s2 === 'started' ? 'ok' : `FAILED (${s1}, ${s2})`;
        } else r.stopStart = '-';
        await shot(`app-${name}`);
        // remove
        ids = await now();
        await page.getByRole('button', { name: 'Remove', exact: true }).click();
        await confirmDialog('Remove');
        const op2 = await waitOp('app_remove', ids, 600);
        const after = await api.get(`/apps/${name}`).catch(() => ({}));
        r.remove = op2?.state === 'succeeded' && !after.installed ? 'ok' : `FAILED ${op2?.error?.split('\n')[0] ?? ''}`;
        const st = await svcState(name);
        const https = Object.prototype.hasOwnProperty.call(after, 'web') && after.web?.https_port;
        r.left = st === 'started' ? 'service still running' : 'nothing running' + (https ? `, https ${https} still listed` : '');
      } catch (e) {
        r.left = `stopped at an error: ${e.message.split('\n')[0]}`;
        await shot(`app-${name}-error`).catch(() => {});
      }
      console.log(JSON.stringify(r));
      check(`apps: ${name}`, !Object.values(r).some((v) => String(v).startsWith('FAILED') || String(v).startsWith('stopped at')), JSON.stringify(r));
    }
    writeFileSync(`${shots}/apps-report.json`, JSON.stringify(report, null, 2));
  },

  async appEmbedded() {
    await open('/open/zeesm');
    const frame = page.frameLocator('iframe');
    const ok = await frame.locator('body').waitFor({ timeout: 20000 }).then(() => true, () => false);
    await page.waitForTimeout(2000);
    await shot('app-zeesm-embedded');
    const src = await page.locator('iframe').getAttribute('src');
    check('app: embedded page loads', ok && /:8787\//.test(src ?? ''), src ?? '');
  },
};

for (const [name, fn] of Object.entries(steps)) {
  if (only && !only.includes(name)) continue;
  console.log(`--- ${name}`);
  try {
    await fn();
  } catch (e) {
    check(`${name} finished`, false, e.message.split('\n')[0]);
    await shot(`${name}-error`).catch(() => {});
  }
}
console.log(`\nFailed requests: ${badResponses.length ? [...new Set(badResponses)].join(' | ') : 'none'}`);
console.log(`JavaScript errors on the pages: ${consoleErrors.length ? consoleErrors.slice(0, 5).join(' | ') : 'none'}`);
const failed = results.filter((r) => !r.ok);
console.log(`${results.length - failed.length} passed, ${failed.length} failed${failed.length ? ': ' + failed.map((f) => f.name).join('; ') : ''}`);
await browser.close();
process.exit(failed.length ? 1 : 0);
