// The differential harness: runs the TypeScript checker of a fixed reference commit and the Rust one on the same
// cases, and compares everything they write: each command's stdout, stderr and exit code, and every file left in the
// case's folder (pages, diagrams, SARIF), byte for byte. A case is a bash script in cases/ that calls "$DELURIO"; it
// runs once per checker, in the same folder each time, so every path in the output is the same.
//
//   node tests/differential/run.mjs --ts "node <ref>/dist/cli.js" --rust target/release/delurio \
//     [--cases tests/differential/cases] [--only <regex>] [--sets <folder>] [--work <folder>] [--report <file>]
//
// --sets is a folder of graph sets (delurio/, qauth/, medaris-main/, medaris-395/: see cases/set-*.sh); a set case
// whose set is not there is skipped. Each invocation's time and peak memory are recorded (GNU time, when installed)
// and summed per case in the report. Exit 0 when every case is identical, 1 otherwise.
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '..', '..');
const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : fallback;
};
const sides = { ts: flag('--ts'), rust: flag('--rust') };
if (sides.ts === undefined || sides.rust === undefined) {
  console.error('usage: node tests/differential/run.mjs --ts "<command>" --rust "<command>" [--cases <dir>] [--only <regex>] [--sets <dir>] [--work <dir>] [--report <file>]');
  process.exit(2);
}
const casesDir = resolve(flag('--cases', join(here, 'cases')));
const only = new RegExp(flag('--only', '.'));
const sets = flag('--sets') === undefined ? '' : resolve(flag('--sets'));
const work = resolve(flag('--work', join(repo, 'target', 'differential')));
const reportFile = resolve(flag('--report', join(work, 'report.md')));
const gnuTime = ['/usr/bin/time', '/usr/local/bin/gtime', '/opt/homebrew/bin/gtime'].find((t) => existsSync(t)) ?? null;

// what the two checkers may write differently by design: the tool's own version in SARIF
const normalize = (text) => text.replace(/("name": "delurio",\n\s*"version": )"[^"]*"/g, '$1"<version>"');

const wrapper = (real, log) => `#!/usr/bin/env bash
out=$(mktemp); err=$(mktemp); tm=$(mktemp)
${gnuTime === null ? '' : `${gnuTime} -f '%e %M' -o "$tm" `}${real} "$@" > "$out" 2> "$err"; code=$?
{ printf '$ delurio'; printf ' %q' "$@"; printf '\\n[exit %s]\\n--- stdout\\n' "$code"; cat "$out"; printf -- '--- stderr\\n'; cat "$err"; } >> '${log}'
printf '%s\\t%s\\n' "$(tail -n 1 "$tm")" "$*" >> '${log}.time'
cat "$out"; cat "$err" >&2; rm -f "$out" "$err" "$tm"; exit $code
`;

// every file under a folder: its path and bytes, folders first by name
function tree(root, out = new Map(), base = root) {
  if (!existsSync(root)) return out;
  for (const name of readdirSync(root).sort()) {
    const path = join(root, name);
    if (statSync(path).isDirectory()) tree(path, out, base);
    else out.set(relative(base, path), readFileSync(path));
  }
  return out;
}

function runCase(file, side) {
  const name = file.replace(/\.sh$/, '');
  const root = join(work, name);
  const logs = join(work, 'logs');
  mkdirSync(logs, { recursive: true });
  const log = join(logs, `${name}.${side}.log`);
  rmSync(root, { recursive: true, force: true });
  for (const f of [log, `${log}.time`]) rmSync(f, { force: true });
  writeFileSync(log, '');
  writeFileSync(`${log}.time`, '');
  mkdirSync(join(root, 'bin'), { recursive: true });
  mkdirSync(join(root, 'tmp'), { recursive: true });
  // the case sees the repository's examples (and its own folder for anything it writes), as the CI step did
  cpSync(join(repo, 'examples'), join(root, 'repo', 'examples'), { recursive: true });
  writeFileSync(join(root, 'bin', 'delurio'), wrapper(sides[side], log), { mode: 0o755 });
  const r = spawnSync('bash', [join(casesDir, file)], {
    cwd: join(root, 'repo'),
    env: { ...process.env, DELURIO: join(root, 'bin', 'delurio'), TMPDIR_CASE: join(root, 'tmp'), SETS: sets, LANG: 'C.UTF-8', LC_ALL: 'C.UTF-8', TZ: 'UTC' },
    encoding: 'utf8',
    maxBuffer: 1 << 30,
  });
  const files = tree(join(root, 'repo'));
  for (const [k, v] of tree(join(root, 'tmp'))) files.set(`tmp/${k}`, v);
  const times = readFileSync(`${log}.time`, 'utf8').trim().split('\n').filter(Boolean).map((l) => {
    const [measure, ...cmd] = l.split('\t');
    const [secs, kb] = measure.split(' ');
    return { secs: Number(secs), kb: Number(kb), cmd: cmd.join('\t') };
  });
  return { log: normalize(readFileSync(log, 'utf8')), script: r.stdout + r.stderr, files, times };
}

function unified(a, b, label) {
  const dir = join(work, 'diff-tmp');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'ts'), a);
  writeFileSync(join(dir, 'rust'), b);
  const r = spawnSync('diff', ['-u', '--label', `ts/${label}`, '--label', `rust/${label}`, join(dir, 'ts'), join(dir, 'rust')], { encoding: 'utf8', maxBuffer: 1 << 30 });
  return r.stdout;
}

const cases = readdirSync(casesDir).filter((f) => f.endsWith('.sh') && only.test(f)).sort();
const rows = [];
const details = [];
const binary = (buf) => buf.includes(0);
for (const file of cases) {
  const declared = readFileSync(join(casesDir, file), 'utf8').split('\n').filter((l) => /^# set: /.test(l)).map((l) => l.slice(7).trim());
  const needsSet = declared.length > 0 && (sets === '' || !declared.every((d) => existsSync(join(sets, d))));
  if (needsSet) {
    rows.push({ file, status: 'skipped (set not given)' });
    continue;
  }
  process.stderr.write(`${file} ...`);
  const ts = runCase(file, 'ts');
  const tsFiles = ts.files;
  const rust = runCase(file, 'rust');
  const problems = [];
  if (ts.log !== rust.log) problems.push(unified(ts.log, rust.log, 'commands.log'));
  for (const path of new Set([...tsFiles.keys(), ...rust.files.keys()])) {
    const a = tsFiles.get(path);
    const b = rust.files.get(path);
    if (a === undefined || b === undefined) {
      problems.push(`only the ${a === undefined ? 'Rust' : 'TypeScript'} checker wrote ${path}\n`);
      continue;
    }
    const na = binary(a) ? a : Buffer.from(normalize(a.toString('utf8')));
    const nb = binary(b) ? b : Buffer.from(normalize(b.toString('utf8')));
    if (Buffer.compare(na, nb) === 0) continue;
    problems.push(binary(a) || binary(b) ? `${path} differs (binary): ${createHash('sha256').update(a).digest('hex').slice(0, 12)} vs ${createHash('sha256').update(b).digest('hex').slice(0, 12)}\n` : unified(na.toString('utf8'), nb.toString('utf8'), path));
  }
  const sum = (t, k) => t.times.reduce((s, x) => s + (Number.isFinite(x[k]) ? x[k] : 0), 0);
  const peak = (t) => t.times.reduce((m, x) => Math.max(m, Number.isFinite(x.kb) ? x.kb : 0), 0);
  rows.push({
    file,
    status: problems.length === 0 ? 'identical' : `DIFFERENT (${problems.length})`,
    commands: ts.times.length,
    files: tsFiles.size,
    tsSecs: sum(ts, 'secs'),
    rustSecs: sum(rust, 'secs'),
    tsKb: peak(ts),
    rustKb: peak(rust),
    timing: ts.times.map((t, i) => ({ cmd: t.cmd, ts: t, rust: rust.times[i] })),
  });
  if (problems.length > 0) details.push(`## ${file}\n\n\`\`\`diff\n${problems.join('\n').split('\n').slice(0, 400).join('\n')}\n\`\`\`\n`);
  process.stderr.write(` ${rows.at(-1).status}\n`);
}

const mb = (kb) => (kb > 0 ? `${(kb / 1024).toFixed(0)} MB` : '-');
const s = (x) => (gnuTime === null ? '-' : `${x.toFixed(2)} s`);
const lines = [
  '# Differential run: TypeScript reference vs Rust',
  '',
  `TypeScript: \`${sides.ts}\`  `,
  `Rust: \`${sides.rust}\`  `,
  `Node: ${process.version}; sets: ${sets || '(none)'}`,
  '',
  '| case | result | commands | files | time TS | time Rust | peak TS | peak Rust |',
  '| --- | --- | --- | --- | --- | --- | --- | --- |',
  ...rows.map((r) => `| ${r.file} | ${r.status} | ${r.commands ?? ''} | ${r.files ?? ''} | ${r.tsSecs === undefined ? '' : s(r.tsSecs)} | ${r.rustSecs === undefined ? '' : s(r.rustSecs)} | ${r.tsKb === undefined ? '' : mb(r.tsKb)} | ${r.rustKb === undefined ? '' : mb(r.rustKb)} |`),
  '',
  '## Time and peak memory of each command of the set cases',
  '',
  '| case | command | time TS | time Rust | peak TS | peak Rust |',
  '| --- | --- | --- | --- | --- | --- |',
  ...rows.filter((r) => /^set-/.test(r.file) && r.timing !== undefined).flatMap((r) => r.timing.map((t) => `| ${r.file} | \`${t.cmd.slice(0, 90)}\` | ${s(t.ts.secs)} | ${t.rust === undefined ? '-' : s(t.rust.secs)} | ${mb(t.ts.kb)} | ${t.rust === undefined ? '-' : mb(t.rust.kb)} |`)),
  '',
  ...details,
];
mkdirSync(dirname(reportFile), { recursive: true });
writeFileSync(reportFile, `${lines.join('\n')}\n`);
const different = rows.filter((r) => r.status.startsWith('DIFFERENT')).length;
console.log(`${rows.length} cases: ${rows.filter((r) => r.status === 'identical').length} identical, ${different} different, ${rows.filter((r) => r.status.startsWith('skipped')).length} skipped; report: ${reportFile}`);
process.exit(different === 0 ? 0 : 1);
