// Extra panels some apps have (`extras` in the contract): Tailscale sign-in
// and the Thread border router's state.
import { useState } from 'preact/hooks';
import { KeyRound, LogOut, Network } from 'lucide-preact';
import { api } from '../../api/client';
import { t } from '../../text/i18n';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { advanced } from '../../state/prefs';
import { save, useFieldErrors } from '../../state/save';
import { Button } from '../../kit/Button';
import { CopyValue } from '../../kit/Bits';
import { ErrorState, Loading } from '../../kit/Feedback';
import { TextField } from '../../kit/Field';
import { Actions, Facts, Section } from '../../kit/Page';
import { Status } from '../../kit/Status';

export function TailscalePanel({ name }: { name: string }) {
  const { data, error, reload, sig } = useGet('/apps/{name}/tailscale', { path: { name } });
  const [key, setKey] = useState('');
  const [busy, setBusy] = useState(false);
  const errors = useFieldErrors();
  const act = async (action: 'login' | 'logout') => {
    if (action === 'logout' && !(await confirm({ title: t('ts.logoutTitle'), body: t('ts.logoutBody'), confirm: t('ts.logout'), tone: 'danger' }))) return;
    setBusy(true);
    const ok = await save({
      target: sig,
      send: () => api.post('/apps/{name}/tailscale/{action}', { path: { name, action }, body: action === 'login' ? { auth_key: key.trim() } : {} }),
      errors,
      success: action === 'login' ? t('ts.loggedIn') : t('ts.loggedOut'),
    });
    if (ok) setKey('');
    setBusy(false);
  };
  return (
    <Section title={t('ts.title')} icon={Network} description={t('ts.description')}>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading />
      ) : (
        <>
          <Facts
            items={[
              [t('ts.state'), data.state === 'Running' ? <Status tone="ok">{t('ts.connected')}</Status> : <Status tone="warn">{data.state}</Status>],
              [t('ts.online'), data.online === null || data.online === undefined ? null : data.online ? t('common.yes') : t('common.no')],
              [t('ts.hostname'), data.hostname && <span class="mono">{data.hostname}</span>],
              [t('ts.addresses'), data.addresses?.length ? <CopyValue value={data.addresses[0]!} label={t('common.copy')} /> : null],
              [t('ts.allAddresses'), advanced.value && (data.addresses?.length ?? 0) > 1 && <span class="mono">{data.addresses!.join(', ')}</span>],
            ]}
          />
          {data.state === 'Running' ? (
            <Actions>
              <Button variant="danger" icon={LogOut} busy={busy} onClick={() => void act('logout')}>
                {t('ts.logout')}
              </Button>
            </Actions>
          ) : (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void act('login');
              }}
            >
              <TextField label={t('ts.authKey')} hint={t('ts.authKeyHint')} type="password" mono value={key} onValue={setKey} error={errors.value.auth_key} autoComplete="off" />
              <Actions>
                <Button type="submit" variant="primary" icon={KeyRound} busy={busy} disabled={!key.trim()}>
                  {t('ts.login')}
                </Button>
              </Actions>
            </form>
          )}
        </>
      )}
    </Section>
  );
}

function show(v: unknown): string {
  if (v === null || v === undefined) return '–';
  if (typeof v === 'object') return JSON.stringify(v);
  return String(v);
}

export function ThreadPanel({ name }: { name: string }) {
  const { data, error, reload } = useGet('/apps/{name}/thread', { path: { name } });
  const entries = Object.entries(data ?? {});
  const main = ['state', 'role', 'networkName', 'network_name', 'channel', 'panId', 'pan_id', 'extPanId', 'ext_pan_id', 'numOfRouter', 'rloc16'];
  const shown = advanced.value ? entries : entries.filter(([k]) => main.includes(k));
  return (
    <Section title={t('thread.title')} icon={Network} description={t('thread.description')}>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !data ? (
        <Loading />
      ) : (
        <Facts items={shown.map(([k, v]) => [k, <span class="mono">{show(v)}</span>])} />
      )}
    </Section>
  );
}
