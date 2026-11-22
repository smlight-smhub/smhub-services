// One app: its state and controls, settings form, live log and changelog.
import { useEffect, useRef, useState } from 'preact/hooks';
import { useLocation, useParams, useSearch } from 'wouter-preact';
import { ArrowUpFromLine, Download, ExternalLink, FileText, Info, Play, RotateCcw, RotateCw, ScrollText, Settings2, Square, Trash } from 'lucide-preact';
import { api } from '../../api/client';
import type { AppDetail } from '../../api/types';
import { t } from '../../text/i18n';
import { bytes } from '../../text/format';
import { appCommand, appWebUrl, setAppOptions } from '../../state/apps';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { latestOperation, startOperation } from '../../state/operations';
import { advanced } from '../../state/prefs';
import { errorText, save, useFieldErrors } from '../../state/save';
import { useLive } from '../../state/store';
import { toast } from '../../state/toasts';
import { AppForm } from '../../kit/AppForm';
import { AppIcon, appTitle } from '../../kit/Bits';
import { Button, LinkButton, Transition, transitionOf } from '../../kit/Button';
import { Banner, EmptyState, ErrorState, Loading, ProgressBar } from '../../kit/Feedback';
import { Switch } from '../../kit/Field';
import { LogView } from '../../kit/LogView';
import { Markdown } from '../../kit/Markdown';
import { Facts, Page, Rows, Section } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import { Tabs, TabPanel } from '../../kit/Tabs';
import { TailscalePanel, ThreadPanel } from './Extras';
import { install, upgrade } from './Store';
import c from '../common.module.css';
import s from './Apps.module.css';

type TabId = 'overview' | 'settings' | 'log' | 'changelog';

function Changelog({ name }: { name: string }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<unknown>();
  const load = () => {
    setError(undefined);
    api
      .get('/apps/{name}/changelog', { path: { name } })
      .then((md) => setText(typeof md === 'string' ? md : ''))
      .catch(setError);
  };
  useEffect(load, [name]);
  if (error) return <ErrorState error={error} retry={load} />;
  if (text === null) return <Loading lines={6} />;
  return text ? <Markdown text={text} /> : <p class="muted">{t('apps.noChangelog')}</p>;
}

function Options({ app }: { app: AppDetail }) {
  const title = appTitle(app.name);
  const rows = [];
  if (app.is_service)
    rows.push(
      <Switch
        key="autostart"
        label={t('apps.autostart')}
        description={t('apps.autostartDesc')}
        checked={app.autostart}
        onChange={(v) => void setAppOptions(app.name, { autostart: v }, v ? t('apps.autostartOn', { app: title }) : t('apps.autostartOff', { app: title }))}
      />,
    );
  if (app.watchdog !== null && app.watchdog !== undefined)
    rows.push(
      <Switch
        key="watchdog"
        label={t('apps.watchdog')}
        description={t('apps.watchdogDesc')}
        checked={app.watchdog}
        onChange={(v) => void setAppOptions(app.name, { watchdog: v }, v ? t('apps.watchdogOn') : t('apps.watchdogOff'))}
      />,
    );
  if (app.web)
    rows.push(
      <Switch
        key="sidebar"
        label={t('apps.sidebar')}
        description={t('apps.sidebarDesc')}
        checked={app.sidebar}
        onChange={(v) => void setAppOptions(app.name, { sidebar: v }, v ? t('apps.sidebarOn') : t('apps.sidebarOff'))}
      />,
    );
  if (!rows.length) return null;
  return (
    <Section title={t('apps.howItRuns')}>
      <Rows>{rows}</Rows>
    </Section>
  );
}

export default function AppPage() {
  const { name } = useParams<{ name: string }>();
  const search = useSearch();
  const [, navigate] = useLocation();
  const { data, error, reload, sig } = useGet('/apps/{name}', { path: { name } });
  const apps = useLive('apps');
  const errors = useFieldErrors();
  const [busy, setBusy] = useState<string>();
  const summary = apps?.apps.find((a) => a.name === name);
  // Live state comes from the `apps` resource; the rest from the detail.
  const app: AppDetail | undefined = data ? { ...data, ...(summary ?? {}) } : undefined;
  const title = appTitle(name);
  const op = latestOperation(['app_install', 'app_upgrade', 'app_remove'], name);
  const running = op?.state === 'running';

  // When an install, upgrade or removal ends, read the app again.
  const lastOp = useRef(op?.state);
  useEffect(() => {
    if (lastOp.current === 'running' && op?.state === 'succeeded') {
      if (op.kind === 'app_remove') navigate('/apps');
      else void reload();
    }
    lastOp.current = op?.state;
  }, [op?.state]);

  const tabs: { id: TabId; label: string; icon: typeof Info }[] = [{ id: 'overview', label: t('apps.overview'), icon: Info }];
  if (app?.installed && app.form.length) tabs.push({ id: 'settings', label: t('apps.settings'), icon: Settings2 });
  if (app?.installed && app.log) tabs.push({ id: 'log', label: t('apps.log'), icon: ScrollText });
  if (app?.has_changelog) tabs.push({ id: 'changelog', label: t('apps.changelog'), icon: FileText });
  const want = new URLSearchParams(search).get('tab') as TabId | null;
  const tab: TabId = tabs.some((x) => x.id === want) ? want! : 'overview';
  const setTab = (id: TabId) => navigate(id === 'overview' ? `/apps/${name}` : `/apps/${name}?tab=${id}`, { replace: true });

  if (error && !data) {
    return (
      <Page title={title} back={{ href: '/apps', label: t('nav.apps') }}>
        {error.code === 'not_found' ? <EmptyState title={t('apps.notFound')}>{t('apps.notFoundText', { app: name })}</EmptyState> : <ErrorState error={error} retry={reload} />}
      </Page>
    );
  }

  const command = async (action: 'start' | 'stop' | 'restart') => {
    setBusy(action);
    await appCommand(name, action);
    setBusy(undefined);
  };
  const url = app?.web ? appWebUrl(app) : null;
  const isRunning = app?.state === 'started' || app?.state === 'starting';

  const remove = async () => {
    const ok = await confirm({ title: t('apps.removeTitle', { app: title }), body: t('apps.removeBody'), confirm: t('apps.remove'), tone: 'danger' });
    if (ok) await startOperation(() => api.post('/apps/{name}/{action}', { path: { name, action: 'remove' } }));
  };
  const reset = async () => {
    const ok = await confirm({ title: t('apps.resetTitle', { app: title }), body: t('apps.resetBody'), confirm: t('apps.reset'), tone: 'danger' });
    if (!ok) return;
    try {
      const d = await api.post('/apps/{name}/{action}', { path: { name, action: 'reset' } });
      if (d && 'form' in d) sig.value = d;
      toast({ text: t('apps.resetDone'), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
  };

  const saveSettings = async (changed: Record<string, unknown>) => {
    const answer = await save({
      target: sig,
      send: () => api.patch('/apps/{name}/settings', { path: { name }, body: changed }),
      accept: (a, cur) => (cur ? { ...cur, settings: a.settings } : cur),
      errors,
      success: t('common.saved'),
      restart: (a) => a.restart_needed && isRunning && { id: `app:${name}`, kind: 'app', target: name, reason: t('restart.appSettings', { app: title }) },
    });
    return !!answer;
  };

  return (
    <Page
      title={title}
      back={{ href: '/apps', label: t('nav.apps') }}
      lead={<AppIcon name={name} src={app?.icon} size={56} />}
      subtitle={
        app && (
          <span class={c.inline}>
            {app.installed && app.is_service && <ServiceStatus state={app.state} />}
            <span class="num">{app.installed ? t('apps.version', { version: app.installed_version ?? '' }) : t('apps.latest', { version: app.latest_version })}</span>
            {app.update_available && <span class={`${c.tag} ${c.tagAccent}`}>{t('apps.updateTag', { version: app.latest_version })}</span>}
          </span>
        )
      }
      actions={
        app &&
        !running && (
          <>
            {!app.installed && (
              <Button variant="primary" icon={Download} onClick={() => void install(name)} disabled={!app.compatible}>
                {t('apps.install')}
              </Button>
            )}
            {app.installed && app.update_available && (
              <Button variant="primary" icon={ArrowUpFromLine} onClick={() => void upgrade(name)} disabled={!app.compatible}>
                {t('apps.upgrade')}
              </Button>
            )}
            {url && isRunning && (
              <LinkButton href={app.sidebar ? `/open/${name}` : url} external={!app.sidebar} icon={ExternalLink} variant={app.update_available ? 'secondary' : 'primary'}>
                {t('apps.openUi')}
              </LinkButton>
            )}
            {app.installed &&
              app.is_service &&
              (transitionOf(app.state) ? (
                <Transition state={transitionOf(app.state)!} name={title} />
              ) : isRunning ? (
                <>
                  <Button icon={RotateCw} busy={busy === 'restart'} disabled={!!busy} onClick={() => void command('restart')}>
                    {t('apps.restart')}
                  </Button>
                  <Button icon={Square} busy={busy === 'stop'} disabled={!!busy} onClick={() => void command('stop')}>
                    {t('apps.stop')}
                  </Button>
                </>
              ) : (
                <Button variant="primary" icon={Play} busy={busy === 'start'} disabled={!!busy} onClick={() => void command('start')}>
                  {t('apps.start')}
                </Button>
              ))}
          </>
        )
      }
    >
      {running && op && (
        <Section>
          <p class={c.between}>
            <strong>{t(`apps.op.${op.kind}` as 'apps.op.app_install', { app: title })}</strong>
          </p>
          <ProgressBar value={op.progress} label={t('apps.working', { app: title })} />
          {op.step && <p class={c.note}>{op.step}</p>}
        </Section>
      )}
      {op?.state === 'failed' && <Banner tone="danger" title={t('apps.opFailed')}>{op.error ?? ''}</Banner>}
      {app && !app.compatible && <Banner tone="warn">{t('apps.needsOs', { version: app.required_os ?? '' })}</Banner>}
      {!app ? (
        <Loading lines={6} />
      ) : (
        <div>
          <Tabs id="app" label={title} tabs={tabs} value={tab} onChange={setTab} />
          <TabPanel id="app" tab={tab}>
            {tab === 'overview' && (
              <div class={c.grid}>
                {app.installed && <Options app={app} />}
                <Section title={t('apps.about')}>
                  {app.description && <p>{app.description}</p>}
                  <Facts
                    items={[
                      [t('apps.installedVersion'), app.installed && <span class="num">{app.installed_version}</span>],
                      [t('apps.latestVersion'), <span class="num">{app.latest_version}</span>],
                      [t('apps.size'), app.size ? bytes(app.size) : null],
                      [t('apps.requiresOs'), app.required_os],
                      [t('apps.section'), advanced.value && app.section],
                      [t('apps.webPort'), advanced.value && app.web && <span class="num">{String(app.web.port)}</span>],
                      [t('apps.dataDirs'), advanced.value && app.data_dirs.length > 0 && <span class="mono">{app.data_dirs.join(', ')}</span>],
                      [
                        t('apps.homepage'),
                        app.homepage && (
                          <a href={app.homepage} target="_blank" rel="noopener noreferrer">
                            {t('apps.homepageLink')}
                          </a>
                        ),
                      ],
                    ]}
                  />
                </Section>
                {app.installed && app.extras.includes('tailscale') && <TailscalePanel name={name} />}
                {app.installed && app.extras.includes('thread') && <ThreadPanel name={name} />}
                {app.installed && (app.can_reset || !app.system) && (
                  <Section title={t('apps.manage')}>
                    {app.can_reset && (
                      <div class={s.danger}>
                        <div>
                          <p>
                            <strong>{t('apps.reset')}</strong>
                          </p>
                          <p class={c.note}>{t('apps.resetDesc')}</p>
                        </div>
                        <Button icon={RotateCcw} onClick={() => void reset()}>
                          {t('apps.reset')}
                        </Button>
                      </div>
                    )}
                    {!app.system && (
                      <div class={s.danger}>
                        <div>
                          <p>
                            <strong>{t('apps.remove')}</strong>
                          </p>
                          <p class={c.note}>{t('apps.removeDesc')}</p>
                        </div>
                        <Button variant="danger" icon={Trash} onClick={() => void remove()} disabled={running}>
                          {t('apps.remove')}
                        </Button>
                      </div>
                    )}
                  </Section>
                )}
              </div>
            )}
            {tab === 'settings' && (
              <Section>
                <AppForm form={app.form} values={app.settings} secrets={app.secrets_set} onSave={saveSettings} errors={errors.value} />
              </Section>
            )}
            {tab === 'log' && app.log && (
              <Section>
                <LogView name={app.log} />
              </Section>
            )}
            {tab === 'changelog' && (
              <Section>
                <Changelog name={name} />
              </Section>
            )}
          </TabPanel>
        </div>
      )}
    </Page>
  );
}
