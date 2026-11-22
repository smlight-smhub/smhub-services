// Generates src/tokens/tones.css: the color tones of the Appearance page, derived from the
// default SMLIGHT green tokens in tokens.css. Each tone keeps every green token's lightness
// and chroma in OKLCH and takes the tone's hue (chroma reduced until the color fits sRGB), so
// all tones look equally strong. Text pairs are then checked against WCAG 2.2 (4.5:1); a
// failing token is darkened (light theme) or lightened (dark theme) until it passes.
// Status colors (--ok, --warn, --danger, --info) and the brand mark are not toned.
//   node scripts/tones.mjs        writes the file and prints the contrast of every pair
import { writeFileSync } from 'node:fs';

export const TONES = { teal: 192, blue: 255, violet: 300, pink: 350 };

// The default (green) tokens that follow the tone, light and dark, from tokens.css.
const LIGHT = {
  '--bg': '#f3f5f4', '--surface-2': '#f7f9f8', '--surface-3': '#edf1ef', '--surface-sunken': '#e9eeeb',
  '--border': '#e0e6e3', '--border-strong': '#c6d0cb', '--text': '#15201a', '--text-2': '#46534c', '--text-3': '#5f6c65',
  '--accent': '#19764a', '--accent-hover': '#136039', '--accent-active': '#0f5131', '--accent-soft': '#e2f1e9',
  '--accent-soft-2': '#cfe8db', '--accent-text': '#11603a', '--focus': '#19764a',
  '--neutral': '#6b7770', '--neutral-soft': '#edf1ef', '--neutral-text': '#46534c',
  '--chart-1': '#19764a', '--hero-from': '#e5f3ec', '--hero-to': '#f6fbf8',
};
const DARK = {
  '--bg': '#0d1210', '--surface': '#141a17', '--surface-2': '#19211d', '--surface-3': '#1f2924', '--surface-sunken': '#0f1512',
  '--border': '#25302a', '--border-strong': '#34413a', '--text': '#e6ede9', '--text-2': '#b3c0b9', '--text-3': '#8e9c95',
  '--text-on-accent': '#04140b', '--accent': '#4cc38a', '--accent-hover': '#62d09b', '--accent-active': '#7ad9ab',
  '--accent-text': '#74d4a6', '--focus': '#6fd3a2', '--neutral': '#8e9c95', '--neutral-text': '#b3c0b9',
  '--chart-1': '#4cc38a', '--hero-from': '#15241c', '--hero-to': '#141a17',
};
// Tokens written with transparency: [token, base token, alpha].
const ALPHA_LIGHT = [['--chart-1-fill', '--chart-1', 0.14]];
const ALPHA_DARK = [['--accent-soft', '--accent', 0.14], ['--accent-soft-2', '--accent', 0.24], ['--neutral-soft', '--neutral', 0.14], ['--chart-1-fill', '--chart-1', 0.16]];

// Pairs that carry text: [foreground, background]; "#fff" is literal white.
const PAIRS_LIGHT = [['#ffffff', '--accent'], ['#ffffff', '--accent-hover'], ['--accent-text', '#ffffff'], ['--accent-text', '--bg'], ['--accent-text', '--accent-soft'], ['--text-3', '--bg'], ['--text-2', '--surface-3']];
const PAIRS_DARK = [['--text-on-accent', '--accent'], ['--text-on-accent', '--accent-hover'], ['--accent-text', '--surface'], ['--accent-text', '--bg'], ['--text-3', '--bg'], ['--text-2', '--surface-3']];

// ---- color math (OKLab, sRGB) ----
const lin = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const gam = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
const hex2rgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
const rgb2hex = (rgb) => '#' + rgb.map((c) => Math.round(Math.min(1, Math.max(0, c)) * 255).toString(16).padStart(2, '0')).join('');
function rgb2oklch(rgb) {
  const [r, g, b] = rgb.map(lin);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [L, Math.hypot(A, B), ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360];
}
function oklch2rgb([L, C, H]) {
  const A = C * Math.cos((H * Math.PI) / 180), B = C * Math.sin((H * Math.PI) / 180);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s].map(gam);
}
const inGamut = (rgb) => rgb.every((c) => c >= -1e-4 && c <= 1 + 1e-4);
/** The color at lightness L and hue H with chroma C, or the most chroma below C that fits sRGB. */
function fit(L, C, H) {
  let c = C;
  while (c > 0 && !inGamut(oklch2rgb([L, c, H]))) c -= 0.002;
  return rgb2hex(oklch2rgb([L, Math.max(c, 0), H]));
}
const lum = (h) => { const [r, g, b] = hex2rgb(h).map(lin); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
const contrast = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

function tone(base, hue, pairs, dark) {
  const out = {};
  const lch = {};
  for (const [k, v] of Object.entries(base)) {
    lch[k] = rgb2oklch(hex2rgb(v));
    out[k] = fit(lch[k][0], lch[k][1], hue);
  }
  const color = (x) => (x.startsWith('#') ? x : out[x]);
  // a failing pair moves its toned color, the text if it is toned, else the background:
  // darker in the light theme (dark text, or white text on it), lighter in the dark theme
  for (let round = 0; round < 60; round++) {
    let changed = false;
    for (const [fg, bg] of pairs) {
      if (contrast(color(fg), color(bg)) >= 4.5) continue;
      const k = fg.startsWith('--') ? fg : bg;
      lch[k][0] += dark ? 0.01 : -0.01;
      out[k] = fit(lch[k][0], lch[k][1], hue);
      changed = true;
    }
    if (!changed) break;
  }
  return { out, report: pairs.map(([fg, bg]) => [fg, bg, contrast(color(fg), color(bg))]) };
}

const rgba = (hex, a) => `rgb(${hex2rgb(hex).map((c) => Math.round(c * 255)).join(' ')} / ${a})`;
let css = `/* Generated by scripts/tones.mjs from the green tokens in tokens.css; do not edit.
 * Color tones (Appearance page): data-tone on <html>; no attribute = SMLIGHT green. */\n`;
let failed = false;
for (const [name, hue] of Object.entries(TONES)) {
  const light = tone(LIGHT, hue, PAIRS_LIGHT, false);
  const dark = tone(DARK, hue, PAIRS_DARK, true);
  const block = (o, alpha) => [...Object.entries(o).map(([k, v]) => `  ${k}: ${v};`), ...alpha.map(([k, b, a]) => `  ${k}: ${rgba(o[b], a)};`)].join('\n');
  css += `\n:root[data-tone='${name}'] {\n${block(light.out, ALPHA_LIGHT)}\n}\n`;
  css += `:root[data-tone='${name}'][data-theme='dark'] {\n${block(dark.out, ALPHA_DARK)}\n}\n`;
  css += `@media (prefers-color-scheme: dark) {\n  :root[data-tone='${name}']:not([data-theme='light']) {\n${block(dark.out, ALPHA_DARK).replace(/^/gm, '  ')}\n  }\n}\n`;
  for (const [mode, r] of [['light', light.report], ['dark', dark.report]]) {
    const worst = r.reduce((a, b) => (b[2] < a[2] ? b : a));
    console.log(`${name.padEnd(7)} ${mode.padEnd(5)} worst ${worst[2].toFixed(2)}:1 (${worst[0]} on ${worst[1]}); accent ${mode === 'light' ? light.out['--accent'] : dark.out['--accent']}`);
    if (worst[2] < 4.5) failed = true;
  }
}
writeFileSync(new URL('../src/tokens/tones.css', import.meta.url), css);
// the accent of each tone in the light theme, for the swatches on the Appearance page
const accents = { green: LIGHT['--accent'], ...Object.fromEntries(Object.entries(TONES).map(([n, h]) => [n, tone(LIGHT, h, PAIRS_LIGHT, false).out['--accent']])) };
writeFileSync(
  new URL('../src/tokens/tones.ts', import.meta.url),
  `// Generated by scripts/tones.mjs; do not edit. Each tone's accent (light theme), for swatches.\nexport const TONE_ACCENTS = ${JSON.stringify(accents, null, 2).replace(/"/g, "'")} as const;\n\nexport type Tone = keyof typeof TONE_ACCENTS;\n`,
);
// the default green, for comparison
const g = (pairs, base) => pairs.map(([f, b]) => contrast(f.startsWith('#') ? f : base[f], b.startsWith('#') ? b : base[b])).reduce((a, b) => Math.min(a, b));
console.log(`green   light worst ${g(PAIRS_LIGHT, LIGHT).toFixed(2)}:1 · dark worst ${g(PAIRS_DARK, DARK).toFixed(2)}:1 (today's tokens)`);
if (failed) {
  console.error('a tone fails 4.5:1');
  process.exit(1);
}
