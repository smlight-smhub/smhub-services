// Appearance: theme and colour tone, text size and spacing, the UI level
// (details), and the home page layout. Changes apply at once (D37). Language,
// formats, units and the time zone are on Regional settings.
import { LayoutGrid, Monitor, Moon, Palette, Sparkles, Sun, Type } from 'lucide-preact';
import type { LucideIcon } from 'lucide-preact';
import type { Preferences } from '../../api/types';
import { TONE_ACCENTS, type Tone } from '../../tokens/tones';
import { t } from '../../text/i18n';
import { effectivePrefs, savePrefs } from '../../state/prefs';
import { Button } from '../../kit/Button';
import { Choice } from '../../kit/Field';
import { Page, Rows, Section } from '../../kit/Page';
import { DEFAULT_DASHBOARD } from '../dashboard/Dashboard';

/** A dot in a tone's accent color, drawn where a choice shows its icon. */
function swatch(color: string): LucideIcon {
  const Swatch = ({ size = 16 }: { size?: number | string }) => (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="9" fill={color} />
    </svg>
  );
  return Swatch as unknown as LucideIcon;
}
const SWATCHES = Object.fromEntries(Object.entries(TONE_ACCENTS).map(([k, c]) => [k, swatch(c)])) as Record<Tone, LucideIcon>;
const TONE_LABEL = {
  green: 'appearance.toneGreen',
  teal: 'appearance.toneTeal',
  blue: 'appearance.toneBlue',
  violet: 'appearance.toneViolet',
  pink: 'appearance.tonePink',
} as const satisfies Record<Tone, string>;

export default function Appearance() {
  const p = effectivePrefs.value;
  const set = (patch: Partial<Preferences>) => void savePrefs(patch, { undo: true });
  return (
    <Page title={t('nav.appearance')} back={{ href: '/settings', label: t('nav.settings') }} subtitle={t('nav.appearanceDesc')} narrow>
      <Section id="theme" title={t('appearance.theme')} icon={Palette}>
        <Choice
          label={t('appearance.theme')}
          hideLabel
          variant="cards"
          value={p.theme}
          onValue={(v) => set({ theme: v })}
          options={[
            { value: 'auto', label: t('appearance.themeAuto'), hint: t('appearance.themeAutoHint'), icon: Monitor },
            { value: 'light', label: t('appearance.themeLight'), icon: Sun },
            { value: 'dark', label: t('appearance.themeDark'), icon: Moon },
          ]}
        />
        <Choice
          label={t('appearance.tone')}
          hint={t('appearance.toneHint')}
          value={p.tone}
          onValue={(v) => set({ tone: v })}
          options={(Object.keys(TONE_ACCENTS) as Tone[]).map((k) => ({ value: k, label: t(TONE_LABEL[k]), icon: SWATCHES[k] }))}
        />
      </Section>
      <Section id="display" title={t('appearance.display')} description={t('appearance.displayDesc')} icon={Type}>
        <Choice
          label={t('appearance.textSize')}
          value={p.text_size}
          onValue={(v) => set({ text_size: v })}
          options={[
            { value: 'small', label: t('appearance.textSmall') },
            { value: 'normal', label: t('appearance.textNormal') },
            { value: 'large', label: t('appearance.textLarge') },
          ]}
        />
        <Choice
          label={t('appearance.density')}
          variant="cards"
          value={p.density}
          onValue={(v) => set({ density: v })}
          options={[
            { value: 'comfortable', label: t('appearance.densityComfortable'), hint: t('appearance.densityComfortableHint') },
            { value: 'compact', label: t('appearance.densityCompact'), hint: t('appearance.densityCompactHint') },
          ]}
        />
      </Section>
      <Section id="level" title={t('level.label')} icon={Sparkles}>
        <Choice
          label={t('level.label')}
          hideLabel
          variant="cards"
          value={p.level}
          onValue={(v) => void savePrefs({ level: v }, { undo: true, success: v === 'advanced' ? t('level.nowAdvanced') : t('level.nowSimple') })}
          options={[
            { value: 'simple', label: t('level.simple'), hint: t('level.simpleHint') },
            { value: 'advanced', label: t('level.advanced'), hint: t('level.advancedHint') },
          ]}
        />
      </Section>
      <Section title={t('appearance.dashboard')} icon={LayoutGrid}>
        <Rows>
          <div>
            <p class="muted">{t('appearance.dashboardDesc')}</p>
          </div>
          <div>
            <Button onClick={() => void savePrefs({ dashboard: DEFAULT_DASHBOARD }, { undo: true, success: t('appearance.dashboardReset') })}>{t('appearance.dashboardResetAction')}</Button>
          </div>
        </Rows>
      </Section>
    </Page>
  );
}
