// The app store: installed, available and community apps, with search,
// install, upgrade, "check for updates" and "upgrade all".
import { useMemo, useState } from 'preact/hooks';
import { Link } from 'wouter-preact';
import { ArrowUpFromLine, Download, ExternalLink, PackageSearch, RefreshCw, Search } from 'lucide-preact';
import { api } from '../../api/client';
import type { AppSummary } from '../../api/types';
import { t } from '../../text/i18n';
import { compare, relative } from '../../text/format';
import { appWebUrl } from '../../state/apps';
import { confirm } from '../../state/confirm';
import { latestOperation, startOperation } from '../../state/operations';
import { advanced } from '../../state/prefs';
import { live, useLive } from '../../state/store';
import { hubTime, useNow } from '../../state/system';
import { Button, LinkButton } from '../../kit/Button';
import { AppIcon, appTitle } from '../../kit/Bits';
import { Banner, EmptyState, ProgressBar, Skeleton } from '../../kit/Feedback';
import { TextField } from '../../kit/Field';
import { Page } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import { Tabs, TabPanel } from '../../kit/Tabs';
import c from '../common.module.css';
import s from './Apps.module.css';

type Section = 'installed' | 'available' | 'community';

function sectionOf(a: AppSummary): Section {
  if (a.installed) return 'installed';
  return a.section === 'community' ? 'community' : 'available';
}

export function install(name: string) {
  return startOperation(() => api.post('/apps/{name}/{action}', { path: { name, action: 'install' } }));
}

export function upgrade(name: string) {
  return startOperation(() => api.post('/apps/{name}/{action}', { path: { name, action: 'upgrade' } }));
}

function AppTile({ a }: { a: AppSummary }) {
  const op = latestOperation(['app_install', 'app_upgrade', 'app_remove'], a.name);
  const busy = op?.state === 'running';
  const title = appTitle(a.name);
  const url = a.installed && a.web ? (a.sidebar ? `/open/${a.name}` : appWebUrl(a)) : null;
  return (
    <li class={s.tile}>
      <Link href={`/apps/${a.name}`} class={s.tileLink}>
        <AppIcon name={a.name} src={a.icon} size={48} />
        <span class={s.tileText}>
          <span class={s.tileName}>{title}</span>
          <span class={s.tileDesc}>{a.description || t('apps.noDescription')}</span>
        </span>
      </Link>
      <div class={s.tileFoot}>
        {busy ? (
          <div class={s.tileProgress}>
            <ProgressBar value={op?.progress} label={t('apps.working', { app: title })} />
          </div>
        ) : (
          <>
            <span class={s.tileMeta}>
              {a.installed && a.is_service ? <ServiceStatus state={a.state} size="sm" /> : <span class="num">{a.installed_version ?? a.latest_version}</span>}
              {a.update_available && <span class={`${c.tag} ${c.tagAccent}`}>{t('apps.updateTag', { version: a.latest_version })}</span>}
            </span>
            <span class={s.tileActions}>
              {!a.installed &&
                (a.compatible ? (
                  <Button size="sm" variant="primary" icon={Download} onClick={() => void install(a.name)}>
                    {t('apps.install')}
                  </Button>
                ) : (
                  <span class={c.note}>{t('apps.needsOs', { version: a.required_os ?? '' })}</span>
                ))}
              {a.installed && a.update_available && (
                <Button size="sm" variant="soft" icon={ArrowUpFromLine} onClick={() => void upgrade(a.name)} disabled={!a.compatible}>
                  {t('apps.upgrade')}
                </Button>
              )}
              {url && (
                <LinkButton size="sm" href={url} external={!a.sidebar} icon={ExternalLink}>
                  {t('apps.open')}
                </LinkButton>
              )}
            </span>
          </>
        )}
      </div>
    </li>
  );
}

export default function Store() {
  const data = useLive('apps');
  const updates = live('updates').value;
  const adv = advanced.value;
  const now = useNow('minute');
  const [q, setQ] = useState('');
  const [tab, setTab] = useState<Section | null>(null);
  const refreshing = latestOperation(['apps_refresh'])?.state === 'running';
  const upgrading = latestOperation(['apps_upgrade'])?.state === 'running';

  const groups = useMemo(() => {
    const g: Record<Section, AppSummary[]> = { installed: [], available: [], community: [] };
    const needle = q.trim().toLocaleLowerCase();
    for (const a of data?.apps ?? []) {
      if (a.section === 'advanced' && !adv && !a.installed) continue;
      if (a.system && !adv && !a.update_available) continue;
      if (needle && !`${a.name} ${a.description ?? ''}`.toLocaleLowerCase().includes(needle)) continue;
      g[sectionOf(a)].push(a);
    }
    for (const k of Object.keys(g) as Section[]) g[k].sort((x, y) => compare(x.name, y.name));
    return g;
  }, [data, q, adv]);

  const current: Section = tab ?? (data && !data.apps.some((a) => a.installed && !a.system) ? 'available' : 'installed');
  const list = groups[current];
  const updateCount = data?.updates ?? 0;

  const upgradeAll = async () => {
    if (!(await confirm({ title: t('apps.upgradeAllTitle'), body: t('apps.upgradeAllBody', { count: updateCount }), confirm: t('apps.upgradeAll') }))) return;
    await startOperation(() => api.post('/apps/upgrade'));
  };

  return (
    <Page
      title={t('nav.apps')}
      subtitle={data?.refreshed_at ? t('apps.checked', { when: relative(data.refreshed_at, hubTime(now)) }) : t('nav.appsDesc')}
      actions={
        <>
          <Button icon={RefreshCw} busy={refreshing} onClick={() => void startOperation(() => api.post('/apps/refresh'))}>
            {t('apps.check')}
          </Button>
          {updateCount > 0 && (
            <Button variant="primary" icon={ArrowUpFromLine} busy={upgrading} onClick={() => void upgradeAll()}>
              {t('apps.upgradeAllCount', { count: updateCount })}
            </Button>
          )}
        </>
      }
    >
      {updates?.os.update_available && (
        <Banner tone="info" title={t('apps.osBannerTitle')} action={<LinkButton href="/updates" size="sm">{t('apps.osBannerAction')}</LinkButton>}>
          {t('apps.osBannerText')}
        </Banner>
      )}
      <TextField label={t('apps.search')} hideLabel type="search" icon={Search} value={q} onValue={setQ} placeholder={t('apps.searchPlaceholder')} />
      <div>
        <Tabs
          id="store"
          label={t('nav.apps')}
          value={current}
          onChange={setTab}
          tabs={[
            { id: 'installed', label: t('apps.installed'), badge: <span class={c.tag}>{groups.installed.length}</span> },
            { id: 'available', label: t('apps.available'), badge: <span class={c.tag}>{groups.available.length}</span> },
            { id: 'community', label: t('apps.community'), badge: <span class={c.tag}>{groups.community.length}</span> },
          ]}
        />
        <TabPanel id="store" tab={current}>
          {!data ? (
            <Skeleton lines={6} />
          ) : list.length ? (
            <>
              {current === 'community' && <Banner tone="warn">{t('apps.communityNote')}</Banner>}
              <ul class={s.tiles}>
                {list.map((a) => (
                  <AppTile key={a.name} a={a} />
                ))}
              </ul>
            </>
          ) : (
            <EmptyState icon={PackageSearch} title={q ? t('apps.noMatch') : current === 'installed' ? t('apps.noneInstalled') : t('apps.noneAvailable')}>
              {q ? t('apps.noMatchText') : current === 'installed' ? t('apps.noneInstalledText') : t('apps.noneAvailableText')}
            </EmptyState>
          )}
        </TabPanel>
      </div>
    </Page>
  );
}
