// Serial bridge (ser2net): share up to four serial ports over TCP.
import { useState } from 'preact/hooks';
import { Play, Plus, RotateCcw, RotateCw, Save, ScrollText, Square, Trash, Unplug } from 'lucide-preact';
import { api } from '../../api/client';
import type { SerialBridge as Bridge, SerialBridgePort } from '../../api/types';
import { t } from '../../text/i18n';
import { list as listText, num } from '../../text/format';
import { useGet } from '../../state/get';
import { errorText, save, useFieldErrors } from '../../state/save';
import { toast } from '../../state/toasts';
import { Button, IconButton, LinkButton } from '../../kit/Button';
import { Banner, EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { NumberField, SelectField, Switch } from '../../kit/Field';
import { Actions, Page, Section } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import c from '../common.module.css';
import s from './Settings.module.css';

const BAUDS = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const MAX = 4;

function check(ports: SerialBridgePort[]): Record<string, string> {
  const e: Record<string, string> = {};
  ports.forEach((p, i) => {
    if (!p.device) e[`${i}.device`] = t('form.required');
    if (!p.tcp_port || p.tcp_port < 1 || p.tcp_port > 65535) e[`${i}.tcp_port`] = t('form.range', { min: 1, max: 65535 });
    else if (ports.findIndex((q) => q.tcp_port === p.tcp_port) !== i) e[`${i}.tcp_port`] = t('serial.portTaken');
    if (ports.findIndex((q) => q.device === p.device) !== i) e[`${i}.device`] = t('serial.deviceTwice');
  });
  return e;
}

export default function SerialBridge() {
  const { data, error, reload, sig } = useGet('/settings/serial-bridge');
  const [ports, setPorts] = useState<SerialBridgePort[] | null>(null);
  const [busy, setBusy] = useState<string>();
  const errors = useFieldErrors();
  const list = ports ?? data?.ports ?? [];
  const local = ports ? check(ports) : {};

  const set = (i: number, patch: Partial<SerialBridgePort>) => setPorts(list.map((p, j) => (j === i ? { ...p, ...patch } : p)));
  const add = () => {
    const free = data?.available_ports.find((a) => !a.in_use_by && !list.some((p) => p.device === a.path));
    const nextPort = Math.max(6637, ...list.map((p) => p.tcp_port)) + 1;
    setPorts([...list, { device: free?.path ?? '', tcp_port: list.length ? nextPort : 6638, baud: 115200 }]);
  };
  const submit = async (e: Event) => {
    e.preventDefault();
    if (!ports || Object.keys(check(ports)).length) return;
    const r = await save<Bridge>({ target: sig, send: () => api.patch('/settings/serial-bridge', { body: { ports } }), errors, success: t('common.saved') });
    if (r) setPorts(null);
  };
  const service = async (action: 'start' | 'stop' | 'restart') => {
    setBusy(action);
    try {
      const sv = await api.post('/services/{name}/{action}', { path: { name: data!.service, action } });
      const cur = sig.peek();
      if (cur) sig.value = { ...cur, state: sv.state };
      // ser2net opens its ports just after it counts as started: read them a moment later
      if (action !== 'stop') {
        await new Promise((r) => setTimeout(r, 1000));
        await reload();
      }
    } catch (err) {
      toast({ text: errorText(err), tone: 'danger' });
    }
    setBusy(undefined);
  };
  const running = data?.state === 'started';
  // OpenRC counts ser2net as started even when it refused its file; then its ports stay closed
  const closed = data && running && !busy ? data.ports.filter((p) => p.listening === false) : [];

  return (
    <Page
      title={t('nav.serial')}
      back={{ href: '/settings', label: t('nav.settings') }}
      subtitle={t('nav.serialDesc')}
      narrow
      actions={
        data && (
          <>
            <ServiceStatus state={data.state} />
            {running ? (
              <>
                <Button size="sm" icon={RotateCw} busy={busy === 'restart'} onClick={() => void service('restart')}>
                  {t('apps.restart')}
                </Button>
                <Button size="sm" icon={Square} busy={busy === 'stop'} onClick={() => void service('stop')}>
                  {t('apps.stop')}
                </Button>
              </>
            ) : (
              <Button size="sm" variant="primary" icon={Play} busy={busy === 'start'} onClick={() => void service('start')}>
                {t('apps.start')}
              </Button>
            )}
          </>
        )
      }
    >
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={5} />
      ) : (
        <>
          {closed.length > 0 && (
            <Banner
              tone="warn"
              live
              action={
                <LinkButton size="sm" icon={ScrollText} href="/tools/logs?name=messages&filter=ser2net">
                  {t('serial.log')}
                </LinkButton>
              }
            >
              {t('serial.notListening', { ports: listText(closed.map((p) => String(p.tcp_port))) })}
            </Banner>
          )}
          <Section>
            <Switch
              label={t('serial.autostart')}
              description={t('serial.autostartDesc')}
              checked={data.autostart}
              onChange={(v) =>
                void save<Bridge>({ target: sig, optimistic: (b) => ({ ...b, autostart: v }), send: () => api.patch('/settings/serial-bridge', { body: { autostart: v } }), success: t('common.saved') })
              }
            />
          </Section>
          <Section title={t('serial.ports')} icon={Unplug} description={t('serial.portsDesc')}>
            <form class={c.form} onSubmit={submit} noValidate>
              {list.length ? (
                <ul class={s.portRows}>
                  {list.map((p, i) => (
                    <li key={i} class={s.portRow}>
                      <SelectField
                        label={t('serial.device')}
                        value={p.device}
                        onValue={(v) => set(i, { device: v })}
                        error={local[`${i}.device`]}
                        options={[
                          ...(p.device ? [] : [{ value: '', label: t('common.choose'), disabled: true }]),
                          ...data.available_ports.map((a) => ({
                            value: a.path,
                            label: `${a.label ?? a.path}${a.in_use_by ? ` (${t('serial.usedBy', { app: a.in_use_by })})` : ''}`,
                            disabled: !!a.in_use_by && a.path !== p.device,
                          })),
                          ...(p.device && !data.available_ports.some((a) => a.path === p.device) ? [{ value: p.device, label: p.device }] : []),
                        ]}
                      />
                      <NumberField label={t('serial.tcpPort')} min={1} max={65535} value={p.tcp_port} onValue={(v) => set(i, { tcp_port: v ?? 0 })} error={local[`${i}.tcp_port`]} />
                      <SelectField label={t('serial.baud')} value={String(p.baud)} onValue={(v) => set(i, { baud: Number(v) })} options={BAUDS.map((b) => ({ value: String(b), label: num(b) }))} />
                      <IconButton icon={Trash} label={t('serial.removePort', { n: String(i + 1) })} onClick={() => setPorts(list.filter((_, j) => j !== i))} class={s.portRemove} />
                    </li>
                  ))}
                </ul>
              ) : (
                <EmptyState title={t('serial.none')}>{t('serial.noneText')}</EmptyState>
              )}
              {list.length < MAX && (
                <Button icon={Plus} onClick={add} class={s.addPort}>
                  {t('serial.add')}
                </Button>
              )}
              {ports && (
                <Actions sticky>
                  <Button variant="ghost" icon={RotateCcw} onClick={() => setPorts(null)}>
                    {t('common.discard')}
                  </Button>
                  <Button type="submit" variant="primary" icon={Save} disabled={Object.keys(local).length > 0}>
                    {t('common.save')}
                  </Button>
                </Actions>
              )}
            </form>
          </Section>
        </>
      )}
    </Page>
  );
}
