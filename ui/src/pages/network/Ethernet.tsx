// Ethernet: live status and address settings.
import { Cable, RotateCcw, Save } from 'lucide-preact';
import { api } from '../../api/client';
import type { IpSettings } from '../../api/types';
import { t } from '../../text/i18n';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { advanced } from '../../state/prefs';
import { save, useDraft, useFieldErrors } from '../../state/save';
import { useLive } from '../../state/store';
import { Button } from '../../kit/Button';
import { ErrorState, Loading } from '../../kit/Feedback';
import { Actions, Facts, Page, Section } from '../../kit/Page';
import { checkIp, cleanIp, IpFields } from './IpFields';
import { IfaceStatus, ifaceFacts } from './Overview';
import c from '../common.module.css';

export default function Ethernet() {
  const n = useLive('network');
  const { data, error, reload, sig } = useGet('/network/ethernet');
  const d = useDraft<IpSettings>(data);
  const errors = useFieldErrors();
  const eth = n?.interfaces.find((i) => i.kind === 'ethernet');
  const local = d.draft ? checkIp(d.draft) : {};

  const submit = async (e: Event) => {
    e.preventDefault();
    if (!d.draft || Object.keys(local).length) return;
    const body = cleanIp(d.draft);
    const risky = body.method !== data?.method || JSON.stringify(body.addresses) !== JSON.stringify(data?.addresses ?? []);
    if (risky && !(await confirm({ title: t('eth.confirmTitle'), body: t('eth.confirmBody'), confirm: t('common.save') }))) return;
    const ok = await save<IpSettings>({ target: sig, send: () => api.patch('/network/ethernet', { body }), errors, success: t('common.saved') });
    if (ok) d.done();
  };

  return (
    <Page title={t('nav.ethernet')} back={{ href: '/network', label: t('nav.network') }} subtitle={t('nav.ethernetDesc')}>
      <Section title={t('eth.status')} icon={Cable} actions={eth && <IfaceStatus state={eth.state} />}>
        {eth ? <Facts items={ifaceFacts(eth, advanced.value)} /> : n ? <p class="muted">{t('eth.none')}</p> : <Loading />}
      </Section>
      <Section title={t('eth.settings')} id="settings">
        {error && !data ? (
          <ErrorState error={error} retry={reload} />
        ) : !d.draft ? (
          <Loading lines={4} />
        ) : (
          <form class={c.form} onSubmit={submit} noValidate>
            <IpFields value={d.draft} onChange={d.replace} errors={{ ...errors.value, ...(d.dirty ? local : {}) }} />
            {d.dirty && (
              <Actions sticky>
                <Button variant="ghost" icon={RotateCcw} onClick={d.reset}>
                  {t('common.discard')}
                </Button>
                <Button type="submit" variant="primary" icon={Save} disabled={Object.keys(local).length > 0}>
                  {t('common.save')}
                </Button>
              </Actions>
            )}
          </form>
        )}
      </Section>
    </Page>
  );
}
