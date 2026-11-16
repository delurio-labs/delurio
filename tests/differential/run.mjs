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
// and summed per case in the report.
//
// What must be the same is the JUDGEMENT (the owner's acceptance, 16 Nov 2026): every command's exit code, and for
// `check` its findings, each as its rule, level, file and line (text) and its fingerprint, the hash of its rule, graph
// and element (SARIF), and the counts of its last line. A case whose bytes differ but whose judgement is the same is
// "same judgement" (its wording differs, which the report shows); a different judgement, or a page or diagram whose
// content differs, fails the run. Exit 0 when no case's judgement differs, 1 otherwise; --bytes fails on any byte.
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
const strictBytes = args.includes('--bytes');

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

// the commands of a log: their arguments, exit code, stdout and stderr
function commands(log) {
  return log.split(/^(?=\$ delurio)/m).filter((b) => b.startsWith('$ delurio')).map((b) => {
    const m = /^\$ delurio(.*)\n\[exit (\d+)\]\n--- stdout\n([\s\S]*?)--- stderr\n([\s\S]*)$/.exec(b);
    return m === null ? { args: b, exit: null, stdout: '', stderr: '' } : { args: m[1].trim(), exit: Number(m[2]), stdout: m[3], stderr: m[4] };
  });
}

// a check's judgement in its text or SARIF output: the sorted findings and the numbers of its last line
const FINDING = /^(.+?):(\d+):\d+: (error|warning|note): .* \[([a-z-]+)\]$/;
function judgement(text) {
  const t = text.trim();
  if (t.startsWith('{')) {
    try {
      const run = JSON.parse(t).runs?.[0];
      const results = (run?.results ?? []).map((r) => `${r.ruleId} ${r.level} ${r.locations?.[0]?.physicalLocation?.artifactLocation?.uri}:${r.locations?.[0]?.physicalLocation?.region?.startLine} ${r.fingerprints?.['delurio/v1']} ${r.baselineState ?? ''}`);
      return [`exit ${run?.invocations?.[0]?.exitCode}`, ...results.sort()].join('\n');
    } catch {
      return t;
    }
  }
  const lines = t.split('\n');
  const found = lines.map((l) => FINDING.exec(l)).filter((m) => m !== null).map((m) => `${m[4]} ${m[3]} ${m[1]}:${m[2]}`);
  const counts = lines.filter((l) => l.startsWith('delurio check: ') && !l.startsWith('delurio check: note:')).map((l) => (l.match(/\d+/g) ?? []).join(' '));
  return [...found.sort(), ...counts].join('\n');
}

// what a case decides: per command its exit code and, for check, its judgement; per file a check's output read for its
// judgement, any other file (a page, a diagram) by its bytes
function decided(run, files) {
  const out = [];
  for (const c of commands(run.log)) {
    out.push(`$ ${c.args} -> exit ${c.exit}`);
    if (/^check\b/.test(c.args)) out.push(judgement(c.stdout), judgement(c.stderr));
  }
  for (const [path, bytes] of files) {
    const text = bytes.includes(0) ? null : normalize(bytes.toString('utf8'));
    if (text !== null && (path.endsWith('.sarif') || path.endsWith('.txt'))) out.push(`${path}: ${judgement(text)}`);
    else out.push(`${path}: ${createHash('sha256').update(text ?? bytes).digest('hex')}`);
  }
  return out.join('\n');
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
  const sameJudgement = decided(ts, tsFiles) === decided(rust, rust.files);
  if (!sameJudgement) problems.unshift(`JUDGEMENT:\n${unified(decided(ts, tsFiles), decided(rust, rust.files), 'judgement')}`);
  const sum = (t, k) => t.times.reduce((s, x) => s + (Number.isFinite(x[k]) ? x[k] : 0), 0);
  const peak = (t) => t.times.reduce((m, x) => Math.max(m, Number.isFinite(x.kb) ? x.kb : 0), 0);
  rows.push({
    file,
    status: problems.length === 0 ? 'identical' : sameJudgement ? `same judgement, wording differs (${problems.length})` : `JUDGEMENT DIFFERS (${problems.length})`,
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
const different = rows.filter((r) => r.status.startsWith('JUDGEMENT')).length;
const worded = rows.filter((r) => r.status.startsWith('same judgement')).length;
console.log(`${rows.length} cases: ${rows.filter((r) => r.status === 'identical').length} identical, ${worded} the same judgement in other words, ${different} judged differently, ${rows.filter((r) => r.status.startsWith('skipped')).length} skipped; report: ${reportFile}`);
process.exit(different === 0 && (!strictBytes || worded === 0) ? 0 : 1);
