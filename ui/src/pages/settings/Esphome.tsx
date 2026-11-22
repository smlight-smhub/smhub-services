// ESPHome on the second core: built-in hardware, Bluetooth proxy, and the
// encryption key Home Assistant needs.
import { useState } from 'preact/hooks';
import { Bluetooth, Cpu, Eye, EyeOff, KeyRound, RefreshCw, RotateCw, ScrollText } from 'lucide-preact';
import { api } from '../../api/client';
import type { Esphome as EspState, EsphomePatch } from '../../api/types';
import { t } from '../../text/i18n';
import { confirm } from '../../state/confirm';
import { useGet } from '../../state/get';
import { removeRestartNotices } from '../../state/notices';
import { advanced } from '../../state/prefs';
import { errorText, save, useFieldErrors } from '../../state/save';
import { features } from '../../state/system';
import { toast } from '../../state/toasts';
import { Button, IconButton, LinkButton } from '../../kit/Button';
import { CopyButton } from '../../kit/Bits';
import { Banner, ErrorState, Loading } from '../../kit/Feedback';
import { Choice, Switch } from '../../kit/Field';
import { Page, Rows, Section } from '../../kit/Page';
import { ServiceStatus } from '../../kit/Status';
import c from '../common.module.css';

/** A new ESPHome API key: 32 random bytes in base64. */
export function newKey(): string {
  const b = new Uint8Array(32);
  crypto.getRandomValues(b);
  return btoa(String.fromCharCode(...b));
}

export default function Esphome() {
  const { data, error, reload, sig } = useGet('/settings/esphome');
  const errors = useFieldErrors();
  const [shown, setShown] = useState(false);
  const [busy, setBusy] = useState(false);
  const f = features.value;

  const patch = (body: EsphomePatch, message: string, undo?: EsphomePatch): Promise<EspState | undefined> =>
    save<EspState>({
      target: sig,
      optimistic: (e) => ({ ...e, ...body }),
      send: () => api.patch('/settings/esphome', { body }),
      errors,
      success: message,
      undo: undo ? () => patch(undo, t('common.undone')) : undefined,
      restart: (e) => e.restart_needed && { id: 'esphome', kind: 'esphome', reason: t('restart.esphome.reason') },
    });
  const toggle = (k: 'ambient_light' | 'buzzer' | 'user_i2c', v: boolean) => void patch({ [k]: v }, t('common.saved'), { [k]: !v });
  const regenerate = async () => {
    if (!(await confirm({ title: t('esphome.newKeyTitle'), body: t('esphome.newKeyBody'), confirm: t('esphome.newKey'), tone: 'danger' }))) return;
    await patch({ encryption_key: newKey() }, t('esphome.newKeyDone'));
    setShown(true);
  };
  const restart = async () => {
    setBusy(true);
    try {
      sig.value = await api.post('/settings/esphome/restart');
      removeRestartNotices('esphome');
      toast({ text: t('esphome.restarted'), tone: 'ok' });
    } catch (e) {
      toast({ text: errorText(e), tone: 'danger' });
    }
    setBusy(false);
  };

  return (
    <Page
      title={t('nav.esphome')}
      back={{ href: '/settings', label: t('nav.settings') }}
      subtitle={t('nav.esphomeDesc')}
      narrow
      actions={
        data && (
          <>
            <ServiceStatus state={data.state} />
            <Button size="sm" icon={RotateCw} busy={busy} onClick={() => void restart()}>
              {t('esphome.restart')}
            </Button>
            {advanced.value && (
              <LinkButton size="sm" icon={ScrollText} href="/tools/logs?name=messages&filter=smhub-rtos">
                {t('esphome.log')}
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
      ) : !data.available ? (
        <Banner tone="info">{t('esphome.unavailable')}</Banner>
      ) : (
        <>
          <Section id="key" title={t('esphome.key')} icon={KeyRound} description={t('esphome.keyDesc')}>
            <div class={c.keyBox}>
              <span class={c.keyText} aria-label={shown ? undefined : t('esphome.keyHidden')}>
                {shown ? data.encryption_key : '•'.repeat(44)}
              </span>
              <IconButton icon={shown ? EyeOff : Eye} label={shown ? t('field.hidePassword') : t('field.showPassword')} aria-pressed={shown} onClick={() => setShown(!shown)} />
              <CopyButton text={data.encryption_key} label={t('esphome.copyKey')} />
            </div>
            {errors.value.encryption_key && <Banner tone="danger">{errors.value.encryption_key}</Banner>}
            <div>
              <Button icon={RefreshCw} onClick={() => void regenerate()}>
                {t('esphome.newKey')}
              </Button>
            </div>
          </Section>
          <Section
            title={t('esphome.hardware')}
            icon={Cpu}
            description={t('esphome.hardwareDesc')}
            actions={
              <>
                <LinkButton size="sm" variant="ghost" href="/peripherals/lights">
                  {t('nav.lights')}
                </LinkButton>
                {f?.buzzer !== false && (
                  <LinkButton size="sm" variant="ghost" href="/peripherals/buzzer">
                    {t('nav.buzzer')}
                  </LinkButton>
                )}
              </>
            }
          >
            <Rows>
              {f?.ambient_light !== false && <Switch label={t('esphome.light')} description={t('esphome.lightDesc')} checked={data.ambient_light} onChange={(v) => toggle('ambient_light', v)} />}
              {f?.buzzer !== false && <Switch label={t('esphome.buzzer')} description={t('esphome.buzzerDesc')} checked={data.buzzer} onChange={(v) => toggle('buzzer', v)} />}
              <Switch label={t('esphome.i2c')} description={t('esphome.i2cDesc')} checked={data.user_i2c} onChange={(v) => toggle('user_i2c', v)} />
            </Rows>
          </Section>
          {f?.bluetooth !== false && (
            <Section title={t('esphome.bluetooth')} icon={Bluetooth}>
              <Choice
                label={t('esphome.bluetooth')}
                hideLabel
                variant="cards"
                value={data.bluetooth_proxy_mode}
                onValue={(v) => void patch({ bluetooth_proxy_mode: v }, t('common.saved'), { bluetooth_proxy_mode: data.bluetooth_proxy_mode })}
                options={[
                  { value: 'off', label: t('esphome.btOff'), hint: t('esphome.btOffHint') },
                  { value: 'proxy', label: t('esphome.btProxy'), hint: t('esphome.btProxyHint') },
                  { value: 'hci', label: t('esphome.btHci'), hint: t('esphome.btHciHint') },
                ]}
              />
            </Section>
          )}
        </>
      )}
    </Page>
  );
}
