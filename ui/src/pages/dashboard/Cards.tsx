// The dashboard's cards. Each names the live resources it shows; values
// change as the hub pushes them.
import { useState } from 'preact/hooks';
import { Link } from 'wouter-preact';
import type { LucideIcon } from 'lucide-preact';
import { Cable, ExternalLink, Globe, Network as NetworkIcon, Play, Send, Smartphone, Square, Usb, Wifi } from 'lucide-preact';
import { sendCode } from '../../state/hardware';
import { Button } from '../../kit/Button';
import type { Interface } from '../../api/types';
import { t } from '../../text/i18n';
import { bytes, duration, mbps, num, percent } from '../../text/format';
import { advanced } from '../../state/prefs';
import { appCommand, appWebUrl } from '../../state/apps';
import { cpuHistory, memoryHistory } from '../../state/metrics';
import { useLive } from '../../state/store';
import { features, system, uptimeAt, useNow } from '../../state/system';
import { IconButton, LinkButton, Transition, transitionOf } from '../../kit/Button';
import { AppIcon, appTitle } from '../../kit/Bits';
import { EmptyState, Meter, Skeleton } from '../../kit/Feedback';
import { Sparkline, Tween } from '../../kit/Live';
import { Dot, ServiceStatus, Status } from '../../kit/Status';
import { Facts } from '../../kit/Page';
import s from './Dashboard.module.css';

export function PerformanceCard() {
  const m = useLive('metrics');
  if (!m) return <Skeleton lines={4} />;
  const memPct = m.memory.total ? (m.memory.used / m.memory.total) * 100 : 0;
  const swapPct = m.swap.total ? (m.swap.used / m.swap.total) * 100 : 0;
  return (
    <div class={s.perf}>
      <div class={s.metric}>
        <div class={s.metricHead}>
          <span class={s.metricLabel}>{t('dash.cpu')}</span>
          <Tween class={s.metricValue} value={m.cpu_percent} format={(v) => percent(v)} />
        </div>
        <Sparkline values={cpuHistory.value} />
      </div>
      <div class={s.metric}>
        <div class={s.metricHead}>
          <span class={s.metricLabel}>{t('dash.memory')}</span>
          <Tween class={s.metricValue} value={memPct} format={(v) => percent(v)} />
        </div>
        <Sparkline values={memoryHistory.value} tone={2} />
        <p class={s.metricNote}>{t('dash.usedOf', { used: bytes(m.memory.used), total: bytes(m.memory.total) })}</p>
      </div>
      {advanced.value && (
        <Facts
          items={[
            [t('dash.load'), <span class="num">{m.load.map((l) => num(l, 2)).join(' · ')}</span>],
            [t('dash.swap'), m.swap.total ? <span class="num">{`${percent(swapPct)} · ${bytes(m.swap.used)}`}</span> : null],
          ]}
        />
      )}
    </div>
  );
}

export function StorageCard() {
  const m = useLive('metrics');
  if (!m) return <Skeleton lines={3} />;
  // A model with a slot says so when the slot is empty, instead of leaving the line out.
  const noCard = !!features.value?.sd_slot && !m.storage.some((st) => st.id === 'sdcard');
  if (!m.storage.length && !noCard) return <p class="muted">{t('dash.noStorage')}</p>;
  return (
    <ul class={s.storage}>
      {m.storage.map((st) => {
        const pct = st.total ? (st.used / st.total) * 100 : 0;
        const label = st.id === 'sdcard' ? t('dash.sdcard') : t('dash.userStorage');
        return (
          <li key={st.id}>
            <div class={s.metricHead}>
              <span class={s.metricLabel}>{label}</span>
              <span class={s.storageFree}>{t('dash.free', { free: bytes(st.total - st.used) })}</span>
            </div>
            <Meter value={pct} label={label} valueText={t('dash.usedOf', { used: bytes(st.used), total: bytes(st.total) })} />
            <p class={s.metricNote}>
              {t('dash.usedOf', { used: bytes(st.used), total: bytes(st.total) })}
              {advanced.value && <span class="mono"> · {st.path}</span>}
            </p>
          </li>
        );
      })}
      {noCard && (
        <li>
          <div class={s.metricHead}>
            <span class={s.metricLabel}>{t('dash.sdcard')}</span>
            <span class={s.storageFree}>{t('dash.sdNone')}</span>
          </div>
        </li>
      )}
    </ul>
  );
}

/** Favourite infrared codes as buttons (full-size SMHUB). */
export function RemoteCard() {
  const ir = useLive('ir');
  if (!ir) return <Skeleton lines={2} />;
  const fav = ir.codes.filter((c) => c.favourite);
  if (!fav.length) {
    return (
      <p class="muted">
        {t('dash.remoteEmpty')} <Link href="/peripherals/infrared">{t('dash.remoteSetUp')}</Link>
      </p>
    );
  }
  return (
    <div class={s.remote}>
      {fav.map((c) => (
        <Button key={c.id} size="sm" icon={Send} onClick={() => void sendCode(c)}>
          {c.device ? `${c.device} · ${c.name}` : c.name}
        </Button>
      ))}
    </div>
  );
}

const KIND_ICON: Record<Interface['kind'], LucideIcon> = { ethernet: Cable, wifi: Wifi, usb: Usb, vpn: Globe, lte: Smartphone, other: NetworkIcon };

export function NetworkCard() {
  const n = useLive('network');
  if (!n) return <Skeleton lines={3} />;
  const up = n.interfaces.filter((i) => i.state === 'connected' && (advanced.value || i.kind !== 'other'));
  return (
    <div class={s.net}>
      <div class={s.internet}>
        {n.internet === false ? (
          <Status tone="warn">{t('net.noInternet')}</Status>
        ) : n.internet ? (
          <Status tone="ok">{t('net.internetOk')}</Status>
        ) : (
          <Status tone="neutral">{t('net.internetUnknown')}</Status>
        )}
        <span class="mono muted">{n.hostname}.local</span>
      </div>
      {up.length ? (
        <ul class={s.ifaces}>
          {up.map((i) => {
            const I = KIND_ICON[i.kind];
            return (
              <li key={i.name} class={s.iface}>
                <I size={18} aria-hidden="true" class={s.ifaceIcon} />
                <div class={s.ifaceText}>
                  <span class={s.ifaceName}>{i.connection ?? t(`net.kind.${i.kind}`)}</span>
                  <span class="mono muted">{i.addresses[0]?.split('/')[0] ?? '–'}</span>
                </div>
                {i.speed_mbps ? <span class="muted num">{mbps(i.speed_mbps)}</span> : null}
              </li>
            );
          })}
        </ul>
      ) : (
        <p class="muted">{t('net.noneConnected')}</p>
      )}
    </div>
  );
}

function AppRow({ name }: { name: string }) {
  const apps = useLive('apps');
  const [busy, setBusy] = useState(false);
  const a = apps?.apps.find((x) => x.name === name);
  if (!a) return null;
  const url = a.web ? (a.sidebar ? `/open/${a.name}` : appWebUrl(a)) : null;
  const running = a.state === 'started' || a.state === 'starting';
  const run = async (action: 'start' | 'stop') => {
    setBusy(true);
    await appCommand(a.name, action);
    setBusy(false);
  };
  return (
    <li class={s.appRow}>
      <Link href={`/apps/${a.name}`} class={s.appLink}>
        <AppIcon name={a.name} src={a.icon} size={36} />
        <span class={s.appText}>
          <span class={s.appName}>{appTitle(a.name)}</span>
          {a.is_service ? <ServiceStatus state={a.state} size="sm" /> : <span class="muted">{a.installed_version}</span>}
        </span>
      </Link>
      <span class={s.appActions}>
        {url && running && (
          <LinkButton
            href={url}
            external={!a.sidebar}
            size="sm"
            variant="soft"
            icon={ExternalLink}
            label={t('apps.openNamed', { app: appTitle(a.name) })}
          />
        )}
        {a.is_service &&
          (transitionOf(a.state) ? (
            <Transition compact state={transitionOf(a.state)!} name={appTitle(a.name)} />
          ) : running ? (
            <IconButton size="sm" icon={Square} label={t('apps.stopNamed', { app: appTitle(a.name) })} busy={busy} onClick={() => void run('stop')} />
          ) : (
            <IconButton size="sm" icon={Play} label={t('apps.startNamed', { app: appTitle(a.name) })} busy={busy} onClick={() => void run('start')} />
          ))}
      </span>
    </li>
  );
}

export function AppsCard() {
  const apps = useLive('apps');
  if (!apps) return <Skeleton lines={4} />;
  const installed = apps.apps.filter((a) => a.installed && !a.system);
  if (!installed.length)
    return (
      <EmptyState title={t('dash.noApps')} action={<LinkButton href="/apps" variant="primary">{t('dash.browseApps')}</LinkButton>}>
        {t('dash.noAppsText')}
      </EmptyState>
    );
  const shown = installed.slice(0, 6);
  return (
    <>
      <ul class={s.apps}>
        {shown.map((a) => (
          <AppRow key={a.name} name={a.name} />
        ))}
      </ul>
      {installed.length > shown.length && (
        <Link href="/apps" class={s.more}>
          {t('dash.allApps', { count: installed.length })}
        </Link>
      )}
    </>
  );
}

export function RadiosCard() {
  const r = useLive('radios');
  const sys = system.value;
  if (!r) return <Skeleton lines={3} />;
  return (
    <ul class={s.radios}>
      {r.radios.map((radio) => (
        <li key={radio.index} class={s.radio}>
          <div class={s.radioHead}>
            <span class={s.radioChip}>{radio.chip}</span>
            {radio.in_use_by ? <Dot tone="ok" label={t('radio.usedBy', { app: appTitle(radio.in_use_by) })} /> : <Dot tone="neutral" label={t('radio.free')} />}
          </div>
          <p class="muted">
            {radio.firmware?.mode ? t(`radio.mode.${radio.firmware.mode}`) : t('radio.unknownFirmware')}
            {radio.firmware?.version && <span class="num"> · {radio.firmware.version}</span>}
          </p>
        </li>
      ))}
      {!r.radios.length && sys && <p class="muted">{t('radio.none')}</p>}
    </ul>
  );
}

export function SystemCard() {
  const sys = system.value;
  const now = useNow('minute');
  if (!sys) return <Skeleton lines={4} />;
  const up = uptimeAt(now);
  return (
    <Facts
      items={[
        [t('sys.model'), sys.product],
        [t('sys.hostname'), <span class="mono">{sys.hostname}</span>],
        [t('sys.os'), <span class="num">{sys.os_version}</span>],
        [t('sys.core'), advanced.value && <span class="num">{sys.core_version}</span>],
        [t('sys.uptime'), up !== undefined ? duration(up, 'long') : null],
        [t('sys.timezone'), sys.timezone],
        [t('sys.hardware'), advanced.value && sys.hardware_revision],
      ]}
    />
  );
}
