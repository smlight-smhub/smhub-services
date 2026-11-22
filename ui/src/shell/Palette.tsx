// The command palette (advanced level): type to jump to any page or setting,
// open an app, or run a common action. ARIA combobox with a listbox.
import type { LucideIcon } from 'lucide-preact';
import { useEffect, useId, useMemo, useRef, useState } from 'preact/hooks';
import { useLocation } from 'wouter-preact';
import { Boxes, CornerDownLeft, ExternalLink, House, LogOut, Moon, Power, Search, Sun, ToggleLeft } from 'lucide-preact';
import { t, type Key } from '../text/i18n';
import { advanced, effectivePrefs, savePrefs } from '../state/prefs';
import { restartHub } from '../state/power';
import { session, signOut } from '../state/session';
import { live } from '../state/store';
import { system } from '../state/system';
import { appTitle, Kbd } from '../kit/Bits';
import { Dialog } from '../kit/Dialog';
import { cx } from '../kit/cx';
import { PAGES, pageById, visible } from './routes';
import { paletteOpen } from './PaletteHost';
import s from './Palette.module.css';

interface Item {
  id: string;
  label: string;
  hint?: string;
  icon: LucideIcon;
  keywords?: string;
  href?: string;
  run?: () => void;
}

/** Settings inside pages, reachable by name. The anchors are section ids on those pages. */
const SETTINGS: [page: string, anchor: string, label: Key, keywords?: string][] = [
  ['general', 'hostname', 'general.name', 'hostname'],
  ['general', 'time', 'general.timeZone', 'timezone clock'],
  ['accounts', 'signin', 'accounts.signIn', 'password security'],
  ['accounts', 'users', 'accounts.users', 'users add'],
  ['appearance', 'language', 'appearance.language', 'region locale'],
  ['appearance', 'theme', 'appearance.theme', 'dark light'],
  ['appearance', 'units', 'appearance.units', 'metric imperial temperature fahrenheit celsius clock currency'],
  ['appearance', 'level', 'level.label', 'simple advanced'],
  ['mqtt', 'users', 'mqtt.users', 'broker accounts'],
  ['mqtt', 'bridge', 'mqtt.bridge', 'remote broker'],
  ['backups', 'create', 'backups.create', 'download'],
  ['backups', 'restore', 'backups.restore', 'upload'],
  ['backups', 'factory', 'backups.factory', 'factory reset'],
  ['system', 'power', 'system.power', 'reboot shutdown'],
  ['updates', 'channel', 'updates.channel', 'stable dev beta'],
  ['esphome', 'key', 'esphome.key', 'encryption home assistant'],
  ['ssh', 'keys', 'ssh.keys', 'public key authorized'],
  ['wifi', 'networks', 'wifi.networks', 'scan connect'],
  ['vpn', 'settings', 'vpn.settings', 'wireguard keys endpoint'],
];

function scrollToAnchor(id: string) {
  let tries = 0;
  const find = () => {
    const el = document.getElementById(id);
    if (el) {
      el.scrollIntoView({ block: 'start', behavior: 'smooth' });
      const focusable = el.querySelector<HTMLElement>('input, select, textarea, button');
      focusable?.focus({ preventScroll: true });
    } else if (tries++ < 60) requestAnimationFrame(find);
  };
  requestAnimationFrame(find);
}

export default function Palette() {
  const [q, setQ] = useState('');
  const [active, setActive] = useState(0);
  const [, navigate] = useLocation();
  const listId = useId();
  const listRef = useRef<HTMLUListElement>(null);
  const sys = system.value;
  const adv = advanced.value;
  const apps = live('apps').value;
  const close = () => (paletteOpen.value = false);

  const items = useMemo<Item[]>(() => {
    const out: Item[] = [{ id: 'home', label: t('nav.home'), icon: House, href: '/' }];
    for (const p of PAGES) {
      if (!visible(p, sys, adv)) continue;
      const parent = p.parent ? pageById(p.parent) : undefined;
      out.push({ id: p.id, label: t(p.label), hint: parent ? t(parent.label) : p.description ? t(p.description) : undefined, icon: p.icon, keywords: p.keywords, href: p.path });
    }
    for (const [pid, anchor, label, kw] of SETTINGS) {
      const p = pageById(pid);
      if (!p || !visible(p, sys, adv)) continue;
      out.push({ id: `${pid}#${anchor}`, label: t(label), hint: t(p.label), icon: p.icon, keywords: kw, href: `${p.path}#${anchor}` });
    }
    for (const a of apps?.apps ?? []) {
      if (!a.installed) continue;
      out.push({ id: `app-${a.name}`, label: appTitle(a.name), hint: t('palette.app'), icon: Boxes, keywords: a.name, href: `/apps/${a.name}` });
      if (a.web) out.push({ id: `open-${a.name}`, label: t('palette.openApp', { app: appTitle(a.name) }), hint: t('palette.webUi'), icon: ExternalLink, keywords: a.name, href: `/open/${a.name}` });
    }
    const dark = document.documentElement.dataset.theme === 'dark' || (!document.documentElement.dataset.theme && matchMedia('(prefers-color-scheme: dark)').matches);
    out.push({ id: 'theme', label: dark ? t('palette.light') : t('palette.dark'), hint: t('palette.action'), icon: dark ? Sun : Moon, run: () => void savePrefs({ theme: dark ? 'light' : 'dark' }, { undo: true }) });
    out.push({ id: 'level', label: t('palette.simple'), hint: t('palette.action'), icon: ToggleLeft, run: () => void savePrefs({ level: 'simple' }, { undo: true, success: t('level.nowSimple') }) });
    out.push({ id: 'restart', label: t('power.restart'), hint: t('palette.action'), icon: Power, keywords: 'reboot', run: () => void restartHub() });
    if (session.value?.mode === 'password') out.push({ id: 'signout', label: t('account.signOut'), hint: t('palette.action'), icon: LogOut, run: () => void signOut() });
    return out;
  }, [sys, adv, apps, effectivePrefs.value.language]);

  const results = useMemo(() => {
    const needle = q.trim().toLocaleLowerCase();
    if (!needle) return items.slice(0, 40);
    const scored: [number, Item][] = [];
    for (const it of items) {
      const label = it.label.toLocaleLowerCase();
      const hay = `${label} ${it.hint ?? ''} ${it.keywords ?? ''}`.toLocaleLowerCase();
      if (label.startsWith(needle)) scored.push([0, it]);
      else if (label.includes(needle)) scored.push([1, it]);
      else if (needle.split(/\s+/).every((w) => hay.includes(w))) scored.push([2, it]);
    }
    return scored.sort((a, b) => a[0] - b[0]).map(([, it]) => it).slice(0, 40);
  }, [q, items]);

  useEffect(() => setActive(0), [q]);
  useEffect(() => {
    listRef.current?.querySelector(`#${CSS.escape(`${listId}-${active}`)}`)?.scrollIntoView({ block: 'nearest' });
  }, [active, listId]);

  const go = (it: Item | undefined) => {
    if (!it) return;
    close();
    if (it.run) return it.run();
    if (!it.href) return;
    const [path, anchor] = it.href.split('#');
    navigate(path!);
    if (anchor) scrollToAnchor(anchor);
  };

  return (
    <Dialog open onClose={close} title={t('palette.title')} size="md">
      <div class={s.search}>
        <Search size={20} aria-hidden="true" />
        <input
          class={s.input}
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={results.length ? `${listId}-${active}` : undefined}
          aria-label={t('palette.title')}
          placeholder={t('palette.placeholder')}
          value={q}
          autoFocus
          onInput={(e) => setQ((e.currentTarget as HTMLInputElement).value)}
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown') setActive((a) => Math.min(results.length - 1, a + 1));
            else if (e.key === 'ArrowUp') setActive((a) => Math.max(0, a - 1));
            else if (e.key === 'Enter') go(results[active]);
            else return;
            e.preventDefault();
          }}
        />
      </div>
      <ul class={s.list} role="listbox" id={listId} ref={listRef} aria-label={t('palette.results')}>
        {results.map((it, i) => {
          const I = it.icon;
          return (
            <li
              key={it.id}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              class={cx(s.item, i === active && s.active)}
              onClick={() => go(it)}
              onMouseMove={() => i !== active && setActive(i)}
            >
              <I size={18} aria-hidden="true" class={s.icon} />
              <span class={s.label}>{it.label}</span>
              {it.hint && <span class={s.hint}>{it.hint}</span>}
              {i === active && <CornerDownLeft size={16} aria-hidden="true" class={s.enter} />}
            </li>
          );
        })}
        {!results.length && <li class={s.none}>{t('common.noMatches')}</li>}
      </ul>
      <p class={s.help}>
        <Kbd>↑</Kbd>
        <Kbd>↓</Kbd> {t('palette.move')} · <Kbd>Enter</Kbd> {t('palette.go')} · <Kbd>Esc</Kbd> {t('common.close')}
      </p>
    </Dialog>
  );
}
