// Strict check of language files against English (D36), for translators:
//   node scripts/check-locale.mjs [code ...]      (no code: every file in src/text/locales)
// A file passes when it is valid JSON and has exactly English's keys; every text keeps
// English's {placeholders} and is not empty; a plural object has exactly the forms the
// browser's plural rules use for the language (Intl.PluralRules), each with its {count}.
// Exits 1 on any problem; lists every problem it finds.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { transformSync } from 'esbuild';

const dir = new URL('../src/text/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const js = transformSync(readFileSync(join(dir, 'en.ts'), 'utf8'), { loader: 'ts', format: 'esm' }).code;
const en = (await import('data:text/javascript;base64,' + Buffer.from(js).toString('base64'))).default;

const holders = (s) => new Set([...String(s).matchAll(/\{(\w+)\}/g)].map((m) => m[1]));
const same = (a, b) => a.size === b.size && [...a].every((x) => b.has(x));
const show = (s) => [...s].map((x) => `{${x}}`).join(' ') || 'none';

const codes = process.argv.slice(2).length
  ? process.argv.slice(2)
  : readdirSync(join(dir, 'locales')).filter((f) => f.endsWith('.json')).map((f) => f.slice(0, -5));
let bad = 0;
for (const code of codes) {
  const problems = [];
  let dict;
  try {
    dict = JSON.parse(readFileSync(join(dir, 'locales', `${code}.json`), 'utf8'));
  } catch (e) {
    console.log(`${code}: cannot read: ${e.message}`);
    bad++;
    continue;
  }
  const forms = new Intl.PluralRules(code).resolvedOptions().pluralCategories;
  for (const k of Object.keys(en)) if (!(k in dict)) problems.push(`missing ${k}`);
  for (const k of Object.keys(dict)) if (!(k in en)) problems.push(`not in English: ${k}`);
  for (const [k, v] of Object.entries(dict)) {
    const src = en[k];
    if (src === undefined) continue;
    if (typeof src === 'string') {
      if (typeof v !== 'string') problems.push(`${k}: must be a string, as in English`);
      else if (!v.trim()) problems.push(`${k}: empty`);
      else if (!same(holders(v), holders(src))) problems.push(`${k}: placeholders ${show(holders(v))}, English has ${show(holders(src))}`);
      continue;
    }
    if (!v || typeof v !== 'object' || Array.isArray(v)) {
      problems.push(`${k}: must be an object of plural forms (${forms.join(', ')})`);
      continue;
    }
    const got = Object.keys(v);
    const want = new Set(forms);
    if (!same(new Set(got), want)) problems.push(`${k}: plural forms ${got.join(', ')}; ${code} uses exactly ${forms.join(', ')}`);
    const srcHolders = holders(Object.values(src).join(' '));
    for (const [f, text] of Object.entries(v)) {
      if (typeof text !== 'string' || !text.trim()) {
        problems.push(`${k}.${f}: empty`);
        continue;
      }
      const h = holders(text);
      for (const x of h) if (!srcHolders.has(x)) problems.push(`${k}.${f}: {${x}} is not in English`);
      for (const x of srcHolders) if (x !== 'count' && !h.has(x)) problems.push(`${k}.${f}: lacks {${x}}`);
      if (srcHolders.has('count') && !h.has('count') && f !== 'one' && f !== 'zero' && f !== 'two') problems.push(`${k}.${f}: lacks {count}`);
    }
  }
  if (problems.length) {
    bad++;
    console.log(`${code}: ${problems.length} problem(s)`);
    for (const p of problems.slice(0, 60)) console.log(`  ${p}`);
    if (problems.length > 60) console.log(`  … and ${problems.length - 60} more`);
  } else {
    console.log(`${code}: OK, ${Object.keys(dict).length} texts, plural forms ${forms.join('/')}`);
  }
}
process.exit(bad ? 1 : 0);
