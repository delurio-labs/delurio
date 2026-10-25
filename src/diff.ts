import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { isAbsolute, join, posix, relative, resolve, sep } from 'node:path';
import { parse } from 'yaml';
import { GraphError, humanize, load } from './graph.ts';
import type { Graph } from './graph.ts';
import { loadNav } from './nav.ts';
import { joinWords } from './compare.ts';
import type { Change, Shift } from './compare.ts';
import { flowBehaviour, flowSameBehaviour, flowStructure, runSide } from './diff-flow.ts';
import type { FlowSide } from './diff-flow.ts';
import { navBehaviour, navSameBehaviour, navStructure, runNav } from './diff-nav.ts';

// `node src/cli.ts diff`: what a revision of a graph changes, for a PR description. Each side is a graph directory on disk
// or the same directory at a git revision (read with `git show <commit>:<path>`, nothing is checked out). No model, no
// network: the same two revisions give the same bytes.

export const DIFF_USAGE = [
  '  node src/cli.ts diff <old dir> <new dir> [--base <rev>] [--head <rev>] [--json] [--fail-on-behaviour] [--fail-on-regression]',
  '  node src/cli.ts diff <graph dir> --base <rev> [--head <rev>] ...',
  '    what the new revision changes, as markdown for a PR description (--json: for machines); --base reads the old side',
  '    from a git revision, --head the new one (default: the directory on disk). Works for flow and navigation graphs.',
  '    --fail-on-behaviour: exit 2 when what the graph does changed;  --fail-on-regression: exit 2 when a guarantee newly',
  '    fails, a goal newly fails or a problem is new',
].join('\n');

// One side of the diff. `read` and `exists` take paths relative to the graph directory; every file read is kept, so the
// header can say which files differ.
interface Source {
  where: string; // the directory as given
  rev: string | null; // the revision as shown, null for the directory on disk
  read: (file: string) => string;
  exists: (file: string) => boolean;
  texts: Map<string, string>;
}

// a directory inside the repository is named from its root, as a PR reader knows it; one outside as given
function shown(dir: string): string {
  const top = git(['rev-parse', '--show-toplevel'], process.cwd())?.trim();
  if (top === undefined || top === '') return dir;
  const rel = relative(top, resolve(dir)).split(sep).join('/');
  return rel.startsWith('..') || isAbsolute(rel) ? dir : rel === '' ? '.' : rel;
}

function dirSource(dir: string): Source {
  const texts = new Map<string, string>();
  if (!existsSync(dir)) throw new GraphError(`${dir} does not exist`);
  return {
    where: shown(dir),
    rev: null,
    texts,
    exists: (file) => existsSync(join(dir, file)),
    read: (file) => {
      const path = join(dir, file);
      if (!existsSync(path)) throw new GraphError(`${path} does not exist`);
      const text = readFileSync(path, 'utf8');
      texts.set(posix.normalize(file), text);
      return text;
    },
  };
}

function git(args: string[], cwd: string): string | null {
  try {
    return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 1 << 28 });
  } catch {
    return null;
  }
}

function gitSource(rev: string, dir: string): Source {
  const top = git(['rev-parse', '--show-toplevel'], process.cwd())?.trim();
  if (top === undefined || top === '') throw new GraphError(`--base and --head read a git revision, and ${process.cwd()} is not in a git repository`);
  const commit = git(['rev-parse', '--verify', '--quiet', `${rev}^{commit}`], top)?.trim();
  if (commit === undefined || commit === '') throw new GraphError(`${rev} is not a revision of ${top}`);
  const rel = relative(top, resolve(dir)).split(sep).join('/');
  if (rel.startsWith('..') || isAbsolute(rel)) throw new GraphError(`${dir} is outside the repository ${top}`);
  const short = commit.slice(0, 7); // fixed length, so the output does not change as the repository grows
  const path = (file: string): string => {
    const p = posix.normalize(posix.join(rel, file));
    if (p.startsWith('../')) throw new GraphError(`${file} is outside the repository`);
    return p;
  };
  const texts = new Map<string, string>();
  const shownRev = commit.startsWith(rev) ? short : `${rev} (${short})`;
  if (git(['cat-file', '-e', `${commit}:${rel === '' ? '.' : rel}`], top) === null) throw new GraphError(`${rel} does not exist at ${shownRev}; a new graph has no diff, render it instead`);
  return {
    where: rel === '' ? '.' : rel,
    rev: shownRev,
    texts,
    exists: (file) => git(['cat-file', '-e', `${commit}:${path(file)}`], top) !== null,
    read: (file) => {
      const text = git(['show', `${commit}:${path(file)}`], top);
      if (text === null) throw new GraphError(`${path(file)} does not exist at ${short}`);
      texts.set(posix.normalize(file), text);
      return text;
    },
  };
}

// The API description of a flow: domain.yaml beside it, else the shared ../domain/domain.yaml of a capture layout,
// else ../../domain/domain.yaml of graphs/ (graphs/<part>/<flow>). When the flow names its domain, the file must be that domain.
function findDomain(src: Source, g: Graph): FlowSide['domain'] {
  for (const file of ['domain.yaml', '../domain/domain.yaml', '../../domain/domain.yaml']) {
    if (!src.exists(file)) continue;
    const doc = parse(src.read(file)) as { id?: string } | null;
    const named = (g.flow as unknown as { domain?: unknown }).domain;
    if (typeof named === 'string' && doc?.id !== named) throw new GraphError(`flow.yaml names the domain ${named}, but ${file} is ${String(doc?.id)}`);
    return { file, doc };
  }
  return null;
}

type Loaded = { kind: 'flow'; side: FlowSide } | { kind: 'nav'; side: ReturnType<typeof loadNav> };

function loadSide(src: Source, name: string, locale: string | undefined): Loaded {
  try {
    if (src.exists('flow.yaml')) {
      const g = load(src.where, src.read, src.exists);
      return { kind: 'flow', side: { g, domain: findDomain(src, g) } };
    }
    if (src.exists('nav.yaml')) return { kind: 'nav', side: loadNav(src.where, locale, src.read) };
  } catch (err) {
    throw new GraphError(`${name} (${label(src)}): ${(err as Error).message}`);
  }
  throw new GraphError(`${name} (${label(src)}) has neither flow.yaml nor nav.yaml`);
}

const label = (s: Source): string => (s.rev === null ? s.where : `${s.where} at ${s.rev}`);

export interface Report {
  kind: 'flow' | 'nav';
  title: string;
  before: Source;
  after: Source;
  structure: Change[];
  behaviour: Shift[];
  same: string; // the sentence when nothing observable changed
}

const HEADINGS: Record<string, string> = {
  flow: 'The flow',
  graph: 'The graph',
  scope: 'What this view shows',
  main: 'The main story',
  steps: 'Steps',
  ways: 'How it moves on',
  guarantees: 'Guarantees',
  situations: 'What it keeps track of',
  conditions: 'Conditions and effects',
  screens: 'What the user sees',
  people: 'Kinds of user',
  pages: 'Pages',
  entries: 'Menus, links and buttons',
  goals: 'Goals',
  frames: 'Frames',
  facts: 'Facts and conditions',
  notModeled: 'Not modeled',
  wording: 'Wording',
  domain: 'The API it calls (domain.yaml)',
  other: 'Other keys',
  citations: 'Source citations',
};

// markdown: escape what would turn plain words into markup
const md = (s: string): string => s.replace(/([\\`*_[\]<])/g, '\\$1');

function tick(s: string): string {
  const longest = Math.max(0, ...(s.match(/`+/g) ?? []).map((r) => r.length));
  const fence = '`'.repeat(longest + 1);
  const pad = s.startsWith('`') || s.endsWith('`') ? ' ' : '';
  return `${fence}${pad}${s}${pad}${fence}`;
}

const value = (v: unknown): string => (v === undefined ? '(none)' : tick(JSON.stringify(v)));

function exactLine(c: Change): string {
  const what = c.field === undefined ? c.key : `${c.key} › ${c.field}`;
  const short = (v: unknown): string => {
    if (v === undefined) return '';
    const text = JSON.stringify(v);
    return text.length <= 160 ? `: ${tick(text)}` : '';
  };
  if (c.op === 'added') return `- ${tick(what)} added${short(c.after)}`;
  if (c.op === 'removed') return `- ${tick(what)} removed${short(c.before)}`;
  if (c.op === 'moved') return `- ${tick(what)} moved from ${String(c.before)} to ${String(c.after)}`;
  return `- ${tick(what)}: ${value(c.before)} → ${value(c.after)}`;
}

// the line that stands for the changes of an area that have no sentence of their own
function summaryLine(area: string, items: Change[]): string {
  const n = items.length;
  const plural = (k: number, w: string): string => `${k} ${w}${k === 1 ? '' : 's'}`;
  const distinct = (xs: string[]): string[] => [...new Set(xs)];
  if (area === 'conditions' || area === 'facts') {
    const guards = items.filter((c) => c.key.startsWith('guard ')).length;
    const other = n - guards;
    const parts = [guards > 0 ? plural(guards, 'named condition') : '', other > 0 ? plural(other, area === 'facts' ? 'fact' : 'effect') : ''].filter((x) => x !== '');
    return `${joinWords(parts)} changed; the exact text is under the exact changes.`;
  }
  if (area === 'domain') {
    const one: Record<string, string> = { entities: 'entity', actions: 'action', policies: 'policy', invariants: 'invariant', relations: 'relation', helpers: 'helper', permissions: 'permission', jobs: 'job' };
    const where = (c: Change): string => {
      const [section, name] = (c.field ?? '').split('.');
      return name === undefined ? section : `${one[section] ?? section} ${name}`;
    };
    return `${plural(n, 'change')} in the API description: ${joinWords(distinct(items.map(where)))}.`;
  }
  if (area === 'ways') return `${plural(n, 'way')} on from removed steps, under the exact changes.`;
  if (area === 'citations') return `${plural(n, 'source citation')} changed.`;
  if (area === 'other') return `${plural(n, 'change')} in keys this tool does not read: ${distinct(items.map((c) => (c.field ?? c.key).split('.')[0])).join(', ')}.`;
  return `${plural(n, 'more change')}, under the exact changes.`;
}

export function renderMarkdown(r: Report): string {
  const out: string[] = [];
  out.push(`## ${md(r.title)}: what this revision changes`, '');
  const kind = r.kind === 'flow' ? 'Flow graph' : 'Navigation graph';
  const at = (s: Source): string => (s.rev === null ? 'the directory on disk' : tick(s.rev));
  out.push(r.before.where === r.after.where ? `${kind} ${tick(r.after.where)}: ${at(r.before)} → ${at(r.after)}.` : `${kind}: ${tick(label(r.before))} → ${tick(label(r.after))}.`);
  const files = [...new Set([...r.after.texts.keys(), ...r.before.texts.keys()])];
  const changed = files.filter((f) => r.before.texts.get(f) !== r.after.texts.get(f));
  out.push(`Compared ${files.join(', ')}; ${changed.length === 0 ? 'no file differs' : `changed: ${changed.join(', ')}`}.`, '');

  out.push('### What it does differently', '');
  if (r.behaviour.length === 0) out.push(md(r.same));
  else if (r.kind === 'flow') for (const s of r.behaviour) out.push(...item(s));
  else {
    const people = [...new Set(r.behaviour.filter((s) => s.persona !== undefined).map((s) => s.persona as string))];
    for (const who of people) {
      const items = r.behaviour.filter((s) => s.persona === who);
      if (items.length > 8) out.push(`<details><summary><b>${md(who)}</b>: ${items.length} changes</summary>`, '', ...items.flatMap(item), '', '</details>', '');
      else out.push(`**${md(who)}**`, '', ...items.flatMap(item), '');
    }
    const rest = r.behaviour.filter((s) => s.persona === undefined);
    if (rest.length > 0) out.push('**Findings**', '', ...rest.flatMap(item));
  }
  out.push('');

  out.push('### What changed in the graph', '');
  if (r.structure.length === 0) out.push(changed.length === 0 ? 'Nothing.' : 'Nothing: the files differ only in comments, layout or key order.');
  for (const area of Object.keys(HEADINGS)) {
    const items = r.structure.filter((c) => c.area === area);
    if (items.length === 0) continue;
    const said = items.filter((c) => c.say !== null);
    const silent = items.filter((c) => c.say === null);
    out.push(`**${HEADINGS[area]}**`, '');
    for (const c of said) out.push(`- ${md(c.say as string)}`);
    if (silent.length > 0) out.push(`- ${md(summaryLine(area, silent))}`);
    out.push('');
  }
  if (r.structure.length > 0) {
    out.push(`<details><summary>Exact changes, by id (${r.structure.length})</summary>`, '');
    for (const c of r.structure) out.push(exactLine(c));
    out.push('', '</details>');
  }
  return `${out.join('\n').replace(/\n{3,}/g, '\n\n').trimEnd()}\n`;
}

function item(s: Shift): string[] {
  return [`- ${s.worse ? `**${md(s.say)}**` : md(s.say)}`, ...s.detail.map((d) => `  ${md(d)}`)];
}

export function renderJson(r: Report): string {
  const side = (s: Source): object => ({ path: s.where, revision: s.rev, files: [...s.texts.keys()] });
  const files = [...new Set([...r.after.texts.keys(), ...r.before.texts.keys()])];
  return `${JSON.stringify(
    {
      schema: 'delurio-graph-diff/0',
      kind: r.kind,
      title: r.title,
      before: side(r.before),
      after: side(r.after),
      changedFiles: files.filter((f) => r.before.texts.get(f) !== r.after.texts.get(f)),
      behaviourChanged: r.behaviour.length > 0,
      regressions: r.behaviour.filter((s) => s.worse).length,
      behaviour: r.behaviour,
      structure: r.structure,
    },
    null,
    2,
  )}\n`;
}

export function compare(before: Source, after: Source, locale?: string): Report {
  const b = loadSide(before, 'before', locale);
  const a = loadSide(after, 'after', locale);
  if (b.kind === 'flow' && a.kind === 'flow') {
    const s = flowStructure(b.side, a.side);
    const rb = runSide(b.side.g, `before (${label(before)})`);
    const ra = runSide(a.side.g, `after (${label(after)})`);
    return { kind: 'flow', title: a.side.g.flow.title ?? humanize(a.side.g.flow.id), before, after, structure: s.changes, behaviour: flowBehaviour(rb, ra, s), same: flowSameBehaviour(ra) };
  }
  if (b.kind === 'nav' && a.kind === 'nav') {
    for (const w of b.side.warnings) console.error(`warning: before: ${w}`);
    for (const w of a.side.warnings) console.error(`warning: after: ${w}`);
    const s = navStructure(b.side, a.side);
    const rb = runNav(b.side.g, `before (${label(before)})`);
    const ra = runNav(a.side.g, `after (${label(after)})`);
    return { kind: 'nav', title: a.side.g.title, before, after, structure: s.changes, behaviour: navBehaviour(rb, ra, s), same: navSameBehaviour(ra) };
  }
  throw new GraphError(`before (${label(before)}) is a ${b.kind} graph and after (${label(after)}) a ${a.kind} graph`);
}

export async function diffMain(args: string[]): Promise<number> {
  const withValue = ['--base', '--head', '--locale'];
  const dirs = args.filter((x, i) => !x.startsWith('--') && !withValue.includes(args[i - 1] ?? ''));
  const flag = (name: string): string | undefined => {
    const i = args.indexOf(name);
    return i >= 0 ? args[i + 1] : undefined;
  };
  const base = flag('--base');
  const head = flag('--head');
  for (const name of withValue) if (args.includes(name) && (flag(name) === undefined || flag(name)?.startsWith('--') === true)) throw new GraphError(`${name} needs a value`);
  if (dirs.length === 0 || dirs.length > 2 || (dirs.length === 1 && base === undefined && head === undefined)) {
    console.error(`usage:\n${DIFF_USAGE}`);
    return 1;
  }
  const [oldDir, newDir] = dirs.length === 2 ? dirs : [dirs[0], dirs[0]];
  const before = base === undefined ? dirSource(oldDir) : gitSource(base, oldDir);
  const after = head === undefined ? dirSource(newDir) : gitSource(head, newDir);
  const report = compare(before, after, flag('--locale'));
  const text = args.includes('--json') ? renderJson(report) : renderMarkdown(report);
  await new Promise<void>((done) => process.stdout.write(text, () => done())); // all of it, before the process exits
  const regressions = report.behaviour.filter((s) => s.worse).length;
  if (args.includes('--fail-on-regression') && regressions > 0) return 2;
  if (args.includes('--fail-on-behaviour') && report.behaviour.length > 0) return 2;
  return 0;
}
