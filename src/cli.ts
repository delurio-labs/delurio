#!/usr/bin/env node
// delurio: the open checker and page of delurio-graph/0 (SPEC.md). Reads graph files and nothing else: no network, no
// account, no model.
import { mkdirSync, writeFileSync } from 'node:fs';
import { existsSync, statSync } from 'node:fs';
import { basename, join, relative, resolve, sep } from 'node:path';
import { GraphError, humanize, load } from './graph.ts';
import { checkAssertions, explore, finalPaths, Model, ModelError } from './model.ts';
import { renderMermaid } from './mermaid.ts';
import { renderSpine } from './spine.ts';
import { renderScreens } from './wireframe.ts';
import { renderPage } from './html.ts';
import { checkChanges } from './changes.ts';
import { checkStarts } from './starts.ts';
import { mainStoryIndex, narrate, stepsByEvents, stepsTo } from './story.ts';
import { checkGoals, findings, loadNav, menuRows, viewOf } from './nav.ts';
import { renderNavMermaid, renderNavPage } from './nav-page.ts';
import { DIFF_USAGE, diffMain } from './diff.ts';
import { KitError, resolveKit } from './kit.ts';
import { CHECK_USAGE, checkMain, count, graphDirs } from './check.ts';
import { renderIndexPage } from './index-page.ts';
import type { IndexEntry } from './index-page.ts';

const USAGE = [
  'usage:',
  CHECK_USAGE,
  '  delurio page <graph dir | graphs folder> [--out <dir>] [--kit <kit>] [--locale <code>] [--strict]',
  '    writes the graph\'s page: <out>/index.html and its diagram (default out: <graph dir>/viz); on a folder of graphs,',
  '    the page of each at <out>/<its path> and an index of them all at <out>/index.html (default out: <folder>/viz)',
  DIFF_USAGE.replaceAll('node src/cli.ts', 'delurio'),
].join('\n').replaceAll('node src/cli.ts', 'delurio');

const flag = (args: string[], name: string): string | undefined => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

interface Paged {
  code: number;
  entry: IndexEntry;
}

function navPage(dir: string, out: string, args: string[]): Paged {
  const { g, warnings } = loadNav(dir, flag(args, '--locale'));
  const views = g.personas.map((p) => viewOf(g, p));
  const rows = menuRows(g, views);
  const goals = checkGoals(g, views);
  const found = findings(g, views, goals);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'index.html'), renderNavPage({ g, views, rows, goals, found, repo: null, warnings }));
  writeFileSync(join(out, 'nav.mmd'), renderNavMermaid(g));
  const problems = found.filter((f) => f.level === 'problem');
  console.log(`${g.id}: ${count(Object.keys(g.pages).length, 'page')}, ${count(g.specs.length, 'way')} to move, ${g.personas.length} ${g.personas.length === 1 ? 'kind' : 'kinds'} of user; ${count(problems.length, 'problem')}, ${count(found.length - problems.length, 'note')}`);
  for (const w of warnings) console.error(`warning: ${w}`);
  return {
    code: args.includes('--strict') && (warnings.length > 0 || problems.length > 0) ? 2 : 0,
    entry: { path: '', kind: 'nav', id: g.id, title: `${g.title}: where people go`, status: g.status, holds: 0, total: problems.length, warnings, error: null },
  };
}

async function flowPage(dir: string, out: string, args: string[]): Promise<Paged> {
  const warnings: string[] = [];
  const warn = (m: string): void => {
    if (!warnings.includes(m)) warnings.push(m);
  };
  const g = load(dir);
  const kit = resolveKit(dir, g.ui.kit, flag(args, '--kit'));
  const model = new Model(g.flow);
  const ex = explore(model);
  const assertions = checkAssertions(model, ex);
  const finals = finalPaths(model, ex);
  const mainIndex = mainStoryIndex(g, ex);
  const declared = g.flow.main === undefined ? null : stepsByEvents(model, g.flow.main);
  if (g.flow.main !== undefined && declared === null) warn('the declared main story cannot be walked: an event is not possible where it is sent');
  const mainSteps = declared ?? (mainIndex === null ? null : stepsTo(model, ex, mainIndex));
  const main = mainSteps === null ? null : narrate(g, model, mainSteps);
  const spineMain = await renderSpine(g, ex, 'main', mainSteps);
  const spineFull = await renderSpine(g, ex, 'full', mainSteps);
  const screens = renderScreens(g, warn, kit);
  const handOvers = checkStarts(g, model, ex);
  for (const h of handOvers) for (const p of h.problems) warn(`${h.from} starts ${h.flow}: ${p}`);
  for (const p of checkChanges(g)?.problems ?? []) warn(p);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'index.html'), renderPage({ g, m: model, ex, assertions, finals, main, spineMain, spineFull, screens, kit, handOvers }));
  writeFileSync(join(out, 'flow.mmd'), renderMermaid(g, ex));
  const violated = assertions.filter((a) => a.violation !== undefined);
  console.log(`${g.flow.id}: ${count(Object.keys(g.flow.states).length, 'state')}, ${count(ex.configs.length, 'configuration')} from ${count(ex.starts, 'start')}, ${assertions.length - violated.length}/${assertions.length} assertions hold`);
  for (const a of violated) console.log(`  violated: ${a.id}`);
  for (const w of warnings) console.error(`warning: ${w}`);
  return {
    code: args.includes('--strict') && (violated.length > 0 || warnings.length > 0) ? 2 : 0,
    entry: { path: '', kind: 'flow', id: g.flow.id, title: g.flow.title ?? humanize(g.flow.id), status: g.flow.status ?? null, holds: assertions.length - violated.length, total: assertions.length, warnings, error: null },
  };
}

const isGraph = (dir: string): boolean => existsSync(join(dir, 'flow.yaml')) || existsSync(join(dir, 'nav.yaml'));

// every graph under the folder, each into <out>/<its path>, and the index of them all. A graph that cannot be loaded
// is listed as such and the rest are still written: exit 1 when one cannot be loaded, else 2 under --strict when one
// would fail on its own, else 0.
async function treePages(root: string, args: string[]): Promise<number> {
  const dirs = graphDirs(root);
  if (dirs.length === 0) {
    console.error(`error: ${root} is not a graph and holds none: there is no flow.yaml or nav.yaml in it or under it`);
    return 1;
  }
  const out = resolve(flag(args, '--out') ?? join(root, 'viz'));
  const entries: IndexEntry[] = [];
  let broken = false;
  let failing = false;
  for (const dir of dirs) {
    const path = relative(root, dir).split(sep).join('/');
    const nav = !existsSync(join(dir, 'flow.yaml'));
    try {
      const { code, entry } = nav ? navPage(dir, join(out, path), args) : await flowPage(dir, join(out, path), args);
      entries.push({ ...entry, path });
      failing ||= code !== 0;
    } catch (err) {
      if (!(err instanceof GraphError || err instanceof ModelError || err instanceof KitError || (err instanceof Error && err.name === 'YAMLParseError'))) throw err;
      broken = true;
      const message = err.message.split('\n')[0].replace(/:$/, '');
      console.error(`error: ${path}: ${message}`);
      entries.push({ path, kind: nav ? 'nav' : 'flow', id: null, title: path, status: null, holds: 0, total: 0, warnings: [], error: message });
    }
  }
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'index.html'), renderIndexPage(basename(resolve(root)), entries));
  console.log(`wrote ${join(out, 'index.html')} and the pages of ${count(entries.length - entries.filter((e) => e.error !== null).length, 'graph')} under ${out}`);
  return broken ? 1 : failing ? 2 : 0;
}

async function main(): Promise<number> {
  const args = process.argv.slice(2);
  if (args[0] === 'check') return checkMain(args.slice(1));
  if (args[0] === 'diff') return diffMain(args.slice(1));
  if (args[0] === 'page') {
    const rest = args.slice(1);
    const withValue = ['--out', '--kit', '--locale'];
    const dir = rest.find((a, i) => !a.startsWith('--') && !withValue.includes(rest[i - 1] ?? ''));
    if (dir === undefined) {
      console.error(USAGE);
      return 1;
    }
    if (!existsSync(dir) || !statSync(dir).isDirectory()) {
      console.error(`error: ${dir} is not a folder`);
      return 1;
    }
    if (!isGraph(dir)) return treePages(dir, rest);
    const out = resolve(flag(rest, '--out') ?? join(dir, 'viz'));
    const { code } = existsSync(join(dir, 'flow.yaml')) ? await flowPage(dir, out, rest) : navPage(dir, out, rest);
    console.log(`wrote ${join(out, 'index.html')} and ${join(out, existsSync(join(dir, 'flow.yaml')) ? 'flow.mmd' : 'nav.mmd')}`);
    return code;
  }
  console.error(USAGE);
  return args.length === 0 || args.includes('--help') ? 0 : 1;
}

main().then(
  (code) => process.exit(code),
  (err: unknown) => {
    if (err instanceof GraphError || err instanceof ModelError || err instanceof KitError) {
      console.error(`error: ${err.message}`);
      process.exit(1);
    }
    throw err;
  },
);
