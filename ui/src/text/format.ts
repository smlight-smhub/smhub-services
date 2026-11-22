// Every displayed value goes through here (D37). It wraps the browser's Intl
// functions and follows the user's preferences; pages never format a value
// themselves. Formatter objects are cached, because creating them is slow.
import { signal } from '@preact/signals';
import { lang } from './lang';

export interface FormatPrefs {
  /** BCP 47 locale for formats, e.g. en-US; empty = the browser's. */
  region: string;
  units: 'metric' | 'imperial';
  temperature: 'c' | 'f';
  clock: 'auto' | '12h' | '24h';
  /** "hub", "browser" or a zone name. */
  timezone: string;
  /** ISO 4217 code. */
  currency: string;
  /** The hub's own zone, from `system`. */
  hubTimezone?: string;
}

const browserLocale = () => (typeof navigator !== 'undefined' && navigator.language) || 'en-US';

export const formatPrefs = signal<FormatPrefs>({
  region: '',
  units: 'metric',
  temperature: 'c',
  clock: 'auto',
  timezone: 'hub',
  currency: 'EUR',
});

const cache = new Map<string, unknown>();
let cacheKey = '';

function valid(locale: string): string | undefined {
  try {
    return Intl.getCanonicalLocales(locale)[0];
  } catch {
    return undefined;
  }
}

/** Locale for numbers, dates and currencies: the region preference. */
export function formatLocale(): string {
  const p = formatPrefs.value;
  return (p.region && valid(p.region)) || browserLocale();
}

/** Locale for words inside formatted values (durations, "3 minutes ago", lists): the UI language with the region's country. */
export function textLocale(): string {
  const l = lang.value;
  try {
    const country = new Intl.Locale(formatLocale()).maximize().region;
    return country ? `${l}-${country}` : l;
  } catch {
    return l;
  }
}

function cached<T>(kind: string, locale: string, opts: object, make: () => T): T {
  const p = formatPrefs.value;
  const k = `${p.region}|${p.clock}|${p.timezone}|${p.hubTimezone}|${lang.value}`;
  if (k !== cacheKey) {
    cache.clear();
    cacheKey = k;
  }
  const key = kind + locale + JSON.stringify(opts);
  let f = cache.get(key) as T | undefined;
  if (!f) {
    f = make();
    cache.set(key, f);
  }
  return f;
}

function nf(opts: Intl.NumberFormatOptions, locale = formatLocale()) {
  return cached('n', locale, opts, () => new Intl.NumberFormat(locale, opts));
}

/** A plain number with at most `digits` decimals. */
export function num(value: number, digits = 0): string {
  if (!Number.isFinite(value)) return '–';
  return nf({ maximumFractionDigits: digits }).format(value);
}

/** A short span in seconds, such as how long a button was held ("6.5 sec"). */
export function seconds(value: number, digits = 1): string {
  if (!Number.isFinite(value)) return '–';
  return nf({ style: 'unit', unit: 'second', unitDisplay: 'short', maximumFractionDigits: digits }).format(value);
}

/** A percentage given as 0–100. */
export function percent(value: number, digits = 0): string {
  if (!Number.isFinite(value)) return '–';
  return nf({ style: 'percent', maximumFractionDigits: digits }).format(value / 100);
}

const BYTE_UNITS = ['byte', 'kilobyte', 'megabyte', 'gigabyte', 'terabyte', 'petabyte'] as const;

/** A data size in bytes, in SI units (1 kB = 1000 B), as the API's sizes are canonical bytes (D8). */
export function bytes(value: number | null | undefined, digits?: number): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '–';
  let i = 0;
  let v = Math.abs(value);
  while (v >= 1000 && i < BYTE_UNITS.length - 1) {
    v /= 1000;
    i++;
  }
  const d = digits ?? (v < 10 && i > 0 ? 1 : 0);
  return nf({ style: 'unit', unit: BYTE_UNITS[i], unitDisplay: i === 0 ? 'long' : 'short', maximumFractionDigits: d }).format(
    Math.sign(value) * v,
  );
}

/** A data rate in megabits per second. */
export function mbps(value: number): string {
  return nf({ style: 'unit', unit: 'megabit-per-second', unitDisplay: 'short', maximumFractionDigits: 0 }).format(value);
}

/** A temperature given in degrees Celsius (canonical), shown in the chosen unit. */
export function temperature(celsius: number, digits = 1): string {
  const f = formatPrefs.value.temperature === 'f';
  const v = f ? (celsius * 9) / 5 + 32 : celsius;
  return nf({ style: 'unit', unit: f ? 'fahrenheit' : 'celsius', maximumFractionDigits: digits }).format(v);
}

/** A length given in metres, in metric or imperial units. */
export function length(metres: number): string {
  if (formatPrefs.value.units === 'imperial') {
    const feet = metres * 3.28084;
    if (feet >= 5280) return nf({ style: 'unit', unit: 'mile', maximumFractionDigits: 1 }).format(feet / 5280);
    return nf({ style: 'unit', unit: 'foot', maximumFractionDigits: 0 }).format(feet);
  }
  if (metres >= 1000) return nf({ style: 'unit', unit: 'kilometer', maximumFractionDigits: 1 }).format(metres / 1000);
  return nf({ style: 'unit', unit: 'meter', maximumFractionDigits: 0 }).format(metres);
}

/** An amount of money with its ISO 4217 code (D8); the user's currency when none is given. */
export function money(amount: number, code?: string): string {
  return nf({ style: 'currency', currency: code ?? formatPrefs.value.currency }).format(amount);
}

/** Durations: the two largest units, e.g. "3 days, 4 hr". */
export function duration(seconds: number, style: 'long' | 'short' | 'narrow' = 'short', parts = 2): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '–';
  const units: [Intl.NumberFormatOptions['unit'], number][] = [
    ['day', 86400],
    ['hour', 3600],
    ['minute', 60],
    ['second', 1],
  ];
  const locale = textLocale();
  const one = (n: number, unit: Intl.NumberFormatOptions['unit']) =>
    nf({ style: 'unit', unit, unitDisplay: style }, locale).format(n);
  const out: string[] = [];
  let rest = Math.floor(seconds);
  for (const [unit, size] of units) {
    const n = Math.floor(rest / size);
    rest -= n * size;
    if (n > 0) out.push(one(n, unit));
    else if (out.length) break; // "1 day", not "1 day, 0 hr"
    if (out.length >= parts) break;
  }
  if (!out.length) out.push(one(0, 'second'));
  return list(out, 'unit');
}

/** The time zone values are shown in. */
export function timeZone(): string | undefined {
  const p = formatPrefs.value;
  if (p.timezone === 'browser') return undefined;
  if (p.timezone === 'hub' || !p.timezone) return p.hubTimezone || undefined;
  return p.timezone;
}

function dtf(opts: Intl.DateTimeFormatOptions) {
  const p = formatPrefs.value;
  const o: Intl.DateTimeFormatOptions = { ...opts, timeZone: timeZone() };
  if (p.clock !== 'auto' && (opts.timeStyle || opts.hour)) o.hourCycle = p.clock === '12h' ? 'h12' : 'h23';
  const locale = formatLocale();
  return cached('d', locale, o, () => {
    try {
      return new Intl.DateTimeFormat(locale, o);
    } catch {
      // An unknown zone name: fall back to the browser's zone.
      return new Intl.DateTimeFormat(locale, { ...o, timeZone: undefined });
    }
  });
}

type When = string | number | Date | null | undefined;
function toDate(v: When): Date | undefined {
  if (v === null || v === undefined || v === '') return undefined;
  const d = v instanceof Date ? v : new Date(v);
  return Number.isNaN(d.getTime()) ? undefined : d;
}

export function date(v: When, style: 'short' | 'medium' | 'long' = 'medium'): string {
  const d = toDate(v);
  return d ? dtf({ dateStyle: style }).format(d) : '–';
}

export function time(v: When, seconds = false): string {
  const d = toDate(v);
  return d ? dtf({ timeStyle: seconds ? 'medium' : 'short' }).format(d) : '–';
}

export function dateTime(v: When): string {
  const d = toDate(v);
  return d ? dtf({ dateStyle: 'medium', timeStyle: 'short' }).format(d) : '–';
}

/** A weekday and date for headings, e.g. "Friday, 6 November". */
export function longDay(v: When): string {
  const d = toDate(v);
  return d ? dtf({ weekday: 'long', day: 'numeric', month: 'long' }).format(d) : '–';
}

/** "3 minutes ago", "in 2 hours", "yesterday". */
export function relative(v: When, now = Date.now()): string {
  const d = toDate(v);
  if (!d) return '–';
  const diff = (d.getTime() - now) / 1000;
  const abs = Math.abs(diff);
  const locale = textLocale();
  const rtf = cached('r', locale, {}, () => new Intl.RelativeTimeFormat(locale, { numeric: 'auto', style: 'long' }));
  if (abs < 45) return rtf.format(0, 'second');
  if (abs < 3600) return rtf.format(Math.round(diff / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(diff / 3600), 'hour');
  if (abs < 86400 * 30) return rtf.format(Math.round(diff / 86400), 'day');
  return date(d);
}

/** "a, b and c" (conjunction), "a, b or c" (disjunction) or "a, b" (unit list). */
export function list(items: string[], type: 'conjunction' | 'disjunction' | 'unit' = 'conjunction'): string {
  const locale = textLocale();
  const f = cached('l', locale, { type }, () => new Intl.ListFormat(locale, { type, style: type === 'unit' ? 'narrow' : 'long' }));
  return f.format(items);
}

function capitalise(s: string, locale: string) {
  return s ? s.charAt(0).toLocaleUpperCase(locale) + s.slice(1) : s;
}

/** A language's name in that language, e.g. "українська" → "Українська". */
export function nativeLanguageName(code: string): string {
  try {
    const n = new Intl.DisplayNames([code], { type: 'language' }).of(code) ?? code;
    return capitalise(n, code);
  } catch {
    return code;
  }
}

/** A locale's name in the UI language, e.g. "English (United States)". */
export function localeName(code: string): string {
  try {
    const n = new Intl.DisplayNames([lang.value], { type: 'language' }).of(code) ?? code;
    return capitalise(n, lang.value);
  } catch {
    return code;
  }
}

export function currencyName(code: string): string {
  try {
    return new Intl.DisplayNames([lang.value], { type: 'currency' }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** All currency codes the browser knows. */
export function currencies(): string[] {
  try {
    return Intl.supportedValuesOf('currency');
  } catch {
    return ['EUR', 'USD', 'GBP', 'UAH', 'PLN', 'CHF', 'CZK', 'SEK', 'NOK', 'DKK', 'CAD', 'AUD', 'JPY', 'CNY'];
  }
}

/** The browser's own time zone. */
export function browserTimeZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone;
}

/** Sorts strings the way the user's language does. */
export function compare(a: string, b: string): number {
  const locale = lang.value;
  return cached('c', locale, {}, () => new Intl.Collator(locale, { sensitivity: 'base', numeric: true })).compare(a, b);
}
