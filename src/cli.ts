#!/usr/bin/env node
// delurio: the open checker and page of delurio-graph/0 (SPEC.md). Reads graph files and nothing else: no network, no
// account, no model.
import { mkdirSync, writeFileSync } from 'node:fs';
import { existsSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { GraphError, load } from './graph.ts';
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
import { CHECK_USAGE, checkMain } from './check.ts';

const USAGE = [
  'usage:',
  CHECK_USAGE,
  '  delurio page <graph dir> [--out <dir>] [--kit <kit>] [--locale <code>] [--strict]',
  '    writes the graph\'s page: <out>/index.html and its diagram (default out: <graph dir>/viz)',
  DIFF_USAGE.replaceAll('node src/cli.ts', 'delurio'),
].join('\n').replaceAll('node src/cli.ts', 'delurio');

const flag = (args: string[], name: string): string | undefined => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

function navPage(dir: string, args: string[]): number {
  const out = resolve(flag(args, '--out') ?? join(dir, 'viz'));
  const { g, warnings } = loadNav(dir, flag(args, '--locale'));
  const views = g.personas.map((p) => viewOf(g, p));
  const rows = menuRows(g, views);
  const goals = checkGoals(g, views);
  const found = findings(g, views, goals);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'index.html'), renderNavPage({ g, views, rows, goals, found, repo: null, warnings }));
  writeFileSync(join(out, 'nav.mmd'), renderNavMermaid(g));
  const problems = found.filter((f) => f.level === 'problem');
  console.log(`${g.id}: ${Object.keys(g.pages).length} pages, ${g.specs.length} ways to move, ${g.personas.length} kinds of user; ${problems.length} problems, ${found.length - problems.length} notes`);
  for (const w of warnings) console.error(`warning: ${w}`);
  console.log(`wrote ${join(out, 'index.html')} and ${join(out, 'nav.mmd')}`);
  return args.includes('--strict') && (warnings.length > 0 || problems.length > 0) ? 2 : 0;
}

async function flowPage(dir: string, args: string[]): Promise<number> {
  const out = resolve(flag(args, '--out') ?? join(dir, 'viz'));
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
  console.log(`${g.flow.id}: ${Object.keys(g.flow.states).length} states, ${ex.configs.length} configurations from ${ex.starts} starts, ${assertions.length - violated.length}/${assertions.length} assertions hold`);
  for (const a of violated) console.log(`  violated: ${a.id}`);
  for (const w of warnings) console.error(`warning: ${w}`);
  console.log(`wrote ${join(out, 'index.html')} and ${join(out, 'flow.mmd')}`);
  return args.includes('--strict') && (violated.length > 0 || warnings.length > 0) ? 2 : 0;
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
    return existsSync(join(dir, 'flow.yaml')) ? flowPage(dir, rest) : navPage(dir, rest);
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
