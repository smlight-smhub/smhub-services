// The one translation function (D36). English is compiled in and is the
// source of the typed keys: a key that is not in en.ts fails the type check,
// and so the build. Every other language is one JSON file in ./locales,
// found at build time and loaded when chosen: adding a language is adding a
// file. Plural forms follow the browser's rules for the language.
import { batch, signal } from '@preact/signals';
import en from './en';
import { num } from './format';
import { lang, RTL } from './lang';

export { lang };

export interface Plural {
  zero?: string;
  one?: string;
  two?: string;
  few?: string;
  many?: string;
  other: string;
}
export type Msg = string | Plural;
export type Key = keyof typeof en;
export type Translation = Partial<Record<Key, Msg>>;

const files = import.meta.glob<{ default: Translation }>('./locales/*.json');

/** Language codes with a translation, English first. */
export const languages: string[] = [
  'en',
  ...Object.keys(files)
    .map((p) => p.replace('./locales/', '').replace('.json', ''))
    .sort(),
];

const current = signal<Translation>({});
const loaded = new Map<string, Translation>();
let plural = new Intl.PluralRules('en');

/** Codes a browser may report for a language whose file has another name. */
const ALIASES: Record<string, string> = { no: 'nb', nn: 'nb', sr: 'sr-latn' };

/** The best language we have for a code such as "uk-UA", "de-CH" or "sr-Latn-RS". */
export function pickLanguage(code: string | undefined | null): string {
  if (!code) return 'en';
  const c = code.toLowerCase();
  if (languages.includes(c)) return c;
  const base = c.split('-')[0]!;
  const alias = ALIASES[base];
  if (alias && languages.includes(alias)) return alias;
  return languages.includes(base) ? base : 'en';
}

/** Switches the UI language, loading its file first. */
export async function setLanguage(code: string): Promise<void> {
  const target = pickLanguage(code);
  let dict: Translation = {};
  if (target !== 'en') {
    const cachedDict = loaded.get(target);
    if (cachedDict) dict = cachedDict;
    else {
      const load = files[`./locales/${target}.json`];
      if (load) {
        dict = (await load()).default;
        loaded.set(target, dict);
      }
    }
  }
  plural = new Intl.PluralRules(target);
  batch(() => {
    current.value = dict;
    lang.value = target;
  });
  const html = document.documentElement;
  html.lang = target;
  html.dir = RTL.has(target) ? 'rtl' : 'ltr';
}

type Vars = Record<string, string | number | null | undefined>;

/**
 * Translates a key. `{name}` placeholders are filled from `vars`; numbers are
 * formatted for the user's region (pass ports and versions as strings).
 * With `count`, the plural form for that number is chosen.
 */
export function t(key: Key, vars?: Vars): string {
  const m: Msg | undefined = current.value[key] ?? (en as Record<Key, Msg>)[key];
  let s: string;
  if (typeof m === 'string') s = m;
  else if (m) {
    const n = Number(vars?.count ?? 0);
    s = m[plural.select(n) as keyof Plural] ?? m.other;
  } else s = key;
  if (!vars) return s;
  // in a right-to-left language each filled-in value is a bidi isolate (FSI…PDI), so a size such
  // as "177 MB" or a name in Latin letters keeps its own order inside the sentence
  const [open, close] = RTL.has(lang.value) ? ['⁨', '⁩'] : ['', ''];
  return s.replace(/\{(\w+)\}/g, (whole, name: string) => {
    const v = vars[name];
    if (v === undefined || v === null) return whole;
    return open + (typeof v === 'number' ? num(v, 2) : v) + close;
  });
}

/** Whether a dynamic string (an error code, a step name) has a translation. */
export function has(key: string): key is Key {
  return key in en;
}

/** Translates a dynamic key if it exists, otherwise answers the fallback. */
export function tx(key: string, fallback: string, vars?: Vars): string {
  return has(key) ? t(key, vars) : fallback;
}
