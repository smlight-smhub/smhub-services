// General: the hub's name, its time zone and the SD-card configuration.
import { Clock, Save, Tag } from 'lucide-preact';
import { api } from '../../api/client';
import type { GeneralSettings } from '../../api/types';
import { t } from '../../text/i18n';
import { dateTime } from '../../text/format';
import { useGet } from '../../state/get';
import { advanced } from '../../state/prefs';
import { save, useDraft, useFieldErrors } from '../../state/save';
import { setLive, live, useLive } from '../../state/store';
import { features, hubTime, useNow } from '../../state/system';
import { Button } from '../../kit/Button';
import { ErrorState, Loading } from '../../kit/Feedback';
import { Switch, TextField } from '../../kit/Field';
import { Actions, Page, Section } from '../../kit/Page';
import { SearchSelect } from '../../kit/SearchSelect';
import c from '../common.module.css';

export const HOSTNAME = /^[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/;

/** Saves general settings and keeps `system` in step (name and zone show everywhere). */
export function saveGeneral(sig: { value: GeneralSettings | undefined; peek(): GeneralSettings | undefined }, patch: GeneralSettings, errors?: Parameters<typeof save>[0]['errors'], message?: string, undo?: () => unknown) {
  return save<GeneralSettings>({
    target: sig as never,
    optimistic: (g) => ({ ...g, ...patch }),
    send: () => api.patch('/settings/general', { body: patch }),
    errors,
    success: message,
    undo,
  }).then((r) => {
    const sys = live('system').peek();
    if (r && sys) setLive('system', { ...sys, hostname: r.hostname ?? sys.hostname, timezone: r.timezone ?? sys.timezone });
    return r;
  });
}

export default function General() {
  const { data, error, reload, sig } = useGet('/settings/general');
  const zones = useGet('/settings/timezones');
  const d = useDraft<GeneralSettings>(data);
  const errors = useFieldErrors();
  const now = useNow('minute');
  // Live, so the note goes away as soon as a card is inserted (features are cached).
  const metrics = useLive('metrics');
  const cardIn = metrics ? metrics.storage.some((st) => st.id === 'sdcard') : !!features.value?.sdcard;
  const name = d.draft?.hostname ?? '';
  const nameError = name && !HOSTNAME.test(name) ? t('general.nameInvalid') : undefined;

  const saveName = async (e: Event) => {
    e.preventDefault();
    if (nameError || !name) return;
    const r = await saveGeneral(sig, { hostname: name }, errors, t('general.nameSaved', { name }));
    if (r) d.done();
  };
  const setZone = (tz: string) => {
    const before = data?.timezone;
    void saveGeneral(sig, { timezone: tz }, errors, t('general.zoneSaved', { zone: tz }), before ? () => saveGeneral(sig, { timezone: before }) : undefined);
  };

  return (
    <Page title={t('nav.general')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.generalDesc')} narrow>
      {error && !data ? (
        <ErrorState error={error} retry={reload} />
      ) : !d.draft ? (
        <Loading lines={5} />
      ) : (
        <>
          <Section id="hostname" title={t('general.name')} icon={Tag} description={t('general.nameDesc')}>
            <form class={c.form} onSubmit={saveName}>
              <TextField
                label={t('general.hostname')}
                value={name}
                onValue={(v) => d.set('hostname', v)}
                maxLength={63}
                autoCapitalize="off"
                spellcheck={false}
                hint={t('general.nameHint', { address: `${(name || 'smhub').toLowerCase()}.local` })}
                error={nameError ?? errors.value.hostname}
              />
              {d.dirty && (
                <Actions>
                  <Button variant="ghost" onClick={d.reset}>
                    {t('common.discard')}
                  </Button>
                  <Button type="submit" variant="primary" icon={Save} disabled={!!nameError || !name}>
                    {t('common.save')}
                  </Button>
                </Actions>
              )}
            </form>
          </Section>
          <Section id="time" title={t('general.timeZone')} icon={Clock} description={t('general.timeNow', { time: dateTime(hubTime(now)) })}>
            <SearchSelect
              label={t('general.timeZone')}
              value={data?.timezone ?? ''}
              onValue={setZone}
              options={(zones.data ?? (data?.timezone ? [data.timezone] : [])).map((z) => ({ value: z, label: z.replace(/_/g, ' ') }))}
              hint={t('general.zoneHint')}
              error={errors.value.timezone}
            />
          </Section>
          {(features.value?.sd_slot || features.value?.sdcard || advanced.value) && (
            <Section title={t('general.sdcard')} description={cardIn ? undefined : t('general.sdcardNone')}>
              <Switch
                label={t('general.sdcardConfig')}
                description={t('general.sdcardDesc')}
                checked={!!data?.sdcard_config}
                onChange={(v) => void saveGeneral(sig, { sdcard_config: v }, errors, v ? t('general.sdcardOn') : t('general.sdcardOff'), () => saveGeneral(sig, { sdcard_config: !v }))}
                error={errors.value.sdcard_config}
              />
            </Section>
          )}
        </>
      )}
    </Page>
  );
}
