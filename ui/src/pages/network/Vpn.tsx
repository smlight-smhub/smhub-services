// VPN: a WireGuard connection. Keys are checked as you type; secrets are
// write-only (the hub only says whether one is set). A .conf file can fill
// the form.
import { useRef, useState } from 'preact/hooks';
import { FileUp, Link2, Link2Off, RotateCcw, Save, Shield, Trash } from 'lucide-preact';
import { api } from '../../api/client';
import type { Vpn as VpnState, WireguardPatch } from '../../api/types';
import { t } from '../../text/i18n';
import { bytes, relative } from '../../text/format';
import { confirm } from '../../state/confirm';
import { advanced } from '../../state/prefs';
import { errorText, save, useFieldErrors } from '../../state/save';
import { live, setLive, useLive } from '../../state/store';
import { hubTime, useNow } from '../../state/system';
import { toast } from '../../state/toasts';
import { Button } from '../../kit/Button';
import { CopyValue } from '../../kit/Bits';
import { Loading } from '../../kit/Feedback';
import { FieldGrid, ListField, NumberField, Switch, TextField } from '../../kit/Field';
import { Actions, Facts, Page, Section } from '../../kit/Page';
import { Status, type StatusTone } from '../../kit/Status';
import { isCidr } from './IpFields';
import c from '../common.module.css';

/** A WireGuard key: 32 bytes in base64 (44 characters ending in "="). */
export function isWgKey(k: string) {
  return /^[A-Za-z0-9+/]{42}[AEIMQUYcgkosw480]=$/.test(k.trim());
}

interface Form {
  addresses: string[];
  private_key: string;
  public_key: string;
  endpoint: string;
  allowed_ips: string[];
  dns: string[];
  keepalive: number | null;
  preshared_key: string;
  autoconnect: boolean;
}

function fromState(v: VpnState): Form {
  const w = v.settings;
  return {
    addresses: w.addresses,
    private_key: '',
    public_key: w.public_key,
    endpoint: w.endpoint,
    allowed_ips: w.allowed_ips.length ? w.allowed_ips : ['0.0.0.0/0'],
    dns: w.dns,
    keepalive: w.keepalive ?? null,
    preshared_key: '',
    autoconnect: w.autoconnect,
  };
}

/** Reads a WireGuard client .conf file into the form. */
function parseConf(text: string, base: Form): Form {
  const f = { ...base };
  const list = (v: string) => v.split(',').map((x) => x.trim()).filter(Boolean);
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/#.*/, '').trim();
    const m = /^(\w+)\s*=\s*(.+)$/.exec(line);
    if (!m) continue;
    const [, k, v] = m as unknown as [string, string, string];
    switch (k.toLowerCase()) {
      case 'privatekey':
        f.private_key = v.trim();
        break;
      case 'address':
        f.addresses = list(v);
        break;
      case 'dns':
        f.dns = list(v);
        break;
      case 'publickey':
        f.public_key = v.trim();
        break;
      case 'presharedkey':
        f.preshared_key = v.trim();
        break;
      case 'endpoint':
        f.endpoint = v.trim();
        break;
      case 'allowedips':
        f.allowed_ips = list(v);
        break;
      case 'persistentkeepalive':
        f.keepalive = Number(v) || null;
        break;
    }
  }
  return f;
}

function check(f: Form, v: VpnState): Record<string, string> {
  const e: Record<string, string> = {};
  const addrs = f.addresses.filter(Boolean);
  if (!addrs.length) e.addresses = t('ip.needAddress');
  else if (addrs.some((a) => !isCidr(a))) e.addresses = t('ip.badCidr');
  if (!f.private_key && !v.settings.private_key_set) e.private_key = t('form.required');
  else if (f.private_key && !isWgKey(f.private_key)) e.private_key = t('vpn.badKey');
  if (!isWgKey(f.public_key)) e.public_key = f.public_key ? t('vpn.badKey') : t('form.required');
  if (f.preshared_key && !isWgKey(f.preshared_key)) e.preshared_key = t('vpn.badKey');
  if (!/^\[?[A-Za-z0-9.:-]+\]?:\d{1,5}$/.test(f.endpoint.trim())) e.endpoint = t('vpn.badEndpoint');
  if (f.allowed_ips.filter(Boolean).some((a) => !isCidr(a))) e.allowed_ips = t('ip.badCidr');
  return e;
}

const TONE: Record<VpnState['state'], StatusTone> = { connected: 'ok', connecting: 'busy', disconnected: 'neutral', unconfigured: 'neutral' };

export default function Vpn() {
  const v = useLive('vpn');
  const now = useNow('minute');
  const [form, setForm] = useState<Form | null>(null);
  const [busy, setBusy] = useState<string>();
  const [tried, setTried] = useState(false);
  const errors = useFieldErrors();
  const file = useRef<HTMLInputElement>(null);
  const f = form ?? (v ? fromState(v) : null);
  const dirty = form !== null;
  const local = f && v && (tried || dirty) ? check(f, v) : {};
  const set = <K extends keyof Form>(k: K, val: Form[K]) => setForm({ ...(f as Form), [k]: val });

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!f || !v) return;
    setTried(true);
    if (Object.keys(check(f, v)).length) return;
    const body: WireguardPatch = {
      addresses: f.addresses.filter(Boolean),
      public_key: f.public_key.trim(),
      endpoint: f.endpoint.trim(),
      allowed_ips: f.allowed_ips.filter(Boolean),
      dns: f.dns.filter(Boolean),
      keepalive: f.keepalive,
      autoconnect: f.autoconnect,
    };
    if (f.private_key) body.private_key = f.private_key.trim();
    if (f.preshared_key) body.preshared_key = f.preshared_key.trim();
    setBusy('save');
    const ok = await save<VpnState>({ target: live('vpn'), send: () => api.patch('/vpn', { body }), errors, success: t('common.saved') });
    setBusy(undefined);
    if (ok) {
      setForm(null);
      setTried(false);
    }
  };
  const act = async (action: 'connect' | 'disconnect') => {
    setBusy(action);
    try {
      setLive('vpn', await api.post('/vpn/{action}', { path: { action } }));
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setBusy(undefined);
  };
  const remove = async () => {
    if (!(await confirm({ title: t('vpn.removeTitle'), body: t('vpn.removeBody'), confirm: t('vpn.remove'), tone: 'danger' }))) return;
    try {
      await api.del('/vpn');
      setForm(null);
      toast({ text: t('vpn.removed'), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };

  return (
    <Page title={t('nav.vpn')} back={{ href: '/network', label: t('nav.network') }} subtitle={t('nav.vpnDesc')}>
      {!v || !f ? (
        <Loading lines={6} />
      ) : (
        <>
          <Section title={t('vpn.status')} icon={Shield} actions={<Status tone={TONE[v.state]}>{t(`vpn.state.${v.state}`)}</Status>}>
            {v.configured ? (
              <>
                <Facts
                  items={[
                    [t('vpn.handshake'), v.handshake_at ? relative(v.handshake_at, hubTime(now)) : null],
                    [t('vpn.received'), v.rx_bytes !== null && v.rx_bytes !== undefined ? bytes(v.rx_bytes) : null],
                    [t('vpn.sent'), v.tx_bytes !== null && v.tx_bytes !== undefined ? bytes(v.tx_bytes) : null],
                    [t('vpn.ownKey'), v.settings.own_public_key && <CopyValue value={v.settings.own_public_key} label={t('vpn.copyKey')} />],
                  ]}
                />
                <Actions>
                  {v.active ? (
                    <Button icon={Link2Off} busy={busy === 'disconnect'} onClick={() => void act('disconnect')}>
                      {t('vpn.disconnect')}
                    </Button>
                  ) : (
                    <Button variant="primary" icon={Link2} busy={busy === 'connect'} onClick={() => void act('connect')}>
                      {t('vpn.connect')}
                    </Button>
                  )}
                </Actions>
              </>
            ) : (
              <p class="muted">{t('vpn.unconfiguredText')}</p>
            )}
          </Section>
          <Section
            id="settings"
            title={t('vpn.settings')}
            description={t('vpn.settingsDesc')}
            actions={
              <>
                <input
                  ref={file}
                  type="file"
                  accept=".conf,text/plain"
                  hidden
                  onChange={async (e) => {
                    // the element, kept before the await: currentTarget is null once the event is over
                    const input = e.currentTarget as HTMLInputElement;
                    const fl = input.files?.[0];
                    if (!fl) return;
                    setForm(parseConf(await fl.text(), f));
                    setTried(true);
                    toast({ text: t('vpn.imported'), tone: 'ok' });
                    input.value = '';
                  }}
                />
                <Button size="sm" icon={FileUp} onClick={() => file.current?.click()}>
                  {t('vpn.import')}
                </Button>
              </>
            }
          >
            <form class={c.form} onSubmit={submit} noValidate>
              <h3 class={c.itemTitle}>{t('vpn.thisHub')}</h3>
              <TextField
                label={t('vpn.privateKey')}
                type="password"
                mono
                value={f.private_key}
                onValue={(x) => set('private_key', x)}
                placeholder={v.settings.private_key_set ? t('vpn.keySet') : ''}
                hint={t('vpn.privateKeyHint')}
                error={local.private_key ?? errors.value.private_key}
                autoComplete="off"
              />
              <ListField label={t('vpn.addresses')} placeholder="10.0.0.2/32" value={f.addresses} onValue={(x) => set('addresses', x)} error={local.addresses ?? errors.value.addresses} />
              <ListField label={t('ip.dns')} placeholder="1.1.1.1" value={f.dns} onValue={(x) => set('dns', x)} error={errors.value.dns} />
              <h3 class={c.itemTitle}>{t('vpn.server')}</h3>
              <TextField label={t('vpn.publicKey')} mono value={f.public_key} onValue={(x) => set('public_key', x)} error={local.public_key ?? errors.value.public_key} autoComplete="off" />
              <TextField label={t('vpn.endpoint')} mono value={f.endpoint} onValue={(x) => set('endpoint', x)} placeholder="vpn.example.com:51820" hint={t('vpn.endpointHint')} error={local.endpoint ?? errors.value.endpoint} />
              <ListField label={t('vpn.allowedIps')} hint={t('vpn.allowedIpsHint')} placeholder="0.0.0.0/0" value={f.allowed_ips} onValue={(x) => set('allowed_ips', x)} error={local.allowed_ips ?? errors.value.allowed_ips} />
              {advanced.value && (
                <FieldGrid>
                  <TextField
                    label={t('vpn.psk')}
                    type="password"
                    mono
                    value={f.preshared_key}
                    onValue={(x) => set('preshared_key', x)}
                    placeholder={v.settings.preshared_key_set ? t('vpn.keySet') : t('vpn.optional')}
                    error={local.preshared_key ?? errors.value.preshared_key}
                    autoComplete="off"
                  />
                  <NumberField label={t('vpn.keepalive')} hint={t('vpn.keepaliveHint')} min={0} max={65535} unit={t('unit.s')} value={f.keepalive} onValue={(x) => set('keepalive', x)} error={errors.value.keepalive} />
                </FieldGrid>
              )}
              <Switch label={t('vpn.autoconnect')} description={t('vpn.autoconnectDesc')} checked={f.autoconnect} onChange={(x) => set('autoconnect', x)} />
              {(dirty || !v.configured) && (
                <Actions sticky={dirty}>
                  {dirty && (
                    <Button variant="ghost" icon={RotateCcw} onClick={() => (setForm(null), setTried(false))}>
                      {t('common.discard')}
                    </Button>
                  )}
                  <Button type="submit" variant="primary" icon={Save} busy={busy === 'save'}>
                    {t('common.save')}
                  </Button>
                </Actions>
              )}
            </form>
          </Section>
          {v.configured && (
            <Section>
              <div class={c.between}>
                <div>
                  <p>
                    <strong>{t('vpn.remove')}</strong>
                  </p>
                  <p class={c.note}>{t('vpn.removeDesc')}</p>
                </div>
                <Button variant="danger" icon={Trash} onClick={() => void remove()}>
                  {t('vpn.remove')}
                </Button>
              </div>
            </Section>
          )}
        </>
      )}
    </Page>
  );
}
