// The dashboard: a summary of the hub's health, then cards the user can
// arrange (drag and drop, or up/down buttons) and hide. The layout is the
// `dashboard` preference.
import type { ComponentChildren } from 'preact';
import { useRef, useState } from 'preact/hooks';
import { Link } from 'wouter-preact';
import type { LucideIcon } from 'lucide-preact';
import {
  Activity,
  ArrowDown,
  ArrowUp,
  Boxes,
  CircleCheck,
  CircleX,
  Cpu,
  Eye,
  EyeOff,
  GripVertical,
  HardDrive,
  LayoutGrid,
  Network as NetworkIcon,
  Radio,
  TriangleExclamationPoint,
  Tv,
} from 'lucide-preact';
import { t } from '../../text/i18n';
import { longDay, percent, time } from '../../text/format';
import { appTitle } from '../../text/names';
import { savePrefs, effectivePrefs, prefs } from '../../state/prefs';
import { useLive } from '../../state/store';
import { hubTime, system, useNow } from '../../state/system';
import { runningOps, operationTitle } from '../../state/operations';
import { Button, IconButton } from '../../kit/Button';
import { Card } from '../../kit/Card';
import { cx } from '../../kit/cx';
import { useDocumentTitle, useHeadingFocus } from '../../kit/Page';
import { AppsCard, NetworkCard, PerformanceCard, RadiosCard, RemoteCard, StorageCard, SystemCard } from './Cards';
import s from './Dashboard.module.css';

export const CARD_IDS = ['performance', 'apps', 'network', 'storage', 'radios', 'system', 'remote'] as const;
type CardId = (typeof CARD_IDS)[number];

/** The default layout, saved when the first-run setup ends. */
export const DEFAULT_DASHBOARD: string[] = [...CARD_IDS];

const HIDDEN = 'hidden:';

function readLayout(pref: string[]): { order: CardId[]; hidden: Set<CardId> } {
  const order: CardId[] = [];
  const hidden = new Set<CardId>();
  for (const raw of pref) {
    const isHidden = raw.startsWith(HIDDEN);
    const id = (isHidden ? raw.slice(HIDDEN.length) : raw) as CardId;
    if (!CARD_IDS.includes(id) || order.includes(id)) continue;
    order.push(id);
    if (isHidden) hidden.add(id);
  }
  // Cards added in a later version appear at the end.
  for (const id of CARD_IDS) if (!order.includes(id)) order.push(id);
  return { order, hidden };
}

function writeLayout(order: CardId[], hidden: Set<CardId>): string[] {
  return order.map((id) => (hidden.has(id) ? HIDDEN + id : id));
}

const META: Record<CardId, { title: () => string; icon: LucideIcon; href?: string }> = {
  performance: { title: () => t('dash.performance'), icon: Activity },
  apps: { title: () => t('dash.apps'), icon: Boxes, href: '/apps' },
  network: { title: () => t('dash.network'), icon: NetworkIcon, href: '/network' },
  storage: { title: () => t('dash.storage'), icon: HardDrive },
  radios: { title: () => t('dash.radios'), icon: Radio, href: '/radios' },
  system: { title: () => t('dash.system'), icon: Cpu, href: '/settings/system' },
  remote: { title: () => t('dash.remote'), icon: Tv, href: '/peripherals/infrared' },
};

interface Attention {
  tone: 'warn' | 'danger' | 'info';
  text: string;
  href: string;
}

function useAttention(): Attention[] {
  const services = useLive('services');
  const updates = useLive('updates');
  const metrics = useLive('metrics');
  const network = useLive('network');
  const apps = useLive('apps');
  const out: Attention[] = [];
  for (const sv of services?.services ?? []) {
    if (sv.state !== 'crashed') continue;
    const name = sv.app ?? sv.name;
    out.push({ tone: 'danger', text: t('hero.crashed', { name: appName(name) }), href: sv.app ? `/apps/${sv.app}` : '/tools/services' });
  }
  if (network && network.internet === false) out.push({ tone: 'warn', text: t('hero.noInternet'), href: '/network' });
  for (const st of metrics?.storage ?? []) {
    const used = st.total ? (st.used / st.total) * 100 : 0;
    if (used >= 90) out.push({ tone: used >= 97 ? 'danger' : 'warn', text: t(st.id === 'sdcard' ? 'hero.sdFull' : 'hero.storageFull', { percent: percent(used) }), href: '/settings/backups' });
  }
  if (updates?.os.update_available) {
    const v = updates.os.releases.find((r) => r.newer)?.version;
    out.push({ tone: 'info', text: v ? t('hero.osUpdate', { version: v }) : t('hero.osUpdateAny'), href: '/updates' });
  }
  const appUpdates = apps?.updates ?? updates?.apps.updates ?? 0;
  if (appUpdates > 0) out.push({ tone: 'info', text: t('hero.appUpdates', { count: appUpdates }), href: '/apps' });
  return out;
}

const appName = appTitle;

function greeting(hour: number) {
  if (hour < 5) return t('hero.evening');
  if (hour < 12) return t('hero.morning');
  if (hour < 18) return t('hero.afternoon');
  return t('hero.evening');
}

function hourIn(ms: number, tz: string | undefined) {
  try {
    return Number(new Intl.DateTimeFormat('en-GB', { hour: 'numeric', hourCycle: 'h23', timeZone: tz }).format(ms));
  } catch {
    return new Date(ms).getHours();
  }
}

function Hero() {
  const sys = system.value;
  const items = useAttention();
  const ops = runningOps.value;
  const now = hubTime(useNow('minute'));
  const known = !!sys;
  const worst = items.some((i) => i.tone === 'danger') ? 'danger' : items.some((i) => i.tone === 'warn') ? 'warn' : items.length ? 'info' : 'ok';
  const Icon = worst === 'danger' ? CircleX : worst === 'warn' ? TriangleExclamationPoint : CircleCheck;
  const needs = items.filter((i) => i.tone !== 'info').length;
  const title = !known ? t('hero.loading') : needs ? t('hero.attention', { count: needs }) : t('hero.allGood');
  const tz = effectivePrefs.value.timezone === 'browser' ? undefined : sys?.timezone;
  const h1 = useRef<HTMLHeadingElement>(null);
  useDocumentTitle(t('nav.home'));
  useHeadingFocus(h1);
  return (
    <section class={cx(s.hero, s[`hero_${worst}`])} aria-labelledby="hero-title">
      <div class={s.heroMain}>
        <span class={cx(s.heroIcon, s[`icon_${worst}`])} aria-hidden="true">
          <Icon size={30} />
        </span>
        <div class={s.heroText}>
          <p class={s.greeting}>{greeting(hourIn(now, tz))}</p>
          <h1 id="hero-title" ref={h1} class={s.heroTitle} tabIndex={-1}>
            {title}
          </h1>
          {!needs && known && <p class={s.heroSub}>{items.length ? t('hero.allGoodWithNews') : t('hero.allGoodText')}</p>}
        </div>
        <div class={s.clock} aria-label={t('hero.hubTime')}>
          <span class={cx(s.clockTime, 'num')}>{time(now)}</span>
          <span class={cx(s.clockDate, 'num')}>{longDay(now)}</span>
        </div>
      </div>
      {(items.length > 0 || ops.length > 0) && (
        <ul class={s.attention}>
          {ops.map((o) => (
            <li key={o.id}>
              <span class={cx(s.chip, s.chip_info)}>
                <span class={s.chipSpinner} aria-hidden="true" />
                {operationTitle(o)}
                {o.progress !== null && o.progress !== undefined && <span class="num"> · {percent(o.progress)}</span>}
              </span>
            </li>
          ))}
          {items.map((i, n) => (
            <li key={n}>
              <Link href={i.href} class={cx(s.chip, s[`chip_${i.tone}`])}>
                {i.tone === 'danger' ? <CircleX size={16} aria-hidden="true" /> : i.tone === 'warn' ? <TriangleExclamationPoint size={16} aria-hidden="true" /> : <ArrowUp size={16} aria-hidden="true" />}
                {i.text}
              </Link>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function CardFrame({
  id,
  arranging,
  index,
  count,
  hidden,
  onMove,
  onToggle,
  drag,
  children,
}: {
  id: CardId;
  arranging: boolean;
  index: number;
  count: number;
  hidden: boolean;
  onMove: (from: number, to: number, fromId?: CardId) => void;
  onToggle: () => void;
  drag: { over: CardId | null; setOver: (c: CardId | null) => void; dragging: CardId | null; setDragging: (c: CardId | null) => void };
  children: ComponentChildren;
}) {
  const meta = META[id];
  const title = meta.title();
  const tools = arranging && (
    <div class={s.arrangeTools}>
      <IconButton size="sm" icon={ArrowUp} label={t('dash.moveUp', { card: title })} disabled={index === 0} onClick={() => onMove(index, index - 1)} />
      <IconButton size="sm" icon={ArrowDown} label={t('dash.moveDown', { card: title })} disabled={index === count - 1} onClick={() => onMove(index, index + 1)} />
      <IconButton size="sm" icon={hidden ? Eye : EyeOff} label={hidden ? t('dash.show', { card: title }) : t('dash.hide', { card: title })} onClick={onToggle} />
    </div>
  );
  return (
    <div
      class={cx(s.cell, arranging && s.arranging, hidden && s.hiddenCard, drag.over === id && drag.dragging !== id && s.dropTarget, drag.dragging === id && s.dragging)}
      draggable={arranging}
      onDragStart={(e) => {
        e.dataTransfer?.setData('text/plain', id);
        if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
        drag.setDragging(id);
      }}
      onDragEnd={() => {
        drag.setDragging(null);
        drag.setOver(null);
      }}
      onDragOver={(e) => {
        if (!arranging) return;
        e.preventDefault();
        if (drag.over !== id) drag.setOver(id);
      }}
      onDrop={(e) => {
        e.preventDefault();
        const from = e.dataTransfer?.getData('text/plain') as CardId | undefined;
        drag.setOver(null);
        drag.setDragging(null);
        if (from && from !== id) onMove(-1, index, from);
      }}
    >
      <Card
        title={
          meta.href && !arranging ? (
            <Link href={meta.href} class={s.cardLink}>
              {title}
            </Link>
          ) : (
            title
          )
        }
        icon={meta.icon}
        lead={arranging && <GripVertical size={18} class={s.grip} aria-hidden="true" />}
        action={tools}
      >
        {hidden ? <p class={s.hiddenNote}>{t('dash.hiddenNote')}</p> : children}
      </Card>
    </div>
  );
}

export function Dashboard() {
  const sys = system.value;
  const p = prefs.value;
  const layout = readLayout(p?.dashboard ?? DEFAULT_DASHBOARD);
  const [arranging, setArranging] = useState(false);
  const [over, setOver] = useState<CardId | null>(null);
  const [dragging, setDragging] = useState<CardId | null>(null);
  const hasRadios = (sys?.radios.length ?? 0) > 0;
  const hasIr = !!sys?.features.ir;
  const cards = layout.order.filter((id) => (id !== 'radios' || hasRadios) && (id !== 'remote' || hasIr));
  const shown = arranging ? cards : cards.filter((id) => !layout.hidden.has(id));

  const persist = (order: CardId[], hidden: Set<CardId>) => {
    // Cards this hub does not show keep their place in the saved order.
    const full = [...order, ...layout.order.filter((id) => !order.includes(id))];
    void savePrefs({ dashboard: writeLayout(full, hidden) });
  };
  const move = (from: number, to: number, fromId?: CardId) => {
    const order = [...cards];
    const i = fromId ? order.indexOf(fromId) : from;
    if (i < 0 || to < 0 || to >= order.length) return;
    const [card] = order.splice(i, 1);
    order.splice(to, 0, card!);
    persist(order, layout.hidden);
  };
  const toggle = (id: CardId) => {
    const hidden = new Set(layout.hidden);
    if (hidden.has(id)) hidden.delete(id);
    else hidden.add(id);
    persist(cards, hidden);
  };

  const body = (id: CardId) => {
    switch (id) {
      case 'performance':
        return <PerformanceCard />;
      case 'apps':
        return <AppsCard />;
      case 'network':
        return <NetworkCard />;
      case 'storage':
        return <StorageCard />;
      case 'radios':
        return <RadiosCard />;
      case 'system':
        return <SystemCard />;
      case 'remote':
        return <RemoteCard />;
    }
  };

  return (
    <div class={s.page}>
      <Hero />
      <div class={s.toolbar}>
        <h2 class="visually-hidden">{t('dash.cards')}</h2>
        {arranging && <p class={s.arrangeHelp}>{t('dash.arrangeHelp')}</p>}
        <Button size="sm" variant={arranging ? 'primary' : 'ghost'} icon={LayoutGrid} onClick={() => setArranging(!arranging)} aria-pressed={arranging}>
          {arranging ? t('common.done') : t('dash.arrange')}
        </Button>
      </div>
      <div class={s.grid}>
        {shown.map((id, i) => (
          <CardFrame
            key={id}
            id={id}
            arranging={arranging}
            index={i}
            count={shown.length}
            hidden={layout.hidden.has(id)}
            onMove={(from, to, fromId?: CardId) => move(from, to, fromId)}
            onToggle={() => toggle(id)}
            drag={{ over, setOver, dragging, setDragging }}
          >
            {body(id)}
          </CardFrame>
        ))}
      </div>
    </div>
  );
}
