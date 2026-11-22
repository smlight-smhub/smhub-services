import { signal } from '@preact/signals';

/** The UI language (a code with a translation file, or "en"). */
export const lang = signal('en');

/** Languages written right to left; the layout uses logical properties (D32). */
export const RTL = new Set(['ar', 'fa', 'he', 'ur', 'yi', 'ps', 'dv', 'ug']);
