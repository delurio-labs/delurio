// `check`: every graph under a folder, as a project's CI runs it (graphs/agents/cli.check, SPEC.md §10). It reads the
// files and nothing else, writes no page, and gives each finding as a GNU error line, or as SARIF 2.1.0 with
// --format sarif. Exit 0 when clean, 1 when a graph cannot be loaded or is written for a newer format, 2 when an error
// is found under --strict (and, with --baseline, new since that run).
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { keyAt, parsingOnce, readYaml } from './yaml.ts';
import type { DomainPart } from './domain.ts';
import { alternatives, GraphError, load, placed } from './graph.ts';
import type { ErrorPlace, Graph } from './graph.ts';
import { checkAssertions, explore, Model, ModelError } from './model.ts';
import { stepsByEvents } from './story.ts';
import { renderScreens } from './wireframe.ts';
import { checkStarts } from './starts.ts';
import { checkChanges, flowDomain } from './changes.ts';
import type { Domain } from './changes.ts';
import { checkUi } from './ui-check.ts';
import { checkInvokes } from './invokes.ts';
import { checkTutorial } from './tutorial-check.ts';
import { KitError, resolveKit } from './kit.ts';
import { checkGoals, findings, loadNav, viewOf } from './nav.ts';
import { schemaKindOf, validateFile } from './schemas.ts';
import { inReadingOrder, mergeDomain } from './domain.ts';
import { checkDomainNames } from './domain-names.ts';
import type { EntityElsewhere } from './domain-names.ts';
import { checkNeeds, planned, readNeeds } from './needs.ts';
import { checkAnchors } from './anchors.ts';

export const CHECK_USAGE = [
  '  node src/cli.ts check <graphs folder>... [--strict] [--baseline <sarif>] [--format sarif --output <file>] [--app <dir> | --no-app]',
  '    checks every graph under the folders and writes nothing else: GNU error lines on stdout, or SARIF 2.1.0;',
  '    exit 0 clean, 1 a graph cannot be loaded or needs a newer format, 2 an error under --strict (new since --baseline);',
  '    the evidence of needs and the source: anchors are found under --app, the application\'s repository root',
  '    (default: the working directory); --no-app checks the graphs without the code, and does not look for them',
].join('\n');

const FORMAT = 'delurio-graph/0';
const SKIP = new Set(['viz', 'video', 'recording', 'node_modules', '.git']);

type Level = 'error' | 'warning' | 'note';

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
  { id: 'schema', text: "A graph file does not follow its kind's schema (SPEC.md)", level: 'error' },
  { id: 'domain-twice', text: 'A value of a domain is given in two of its files (SPEC.md §5)', level: 'error' },
  { id: 'domain-place', text: 'A domain file is not with the rest of its domain (SPEC.md §1)', level: 'error' },
  { id: 'server-fn-http', text: 'A server function is given a route, a query or an operationId, which only an HTTP operation has (SPEC.md §5)', level: 'error' },
  { id: 'entity-unknown', text: "An action's returns or a references names an entity its domain does not have (SPEC.md §5)", level: 'error' },
  { id: 'field-unknown', text: "An entity's key or required names a field the entity does not have (SPEC.md §5)", level: 'error' },
  { id: 'assertion-violated', text: 'A guarantee of the flow is broken by a reachable situation', level: 'error' },
  { id: 'main-story-unwalkable', text: 'The declared main story cannot be walked', level: 'error' },
  { id: 'catalog-key-missing', text: "A catalog key the screens use is missing from a locale's catalog", level: 'error' },
  { id: 'sample-missing', text: 'A catalog message has a placeholder with no sample value', level: 'error' },
  { id: 'no-renderer', text: 'A component has no renderer', level: 'error' },
  { id: 'kit-uncovered', text: "The kit does not cover a component or an intent", level: 'error' },
  { id: 'list-item-outside-list', text: 'A list item is drawn outside a list', level: 'error' },
  { id: 'hand-over', text: 'A started flow cannot begin with what it is given', level: 'error' },
  { id: 'changes-note-missing', text: 'A flow whose steps write has no changes note (SPEC.md §4)', level: 'error' },
  { id: 'changes-note-locale', text: "The changes note is missing from a locale's catalog", level: 'error' },
  { id: 'changes-note-gap', text: 'A writing flow says why it has no changes note yet: counted, and fails nothing (SPEC.md §4)', level: 'note' },
  { id: 'task-locale', text: "The flow's name for its users is missing from a locale's catalog (SPEC.md §4)", level: 'error' },
  { id: 'source-missing', text: 'A state of a captured flow names no code (SPEC.md §9)', level: 'error' },
  { id: 'source-unresolved', text: 'A source anchor names a file that is not there, or a line past its end (SPEC.md §9)', level: 'error' },
  { id: 'citation-unresolved', text: "Words that cite the code name a file that is not there, or a line past its end (SPEC.md §9)", level: 'warning' },
  { id: 'focus-not-shown', text: 'A focus or a returnsFocus gives the focus to a node that is not shown there (SPEC.md §4)', level: 'error' },
  { id: 'returns-focus-not-dialog', text: 'A returnsFocus does not start at a dialog (SPEC.md §4)', level: 'error' },
  { id: 'role-missing', text: 'A field or an action gives no role, by which a screen reader finds it (SPEC.md §4)', level: 'warning' },
  { id: 'state-no-way-out', text: 'A state that is not an exit has no way out (SPEC.md §2)', level: 'error' },
  { id: 'state-unreachable', text: 'No situation the flow starts in reaches the state (SPEC.md §2)', level: 'error' },
  { id: 'tutorial-mismatch', text: 'A tutorial no longer matches its flow (SPEC.md §7)', level: 'error' },
  { id: 'evidence-unresolved', text: "A need's evidence names what is not there (SPEC.md §13)", level: 'error' },
  { id: 'need-no-evidence', text: 'A need that is not missing names no evidence outside the graph (SPEC.md §13)', level: 'error' },
  { id: 'need-names-built', text: 'A missing need names a state that is built as its evidence (SPEC.md §13)', level: 'warning' },
  { id: 'need-unlisted', text: "A missing need is not in the product's gaps (SPEC.md §13)", level: 'error' },
  { id: 'need-twice', text: "A need's id is given twice in its flow's needs (SPEC.md §13)", level: 'error' },
  { id: 'needs-place', text: 'A needs file is beside no flow, or is the needs of another one (SPEC.md §13)', level: 'error' },
  { id: 'gap-met', text: 'The gaps list a need that is met (SPEC.md §13)', level: 'error' },
  { id: 'gap-unknown', text: 'The gaps list a need that no flow under them has (SPEC.md §13)', level: 'error' },
  { id: 'need-gap', text: 'A need that is missing, listed in the gaps: counted, and fails nothing (SPEC.md §13)', level: 'note' },
  { id: 'invoke-unknown', text: "An invoke or a failures entry names an action its flow's domain does not have, or the domain is not found (SPEC.md §5)", level: 'error' },
  { id: 'refusal-unknown', text: 'A failure class lists a code that is neither a refusal of its action nor a transport code (SPEC.md §2, §5)', level: 'error' },
  { id: 'policy-unknown', text: 'A policy names an action or an entity its domain does not have (SPEC.md §5)', level: 'error' },
  { id: 'planned-built', text: 'A planned entity or action names its code (SPEC.md §5)', level: 'error' },
  { id: 'graph-warning', text: 'Another warning of the graph', level: 'error' },
  { id: 'nav-problem', text: 'A problem of the navigation graph', level: 'error' },
  { id: 'nav-note', text: 'A note on the navigation graph', level: 'warning' },
];
const LEVEL = new Map(RULES.map((r) => [r.id, r.level]));
// the rules whose errors stop the run (exit 1) however --strict and --baseline read them
const BREAKING = new Set(['load', 'format-newer']);

export const count = (n: number, word: string): string => `${n} ${word}${n === 1 ? '' : 's'}`;

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

// every graph folder at or under root, in path order
export function graphDirs(root: string, out: string[] = []): string[] {
  if (existsSync(join(root, 'flow.yaml')) || existsSync(join(root, 'nav.yaml'))) out.push(root);
  for (const name of readdirSync(root).sort()) {
    if (SKIP.has(name) || name.startsWith('.')) continue;
    const sub = join(root, name);
    if (statSync(sub).isDirectory()) graphDirs(sub, out);
  }
  return out;
}

// every file under root that a schema covers (flow, ui, nav, nav.extracted, domain, catalog), in path order
function graphFiles(root: string, out: string[] = []): string[] {
  for (const name of readdirSync(root).sort()) {
    if (SKIP.has(name) || name.startsWith('.')) continue;
    const path = join(root, name);
    if (statSync(path).isDirectory()) graphFiles(path, out);
    else if (schemaKindOf(path) !== null) out.push(path);
  }
  return out;
}

// the format the file says it is written in: null when it is this one
function newerFormat(abs: string): string | null {
  const doc = readYaml(readFileSync(abs, 'utf8')) as { schema?: unknown } | null;
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

// the words of the screens in each locale but the one ui.yaml names (SPEC.md §4): a key the screens use that a locale's
// catalog lacks, at the key's line, as for the catalog ui.yaml names; a locale whose catalog is not there, at `locales`
function checkLocales(g: Graph, dir: string): CheckResult[] {
  if (g.ui.absent === true) return [];
  const ui = join(dir, 'ui.yaml');
  const named = join(dir, g.ui.catalog);
  const out: CheckResult[] = [];
  for (const locale of g.ui.locales ?? []) {
    const name = join(dirname(g.ui.catalog), `${locale}.json`);
    const abs = join(dir, name);
    if (abs === named) continue;
    let catalog: Record<string, string>;
    try {
      catalog = JSON.parse(readFileSync(abs, 'utf8')) as Record<string, string>;
    } catch (err) {
      const text = readFileSync(ui, 'utf8');
      const why = existsSync(abs) ? `is not JSON: ${(err as Error).message}` : 'does not exist';
      out.push(result('catalog-key-missing', ui, keyAt(text, ['locales']), `the ${locale} catalog, ${name}, ${why}: every word the screens use is missing in ${locale}`, g.flow.id, `catalog/${locale}`));
      continue;
    }
    const missing: string[] = [];
    const collect = (m: string): void => {
      const key = /^catalog key (\S+) is missing/.exec(m)?.[1];
      if (key !== undefined && !missing.includes(key)) missing.push(key);
    };
    const other: Graph = { ...g, catalog: catalog !== null && typeof catalog === 'object' ? catalog : {} };
    renderScreens(other, collect, null);
    checkUi(other, collect);
    for (const key of missing) out.push(result('catalog-key-missing', ui, placeOf(ui, key), `catalog key ${key} is missing from the ${locale} catalog, ${name}`, g.flow.id, `${key}/${locale}`));
  }
  return out;
}

// the states that some transition leads to, whatever its guard
function targeted(g: Graph): Set<string> {
  const out = new Set<string>();
  for (const st of Object.values(g.flow.states)) {
    for (const a of st.always ?? []) out.add(a.target);
    for (const spec of Object.values(st.on ?? {})) for (const a of alternatives(spec)) out.add(a.target);
  }
  return out;
}

function checkFlow(dir: string, domains: Map<string, Domain | null>): CheckResult[] {
  const file = join(dir, 'flow.yaml');
  const g = load(dir);
  let kit: ReturnType<typeof resolveKit>;
  try {
    kit = resolveKit(dir, g.ui.kit, undefined);
  } catch (err) {
    throw g.ui.kit === undefined ? err : placed(err, { file: 'ui.yaml', path: ['kit'] });
  }
  const model = new Model(g.flow);
  const ex = explore(model);
  const out: CheckResult[] = [];
  const flowText = readFileSync(file, 'utf8');
  // a state the flow stops in for good though it is no exit, and a state no situation reaches (SPEC.md §2)
  const led = targeted(g);
  for (const [name, st] of Object.entries(g.flow.states)) {
    const at = (): { line: number; column: number } => keyAt(flowText, ['states', name]);
    const element = `states/${name}`;
    const ways = Object.keys(st.on ?? {}).length + (st.always ?? []).length;
    if (st.type !== 'final' && ways === 0 && st.invoke === undefined) {
      out.push(result('state-no-way-out', file, at(), `state ${name} is not an exit and has no way out: it has no on, always or invoke, so the flow stops there; make it an exit (type: final) or give it a transition (SPEC.md §2)`, g.flow.id, element));
    }
    if (!ex.atState.has(name)) {
      const why = led.has(name) ? 'the transitions that lead to it are never taken: the states they leave are not reached, or their guards never hold' : 'no transition leads to it';
      out.push(result('state-unreachable', file, at(), `state ${name} is reached by no situation the flow starts in: ${why} (SPEC.md §2)`, g.flow.id, element));
    }
  }
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
  if (g.ui.absent !== true) {
    const ui = join(dir, 'ui.yaml');
    const text = readFileSync(ui, 'utf8');
    for (const p of checkUi(g, warn)) out.push(result(p.rule, ui, keyAt(text, p.path), p.message, g.flow.id, p.element));
  }
  out.push(...checkLocales(g, dir));
  for (const h of checkStarts(g, model, ex)) for (const p of h.problems) warn(`${h.from} starts ${h.flow}: ${p}`);
  // the flow's domain, looked up once for the rules that read it
  let domain: Domain | null | undefined;
  const domainOf = (): Domain | null => (domain === undefined ? (domain = flowDomain(g, domains)) : domain);
  const changes = checkChanges(g, domainOf);
  for (const p of changes?.problems ?? []) warn(p);
  for (const w of warnings) out.push(fromWarning(dir, g.flow.id, w));
  // a writing flow whose app has no changes message yet says why: a gap of the flow, at the level note (SPEC.md §4)
  if (changes !== null && changes.missing !== null) {
    const ui = join(dir, 'ui.yaml');
    const message = `its steps write (${changes.writes.join(', ')}), and its changes note is missing: ${changes.missing}; Rehber does not guide its writing steps (SPEC.md §4)`;
    out.push(result('changes-note-gap', ui, keyAt(readFileSync(ui, 'utf8'), ['changes', 'missing']), message, g.flow.id, 'changes/missing'));
  }
  for (const p of checkInvokes(g, domainOf)) out.push(result(p.rule, file, keyAt(flowText, p.path), p.message, g.flow.id, p.element));
  const tutorial = join(dir, 'tutorial.yaml');
  if (existsSync(tutorial)) {
    const text = readFileSync(tutorial, 'utf8');
    for (const p of checkTutorial(g, model, ex, text)) {
      const at = p.line === undefined ? keyAt(text, p.path) : { line: p.line, column: p.column ?? 1 };
      out.push(result('tutorial-mismatch', tutorial, at, p.message, g.flow.id, `tutorial.yaml#/${p.path.join('/')}`));
    }
  }
  // a captured flow is read from code, so every state names the code that makes it true; an exit may leave it out, as
  // the transition into it names that code, and so may a state that only missing needs name, which is not built yet
  // (SPEC.md §9, §13)
  if (g.flow.status === 'captured') {
    let needs: ReturnType<typeof readNeeds> = null;
    try {
      needs = readNeeds(dir);
    } catch {
      // a needs.yaml that cannot be read plans nothing; its schema check says why
    }
    const notBuilt = planned(needs).states;
    for (const [name, st] of Object.entries(g.flow.states)) {
      if (st.source !== undefined || st.type === 'final' || notBuilt.has(name)) continue;
      out.push(result('source-missing', file, keyAt(flowText, ['states', name]), `state ${name} names no code: in a captured flow every state but an exit carries source: <path>:<line> (SPEC.md §9)`, g.flow.id, `states/${name}`));
    }
  }
  return out;
}

// what only an HTTP operation has (SPEC.md §5), and why a server function has none of it
const HTTP_ONLY: Record<string, string> = {
  route: 'a route is an operation that others call over HTTP, and a server function is called only by the product\'s own pages: give one of them',
  query: 'a server function\'s input is its body, whatever its method',
  operationId: 'a server function is no operation of an OpenAPI document',
};

// a server function that is also given what only an HTTP operation has (SPEC.md §5), at its `serverFn` with a note at
// the other key; the two may come from two files of a split domain, so the domain is read as one first
function checkServerFunctions(parts: DomainPart[], doc: Record<string, unknown>, id: string): CheckResult[] {
  const out: CheckResult[] = [];
  const read = parts.map((p) => {
    try {
      return readYaml(p.text) as { actions?: Record<string, Record<string, unknown> | null> } | null;
    } catch {
      return null;
    }
  });
  // the part that gives the action's key, and where in it
  const given = (name: string, key: string): { file: string; line: number; column: number } | null => {
    const i = read.findIndex((d) => d?.actions?.[name]?.[key] !== undefined);
    return i < 0 ? null : { file: parts[i].file, ...keyAt(parts[i].text, ['actions', name, key]) };
  };
  const actions = (doc.actions ?? {}) as Record<string, Record<string, unknown> | null>;
  for (const [name, a] of Object.entries(actions)) {
    if (a === null || typeof a !== 'object' || a.serverFn === undefined) continue;
    const at = given(name, 'serverFn');
    if (at === null) continue;
    for (const [key, why] of Object.entries(HTTP_ONLY)) {
      if (a[key] === undefined) continue;
      const other = given(name, key);
      const said = key === 'route' ? `a route, ${String(a.route)}` : key === 'query' ? 'a query' : `an operationId, ${String(a.operationId)}`;
      const notes: Note[] = other === null ? [] : [{ file: rel(other.file), line: other.line, column: other.column, message: `its ${key} is given here` }];
      out.push(result('server-fn-http', at.file, at, `the action ${name} is a server function (serverFn: ${String(a.serverFn)}) and has ${said}: ${why} (SPEC.md §5)`, id, `actions/${name}/${key}`, notes));
    }
  }
  return out;
}

// an entity or an action that says it is planned, written before its code, and names its code all the same (SPEC.md
// §5): at its `planned`, with a note where its `source` is given, as the two may come from two files of a split domain
function checkPlanned(parts: DomainPart[], doc: Record<string, unknown>, id: string): CheckResult[] {
  const out: CheckResult[] = [];
  const read = parts.map((p) => {
    try {
      return readYaml(p.text) as Record<string, Record<string, Record<string, unknown> | null> | undefined> | null;
    } catch {
      return null;
    }
  });
  const given = (section: string, name: string, key: string): { file: string; line: number; column: number } | null => {
    const i = read.findIndex((d) => d?.[section]?.[name]?.[key] !== undefined);
    return i < 0 ? null : { file: parts[i].file, ...keyAt(parts[i].text, [section, name, key]) };
  };
  for (const [section, word] of [['entities', 'entity'], ['actions', 'action']] as const) {
    const items = (doc[section] ?? {}) as Record<string, Record<string, unknown> | null>;
    if (items === null || typeof items !== 'object') continue;
    for (const [name, item] of Object.entries(items)) {
      if (item === null || typeof item !== 'object' || typeof item.planned !== 'string' || typeof item.source !== 'string') continue;
      const at = given(section, name, 'planned');
      const src = given(section, name, 'source');
      if (at === null) continue;
      const notes: Note[] = src === null ? [] : [{ file: rel(src.file), line: src.line, column: src.column, message: `its source is given here` }];
      out.push(result('planned-built', at.file, at, `the ${word} ${name} says it is planned (${item.planned}), and names its code, ${item.source}: a planned ${word} has no code yet; once it is built, take out planned (SPEC.md §5)`, id, `${section}/${name}/planned`, notes));
    }
  }
  return out;
}

// whether a domain's file gives the entity
function givesEntity(text: string, entity: string): boolean {
  try {
    const entities = (readYaml(text) as { entities?: unknown } | null)?.entities;
    return entities !== null && typeof entities === 'object' && Object.hasOwn(entities, entity);
  } catch {
    return false;
  }
}

// the domains under root, each the domain files of one directory read as one (SPEC.md §5): a value given in two of
// them, a file whose id is not its directory's, a domain whose files sit in two directories, a server function given
// what only an HTTP operation has, and a name of an entity, a field or an action that the domain does not have
function checkDomains(files: string[]): CheckResult[] {
  const byDir = new Map<string, string[]>();
  for (const f of files) if (schemaKindOf(f) === 'domain') byDir.set(dirname(f), [...(byDir.get(dirname(f)) ?? []), f]);
  const out: CheckResult[] = [];
  const dirsOf = new Map<string, string[]>(); // a domain's id -> the directories that hold it
  const domains = [...byDir].map(([dir, list]) => {
    const parts = inReadingOrder(list).map((file) => ({ file, text: readFileSync(file, 'utf8') }));
    return { dir, parts, merged: mergeDomain(parts) };
  });
  // an entity of another domain, for the note of a name that this one does not have
  const elsewhere = (self: string): EntityElsewhere => (entity) => {
    for (const { dir, parts, merged } of domains) {
      const id = merged.id ?? rel(dir);
      const entities = merged.doc.entities as Record<string, unknown> | undefined;
      if (id === self || entities === null || typeof entities !== 'object' || !Object.hasOwn(entities, entity)) continue;
      const part = parts.find((p) => givesEntity(p.text, entity)) ?? parts[0];
      return { domain: id, file: part.file, ...keyAt(part.text, ['entities', entity]) };
    }
    return null;
  };
  for (const { dir, parts, merged } of domains) {
    const id = merged.id ?? rel(dir);
    for (const p of merged.problems) {
      const notes: Note[] = p.first === undefined ? [] : [{ file: rel(p.first.file), line: p.first.line, column: p.first.column, message: 'given first here' }];
      out.push(result(p.rule, p.file, p, p.message, id, p.element, notes));
    }
    out.push(...checkServerFunctions(parts, merged.doc, id));
    out.push(...checkPlanned(parts, merged.doc, id));
    for (const p of checkDomainNames(parts, merged.doc, id, elsewhere(id))) {
      out.push(result(p.rule, p.file, p, p.message, id, p.element, p.notes.map((n) => ({ ...n, file: rel(n.file) }))));
    }
    if (merged.id !== null) dirsOf.set(merged.id, [...(dirsOf.get(merged.id) ?? []), dir]);
  }
  for (const [id, dirs] of dirsOf) {
    for (const dir of dirs.slice(1)) {
      const file = inReadingOrder(byDir.get(dir) ?? [])[0];
      out.push(result('domain-place', file, placeOf(file, 'id:'), `the domain ${id} is in ${rel(dirs[0])}/ too: a domain sits in one directory, so every flow that names it reads all of it (SPEC.md §1)`, id, `${basename(file)}#/id`));
    }
  }
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

// the level a finding is printed at, and counted at: an error known from the baseline is a warning, as it fails no
// run, but one that stops the run (it cannot be loaded) is an error however often it was seen
const printedLevel = (r: CheckResult): Level => (r.level === 'error' && r.baselineState === 'unchanged' && !BREAKING.has(r.rule) ? 'warning' : r.level);

// where a load error is (SPEC.md §10): the place the loader or the model gave it, at the keys it names or the line a
// parser gave, in the file it names; else the line a YAML parser gave; else the start of the graph's own file
function loadPlace(dir: string, file: string, err: Error): Place {
  const place = (err as { place?: ErrorPlace }).place;
  const at = place === undefined ? file : join(dir, place.file);
  if (place?.line !== undefined) return { file: at, line: place.line, column: place.column ?? 1 };
  const linePos = (err as { linePos?: Array<{ line: number; col: number }> }).linePos?.[0];
  if (linePos !== undefined) return { file: at, line: linePos.line, column: linePos.col };
  if (place?.path !== undefined && existsSync(at)) return { file: at, ...keyAt(readFileSync(at, 'utf8'), place.path) };
  return { file: at, line: 1, column: 1 };
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

function sarif(results: CheckResult[], exitCode: number, said: string[]): object {
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
        invocations: [
          {
            executionSuccessful: exitCode !== 1,
            exitCode,
            ...(said.length === 0 ? {} : { toolExecutionNotifications: said.map((text) => ({ level: 'note', message: { text } })) }),
          },
        ],
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

// every text the check reads is parsed once in it (`parsingOnce`), however many graphs read it
export function checkMain(args: string[]): number {
  return parsingOnce(() => checkRun(args));
}

function checkRun(args: string[]): number {
  const flag = (name: string): string | undefined => {
    const i = args.indexOf(name);
    return i >= 0 ? args[i + 1] : undefined;
  };
  const withValue = ['--baseline', '--format', '--output', '--app'];
  const dirs = args.filter((a, i) => !a.startsWith('--') && !withValue.includes(args[i - 1] ?? ''));
  if (dirs.length === 0) {
    console.error(`usage:\n${CHECK_USAGE}`);
    return 1;
  }
  const notFolder = dirs.find((d) => !existsSync(d) || !statSync(d).isDirectory());
  if (notFolder !== undefined) {
    console.error(`${notFolder} is not a folder\nusage:\n${CHECK_USAGE}`);
    return 1;
  }
  // every folder given, each graph and file once though one folder holds another
  const roots = dirs.map((d) => resolve(d));
  const dirsOfGraphs = [...new Set(roots.flatMap((r) => graphDirs(r)))];
  const strict = args.includes('--strict');
  const format = flag('--format') ?? 'text';
  const output = flag('--output');
  const baseline = flag('--baseline');
  if (format !== 'text' && format !== 'sarif') {
    console.error(`--format is text or sarif, not ${format}`);
    return 1;
  }
  const appGiven = flag('--app');
  if (appGiven !== undefined && (!existsSync(appGiven) || !statSync(appGiven).isDirectory())) {
    console.error(`--app ${appGiven} is not a folder\nusage:\n${CHECK_USAGE}`);
    return 1;
  }
  // --no-app: the graphs are here without the application's code, so what they name of it is not looked for (SPEC.md §10)
  const noApp = args.includes('--no-app');
  if (noApp && appGiven !== undefined) {
    console.error(`--app names the application's code and --no-app says it is not here: give one of them\nusage:\n${CHECK_USAGE}`);
    return 1;
  }
  const app = noApp ? null : resolve(appGiven ?? '.');

  const results: CheckResult[] = [];
  const domains = new Map<string, Domain | null>(); // the domains found, for every flow that names one
  let broken = false;
  for (const d of dirsOfGraphs) {
    const file = existsSync(join(d, 'flow.yaml')) ? join(d, 'flow.yaml') : join(d, 'nav.yaml');
    try {
      const newer = newerFormat(file);
      if (newer !== null) {
        broken = true;
        results.push(result('format-newer', file, placeOf(file, 'schema:'), `written in ${newer}; this checker reads ${FORMAT}: pin a version that reads it`, rel(d), 'schema'));
        continue;
      }
      results.push(...(file.endsWith('flow.yaml') ? checkFlow(d, domains) : checkNav(d)));
    } catch (err) {
      if (!(err instanceof GraphError || err instanceof ModelError || err instanceof KitError || (err instanceof Error && err.name === 'YAMLParseError'))) throw err;
      broken = true;
      const at = loadPlace(d, file, err);
      results.push(result('load', at.file, at, err.message.split('\n')[0], rel(d), 'load'));
    }
  }
  // every graph file against its kind's schema: what an editor with the schemas marks, check reports
  const files = [...new Set(roots.flatMap((r) => graphFiles(r)))];
  for (const file of files) {
    for (const f of validateFile(file)) results.push(result('schema', file, f, f.message, f.graph ?? rel(file), f.element));
  }
  results.push(...checkDomains(files));
  // each flow's needs held to their evidence, and the product's gaps to its needs (SPEC.md §13)
  const needs = checkNeeds(files, app);
  for (const p of needs.problems) results.push(result(p.rule, p.file, p, p.message, p.graph, p.element));
  // every source anchor, and every citation of the code in a graph's words, held to the application's files (SPEC.md §9)
  const anchors = checkAnchors(files, app);
  for (const p of anchors.problems) results.push(result(p.rule, p.file, p, p.message, p.graph, p.element));
  results.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.column - b.column || a.rule.localeCompare(b.rule) || a.message.localeCompare(b.message));

  if (baseline !== undefined) {
    const prints = baselinePrints(baseline);
    for (const r of results) r.baselineState = prints.has(fingerprint(r)) ? 'unchanged' : 'new';
  }
  // what --no-app did not look for, said once rather than as a finding each (SPEC.md §10)
  const said: string[] = [];
  if (noApp) {
    const of = (n: number, one: string, many: string): string => `${n} ${n === 1 ? one : many}`;
    const what = [count(anchors.anchors, 'source anchor'), of(anchors.citations, 'citation in words', 'citations in words'), of(needs.notLookedFor, 'piece of evidence', 'pieces of evidence')];
    said.push(`--no-app: ${what.slice(0, -1).join(', ')} and ${what[what.length - 1]} name files of the application, which are not here, and were not looked for (SPEC.md §10)`);
  }
  const failing = results.filter((r) => r.level === 'error' && !BREAKING.has(r.rule) && (r.baselineState === undefined || r.baselineState === 'new'));
  const exitCode = broken ? 1 : strict && failing.length > 0 ? 2 : 0;

  if (format === 'sarif') {
    const text = `${JSON.stringify(sarif(results, exitCode, said), null, 2)}\n`;
    if (output === undefined) process.stdout.write(text);
    else writeFileSync(output, text);
  }
  if (format === 'text' || output !== undefined) {
    for (const r of results) {
      console.log(`${r.file}:${r.line}:${r.column}: ${printedLevel(r)}: ${r.message} [${r.rule}]`);
      for (const n of r.notes) console.log(`${n.file}:${n.line}:${n.column}: note: ${n.message}`);
    }
    const graphs = dirsOfGraphs.length;
    const errors = results.filter((r) => printedLevel(r) === 'error').length;
    const warnings = results.filter((r) => printedLevel(r) === 'warning').length;
    // the gaps, once a flow has needs: every need that is missing, listed or not, and those the base run did not have
    const fresh = results.filter((r) => r.rule === 'need-gap' && r.baselineState === 'new').length;
    const gaps = needs.files === 0 ? '' : `, ${count(needs.gaps, 'gap')} of ${count(needs.needs, 'need')}${fresh > 0 ? ` (${fresh} new)` : ''}`;
    // the changes notes that writing flows say are missing (SPEC.md §4), once there is one
    const notes = results.filter((r) => r.rule === 'changes-note-gap');
    const newNotes = notes.filter((r) => r.baselineState === 'new').length;
    const missingNotes = notes.length === 0 ? '' : `, ${count(notes.length, 'missing changes note')}${newNotes > 0 ? ` (${newNotes} new)` : ''}`;
    for (const s of said) console.error(`delurio check: note: ${s}`);
    console.error(`delurio check: ${count(graphs, 'graph')}, ${count(errors, 'error')}, ${count(warnings, 'warning')}${gaps}${missingNotes}; exit ${exitCode}`);
  }
  return exitCode;
}
