// The signed-in user's preferences (GET/PATCH /preferences) and what follows
// from them: theme, language, formats and the UI level (D35–D37).
import { computed, effect, signal } from '@preact/signals';
import { api } from '../api/client';
import type { Preferences, PreferencesPatch } from '../api/types';
import { formatPrefs } from '../text/format';
import { lang, pickLanguage, setLanguage } from '../text/i18n';
import { live } from './store';
import { save } from './save';

const LOCAL_KEY = 'smhub.prefs';

export const prefs = signal<Preferences | null>(null);

function readLocal(): Partial<Preferences> {
  try {
    return JSON.parse(localStorage.getItem(LOCAL_KEY) ?? '{}') as Partial<Preferences>;
  } catch {
    return {};
  }
}
const local = readLocal();

function regionDefaults(region: string) {
  const country = (() => {
    try {
      return new Intl.Locale(region).maximize().region ?? '';
    } catch {
      return '';
    }
  })();
  const imperial = ['US', 'LR', 'MM'].includes(country);
  return { units: imperial ? 'imperial' : 'metric', temperature: imperial ? 'f' : 'c' } as const;
}

/** What is used while the hub's preferences are unknown: this browser's last choice, then its locale. */
export function fallbackPrefs(): Preferences {
  const region = local.region || navigator.language || 'en-US';
  const d = regionDefaults(region);
  return {
    language: local.language ?? pickLanguage(navigator.language),
    region: local.region ?? '',
    theme: local.theme ?? 'auto',
    units: local.units ?? d.units,
    temperature: local.temperature ?? d.temperature,
    clock: local.clock ?? 'auto',
    timezone: local.timezone ?? 'hub',
    level: local.level ?? 'simple',
    dashboard: [],
    currency: local.currency ?? 'EUR',
    text_size: local.text_size ?? 'normal',
    density: local.density ?? 'comfortable',
    tone: local.tone ?? 'green',
  };
}

/** Preferences with fallbacks, for reading. */
export const effectivePrefs = computed<Preferences>(() => prefs.value ?? fallbackPrefs());
export const level = computed(() => effectivePrefs.value.level);
export const advanced = computed(() => level.value === 'advanced');

/** Sets or removes a data attribute on <html>; the default value has none. */
function attr(name: 'theme' | 'text' | 'density' | 'tone', value: string | undefined, fallback: string) {
  const el = document.documentElement;
  if (value && value !== fallback) el.dataset[name] = value;
  else delete el.dataset[name];
}

/** Theme, text size, spacing and color tone, as attributes on <html> (tokens.css, tones.css). */
export function applyLook(p: Pick<Preferences, 'theme' | 'text_size' | 'density' | 'tone'>) {
  attr('theme', p.theme === 'light' || p.theme === 'dark' ? p.theme : undefined, '');
  attr('text', p.text_size, 'normal');
  attr('density', p.density, 'comfortable');
  attr('tone', p.tone, 'green');
}

function applyFormats(p: Preferences) {
  formatPrefs.value = {
    ...formatPrefs.peek(),
    region: p.region,
    units: p.units,
    temperature: p.temperature,
    clock: p.clock,
    timezone: p.timezone,
    currency: p.currency,
  };
}

/** Before the hub answers: this browser's last theme and language, so nothing flashes. */
export function restoreLocalPrefs(): Promise<void> {
  const p = fallbackPrefs();
  applyLook(p);
  applyFormats(p);
  return setLanguage(p.language);
}

effect(() => {
  const p = prefs.value;
  if (!p) return;
  applyLook(p);
  applyFormats(p);
  if (pickLanguage(p.language) !== lang.peek()) void setLanguage(p.language);
  try {
    const { dashboard: _d, ...rest } = p;
    localStorage.setItem(LOCAL_KEY, JSON.stringify(rest));
  } catch {
    /* private mode: nothing kept */
  }
});

effect(() => {
  const s = live('system').value;
  if (s && s.timezone !== formatPrefs.peek().hubTimezone) formatPrefs.value = { ...formatPrefs.peek(), hubTimezone: s.timezone };
});

/** Changes preferences through the one save path. */
export function savePrefs(patch: PreferencesPatch, o: { undo?: boolean; success?: string } = {}) {
  const before = prefs.peek();
  const undo =
    o.undo && before
      ? () => {
          const back: PreferencesPatch = {};
          for (const k of Object.keys(patch) as (keyof PreferencesPatch)[]) (back as Record<string, unknown>)[k] = before[k];
          return savePrefs(back);
        }
      : undefined;
  return save<Preferences>({
    target: prefs,
    optimistic: (p) => ({ ...p, ...patch }) as Preferences,
    send: () => api.patch('/preferences', { body: patch }),
    success: o.success,
    undo,
  });
}
