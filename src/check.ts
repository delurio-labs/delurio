// `check`: every graph under a folder, as a project's CI runs it (graphs/agents/cli.check, SPEC.md §10). It reads the
// files and nothing else, writes no page, and gives each finding as a GNU error line, or as SARIF 2.1.0 with
// --format sarif. Exit 0 when clean, 1 when a graph cannot be loaded or is written for a newer format, 2 when an error
// is found under --strict (and, with --baseline, new since that run).
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { parse } from 'yaml';
import { GraphError, load } from './graph.ts';
import { checkAssertions, explore, Model, ModelError } from './model.ts';
import { stepsByEvents } from './story.ts';
import { renderScreens } from './wireframe.ts';
import { checkStarts } from './starts.ts';
import { checkChanges } from './changes.ts';
import { KitError, resolveKit } from './kit.ts';
import { checkGoals, findings, loadNav, viewOf } from './nav.ts';

export const CHECK_USAGE = [
  '  node src/cli.ts check <graphs folder> [--strict] [--baseline <sarif>] [--format sarif --output <file>]',
  '    checks every graph under the folder and writes nothing else: GNU error lines on stdout, or SARIF 2.1.0;',
  '    exit 0 clean, 1 a graph cannot be loaded or needs a newer format, 2 an error under --strict (new since --baseline)',
].join('\n');

const FORMAT = 'delurio-graph/0';
const SKIP = new Set(['viz', 'video', 'recording', 'node_modules', '.git']);

type Level = 'error' | 'warning';

interface Place {
  file: string; // as given on the command line's side: relative to the working directory
  line: number;
  column: number;
}

interface Note extends Place {
  message: string;
}

export interface CheckResult extends Place {
  rule: string;
  level: Level;
  message: string;
  graph: string; // the graph's id
  element: string; // what in the graph it is about, for the fingerprint and the logical location
  notes: Note[];
  baselineState?: 'new' | 'unchanged';
}

interface Rule {
  id: string;
  text: string;
  level: Level;
}

const RULES: Rule[] = [
  { id: 'load', text: 'The graph cannot be loaded', level: 'error' },
  { id: 'format-newer', text: 'The graph is written for a newer format than this checker reads', level: 'error' },
  { id: 'assertion-violated', text: 'A guarantee of the flow is broken by a reachable situation', level: 'error' },
  { id: 'main-story-unwalkable', text: 'The declared main story cannot be walked', level: 'error' },
  { id: 'catalog-key-missing', text: 'A catalog key the screens use is missing', level: 'error' },
  { id: 'sample-missing', text: 'A catalog message has a placeholder with no sample value', level: 'error' },
  { id: 'no-renderer', text: 'A component has no renderer', level: 'error' },
  { id: 'kit-uncovered', text: "The kit does not cover a component or an intent", level: 'error' },
  { id: 'list-item-outside-list', text: 'A list item is drawn outside a list', level: 'error' },
  { id: 'hand-over', text: 'A started flow cannot begin with what it is given', level: 'error' },
  { id: 'changes-note-missing', text: 'A flow whose steps write has no changes note (SPEC.md §4)', level: 'error' },
  { id: 'changes-note-locale', text: "The changes note is missing from a locale's catalog", level: 'error' },
  { id: 'graph-warning', text: 'Another warning of the graph', level: 'error' },
  { id: 'nav-problem', text: 'A problem of the navigation graph', level: 'error' },
  { id: 'nav-note', text: 'A note on the navigation graph', level: 'warning' },
];
const LEVEL = new Map(RULES.map((r) => [r.id, r.level]));

// the 1-based line and column of the first occurrence of `needle`, else the file's start
function placeOf(abs: string, needle: string | null): { line: number; column: number } {
  if (needle === null || !existsSync(abs)) return { line: 1, column: 1 };
  const lines = readFileSync(abs, 'utf8').split('\n');
  for (let i = 0; i < lines.length; i++) {
    const c = lines[i].indexOf(needle);
    if (c >= 0) return { line: i + 1, column: c + 1 };
  }
  return { line: 1, column: 1 };
}

function graphDirs(root: string, out: string[] = []): string[] {
  if (existsSync(join(root, 'flow.yaml')) || existsSync(join(root, 'nav.yaml'))) out.push(root);
  for (const name of readdirSync(root).sort()) {
    if (SKIP.has(name) || name.startsWith('.')) continue;
    const sub = join(root, name);
    if (statSync(sub).isDirectory()) graphDirs(sub, out);
  }
  return out;
}

// the format the file says it is written in: null when it is this one
function newerFormat(abs: string): string | null {
  const doc = parse(readFileSync(abs, 'utf8')) as { schema?: unknown } | null;
  const s = typeof doc?.schema === 'string' ? doc.schema : FORMAT;
  return s === FORMAT ? null : s;
}

function rel(abs: string): string {
  return relative(process.cwd(), abs) || '.';
}

function result(rule: string, file: string, at: { line: number; column: number }, message: string, graph: string, element: string, notes: Note[] = []): CheckResult {
  return { rule, level: LEVEL.get(rule) ?? 'error', file: rel(file), line: at.line, column: at.column, message, graph, element, notes };
}

// a warning raised while checking a flow, sorted into its rule and placed in its file
function fromWarning(dir: string, graph: string, w: string): CheckResult {
  const flow = join(dir, 'flow.yaml');
  const ui = join(dir, 'ui.yaml');
  let m: RegExpMatchArray | null;
  if ((m = /^catalog key (\S+) is missing/.exec(w))) return result('catalog-key-missing', ui, placeOf(ui, m[1]), w, graph, m[1]);
  if ((m = /^no sample value for \{(\w+)\} in (\S+)/.exec(w))) return result('sample-missing', ui, placeOf(ui, m[2]), w, graph, `${m[2]}/${m[1]}`);
  if ((m = /^(\S+): ListItem outside a List/.exec(w))) return result('list-item-outside-list', ui, placeOf(ui, `${m[1]}:`), w, graph, m[1]);
  if ((m = /\(node (\S+?)\)/.exec(w)) && /^kit /.test(w)) return result('kit-uncovered', ui, placeOf(ui, `${m[1]}:`), w, graph, m[1]);
  if ((m = /^(\S+): component .* has no wireframe renderer/.exec(w))) return result('no-renderer', ui, placeOf(ui, `${m[1]}:`), w, graph, m[1]);
  if (/declared main story cannot be walked/.test(w)) return result('main-story-unwalkable', flow, placeOf(flow, 'main:'), w, graph, 'main');
  if ((m = /^(\S+) starts (\S+): (.*)$/.exec(w))) return result('hand-over', flow, placeOf(flow, `flow: ${m[2]}`), w, graph, `${m[1]}->${m[2]}`);
  if (/has no changes note/.test(w)) return result('changes-note-missing', ui, placeOf(ui, 'samples:'), w, graph, 'changes');
  if ((m = /^the changes note (\S+) is not in the (\w+) catalog/.exec(w))) return result('changes-note-locale', ui, placeOf(ui, 'changes:'), w, graph, `changes/${m[2]}`);
  return result('graph-warning', existsSync(ui) ? ui : flow, { line: 1, column: 1 }, w, graph, w);
}

function checkFlow(dir: string): CheckResult[] {
  const file = join(dir, 'flow.yaml');
  const g = load(dir);
  const kit = resolveKit(dir, g.ui.kit, undefined);
  const model = new Model(g.flow);
  const ex = explore(model);
  const out: CheckResult[] = [];
  const warnings: string[] = [];
  const warn = (m: string): void => {
    if (!warnings.includes(m)) warnings.push(m);
  };
  for (const a of checkAssertions(model, ex)) {
    if (a.violation === undefined) continue;
    const at = placeOf(file, `id: ${a.id}`);
    const start = Object.entries(a.violation.start).map(([k, v]) => `${k} = ${String(v)}`).join(', ');
    const notes: Note[] = [{ file: rel(file), ...at, message: `start: ${start || 'the initial context'}` }];
    a.violation.events.forEach((e, i) => notes.push({ file: rel(file), ...at, message: `step ${i + 1}: ${e}` }));
    notes.push({ file: rel(file), ...at, message: `reached: ${a.violation.state}` });
    out.push(result('assertion-violated', file, at, `${a.id} is broken: ${a.say ?? a.never}`, g.flow.id, a.id, notes));
  }
  if (g.flow.main !== undefined && stepsByEvents(model, g.flow.main) === null) warn('the declared main story cannot be walked: an event is not possible where it is sent');
  renderScreens(g, warn, kit);
  for (const h of checkStarts(g, model, ex)) for (const p of h.problems) warn(`${h.from} starts ${h.flow}: ${p}`);
  for (const p of checkChanges(g)?.problems ?? []) warn(p);
  for (const w of warnings) out.push(fromWarning(dir, g.flow.id, w));
  return out;
}

function checkNav(dir: string): CheckResult[] {
  const file = join(dir, 'nav.yaml');
  const { g, warnings } = loadNav(dir, undefined);
  const views = g.personas.map((p) => viewOf(g, p));
  const found = findings(g, views, checkGoals(g, views));
  const out: CheckResult[] = found.map((f) =>
    result(f.level === 'problem' ? 'nav-problem' : 'nav-note', file, { line: 1, column: 1 }, `${f.say} (${f.code})`, g.id, f.code, f.detail.map((d) => ({ file: rel(file), line: 1, column: 1, message: d }))),
  );
  for (const w of warnings) out.push(result('graph-warning', file, { line: 1, column: 1 }, w, g.id, w));
  return out;
}

const fingerprint = (r: CheckResult): string => createHash('sha256').update(`${r.rule}|${r.graph}|${r.element}`).digest('hex').slice(0, 32);

function baselinePrints(file: string): Set<string> {
  const doc = JSON.parse(readFileSync(file, 'utf8')) as { runs?: Array<{ results?: Array<{ fingerprints?: Record<string, string> }> }> };
  const out = new Set<string>();
  for (const run of doc.runs ?? []) for (const r of run.results ?? []) if (r.fingerprints?.['delurio/v1'] !== undefined) out.add(r.fingerprints['delurio/v1']);
  return out;
}

function toolVersion(): string {
  const pkg = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as { version?: string };
  return pkg.version ?? '0.0.0';
}

function sarif(results: CheckResult[], exitCode: number): object {
  const used = RULES.filter((r) => results.some((x) => x.rule === r.id));
  return {
    $schema: 'https://json.schemastore.org/sarif-2.1.0.json',
    version: '2.1.0',
    runs: [
      {
        tool: {
          driver: {
            name: 'delurio',
            version: toolVersion(),
            informationUri: 'https://delur.io',
            rules: used.map((r) => ({ id: r.id, shortDescription: { text: r.text }, defaultConfiguration: { level: r.level } })),
          },
        },
        automationDetails: { id: 'delurio/check/' },
        invocations: [{ executionSuccessful: exitCode !== 1, exitCode }],
        results: results.map((r) => ({
          ruleId: r.rule,
          ruleIndex: used.findIndex((u) => u.id === r.rule),
          level: r.level,
          message: { text: r.message },
          locations: [
            {
              physicalLocation: { artifactLocation: { uri: r.file.split('\\').join('/') }, region: { startLine: r.line, startColumn: r.column } },
              logicalLocations: [{ fullyQualifiedName: `${r.graph}/${r.element}` }],
            },
          ],
          fingerprints: { 'delurio/v1': fingerprint(r) },
          ...(r.notes.length > 0 && r.rule === 'assertion-violated'
            ? { codeFlows: [{ threadFlows: [{ locations: r.notes.map((n) => ({ location: { message: { text: n.message } } })) }] }] }
            : {}),
          ...(r.baselineState === undefined ? {} : { baselineState: r.baselineState }),
        })),
      },
    ],
  };
}

export function checkMain(args: string[]): number {
  const flag = (name: string): string | undefined => {
    const i = args.indexOf(name);
    return i >= 0 ? args[i + 1] : undefined;
  };
  const withValue = ['--baseline', '--format', '--output'];
  const dir = args.find((a, i) => !a.startsWith('--') && !withValue.includes(args[i - 1] ?? ''));
  if (dir === undefined) {
    console.error(`usage:\n${CHECK_USAGE}`);
    return 1;
  }
  const root = resolve(dir);
  const strict = args.includes('--strict');
  const format = flag('--format') ?? 'text';
  const output = flag('--output');
  const baseline = flag('--baseline');
  if (format !== 'text' && format !== 'sarif') {
    console.error(`--format is text or sarif, not ${format}`);
    return 1;
  }

  const results: CheckResult[] = [];
  let broken = false;
  for (const d of graphDirs(root)) {
    const file = existsSync(join(d, 'flow.yaml')) ? join(d, 'flow.yaml') : join(d, 'nav.yaml');
    try {
      const newer = newerFormat(file);
      if (newer !== null) {
        broken = true;
        results.push(result('format-newer', file, placeOf(file, 'schema:'), `written in ${newer}; this checker reads ${FORMAT}: pin a version that reads it`, rel(d), 'schema'));
        continue;
      }
      results.push(...(file.endsWith('flow.yaml') ? checkFlow(d) : checkNav(d)));
    } catch (err) {
      if (!(err instanceof GraphError || err instanceof ModelError || err instanceof KitError || (err instanceof Error && err.name === 'YAMLParseError'))) throw err;
      broken = true;
      const linePos = (err as { linePos?: Array<{ line: number; col: number }> }).linePos?.[0];
      results.push(result('load', file, linePos === undefined ? { line: 1, column: 1 } : { line: linePos.line, column: linePos.col }, err.message.split('\n')[0], rel(d), 'load'));
    }
  }
  results.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.column - b.column || a.rule.localeCompare(b.rule) || a.message.localeCompare(b.message));

  if (baseline !== undefined) {
    const prints = baselinePrints(baseline);
    for (const r of results) r.baselineState = prints.has(fingerprint(r)) ? 'unchanged' : 'new';
  }
  const failing = results.filter((r) => r.level === 'error' && r.rule !== 'load' && r.rule !== 'format-newer' && (r.baselineState === undefined || r.baselineState === 'new'));
  const exitCode = broken ? 1 : strict && failing.length > 0 ? 2 : 0;

  if (format === 'sarif') {
    const text = `${JSON.stringify(sarif(results, exitCode), null, 2)}\n`;
    if (output === undefined) process.stdout.write(text);
    else writeFileSync(output, text);
  }
  if (format === 'text' || output !== undefined) {
    for (const r of results) {
      const level = r.level === 'error' && r.baselineState === 'unchanged' ? 'warning' : r.level;
      console.log(`${r.file}:${r.line}:${r.column}: ${level}: ${r.message} [${r.rule}]`);
      for (const n of r.notes) console.log(`${n.file}:${n.line}:${n.column}: note: ${n.message}`);
    }
    const graphs = graphDirs(root).length;
    const count = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
    const errors = results.filter((r) => r.level === 'error').length;
    const warnings = results.filter((r) => r.level === 'warning').length;
    console.error(`delurio check: ${count(graphs, 'graph')}, ${count(errors, 'error')}, ${count(warnings, 'warning')}; exit ${exitCode}`);
  }
  return exitCode;
}
