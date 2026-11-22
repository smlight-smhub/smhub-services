// SSH: how people may sign in over SSH, and the device user's public keys.
import { useState } from 'preact/hooks';
import { KeyRound, Plus, Terminal, Trash } from 'lucide-preact';
import { api } from '../../api/client';
import type { Ssh as SshState, SshPatch } from '../../api/types';
import { t } from '../../text/i18n';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { save, useFieldErrors } from '../../state/save';
import { Button, IconButton } from '../../kit/Button';
import { EmptyState, ErrorState, Loading } from '../../kit/Feedback';
import { Switch, TextArea } from '../../kit/Field';
import { Actions, Page, Rows, Section } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import c from '../common.module.css';

const KEY = /^(ssh-(rsa|ed25519|dss)|ecdsa-sha2-nistp\d+|sk-(ssh-ed25519|ecdsa-sha2-nistp256)@openssh\.com)\s+[A-Za-z0-9+/=]+(\s+.*)?$/;

export default function Ssh() {
  const { data, error, reload, sig } = useGet('/settings/ssh');
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const keyError = key.trim() && !KEY.test(key.trim()) ? t('ssh.badKey') : undefined;

  const toggle = (k: keyof SshPatch, v: boolean, message: string) => {
    const go = (val: boolean, msg: string, undo: boolean): Promise<unknown> =>
      save<SshState>({ target: sig, optimistic: (s) => ({ ...s, [k]: val }), send: () => api.patch('/settings/ssh', { body: { [k]: val } }), errors, success: msg, undo: undo ? () => go(!val, t('common.undone'), false) : undefined });
    void go(v, message, true);
  };
  const add = async (e: Event) => {
    e.preventDefault();
    if (!key.trim() || keyError) return;
    setBusy(true);
    const r = await save<SshState>({ target: sig, send: () => api.post('/settings/ssh/keys', { body: { key: key.trim() } }), errors, success: t('ssh.keyAdded') });
    setBusy(false);
    if (r) setKey('');
  };
  const remove = async (fingerprint: string, label: string) => {
    if (!(await confirm({ title: t('ssh.removeTitle'), body: t('ssh.removeBody', { key: label }), confirm: t('ssh.remove'), tone: 'danger' }))) return;
    await save<SshState>({ target: sig, send: () => api.del('/settings/ssh/keys', { query: { fingerprint } }), success: t('ssh.keyRemoved') });
  };

  return (
    <Page title={t('nav.ssh')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.sshDesc')} narrow actions={data && <ServiceStatus state={data.state} />}>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading lines={5} />
      ) : (
        <>
          <Section title={t('ssh.access')} icon={Terminal}>
            <Rows>
              <Switch label={t('ssh.passwordAuth')} description={t('ssh.passwordAuthDesc')} checked={data.password_auth} onChange={(v) => toggle('password_auth', v, t('common.saved'))} error={errors.value.password_auth} />
              <Switch label={t('ssh.pubkeyAuth')} description={t('ssh.pubkeyAuthDesc')} checked={data.pubkey_auth} onChange={(v) => toggle('pubkey_auth', v, t('common.saved'))} error={errors.value.pubkey_auth} />
              <Switch label={t('ssh.rootLogin')} description={t('ssh.rootLoginDesc')} checked={data.root_login} onChange={(v) => toggle('root_login', v, t('common.saved'))} error={errors.value.root_login} />
            </Rows>
          </Section>
          <Section id="keys" title={t('ssh.keys')} icon={KeyRound} description={t('ssh.keysDesc')}>
            {data.keys.length ? (
              <ul class={c.list}>
                {data.keys.map((k) => (
                  <li key={k.fingerprint} class={c.item}>
                    <KeyRound size={18} class={c.itemIcon} aria-hidden="true" />
                    <div class={c.itemMain}>
                      <span class={c.itemTitle}>
                        {k.comment || k.type}
                        <span class={c.tag}>{k.type}</span>
                      </span>
                      <span class={`${c.itemMeta} mono`}>{k.fingerprint}</span>
                    </div>
                    <IconButton icon={Trash} label={t('ssh.removeNamed', { key: k.comment || k.fingerprint })} onClick={() => void remove(k.fingerprint, k.comment || k.fingerprint)} />
                  </li>
                ))}
              </ul>
            ) : (
              <EmptyState title={t('ssh.noKeys')} />
            )}
            <form class={c.form} onSubmit={add}>
              <TextArea label={t('ssh.addKey')} hint={t('ssh.addKeyHint')} mono rows={3} value={key} onValue={setKey} placeholder="ssh-ed25519 AAAA… user@laptop" error={keyError ?? errors.value.key} />
              <Actions>
                <Button type="submit" variant="primary" icon={Plus} busy={busy} disabled={!key.trim() || !!keyError}>
                  {t('ssh.add')}
                </Button>
              </Actions>
            </form>
          </Section>
        </>
      )}
    </Page>
  );
}
