// Regional settings: language, region (formats), units, currency and the time
// zone for times. Changes apply at once, with a live preview of every kind of
// formatted value (D37). The first-run setup uses the same choices.
import { Languages, Ruler } from 'lucide-preact';
import type { Preferences } from '../../api/types';
import { languages, pickLanguage, t } from '../../text/i18n';
import { browserTimeZone, bytes, currencies, currencyName, date, duration, length, localeName, money, nativeLanguageName, num, percent, temperature, time } from '../../text/format';
import { useGet } from '../../state/get';
import { effectivePrefs, savePrefs } from '../../state/prefs';
import { system, hubTime, useNow } from '../../state/system';
import { Choice } from '../../kit/Field';
import { Facts, Page, Section } from '../../kit/Page';
import { SearchSelect } from '../../kit/SearchSelect';

/** Common regions; any BCP 47 locale works, these are offered in the list. */
export const REGIONS = [
  'en-US', 'en-GB', 'en-AU', 'en-CA', 'en-IE', 'en-IN', 'en-NZ', 'en-ZA', 'uk-UA', 'pl-PL', 'de-DE', 'de-AT', 'de-CH', 'fr-FR', 'fr-BE', 'fr-CA', 'fr-CH',
  'it-IT', 'es-ES', 'es-MX', 'es-AR', 'pt-PT', 'pt-BR', 'nl-NL', 'nl-BE', 'sv-SE', 'nb-NO', 'da-DK', 'fi-FI', 'is-IS', 'et-EE', 'lv-LV', 'lt-LT', 'cs-CZ',
  'sk-SK', 'hu-HU', 'ro-RO', 'bg-BG', 'hr-HR', 'sl-SI', 'sr-RS', 'el-GR', 'tr-TR', 'he-IL', 'ar-SA', 'ar-AE', 'ka-GE', 'kk-KZ', 'ja-JP', 'ko-KR', 'zh-CN',
  'zh-TW', 'th-TH', 'vi-VN', 'id-ID', 'ms-MY', 'hi-IN',
];

export function regionOptions() {
  return [{ value: '', label: t('appearance.regionBrowser'), detail: navigator.language }, ...REGIONS.map((r) => ({ value: r, label: localeName(r), detail: r }))];
}

/** Every language in its own name, with its name in the current UI language to search by. */
export function LanguageChoice() {
  const p = effectivePrefs.value;
  return (
    <SearchSelect
      label={t('appearance.language')}
      value={pickLanguage(p.language)}
      onValue={(v) => void savePrefs({ language: v }, { undo: true })}
      options={languages.map((l) => {
        const own = nativeLanguageName(l);
        const here = localeName(l);
        return { value: l, label: own, detail: here.toLocaleLowerCase() === own.toLocaleLowerCase() ? undefined : here };
      })}
    />
  );
}

export function RegionSelect() {
  const p = effectivePrefs.value;
  return (
    <SearchSelect
      label={t('appearance.region')}
      hint={t('appearance.regionHint')}
      value={p.region}
      onValue={(v) => void savePrefs({ region: v }, { undo: true })}
      options={regionOptions()}
    />
  );
}

export function UnitChoices() {
  const p = effectivePrefs.value;
  const set = (patch: Partial<Preferences>) => void savePrefs(patch, { undo: true });
  return (
    <>
      <Choice label={t('appearance.unitSystem')} value={p.units} onValue={(v) => set({ units: v })} options={[{ value: 'metric', label: t('appearance.metric') }, { value: 'imperial', label: t('appearance.imperial') }]} />
      <Choice label={t('appearance.temperature')} value={p.temperature} onValue={(v) => set({ temperature: v })} options={[{ value: 'c', label: t('appearance.celsius') }, { value: 'f', label: t('appearance.fahrenheit') }]} />
      <Choice
        label={t('appearance.clock')}
        value={p.clock}
        onValue={(v) => set({ clock: v })}
        options={[
          { value: 'auto', label: t('appearance.clockAuto') },
          { value: '12h', label: t('appearance.clock12') },
          { value: '24h', label: t('appearance.clock24') },
        ]}
      />
    </>
  );
}

export function Preview() {
  const now = hubTime(useNow('minute'));
  return (
    <Facts
      columns
      items={[
        [t('appearance.exNumber'), <span class="num">{num(1234567.89, 2)}</span>],
        [t('appearance.exPercent'), <span class="num">{percent(42.5, 1)}</span>],
        [t('appearance.exSize'), <span class="num">{bytes(3_221_225_472)}</span>],
        [t('appearance.exTemperature'), <span class="num">{temperature(21.5)}</span>],
        [t('appearance.exLength'), <span class="num">{length(1500)}</span>],
        [t('appearance.exDate'), date(now, 'long')],
        [t('appearance.exTime'), <span class="num">{time(now)}</span>],
        [t('appearance.exDuration'), duration(273_600, 'long')],
        [t('appearance.exMoney'), <span class="num">{money(1299.5)}</span>],
      ]}
    />
  );
}

export default function Regional() {
  const p = effectivePrefs.value;
  const sys = system.value;
  const zones = useGet('/settings/timezones', { skip: p.timezone === 'hub' || p.timezone === 'browser' });
  const set = (patch: Partial<Preferences>) => void savePrefs(patch, { undo: true });
  const tzMode = p.timezone === 'hub' || p.timezone === 'browser' ? p.timezone : 'zone';
  return (
    <Page title={t('nav.regional')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.regionalDesc')} narrow>
      <Section id="language" title={t('appearance.languageRegion')} icon={Languages}>
        <LanguageChoice />
        <RegionSelect />
      </Section>
      <Section id="units" title={t('appearance.units')} icon={Ruler}>
        <UnitChoices />
        <SearchSelect
          label={t('appearance.currency')}
          hint={t('appearance.currencyHint')}
          value={p.currency}
          onValue={(v) => set({ currency: v })}
          options={currencies().map((code) => ({ value: code, label: currencyName(code), detail: code }))}
        />
        <Choice
          label={t('appearance.timesIn')}
          value={tzMode}
          onValue={(v) => set({ timezone: v === 'zone' ? sys?.timezone ?? browserTimeZone() : v })}
          options={[
            { value: 'hub', label: t('appearance.tzHub', { zone: sys?.timezone ?? '' }) },
            { value: 'browser', label: t('appearance.tzBrowser', { zone: browserTimeZone() }) },
            { value: 'zone', label: t('appearance.tzOther') },
          ]}
        />
        {tzMode === 'zone' && (
          <SearchSelect
            label={t('appearance.tzZone')}
            value={p.timezone}
            onValue={(v) => set({ timezone: v })}
            options={(zones.data ?? [p.timezone]).map((z) => ({ value: z, label: z.replace(/_/g, ' ') }))}
          />
        )}
      </Section>
      <Section title={t('appearance.preview')} description={t('appearance.previewDesc')}>
        <Preview />
      </Section>
    </Page>
  );
}
