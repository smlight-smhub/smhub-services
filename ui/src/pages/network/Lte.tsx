// The 4G modem: registration, signal, power and connection, and its settings.
import { useState } from 'preact/hooks';
import { Link2, Link2Off, Power, RefreshCw, RotateCcw, Save, Smartphone } from 'lucide-preact';
import { api } from '../../api/client';
import type { Lte as LteState, LteSettings } from '../../api/types';
import { t } from '../../text/i18n';
import { num } from '../../text/format';
import { addRestartNotice } from '../../state/notices';
import { advanced } from '../../state/prefs';
import { errorText, save, useDraft, useFieldErrors } from '../../state/save';
import { live, setLive, useLive } from '../../state/store';
import { toast } from '../../state/toasts';
import { Button } from '../../kit/Button';
import { SignalBars } from '../../kit/Bits';
import { Banner, Loading } from '../../kit/Feedback';
import { Switch, TextField } from '../../kit/Field';
import { Actions, Facts, Page, Section } from '../../kit/Page';
import { Status, type StatusTone } from '../../kit/Status';
import c from '../common.module.css';

const TONE: Record<LteState['state'], StatusTone> = { absent: 'neutral', off: 'neutral', searching: 'busy', registered: 'ok', roaming: 'warn', denied: 'danger', connected: 'ok', error: 'danger' };

export default function Lte() {
  const m = useLive('lte');
  const d = useDraft<LteSettings>(m?.settings);
  const errors = useFieldErrors();
  const [busy, setBusy] = useState<string>();

  const act = async (action: 'power' | 'reset' | 'connect' | 'disconnect' | 'refresh') => {
    setBusy(action);
    try {
      setLive('lte', await api.post('/lte/{action}', { path: { action } }));
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setBusy(undefined);
  };
  const hostMode = async () => {
    setBusy('usb');
    const r = await save({
      send: () => api.patch('/settings/usb', { body: { mode: 'host' } }),
      success: t('lte.usbSwitched'),
      restart: (u) => (!!u.pending_mode && u.pending_mode !== u.mode ? { id: 'usb-mode', kind: 'hub', reason: t('restart.usbMode') } : null),
    });
    if (r && !r.pending_mode) addRestartNotice({ id: 'usb-mode', kind: 'hub', reason: t('restart.usbMode') });
    setBusy(undefined);
  };
  const submit = async (e: Event) => {
    e.preventDefault();
    const ok = await save<LteState>({
      target: live('lte'),
      optimistic: (x) => ({ ...x, settings: { ...x.settings, ...d.changes() } }),
      send: () => api.patch('/lte', { body: d.changes() }),
      errors,
      success: t('common.saved'),
    });
    if (ok) d.done();
  };

  return (
    <Page title={t('nav.lte')} back={{ href: '/network', label: t('nav.network') }} subtitle={t('nav.lteDesc')}>
      {!m ? (
        <Loading lines={5} />
      ) : (
        <>
          {m.usb_host_needed && (
            <Banner tone="warn" title={t('lte.usbTitle')} action={<Button size="sm" variant="primary" busy={busy === 'usb'} onClick={() => void hostMode()}>{t('lte.usbAction')}</Button>}>
              {t('lte.usbText')}
            </Banner>
          )}
          <Section title={t('lte.status')} icon={Smartphone} actions={<Status tone={TONE[m.state]}>{t(`lte.state.${m.state}`)}</Status>}>
            {!m.present ? (
              <p class="muted">{t('lte.absent')}</p>
            ) : (
              <>
                <Facts
                  items={[
                    [t('lte.operator'), m.operator],
                    [t('lte.technology'), m.technology],
                    [
                      t('lte.signal'),
                      m.signal_percent !== null && m.signal_percent !== undefined ? (
                        <span class={c.inline}>
                          <SignalBars value={m.signal_percent} />
                          {m.signal_dbm !== null && m.signal_dbm !== undefined && <span class="num">{t('unit.dbm', { value: num(m.signal_dbm) })}</span>}
                        </span>
                      ) : null,
                    ],
                    [t('lte.imei'), advanced.value && m.imei && <span class="mono">{m.imei}</span>],
                  ]}
                />
                <Actions>
                  <Button icon={RefreshCw} busy={busy === 'refresh'} onClick={() => void act('refresh')}>
                    {t('lte.refresh')}
                  </Button>
                  {advanced.value && (
                    <Button icon={RotateCcw} busy={busy === 'reset'} onClick={() => void act('reset')}>
                      {t('lte.reset')}
                    </Button>
                  )}
                  {!m.powered && (
                    <Button icon={Power} busy={busy === 'power'} onClick={() => void act('power')}>
                      {t('lte.powerOn')}
                    </Button>
                  )}
                  {m.powered &&
                    (m.state === 'connected' ? (
                      <Button icon={Link2Off} busy={busy === 'disconnect'} onClick={() => void act('disconnect')}>
                        {t('lte.disconnect')}
                      </Button>
                    ) : (
                      <Button variant="primary" icon={Link2} busy={busy === 'connect'} onClick={() => void act('connect')}>
                        {t('lte.connect')}
                      </Button>
                    ))}
                </Actions>
              </>
            )}
          </Section>
          {d.draft && (
            <Section title={t('lte.settings')}>
              <form class={c.form} onSubmit={submit}>
                <TextField label={t('lte.apn')} hint={t('lte.apnHint')} mono value={d.draft.apn ?? ''} onValue={(v) => d.set('apn', v)} error={errors.value.apn} />
                <Switch label={t('lte.autostart')} description={t('lte.autostartDesc')} checked={!!d.draft.autostart} onChange={(v) => d.set('autostart', v)} />
                <Switch label={t('lte.led')} description={t('lte.ledDesc')} checked={!!d.draft.led} onChange={(v) => d.set('led', v)} />
                {d.dirty && (
                  <Actions sticky>
                    <Button variant="ghost" icon={RotateCcw} onClick={d.reset}>
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
    </Page>
  );
}
