// Prints the size of every file in dist/ and fails when the first load
// (index.html and everything it loads before the dashboard is usable)
// exceeds the budget (D31, vision.md "Targets"). The hub's nginx sends files
// uncompressed, so sizes are as on disk.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const DIST = new URL('../dist/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');
const BUDGET = 400_000; // bytes, "400 KB or less"

function walk(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
}

const html = readFileSync(join(DIST, 'index.html'), 'utf8');
const firstLoad = new Set(['index.html']);
for (const m of html.matchAll(/<(?:script[^>]*\ssrc|link[^>]*\shref)="\/([^"]+)"/g)) firstLoad.add(m[1]);

const files = walk(DIST)
  .map((p) => ({ path: relative(DIST, p).split('\\').join('/'), size: statSync(p).size }))
  .sort((a, b) => Number(firstLoad.has(b.path)) - Number(firstLoad.has(a.path)) || b.size - a.size);

const kb = (n) => (n / 1000).toFixed(1).padStart(8) + ' KB';
let first = 0;
let total = 0;
console.log('\nUI build (uncompressed, as the hub sends it):\n');
for (const f of files) {
  const isFirst = firstLoad.has(f.path);
  if (isFirst) first += f.size;
  total += f.size;
  console.log(`${isFirst ? '*' : ' '} ${kb(f.size)}  ${f.path}`);
}
const missing = [...firstLoad].filter((p) => !files.some((f) => f.path === p));
console.log(`\n* first load: ${kb(first).trim()} of ${kb(BUDGET).trim()} budget (${files.filter((f) => firstLoad.has(f.path)).length} files)`);
console.log(`  everything: ${kb(total).trim()} in ${files.length} files\n`);
if (missing.length) {
  console.error(`index.html refers to files that are not in dist: ${missing.join(', ')}`);
  process.exit(1);
}
if (first > BUDGET) {
  console.error(`The first load is ${kb(first - BUDGET).trim()} over the budget.`);
  process.exit(1);
}
