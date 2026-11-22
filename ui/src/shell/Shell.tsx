// The frame around every page: a collapsible sidebar on wide screens; a top
// bar, a bottom bar and a sheet for the rest on phones. Plus the notices,
// the operations tray, toasts and dialogs that every page shares.
import type { ComponentChildren } from 'preact';
import { Component, Fragment } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { Link, Redirect, Route, Switch, useLocation } from 'wouter-preact';
import type { LucideIcon } from 'lucide-preact';
import { Ellipsis, House, LockOpen, LogOut, Monitor, Moon, PanelLeftClose, PanelLeftOpen, RefreshCw, Search, Sun } from 'lucide-preact';
import { t, type Key } from '../text/i18n';
import { advanced, effectivePrefs, savePrefs } from '../state/prefs';
import { pageView } from '../state/get';
import { session, signOut } from '../state/session';
import { connection, live, useLive } from '../state/store';
import { system } from '../state/system';
import { Button, IconButton } from '../kit/Button';
import { AppIcon, appTitle } from '../kit/Bits';
import { Dialog } from '../kit/Dialog';
import { Banner } from '../kit/Feedback';
import { Choice } from '../kit/Field';
import { cx } from '../kit/cx';
import { Dashboard } from '../pages/dashboard/Dashboard';
import { NotFound } from '../pages/NotFound';
import { AdvancedOnly, NotOnThisHub } from '../pages/Unavailable';
import { legacyTarget } from './legacy';
import { AppFrame, AppPage, PAGES, activePage, visible, type PageDef } from './routes';
import { ConfirmHost, Notices, OpsTray, Toasts } from './Overlays';
import { openPalette, PaletteHost } from './PaletteHost';
import { Logo } from './Logo';
import s from './Shell.module.css';

const COLLAPSE_KEY = 'smhub.sidebar';

function useNav() {
  const sys = system.value;
  const adv = advanced.value;
  const shown = PAGES.filter((p) => visible(p, sys, adv));
  return {
    top: shown.filter((p) => !p.parent && p.group === 'main'),
    tools: shown.filter((p) => p.group === 'tools'),
    children: (id: string) => shown.filter((p) => p.parent === id),
  };
}

function useBadges(): Record<string, number> {
  const updates = live('updates').value;
  const apps = live('apps').value;
  return {
    updates: (updates?.os.update_available ? 1 : 0) + (updates?.apps.updates ?? 0),
    apps: apps?.updates ?? 0,
  };
}

function NavItem({
  href,
  icon: I,
  label,
  active,
  badge,
  sub,
  lead,
  onNavigate,
  preload,
}: {
  href: string;
  icon?: LucideIcon;
  label: string;
  active: boolean;
  badge?: number;
  sub?: boolean;
  lead?: ComponentChildren;
  onNavigate?: () => void;
  preload?: () => void;
}) {
  return (
    <li>
      <Link
        href={href}
        class={cx(s.navLink, sub && s.sub, active && s.active)}
        aria-current={active ? 'page' : undefined}
        title={label}
        onClick={onNavigate}
        onPointerEnter={preload}
        onFocus={preload}
      >
        {lead ?? (I && <I size={20} aria-hidden="true" class={s.navIcon} />)}
        <span class={s.navLabel}>{label}</span>
        {!!badge && (
          <span class={s.badge} aria-label={t('nav.badge', { count: badge })}>
            {badge}
          </span>
        )}
      </Link>
    </li>
  );
}

function YourApps({ onNavigate, loc }: { onNavigate?: () => void; loc: string }) {
  const apps = live('apps').value?.apps.filter((a) => a.installed && a.web && a.sidebar) ?? [];
  if (!apps.length) return null;
  return (
    <div class={s.group}>
      <h2 class={s.groupTitle}>{t('nav.yourApps')}</h2>
      <ul class={s.navList}>
        {apps.map((a) => (
          <NavItem
            key={a.name}
            href={`/open/${a.name}`}
            label={appTitle(a.name)}
            active={loc === `/open/${a.name}`}
            lead={<AppIcon name={a.name} src={a.icon} size={22} />}
            onNavigate={onNavigate}
          />
        ))}
      </ul>
    </div>
  );
}

function LevelSwitch() {
  const level = advanced.value ? 'advanced' : 'simple';
  return (
    <Choice
      label={t('level.label')}
      hideLabel
      value={level}
      onValue={(v) =>
        savePrefs(
          { level: v },
          { undo: true, success: v === 'advanced' ? t('level.nowAdvanced') : t('level.nowSimple') },
        )
      }
      options={[
        { value: 'simple', label: t('level.simple') },
        { value: 'advanced', label: t('level.advanced') },
      ]}
    />
  );
}

const THEMES = ['auto', 'light', 'dark'] as const;
const THEME_LOOK = {
  auto: [Monitor, 'appearance.themeAuto'],
  light: [Sun, 'appearance.themeLight'],
  dark: [Moon, 'appearance.themeDark'],
} as const;

/** One quiet button for the theme, beside the account: each press goes Automatic → Light → Dark.
 *  The same choice as on the Appearance page, without going there. */
function ThemeButton() {
  const now = THEMES.find((x) => x === effectivePrefs.value.theme) ?? 'auto';
  const next = THEMES[(THEMES.indexOf(now) + 1) % THEMES.length]!;
  const [icon, label] = THEME_LOOK[now];
  return <IconButton icon={icon} label={`${t('appearance.theme')}: ${t(label)}`} size="sm" onClick={() => void savePrefs({ theme: next })} />;
}

function Account({ compact }: { compact?: boolean }) {
  const sess = session.value;
  if (!sess) return null;
  if (sess.mode === 'open') {
    return (
      <Link href="/settings/accounts" class={s.account} title={t('account.openHub')}>
        <LockOpen size={18} aria-hidden="true" />
        {!compact && <span>{t('account.openHub')}</span>}
      </Link>
    );
  }
  return (
    <div class={s.accountRow}>
      {!compact && <span class={s.user}>{sess.user.username}</span>}
      <IconButton icon={LogOut} label={t('account.signOut')} size="sm" onClick={() => void signOut()} />
    </div>
  );
}

function ConnectionDot({ compact }: { compact?: boolean }) {
  const c = connection.value;
  const ok = c === 'open';
  return (
    <span class={s.conn} title={ok ? t('conn.live') : t('conn.reconnecting')}>
      <span class={cx(s.connDot, ok ? s.connOk : s.connBad)} aria-hidden="true" />
      <span class={compact ? 'visually-hidden' : undefined}>{ok ? t('conn.live') : t('conn.reconnecting')}</span>
    </span>
  );
}

function Sidebar({ collapsed, onToggle, loc }: { collapsed: boolean; onToggle: () => void; loc: string }) {
  const nav = useNav();
  const badges = useBadges();
  const active = activePage(loc);
  const sys = system.value;
  const item = (p: PageDef, sub = false) => (
    <NavItem
      key={p.id}
      href={p.href ?? p.path}
      icon={p.icon}
      label={t(p.label)}
      active={sub ? loc === p.path || loc.startsWith(p.path + '/') : active?.id === p.id || (!!active?.parent && active.parent === p.id && collapsed)}
      badge={badges[p.id]}
      sub={sub}
      preload={p.page.preload}
    />
  );
  return (
    <aside class={s.sidebar}>
      <div class={s.brand}>
        <Logo />
        <div class={s.brandText}>
          <span class={s.brandName}>{sys?.product ?? 'SMHUB'}</span>
          <span class={s.brandHost}>{sys?.hostname ?? ' '}</span>
        </div>
      </div>
      <nav class={s.nav} aria-label={t('nav.main')}>
        <ul class={s.navList}>
          <NavItem href="/" icon={House} label={t('nav.home')} active={loc === '/'} />
          {nav.top.map((p) => {
            const kids = nav.children(p.id);
            const open = !collapsed && kids.length > 0 && (active?.id === p.id || active?.parent === p.id);
            return (
              <Fragment key={p.id}>
                {item(p)}
                {open && (
                  <li class={s.subList}>
                    <ul>{kids.map((k) => item(k, true))}</ul>
                  </li>
                )}
              </Fragment>
            );
          })}
        </ul>
        <YourApps loc={loc} />
        {nav.tools.length > 0 && (
          <div class={s.group}>
            <h2 class={s.groupTitle}>{t('nav.tools')}</h2>
            <ul class={s.navList}>{nav.tools.map((p) => item(p))}</ul>
          </div>
        )}
      </nav>
      <div class={s.sideFoot}>
        {advanced.value && !collapsed && (
          <button type="button" class={s.paletteHint} onClick={openPalette}>
            <Search size={16} aria-hidden="true" />
            <span>{t('palette.open')}</span>
            <kbd>{/Mac|iPhone|iPad/.test(navigator.platform) ? '⌘K' : 'Ctrl K'}</kbd>
          </button>
        )}
        {!collapsed && <LevelSwitch />}
        <div class={s.footRow}>
          <ConnectionDot compact={collapsed} />
          <Account compact={collapsed} />
        </div>
        <div class={s.footTools}>
          <IconButton
            icon={collapsed ? PanelLeftOpen : PanelLeftClose}
            label={collapsed ? t('nav.expand') : t('nav.collapse')}
            onClick={onToggle}
            aria-expanded={!collapsed}
          />
          <ThemeButton />
        </div>
      </div>
    </aside>
  );
}

function TopBar() {
  const sys = system.value;
  return (
    <header class={s.topbar}>
      <Link href="/" class={s.topBrand} title={t('nav.home')}>
        <Logo small />
        <span class={s.topHost}>{sys?.hostname ?? 'SMHUB'}</span>
      </Link>
      <div class={s.topActions}>
        <ConnectionDot compact />
        {advanced.value && <IconButton icon={Search} label={t('palette.open')} onClick={openPalette} />}
      </div>
    </header>
  );
}

function BottomNav({ loc }: { loc: string }) {
  const nav = useNav();
  const badges = useBadges();
  const [more, setMore] = useState(false);
  const active = activePage(loc);
  const bottom = nav.top.filter((p) => p.bottom);
  const rest = nav.top.filter((p) => !p.bottom);
  const inMore = !!active && !bottom.some((p) => p.id === active.id || p.id === active.parent) || loc.startsWith('/open/');
  useEffect(() => setMore(false), [loc]);
  const tab = (href: string, I: LucideIcon, label: Key, on: boolean, badge?: number, preload?: () => void) => (
    <li key={href}>
      <Link href={href} class={cx(s.tab, on && s.tabOn)} aria-current={on ? 'page' : undefined} onPointerDown={preload}>
        <span class={s.tabIcon}>
          <I size={22} aria-hidden="true" />
          {!!badge && <span class={s.dotBadge} aria-label={t('nav.badge', { count: badge })} />}
        </span>
        <span>{t(label)}</span>
      </Link>
    </li>
  );
  return (
    <>
      <nav class={s.bottom} aria-label={t('nav.main')}>
        <ul>
          {tab('/', House, 'nav.home', loc === '/')}
          {bottom.map((p) => tab(p.path, p.icon, p.label, active?.id === p.id || active?.parent === p.id, badges[p.id], p.page.preload))}
          <li>
            <button type="button" class={cx(s.tab, inMore && s.tabOn)} onClick={() => setMore(true)} aria-haspopup="dialog">
              <span class={s.tabIcon}>
                <Ellipsis size={22} aria-hidden="true" />
                {!!(badges.updates && rest.some((p) => p.id === 'updates')) && <span class={s.dotBadge} />}
              </span>
              <span>{t('nav.more')}</span>
            </button>
          </li>
        </ul>
      </nav>
      <Dialog open={more} onClose={() => setMore(false)} title={t('nav.more')} size="sm">
        <nav aria-label={t('nav.more')}>
          <ul class={s.sheetList}>
            {rest.map((p) => (
              <NavItem key={p.id} href={p.path} icon={p.icon} label={t(p.label)} active={active?.id === p.id} badge={badges[p.id]} onNavigate={() => setMore(false)} />
            ))}
          </ul>
          <YourApps loc={loc} onNavigate={() => setMore(false)} />
          {nav.tools.length > 0 && (
            <div class={s.group}>
              <h2 class={s.groupTitle}>{t('nav.tools')}</h2>
              <ul class={s.sheetList}>
                {nav.tools.map((p) => (
                  <NavItem key={p.id} href={p.path} icon={p.icon} label={t(p.label)} active={active?.id === p.id} onNavigate={() => setMore(false)} />
                ))}
              </ul>
            </div>
          )}
        </nav>
        <div class={s.sheetFoot}>
          <LevelSwitch />
          <div class={s.footRow}>
            <Account />
            <ThemeButton />
          </div>
        </div>
      </Dialog>
    </>
  );
}

/** A failing page shows an error in its own area; the rest keeps working (D44). */
class PageBoundary extends Component<{ children: ComponentChildren }, { error: unknown }> {
  override state = { error: undefined as unknown };
  override componentDidCatch(error: unknown) {
    this.setState({ error });
    console.error(error);
  }
  override render() {
    if (this.state.error) {
      return (
        <div class={s.pageError}>
          <Banner
            tone="danger"
            title={t('error.pageTitle')}
            action={
              <Button icon={RefreshCw} onClick={() => location.reload()}>
                {t('common.reload')}
              </Button>
            }
          >
            {t('error.pageText')}
          </Banner>
        </div>
      );
    }
    return this.props.children;
  }
}

function Gate({ p }: { p: PageDef }) {
  const sys = system.value;
  if (p.advanced && !advanced.value) return <AdvancedOnly title={t(p.label)} />;
  if (p.available && sys && !p.available(sys)) return <NotOnThisHub title={t(p.label)} />;
  const P = p.page;
  return <P />;
}

function Fallback() {
  const [loc] = useLocation();
  const apps = live('apps').value;
  const target = legacyTarget(loc, false);
  if (target && target !== loc) return <Redirect to={target} replace />;
  const single = loc.replace(/^\/+|\/+$/g, '');
  if (apps?.apps.some((a) => a.name === single && a.installed)) return <Redirect to={`/open/${single}`} replace />;
  return <NotFound />;
}

export function Shell({ inert }: { inert?: boolean }) {
  const [loc] = useLocation();
  const [collapsed, setCollapsed] = useState(() => localStorage.getItem(COLLAPSE_KEY) === '1');
  // Shared by the whole frame: what the hub is, its apps (sidebar links),
  // updates (badges) and operations (the tray).
  useLive('system');
  useLive('apps');
  useLive('updates');
  useLive('operations');
  useEffect(() => {
    pageView.value++;
  }, [loc]);
  const toggle = () => {
    setCollapsed(!collapsed);
    try {
      localStorage.setItem(COLLAPSE_KEY, collapsed ? '0' : '1');
    } catch {
      /* not kept */
    }
  };
  const framed = loc.startsWith('/open/');
  return (
    <div class={cx(s.shell, collapsed && s.collapsed, framed && s.framed)} inert={inert || undefined} aria-hidden={inert || undefined}>
      <a class="skip-link" href="#main">
        {t('nav.skip')}
      </a>
      <Sidebar collapsed={collapsed} onToggle={toggle} loc={loc} />
      <TopBar />
      <main id="main" class={s.main} tabIndex={-1}>
        <Notices loc={loc} />
        <PageBoundary key={loc}>
          <Switch>
            <Route path="/">
              <Dashboard />
            </Route>
            {PAGES.map((p) => (
              <Route key={p.id} path={p.path}>
                <Gate p={p} />
              </Route>
            ))}
            <Route path="/apps/:name">
              <AppPage />
            </Route>
            <Route path="/open/:name">
              <AppFrame />
            </Route>
            <Route>
              <Fallback />
            </Route>
          </Switch>
        </PageBoundary>
      </main>
      <BottomNav loc={loc} />
      <OpsTray />
      <Toasts />
      <ConfirmHost />
      <PaletteHost />
    </div>
  );
}
