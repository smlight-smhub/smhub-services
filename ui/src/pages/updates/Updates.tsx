// Updates: the hub's own software (OS) with changelogs, install with live
// progress and a restart prompt; channel and automatic updates; app updates.
import { useEffect, useRef, useState } from 'preact/hooks';
import { ArrowUpFromLine, Boxes, CircleCheck, Download, RefreshCw, RotateCw } from 'lucide-preact';
import { api } from '../../api/client';
import type { Channel, OsRelease, Updates as UpdatesState } from '../../api/types';
import { t } from '../../text/i18n';
import { relative } from '../../text/format';
import { confirm } from '../../state/confirm';
import { addRestartNotice } from '../../state/notices';
import { latestOperation, operationStep, startOperation } from '../../state/operations';
import { restartHub } from '../../state/power';
import { advanced } from '../../state/prefs';
import { errorText, save } from '../../state/save';
import { live, setLive, useLive } from '../../state/store';
import { hubTime, useNow } from '../../state/system';
import { toast } from '../../state/toasts';
import { Button, LinkButton } from '../../kit/Button';
import { Banner, Loading, ProgressBar } from '../../kit/Feedback';
import { Choice, Switch } from '../../kit/Field';
import { Markdown } from '../../kit/Markdown';
import { Page, Rows, Section } from '../../kit/Page';
import { Status } from '../../kit/Status';
import c from '../common.module.css';

function Release({ r, current, onInstall, busy }: { r: OsRelease; current: string; onInstall: () => void; busy: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <li class={c.item}>
      <div class={c.itemMain}>
        <span class={c.itemTitle}>
          <span class="num">{r.version}</span>
          {r.channel === 'dev' && <span class={`${c.tag} ${c.tagWarn}`}>{t('updates.devTag')}</span>}
          {r.version === current && <span class={c.tag}>{t('updates.installedTag')}</span>}
          {r.newer && <span class={`${c.tag} ${c.tagAccent}`}>{t('updates.newTag')}</span>}
        </span>
        {r.changelog && (
          <details class={c.details} onToggle={(e) => setOpen((e.currentTarget as HTMLDetailsElement).open)}>
            <summary>{t('updates.whatsNew')}</summary>
            <div class={c.detailsBody}>{open && <Markdown text={r.changelog} />}</div>
          </details>
        )}
      </div>
      {r.version !== current && (
        <Button variant={r.newer ? 'primary' : 'secondary'} size="sm" icon={Download} onClick={onInstall} disabled={busy}>
          {r.newer ? t('updates.install') : t('updates.installOlder')}
        </Button>
      )}
    </li>
  );
}

export default function Updates() {
  const u = useLive('updates');
  const apps = useLive('apps');
  const now = useNow('minute');
  const [checking, setChecking] = useState(false);
  const op = latestOperation(['os_update']);
  const running = op?.state === 'running';

  // When an OS update finishes and needs a restart, say so in the one place for it.
  const prev = useRef(op?.state);
  useEffect(() => {
    if (prev.current === 'running' && op?.state === 'succeeded' && op.reboot_needed) addRestartNotice({ id: 'os-update', kind: 'hub', reason: t('restart.osUpdate') });
    prev.current = op?.state;
  }, [op?.state]);

  const check = async () => {
    setChecking(true);
    try {
      setLive('updates', await api.post('/updates/check'));
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setChecking(false);
  };
  const install = async (r: OsRelease) => {
    const ok = await confirm({
      title: t('updates.confirmTitle', { version: r.version }),
      body: r.newer ? t('updates.confirmBody') : t('updates.confirmOlder'),
      confirm: t('updates.install'),
      tone: r.newer ? 'primary' : 'danger',
    });
    if (ok) await startOperation(() => api.post('/updates/os/install', { body: { version: r.version } }));
  };
  const patch = (body: { channel?: Channel; auto_update?: boolean }, message: string) =>
    save<UpdatesState>({
      target: live('updates'),
      optimistic: (x) => ({ ...x, auto_update: body.auto_update ?? x.auto_update, os: { ...x.os, channel: body.channel ?? x.os.channel } }),
      send: () => api.patch('/updates', { body }),
      success: message,
    });
  const setChannel = async (ch: Channel) => {
    if (ch === 'dev' && !(await confirm({ title: t('updates.devTitle'), body: t('updates.devBody'), confirm: t('updates.devConfirm') }))) return;
    void patch({ channel: ch }, ch === 'dev' ? t('updates.nowDev') : t('updates.nowStable'));
  };

  const releases = u ? (advanced.value ? u.os.releases : u.os.releases.filter((r) => r.newer)) : [];
  const appUpdates = apps?.updates ?? u?.apps.updates ?? 0;

  return (
    <Page
      title={t('nav.updates')}
      subtitle={t('nav.updatesDesc')}
      actions={
        <Button icon={RefreshCw} busy={checking} onClick={() => void check()}>
          {t('updates.check')}
        </Button>
      }
    >
      {!u ? (
        <Loading lines={6} />
      ) : (
        <>
          <Section
            title={t('updates.hub')}
            icon={ArrowUpFromLine}
            actions={u.os.update_available ? <Status tone="info">{t('updates.available')}</Status> : <Status tone="ok">{t('updates.upToDate')}</Status>}
            description={
              <>
                {t('updates.current', { version: u.os.current })}
                {u.os.checked_at && ` · ${t('updates.checked', { when: relative(u.os.checked_at, hubTime(now)) })}`}
              </>
            }
          >
            {u.os.error && <Banner tone="warn" title={t('updates.checkFailed')}>{advanced.value ? u.os.error : t('updates.checkFailedText')}</Banner>}
            {op && (running || op.state === 'failed' || (op.state === 'succeeded' && op.reboot_needed)) && (
              <div class={c.form} role="status">
                {running ? (
                  <>
                    <p>
                      <strong>{t('updates.installing', { version: op.target ?? '' })}</strong>
                    </p>
                    <ProgressBar value={op.progress} label={t('updates.installing', { version: op.target ?? '' })} />
                    {operationStep(op) && <p class={c.note}>{operationStep(op)}</p>}
                    <p class={c.note}>{t('updates.keepOn')}</p>
                  </>
                ) : op.state === 'failed' ? (
                  <Banner tone="danger" title={t('updates.failed')}>
                    {op.error ?? ''}
                  </Banner>
                ) : (
                  <Banner
                    tone="ok"
                    icon={CircleCheck}
                    title={t('updates.ready')}
                    action={
                      <Button variant="primary" icon={RotateCw} onClick={() => void restartHub()}>
                        {t('restart.hub')}
                      </Button>
                    }
                  >
                    {t('updates.readyText')}
                  </Banner>
                )}
              </div>
            )}
            {releases.length ? (
              <ul class={c.list}>
                {releases.map((r) => (
                  <Release key={r.version} r={r} current={u.os.current} busy={running} onInstall={() => void install(r)} />
                ))}
              </ul>
            ) : (
              <p class="muted">{u.os.update_available ? '' : t('updates.noneText')}</p>
            )}
          </Section>
          <Section title={t('updates.apps')} icon={Boxes}>
            <div class={c.between}>
              <p>{appUpdates ? t('updates.appUpdates', { count: appUpdates }) : t('updates.appsUpToDate')}</p>
              <LinkButton href="/apps" variant={appUpdates ? 'primary' : 'secondary'}>
                {t('updates.toStore')}
              </LinkButton>
            </div>
          </Section>
          <Section title={t('updates.settings')} id="channel">
            <Rows>
              <Switch label={t('updates.auto')} description={t('updates.autoDesc')} checked={u.auto_update} onChange={(v) => void patch({ auto_update: v }, v ? t('updates.autoOn') : t('updates.autoOff'))} />
              <Choice
                label={t('updates.channel')}
                hint={t('updates.channelHint')}
                value={u.os.channel}
                onValue={(v) => void setChannel(v)}
                options={[
                  { value: 'stable', label: t('updates.stable') },
                  { value: 'dev', label: t('updates.dev') },
                ]}
              />
            </Rows>
          </Section>
        </>
      )}
    </Page>
  );
}
