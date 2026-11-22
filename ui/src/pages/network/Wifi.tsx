// Wi-Fi: radio on/off, the current network, scanning, connecting (with
// password, security, hidden networks and static addresses) and saved networks.
import { useEffect, useState } from 'preact/hooks';
import { Lock, Plus, RefreshCw, Trash, Unplug, Wifi as WifiIcon, WifiOff } from 'lucide-preact';
import { api } from '../../api/client';
import type { IpSettings, Wifi as WifiState, WifiConnect, WifiNetwork } from '../../api/types';
import { t } from '../../text/i18n';
import { num, relative } from '../../text/format';
import { confirm } from '../../state/confirm';
import { advanced } from '../../state/prefs';
import { errorText, save, useFieldErrors } from '../../state/save';
import { live, setLive, useLive } from '../../state/store';
import { hubTime, useNow } from '../../state/system';
import { toast } from '../../state/toasts';
import { Button, IconButton } from '../../kit/Button';
import { SignalBars } from '../../kit/Bits';
import { Dialog } from '../../kit/Dialog';
import { Banner, EmptyState, Loading } from '../../kit/Feedback';
import { Checkbox, SelectField, Switch, TextField } from '../../kit/Field';
import { Facts, Page, Section } from '../../kit/Page';
import { Status } from '../../kit/Status';
import { checkIp, cleanIp, IpFields } from './IpFields';
import c from '../common.module.css';

const SECURITY: WifiConnect['security'][] = ['auto', 'none', 'WPA', 'WPA2', 'WPA3', 'WPA2WPA3', 'WPAMixed'];

/** The Wi-Fi band a channel belongs to (2.4, 5 or 6 GHz), not the channel's own frequency. */
function band(mhz: number | null | undefined) {
  if (!mhz) return null;
  const ghz = mhz < 3000 ? 2.4 : mhz < 5925 ? 5 : 6;
  return t('wifi.band', { ghz: num(ghz, ghz % 1 ? 1 : 0) });
}

function ConnectDialog({ target, onClose }: { target: { ssid: string; security: string; saved: boolean } | 'hidden' | null; onClose: () => void }) {
  const hidden = target === 'hidden';
  const net = target && target !== 'hidden' ? target : null;
  const open = net ? net.security !== 'none' && !net.saved : true;
  const [ssid, setSsid] = useState('');
  const [password, setPassword] = useState('');
  const [security, setSecurity] = useState<NonNullable<WifiConnect['security']>>('auto');
  const [staticIp, setStaticIp] = useState(false);
  const [ip, setIp] = useState<IpSettings>({ method: 'manual', addresses: [''], gateway: '', dns: [''] });
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  useEffect(() => {
    setSsid('');
    setPassword('');
    setSecurity('auto');
    setStaticIp(false);
  }, [target]);
  const ipErrors = staticIp ? checkIp(ip) : {};
  const name = net?.ssid ?? ssid;
  const submit = async () => {
    if (!name.trim() || Object.keys(ipErrors).length) return;
    setBusy(true);
    const body: WifiConnect = { ssid: name.trim(), password: password || null, security, hidden };
    if (staticIp) body.ip = cleanIp(ip);
    const answer = await save<WifiState, WifiState>({ target: live('wifi'), send: () => api.post('/wifi/connect', { body }), errors, success: t('wifi.connected', { ssid: name }) });
    setBusy(false);
    if (answer) onClose();
  };
  return (
    <Dialog
      open={!!target}
      onClose={onClose}
      title={hidden ? t('wifi.hiddenTitle') : t('wifi.connectTo', { ssid: name })}
      onSubmit={submit}
      footer={
        <>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!name.trim()}>
            {t('wifi.connect')}
          </Button>
        </>
      }
    >
      {hidden && <TextField label={t('wifi.ssid')} value={ssid} onValue={setSsid} required maxLength={32} error={errors.value.ssid} autoFocus />}
      {/* a network picked from the list has no name field to show this at */}
      {!hidden && errors.value.ssid && (
        <Banner tone="danger" live>
          {errors.value.ssid}
        </Banner>
      )}
      {(open || hidden) && (
        <TextField label={t('wifi.password')} type="password" value={password} onValue={setPassword} maxLength={63} autoComplete="off" error={errors.value.password} autoFocus={!hidden} hint={hidden ? t('wifi.passwordOptional') : undefined} />
      )}
      {(hidden || advanced.value) && (
        <SelectField label={t('wifi.security')} value={security} onValue={setSecurity} options={SECURITY.map((x) => ({ value: x!, label: x === 'auto' ? t('wifi.securityAuto') : x === 'none' ? t('wifi.securityNone') : x! }))} />
      )}
      {advanced.value && (
        <>
          <Checkbox label={t('wifi.staticIp')} checked={staticIp} onChange={setStaticIp} hint={t('wifi.staticIpHint')} />
          {staticIp && <IpFields value={ip} onChange={setIp} errors={{ ...ipErrors, ...errors.value }} />}
        </>
      )}
    </Dialog>
  );
}

export default function Wifi() {
  const w = useLive('wifi');
  const now = useNow('minute');
  const [target, setTarget] = useState<{ ssid: string; security: string; saved: boolean } | 'hidden' | null>(null);
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState<string>();

  // A scan ends when new results arrive (scanned_at changes), or after a while.
  const scannedAt = w?.scanned_at;
  useEffect(() => setScanning(false), [scannedAt]);
  useEffect(() => {
    if (!scanning) return;
    const id = setTimeout(() => setScanning(false), 20_000);
    return () => clearTimeout(id);
  }, [scanning]);

  const scan = async () => {
    setScanning(true);
    try {
      await api.post('/wifi/scan');
    } catch (e) {
      setScanning(false);
      toast({ text: errorText(e), tone: 'danger' });
    }
  };
  const toggle = (enabled: boolean): Promise<WifiState | undefined> =>
    save<WifiState>({ target: live('wifi'), optimistic: (x) => ({ ...x, enabled }), send: () => api.patch('/wifi', { body: { enabled } }), success: enabled ? t('wifi.turnedOn') : t('wifi.turnedOff'), undo: () => toggle(!enabled) });
  const disconnect = async () => {
    setBusy('disconnect');
    try {
      setLive('wifi', await api.post('/wifi/disconnect'));
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setBusy(undefined);
  };
  const forget = async (name: string, ssid: string) => {
    if (!(await confirm({ title: t('wifi.forgetTitle', { ssid }), body: t('wifi.forgetBody'), confirm: t('wifi.forget'), tone: 'danger' }))) return;
    try {
      await api.del('/wifi/saved', { query: { name } });
      const cur = live('wifi').peek();
      if (cur) setLive('wifi', { ...cur, saved: cur.saved.filter((x) => x.name !== name) });
      toast({ text: t('wifi.forgotten', { ssid }), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };

  const networks: WifiNetwork[] = [...(w?.networks ?? [])].sort((a, b) => Number(b.in_use) - Number(a.in_use) || b.signal - a.signal);

  return (
    <Page title={t('nav.wifi')} back={{ href: '/network', label: t('nav.network') }} subtitle={t('nav.wifiDesc')}>
      {!w ? (
        <Loading lines={5} />
      ) : !w.available ? (
        <EmptyState icon={WifiOff} title={t('wifi.unavailable')} />
      ) : (
        <>
          <Section>
            <Switch label={t('wifi.radio')} description={t('wifi.radioDesc')} checked={w.enabled} onChange={(v) => void toggle(v)} />
          </Section>
          {w.enabled && (
            <>
              <Section
                title={t('wifi.current')}
                icon={WifiIcon}
                actions={w.current ? <Status tone="ok">{t('net.state.connected')}</Status> : <Status tone={w.state === 'connecting' ? 'busy' : 'neutral'}>{t(`net.state.${w.state}`)}</Status>}
              >
                {w.current ? (
                  <>
                    <div class={c.between}>
                      <span class={c.inline}>
                        <SignalBars value={w.current.signal} />
                        <strong>{w.current.ssid}</strong>
                      </span>
                      <Button icon={Unplug} busy={busy === 'disconnect'} onClick={() => void disconnect()}>
                        {t('wifi.disconnect')}
                      </Button>
                    </div>
                    <Facts
                      items={[
                        [t('net.address'), w.current.addresses?.length ? <span class="mono">{w.current.addresses.map((a) => a.split('/')[0]).join(', ')}</span> : null],
                        [t('wifi.security'), w.current.security],
                        [t('wifi.frequency'), band(w.current.frequency_mhz)],
                      ]}
                    />
                  </>
                ) : (
                  <p class="muted">{t('wifi.notConnected')}</p>
                )}
              </Section>
              <Section
                id="networks"
                title={t('wifi.networks')}
                description={w.scanned_at ? t('wifi.scanned', { when: relative(w.scanned_at, hubTime(now)) }) : undefined}
                actions={
                  <>
                    <Button size="sm" icon={Plus} onClick={() => setTarget('hidden')}>
                      {t('wifi.hidden')}
                    </Button>
                    <Button size="sm" variant="soft" icon={RefreshCw} busy={scanning} onClick={() => void scan()}>
                      {scanning ? t('wifi.scanning') : t('wifi.scan')}
                    </Button>
                  </>
                }
              >
                {networks.length ? (
                  <ul class={c.list}>
                    {networks.map((n) => (
                      <li key={n.ssid} class={c.item}>
                        <SignalBars value={n.signal} />
                        <div class={c.itemMain}>
                          <span class={c.itemTitle}>
                            {n.ssid || t('wifi.noName')}
                            {n.in_use && <span class={`${c.tag} ${c.tagAccent}`}>{t('wifi.inUse')}</span>}
                            {n.saved && !n.in_use && <span class={c.tag}>{t('wifi.saved')}</span>}
                          </span>
                          <span class={c.itemMeta}>
                            {[n.security === 'none' ? t('wifi.open') : n.security, band(n.frequency_mhz)].filter(Boolean).join(' · ')}
                          </span>
                        </div>
                        {n.security !== 'none' && <Lock size={16} class={c.itemIcon} aria-label={t('wifi.secured')} />}
                        {!n.in_use && (
                          <Button size="sm" onClick={() => setTarget({ ssid: n.ssid, security: n.security, saved: n.saved })}>
                            {t('wifi.connect')}
                          </Button>
                        )}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p class="muted">{scanning ? t('wifi.scanning') : t('wifi.noNetworks')}</p>
                )}
              </Section>
              {w.saved.length > 0 && (
                <Section title={t('wifi.savedNetworks')} description={t('wifi.savedDesc')}>
                  <ul class={c.list}>
                    {w.saved.map((sv) => (
                      <li key={sv.name} class={c.item}>
                        <WifiIcon size={18} class={c.itemIcon} aria-hidden="true" />
                        <div class={c.itemMain}>
                          <span class={c.itemTitle}>{sv.ssid}</span>
                          <span class={c.itemMeta}>{sv.autoconnect ? t('wifi.autoconnect') : t('wifi.manualConnect')}</span>
                        </div>
                        <IconButton icon={Trash} label={t('wifi.forgetNamed', { ssid: sv.ssid })} onClick={() => void forget(sv.name, sv.ssid)} />
                      </li>
                    ))}
                  </ul>
                </Section>
              )}
            </>
          )}
          {!w.enabled && <Banner tone="info">{t('wifi.offText')}</Banner>}
        </>
      )}
      <ConnectDialog target={target} onClose={() => setTarget(null)} />
    </Page>
  );
}
