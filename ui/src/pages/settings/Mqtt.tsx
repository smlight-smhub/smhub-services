// The MQTT broker (Mosquitto): access, accounts and a bridge to another broker.
import { useState } from 'preact/hooks';
import { KeyRound, Pencil, RotateCcw, Save, ScrollText, Trash, UserPlus, Users, Workflow } from 'lucide-preact';
import { api } from '../../api/client';
import type { Mqtt as MqttState, MqttPatch } from '../../api/types';
import { t } from '../../text/i18n';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { advanced } from '../../state/prefs';
import { save, useDraft, useFieldErrors } from '../../state/save';
import { Button, IconButton, LinkButton } from '../../kit/Button';
import { Dialog } from '../../kit/Dialog';
import { EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { FieldGrid, ListField, NumberField, Switch, TextField } from '../../kit/Field';
import { Actions, Page, Rows, Section } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import c from '../common.module.css';

type Bridge = NonNullable<MqttPatch['bridge']>;

function UserDialog({ name, onClose, onSaved }: { name: string | 'new' | null; onClose: () => void; onSaved: (m: MqttState) => void }) {
  const isNew = name === 'new';
  const [username, setUsername] = useState('');
  const [pw, setPw] = useState('');
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const submit = async () => {
    const u = isNew ? username.trim() : (name as string);
    if (!u || !pw) return;
    setBusy(true);
    const r = await save({ send: () => api.post('/settings/mqtt/users', { body: { username: u, password: pw } }), errors, success: isNew ? t('mqtt.userAdded', { name: u }) : t('mqtt.passwordChanged') });
    setBusy(false);
    if (r) {
      onSaved(r);
      setUsername('');
      setPw('');
      onClose();
    }
  };
  return (
    <Dialog
      open={!!name}
      onClose={onClose}
      onSubmit={submit}
      size="sm"
      title={isNew ? t('mqtt.addUser') : t('mqtt.changePassword', { name: name ?? '' })}
      footer={
        <>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!pw || (isNew && !username.trim())}>
            {t('common.save')}
          </Button>
        </>
      }
    >
      {isNew && <TextField label={t('mqtt.username')} value={username} onValue={setUsername} maxLength={64} required autoCapitalize="off" autoComplete="off" error={errors.value.username} autoFocus />}
      <TextField label={t('mqtt.password')} type="password" value={pw} onValue={setPw} maxLength={128} required autoComplete="new-password" error={errors.value.password} autoFocus={!isNew} />
    </Dialog>
  );
}

export default function Mqtt() {
  const { data, error, reload, sig } = useGet('/settings/mqtt');
  const [user, setUser] = useState<string | 'new' | null>(null);
  const errors = useFieldErrors();
  const bridge = useDraft<Bridge>(data ? { ...data.bridge, password: '' } : undefined);
  const port = useDraft<{ port: number | null }>(data ? { port: data.port } : undefined);

  const patch = (body: MqttPatch, message: string, undo?: () => unknown) =>
    save<MqttState>({
      target: sig,
      optimistic: (m) => ({ ...m, ...(body.allow_external !== undefined ? { allow_external: body.allow_external } : {}), ...(body.allow_anonymous !== undefined ? { allow_anonymous: body.allow_anonymous } : {}) }),
      send: () => api.patch('/settings/mqtt', { body }),
      errors,
      success: message,
      undo,
    });
  const toggle = async (key: 'allow_external' | 'allow_anonymous', v: boolean, message: string) => {
    // apps on the hub such as Zigbee2MQTT connect without a password unless given an account
    if (key === 'allow_anonymous' && !v && !(await confirm({ title: t('mqtt.anonymousOffTitle'), body: t('mqtt.anonymousOffBody'), confirm: t('mqtt.anonymousOffConfirm'), tone: 'danger' }))) return;
    void patch({ [key]: v }, message, () => patch({ [key]: !v }, t('common.undone')));
  };
  const removeUser = async (u: string) => {
    if (!(await confirm({ title: t('mqtt.removeTitle', { name: u }), body: t('mqtt.removeBody'), confirm: t('mqtt.remove'), tone: 'danger' }))) return;
    await save<MqttState>({ target: sig, send: () => api.del('/settings/mqtt/users', { query: { username: u } }), success: t('mqtt.removed', { name: u }) });
  };
  const saveBridge = async (e: Event) => {
    e.preventDefault();
    const b = bridge.draft!;
    const body: Bridge = { ...b, topics: (b.topics ?? []).filter(Boolean) };
    if (!b.password) delete body.password;
    if (await patch({ bridge: body }, t('common.saved'))) bridge.done();
  };
  const savePort = async (e: Event) => {
    e.preventDefault();
    const p = port.draft?.port;
    if (!p || p < 1 || p > 65535) return;
    if (await patch({ port: p }, t('common.saved'))) port.done();
  };

  return (
    <Page
      title={t('nav.mqtt')}
      back={{ href: '/settings', label: t('nav.settings') }}
      subtitle={t('nav.mqttDesc')}
      narrow
      actions={
        data && (
          <>
            <ServiceStatus state={data.state} />
            {advanced.value && (
              <LinkButton href="/tools/logs?name=mosquitto" size="sm" icon={ScrollText}>
                {t('mqtt.log')}
              </LinkButton>
            )}
          </>
        )
      }
    >
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={6} />
      ) : (
        <>
          <Section title={t('mqtt.access')} icon={Workflow}>
            <Rows>
              <Switch label={t('mqtt.external')} description={t('mqtt.externalDesc')} checked={data.allow_external} onChange={(v) => toggle('allow_external', v, v ? t('mqtt.externalOn') : t('mqtt.externalOff'))} error={errors.value.allow_external} />
              <Switch label={t('mqtt.anonymous')} description={t('mqtt.anonymousDesc')} checked={data.allow_anonymous} onChange={(v) => toggle('allow_anonymous', v, v ? t('mqtt.anonymousOn') : t('mqtt.anonymousOff'))} error={errors.value.allow_anonymous} />
            </Rows>
            {advanced.value && port.draft && (
              <form class={c.form} onSubmit={savePort}>
                <NumberField label={t('mqtt.port')} hint={t('mqtt.portHint')} min={1} max={65535} value={port.draft.port} onValue={(v) => port.set('port', v)} error={errors.value.port} />
                {port.dirty && (
                  <Actions>
                    <Button variant="ghost" onClick={port.reset}>
                      {t('common.discard')}
                    </Button>
                    <Button type="submit" variant="primary" icon={Save}>
                      {t('common.save')}
                    </Button>
                  </Actions>
                )}
              </form>
            )}
          </Section>
          <Section
            id="users"
            title={t('mqtt.users')}
            icon={Users}
            description={t('mqtt.usersDesc')}
            actions={
              <Button size="sm" icon={UserPlus} onClick={() => setUser('new')}>
                {t('mqtt.addUser')}
              </Button>
            }
          >
            {data.users.length ? (
              <ul class={c.list}>
                {data.users.map((u) => (
                  <li key={u} class={c.item}>
                    <KeyRound size={18} class={c.itemIcon} aria-hidden="true" />
                    <span class={`${c.itemMain} ${c.itemTitle}`}>{u}</span>
                    <span class={c.itemActions}>
                      <IconButton icon={Pencil} label={t('mqtt.changePassword', { name: u })} onClick={() => setUser(u)} />
                      <IconButton icon={Trash} label={t('mqtt.removeNamed', { name: u })} onClick={() => void removeUser(u)} />
                    </span>
                  </li>
                ))}
              </ul>
            ) : (
              <EmptyState title={t('mqtt.noUsers')}>{t('mqtt.noUsersText')}</EmptyState>
            )}
          </Section>
          {advanced.value && bridge.draft && (
            <Section id="bridge" title={t('mqtt.bridge')} description={t('mqtt.bridgeDesc')}>
              <form class={c.form} onSubmit={saveBridge}>
                <Switch label={t('mqtt.bridgeOn')} checked={!!bridge.draft.enabled} onChange={(v) => bridge.set('enabled', v)} />
                {bridge.draft.enabled && (
                  <>
                    <FieldGrid>
                      <TextField label={t('mqtt.bridgeName')} value={bridge.draft.connection ?? ''} onValue={(v) => bridge.set('connection', v)} error={errors.value['bridge.connection']} />
                      <TextField label={t('mqtt.bridgeAddress')} mono placeholder="broker.example.com:1883" value={bridge.draft.address ?? ''} onValue={(v) => bridge.set('address', v)} error={errors.value['bridge.address']} />
                      <TextField label={t('mqtt.username')} value={bridge.draft.username ?? ''} onValue={(v) => bridge.set('username', v)} autoComplete="off" />
                      <TextField
                        label={t('mqtt.password')}
                        type="password"
                        value={bridge.draft.password ?? ''}
                        onValue={(v) => bridge.set('password', v)}
                        placeholder={data.bridge.password_set ? t('form.secretSet') : ''}
                        autoComplete="new-password"
                      />
                    </FieldGrid>
                    <ListField label={t('mqtt.topics')} hint={t('mqtt.topicsHint')} placeholder="zigbee2mqtt/# both 0" value={bridge.draft.topics ?? []} onValue={(v) => bridge.set('topics', v)} error={errors.value['bridge.topics']} />
                    <Switch label={t('mqtt.cleanSession')} description={t('mqtt.cleanSessionDesc')} checked={!!bridge.draft.clean_session} onChange={(v) => bridge.set('clean_session', v)} />
                  </>
                )}
                {bridge.dirty && (
                  <Actions sticky>
                    <Button variant="ghost" icon={RotateCcw} onClick={bridge.reset}>
                      {t('common.discard')}
                    </Button>
                    <Button type="submit" variant="primary" icon={Save}>
                      {t('common.save')}
                    </Button>
                  </Actions>
                )}
              </form>
            </Section>
          )}
        </>
      )}
      <UserDialog name={user} onClose={() => setUser(null)} onSaved={(m) => (sig.value = m)} />
    </Page>
  );
}
