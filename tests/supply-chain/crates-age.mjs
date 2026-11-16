// Every crate Cargo.lock pins was published at least 7 days before this run (the project's supply-chain rule: a new
// version waits a week; an exact newer one only for a security fix, listed in crates-age.allow with its reason).
//   node tests/supply-chain/crates-age.mjs [Cargo.lock]
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const lock = readFileSync(process.argv[2] ?? join(here, '..', '..', 'Cargo.lock'), 'utf8');
const allowFile = join(here, 'crates-age.allow');
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
const packages = [...lock.matchAll(/\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\nsource = "registry\+https:\/\/github\.com\/rust-lang\/crates\.io-index"/g)].map((m) => ({ name: m[1], version: m[2] }));
const week = 7 * 24 * 3600 * 1000;
const now = Date.now();
let young = 0;
for (const { name, version } of packages) {
  const res = await fetch(`https://crates.io/api/v1/crates/${name}/${version}`, { headers: { 'User-Agent': 'delurio-ci (supply-chain age check)' } });
  if (!res.ok) throw new Error(`${name} ${version}: crates.io answered ${res.status}`);
  const created = Date.parse((await res.json()).version.created_at);
  const days = (now - created) / (24 * 3600 * 1000);
  if (now - created >= week) continue;
  const why = allowed.get(`${name} ${version}`);
  if (why !== undefined) {
    console.log(`${name} ${version}: ${days.toFixed(1)} days old, allowed: ${why}`);
    continue;
  }
  young++;
  console.log(`${name} ${version}: published ${days.toFixed(1)} days ago; a version waits 7 days`);
  await new Promise((r) => setTimeout(r, 250));
}
console.log(`${packages.length} crates, ${young} younger than 7 days`);
process.exit(young === 0 ? 0 : 1);
