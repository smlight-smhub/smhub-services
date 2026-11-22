// Services: every OpenRC service of the hub and its apps, live, with start,
// stop and restart.
import { useState } from 'preact/hooks';
import { Link } from 'wouter-preact';
import { Play, RotateCw, Search, Square } from 'lucide-preact';
import { api } from '../../api/client';
import type { Service } from '../../api/types';
import { t } from '../../text/i18n';
import { compare } from '../../text/format';
import { confirm } from '../../state/confirm';
import { errorText } from '../../state/save';
import { live, setLive, useLive } from '../../state/store';
import { toast } from '../../state/toasts';
import { IconButton, Transition, transitionOf } from '../../kit/Button';
import { appTitle } from '../../kit/Bits';
import { Loading } from '../../kit/Feedback';
import { Choice, TextField } from '../../kit/Field';
import { Page, Section } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import c from '../common.module.css';

/** Services the hub's own UI and API depend on: changing them needs a second thought. */
const CORE = new Set(['smhub-services', 'networkmanager', 'stunnel', 'nginx', 'tailscale', 'sshd']);
/** The hub refuses to stop these (403); they can only be restarted. */
const NO_STOP = new Set(['networkmanager', 'sshd', 'nginx', 'stunnel']);
/** The hub's own service: the hub refuses every action on it (403). */
const SELF = 'smhub-services';

export default function Services() {
  const data = useLive('services');
  const [q, setQ] = useState('');
  const [kind, setKind] = useState<'all' | 'app' | 'system'>('all');
  const [busy, setBusy] = useState<string>();
  const act = async (sv: Service, action: 'start' | 'stop' | 'restart') => {
    if (action !== 'start' && CORE.has(sv.name) && !(await confirm({ title: t('services.coreTitle', { name: sv.name }), body: t('services.coreBody'), confirm: action === 'stop' ? t('apps.stop') : t('apps.restart'), tone: 'danger' }))) return;
    setBusy(sv.name + action);
    try {
      const next = await api.post('/services/{name}/{action}', { path: { name: sv.name, action } });
      const cur = live('services').peek();
      if (cur) setLive('services', { services: cur.services.map((x) => (x.name === next.name ? next : x)) });
      const done = { start: 'services.started', stop: 'services.stopped', restart: 'services.restarted' } as const;
      toast({ text: t(done[action], { name: sv.name }), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setBusy(undefined);
  };
  const needle = q.trim().toLowerCase();
  const list = [...(data?.services ?? [])]
    .filter((sv) => (kind === 'all' || sv.kind === kind) && (!needle || `${sv.name} ${sv.app ?? ''}`.toLowerCase().includes(needle)))
    .sort((a, b) => compare(a.name, b.name));
  return (
    <Page title={t('nav.services')} subtitle={t('nav.servicesDesc')}>
      <div class={c.toolbar}>
        <TextField label={t('services.search')} hideLabel type="search" icon={Search} value={q} onValue={setQ} placeholder={t('services.search')} class={c.grow} />
        <Choice
          label={t('services.kind')}
          hideLabel
          value={kind}
          onValue={setKind}
          options={[
            { value: 'all', label: t('services.all') },
            { value: 'app', label: t('services.apps') },
            { value: 'system', label: t('services.system') },
          ]}
        />
      </div>
      <Section>
        {!data ? (
          <Loading lines={8} />
        ) : (
          <div class={c.tableWrap}>
            <table class={c.table}>
              <thead>
                <tr>
                  <th>{t('services.name')}</th>
                  <th>{t('services.state')}</th>
                  <th>{t('services.autostart')}</th>
                  <th class={c.end}>
                    <span class="visually-hidden">{t('files.actions')}</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {list.map((sv) => {
                  const running = sv.state === 'started' || sv.state === 'starting';
                  return (
                    <tr key={sv.name}>
                      <td>
                        <div class={c.itemMain}>
                          <span class="mono">{sv.name}</span>
                          {sv.app && (
                            <Link href={`/apps/${sv.app}`} class={c.itemMeta}>
                              {appTitle(sv.app)}
                            </Link>
                          )}
                        </div>
                      </td>
                      <td>
                        <ServiceStatus state={sv.state} size="sm" />
                      </td>
                      <td class="muted">{sv.autostart ? t('common.yes') : t('common.no')}</td>
                      <td class={c.end}>
                        <span class={c.itemActions}>
                          {SELF === sv.name ? null : transitionOf(sv.state) ? (
                            <Transition compact state={transitionOf(sv.state)!} name={sv.name} />
                          ) : running ? (
                            <>
                              <IconButton size="sm" icon={RotateCw} label={t('services.restartNamed', { name: sv.name })} busy={busy === sv.name + 'restart'} onClick={() => void act(sv, 'restart')} />
                              {!NO_STOP.has(sv.name) && (
                                <IconButton size="sm" icon={Square} label={t('services.stopNamed', { name: sv.name })} busy={busy === sv.name + 'stop'} onClick={() => void act(sv, 'stop')} />
                              )}
                            </>
                          ) : (
                            <IconButton size="sm" icon={Play} label={t('services.startNamed', { name: sv.name })} busy={busy === sv.name + 'start'} onClick={() => void act(sv, 'start')} />
                          )}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </Section>
    </Page>
  );
}
