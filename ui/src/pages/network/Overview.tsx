// Network overview: internet, the hub's address, each kind of connection.
import { Link } from 'wouter-preact';
import type { LucideIcon } from 'lucide-preact';
import { Cable, ChevronRight, Globe, Network as NetworkIcon, Shield, Smartphone, Usb, Wifi } from 'lucide-preact';
import type { Interface } from '../../api/types';
import { t } from '../../text/i18n';
import { mbps } from '../../text/format';
import { advanced } from '../../state/prefs';
import { useLive } from '../../state/store';
import { features } from '../../state/system';
import { CopyValue } from '../../kit/Bits';
import { Card } from '../../kit/Card';
import { Loading } from '../../kit/Feedback';
import { Facts, Page } from '../../kit/Page';
import { Status, type StatusTone } from '../../kit/Status';
import c from '../common.module.css';

export const KIND_ICON: Record<Interface['kind'], LucideIcon> = { ethernet: Cable, wifi: Wifi, usb: Usb, vpn: Shield, lte: Smartphone, other: NetworkIcon };

export function ifaceTone(state: Interface['state']): StatusTone {
  return state === 'connected' ? 'ok' : state === 'connecting' ? 'busy' : state === 'disconnected' ? 'neutral' : 'neutral';
}

export function IfaceStatus({ state }: { state: Interface['state'] }) {
  return <Status tone={ifaceTone(state)}>{t(`net.state.${state}`)}</Status>;
}

export function ifaceFacts(i: Interface, adv: boolean): [string, preact.ComponentChildren][] {
  return [
    [t('net.address'), i.addresses.length ? <span class="mono">{(adv ? i.addresses : i.addresses.filter((a) => !a.startsWith('fe80')).map((a) => a.split('/')[0])).join(', ')}</span> : null],
    [t('net.gateway'), i.gateway && <span class="mono">{i.gateway}</span>],
    [t('net.dns'), i.dns?.length ? <span class="mono">{i.dns.join(', ')}</span> : null],
    [t('net.speed'), i.speed_mbps ? mbps(i.speed_mbps) : null],
    [t('net.mac'), adv && i.mac && <span class="mono">{i.mac}</span>],
  ];
}

function LinkTile({ href, icon: I, title, children }: { href: string; icon: LucideIcon; title: string; children: preact.ComponentChildren }) {
  return (
    <Link href={href} class={c.linkCard}>
      <span class={c.linkIcon} aria-hidden="true">
        <I size={22} />
      </span>
      <span class={c.linkText}>
        <span class={c.linkTitle}>{title}</span>
        <span class={c.linkDesc}>{children}</span>
      </span>
      <ChevronRight size={20} class={c.chevron} aria-hidden="true" />
    </Link>
  );
}

export default function Overview() {
  const n = useLive('network');
  const wifi = useLive('wifi');
  const vpn = useLive('vpn');
  const f = features.value;
  const adv = advanced.value;
  const eth = n?.interfaces.find((i) => i.kind === 'ethernet');
  const ip = (i?: Interface) => i?.addresses.find((a) => !a.includes(':'))?.split('/')[0];
  return (
    <Page title={t('nav.network')} subtitle={t('nav.networkDesc')}>
      {!n ? (
        <Loading lines={4} />
      ) : (
        <>
          <Card title={t('net.internet')} icon={Globe}>
            <div class={c.between}>
              {n.internet === false ? <Status tone="warn">{t('net.noInternet')}</Status> : n.internet ? <Status tone="ok">{t('net.internetOk')}</Status> : <Status tone="neutral">{t('net.internetUnknown')}</Status>}
              <span class={c.inline}>
                <span class="muted">{t('net.reachAt')}</span>
                <CopyValue value={`${n.hostname.toLowerCase()}.local`} label={t('common.copy')} />
              </span>
            </div>
          </Card>
          <div class={c.grid}>
            <LinkTile href="/network/ethernet" icon={Cable} title={t('nav.ethernet')}>
              {eth?.state === 'connected' ? t('net.connectedAt', { address: ip(eth) ?? '–' }) : t(`net.state.${eth?.state ?? 'unavailable'}`)}
            </LinkTile>
            {f?.wifi && (
              <LinkTile href="/network/wifi" icon={Wifi} title={t('nav.wifi')}>
                {!wifi ? '…' : !wifi.enabled ? t('wifi.off') : wifi.current ? t('wifi.connectedTo', { ssid: wifi.current.ssid }) : t('wifi.notConnected')}
              </LinkTile>
            )}
            <LinkTile href="/network/vpn" icon={Shield} title={t('nav.vpn')}>
              {!vpn ? '…' : t(`vpn.state.${vpn.state}`)}
            </LinkTile>
            {f?.lte && (
              <LinkTile href="/network/lte" icon={Smartphone} title={t('nav.lte')}>
                {t('nav.lteDesc')}
              </LinkTile>
            )}
          </div>
          <h2 class={c.big}>{t('net.interfaces')}</h2>
          <div class={c.grid}>
            {n.interfaces
              .filter((i) => adv || i.state === 'connected')
              .map((i) => (
                <Card key={i.name} title={i.connection && i.connection !== i.name ? `${i.connection} (${i.name})` : i.name} icon={KIND_ICON[i.kind]} action={<IfaceStatus state={i.state} />} headingLevel={3}>
                  <Facts items={ifaceFacts(i, adv)} />
                </Card>
              ))}
          </div>
        </>
      )}
    </Page>
  );
}
