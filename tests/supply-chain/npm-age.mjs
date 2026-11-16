// Every npm package package-lock.json pins was published at least 7 days before this run (the project's supply-chain
// rule: a new version waits a week; an exact newer one only for a security fix, listed in npm-age.allow with its
// reason). The package's own platform packages (@delurio/cli-*) are left out: they are published with it.
//   node tests/supply-chain/npm-age.mjs [package-lock.json]
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const lock = JSON.parse(readFileSync(process.argv[2] ?? join(here, '..', '..', 'package-lock.json'), 'utf8'));
const allowFile = join(here, 'npm-age.allow');
// lines `<name> <version> <why>`
const allowed = new Map(
  (existsSync(allowFile) ? readFileSync(allowFile, 'utf8') : '')
    .split('\n')
    .filter((l) => l.trim() !== '' && !l.startsWith('#'))
    .map((l) => {
      const [name, version, ...why] = l.trim().split(/\s+/);
      return [`${name} ${version}`, why.join(' ')];
    }),
);
const pinned = new Map();
for (const [path, p] of Object.entries(lock.packages ?? {})) {
  if (path === '' || p.link === true || typeof p.version !== 'string') continue;
  const name = p.name ?? path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length);
  if (name.startsWith('@delurio/cli-')) continue;
  pinned.set(`${name} ${p.version}`, { name, version: p.version });
}
const week = 7 * 24 * 3600 * 1000;
const now = Date.now();
const times = new Map();
let young = 0;
for (const { name, version } of pinned.values()) {
  if (!times.has(name)) {
    const res = await fetch(`https://registry.npmjs.org/${name.replace('/', '%2f')}`, { headers: { accept: 'application/json' } });
    if (!res.ok) throw new Error(`${name}: the registry answered ${res.status}`);
    times.set(name, (await res.json()).time ?? {});
  }
  const published = Date.parse(times.get(name)[version]);
  if (!Number.isFinite(published)) throw new Error(`${name} ${version}: the registry gives no publish time`);
  const days = (now - published) / (24 * 3600 * 1000);
  if (now - published >= week) continue;
  const why = allowed.get(`${name} ${version}`);
  if (why !== undefined) {
    console.log(`${name} ${version}: ${days.toFixed(1)} days old, allowed: ${why}`);
    continue;
  }
  young++;
  console.log(`${name} ${version}: published ${days.toFixed(1)} days ago; a version waits 7 days`);
}
console.log(`${pinned.size} npm packages, ${young} younger than 7 days`);
process.exit(young === 0 ? 0 : 1);
