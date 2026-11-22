// Reports, per language file in src/text/locales, the keys it lacks, keys
// English does not have, and placeholders that differ from English (D36).
// English itself is checked by the type checker: a missing key fails it.
// Missing translations fall back to English, so they are reported, not fatal;
// a file that is not valid JSON fails.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const root = new URL('../src/text/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const src = readFileSync(join(root, 'en.ts'), 'utf8');

// Keys and their placeholders from en.ts ('key': 'text' or 'key': { one: ..., other: ... }).
const en = new Map();
for (const m of src.matchAll(/^\s*'([^']+)':\s*(.+)$/gm)) en.set(m[1], new Set([...m[2].matchAll(/\{(\w+)\}/g)].map((x) => x[1])));

const placeholders = (v) => new Set([...JSON.stringify(v).matchAll(/\{(\w+)\}/g)].map((x) => x[1]));
let failed = false;
for (const file of readdirSync(join(root, 'locales')).filter((f) => f.endsWith('.json'))) {
  let dict;
  try {
    dict = JSON.parse(readFileSync(join(root, 'locales', file), 'utf8'));
  } catch (e) {
    console.error(`${file}: not valid JSON: ${e.message}`);
    failed = true;
    continue;
  }
  const missing = [...en.keys()].filter((k) => !(k in dict));
  const extra = Object.keys(dict).filter((k) => !en.has(k));
  const wrong = Object.entries(dict).filter(([k, v]) => {
    const want = en.get(k);
    if (!want) return false;
    const have = placeholders(v);
    return [...have].some((p) => !want.has(p) && p !== 'count' && p !== 'n');
  });
  const size = readFileSync(join(root, 'locales', file)).length;
  console.log(`${file}: ${Object.keys(dict).length}/${en.size} keys, ${(size / 1000).toFixed(1)} KB`);
  if (missing.length) console.log(`  missing (English is shown): ${missing.join(', ')}`);
  if (extra.length) console.log(`  not in English (unused): ${extra.join(', ')}`);
  if (wrong.length) console.log(`  unknown placeholders: ${wrong.map(([k]) => k).join(', ')}`);
}
if (failed) process.exit(1);
