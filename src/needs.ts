// A flow's needs and a product's gaps (SPEC.md §13). needs.yaml, beside a flow, is a closed list of what must exist for
// the flow's users: each need names the evidence that it does, or says `missing` and why. needs.gaps.yaml, at the root
// of a product's graphs, lists every need of its flows that is missing. `checkNeeds` holds each piece of evidence to
// what it names, each missing need to the gaps file above it, and each entry of a gaps file to a need under its folder
// that is still missing, so the list only shrinks on its own. `planned` gives a tool that walks a flow on the app the
// states and nodes that only missing needs are drawn as, for it to leave out.
import { existsSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, isAbsolute, join } from 'node:path';
import { keyAt, readYaml } from './yaml.ts';
import { AppFiles } from './anchors.ts';

export const NEEDS_FILE = 'needs.yaml';
export const GAPS_FILE = 'needs.gaps.yaml';

const FORMAT = 'delurio-graph/0';

export interface Evidence {
  source?: string; // <path>[:<line>], from the application's repository root (SPEC.md §9)
  test?: string;
  name?: string; // with source or test: a name the file holds
  scenario?: string;
  binding?: string; // with scenario: the binding, when it is not the flow's own binding.yaml
  state?: string;
  node?: string;
  setting?: string;
  in?: string; // with setting: the file that sets it
}

export interface Need {
  id: string;
  need: string;
  evidence?: Evidence[];
  missing?: string;
  note?: string;
}

export interface Needs {
  schema?: string;
  kind: 'needs';
  id: string; // the flow's
  status?: string;
  needs: Need[];
}

export type NeedRule = 'evidence-unresolved' | 'need-no-evidence' | 'need-names-built' | 'need-unlisted' | 'need-gap' | 'need-twice' | 'needs-place' | 'gap-met' | 'gap-unknown';

export interface NeedProblem {
  rule: NeedRule;
  file: string; // as found: absolute when the check was given absolute folders
  line: number;
  column: number;
  message: string;
  graph: string; // the flow's id for a needs file, the product's for a gaps file
  element: string; // what it is about, stable when lines move: needs/<id>/evidence/<kind>:<value>, gaps/<flow>/<need>
}

export interface NeedsCheck {
  problems: NeedProblem[];
  files: number; // the needs files read
  needs: number;
  gaps: number; // the needs that say missing
  notLookedFor: number; // under --no-app: the pieces of evidence that name a file of the application, not looked for
}

const KINDS = ['source', 'test', 'scenario', 'state', 'node', 'setting'] as const;
type Kind = (typeof KINDS)[number];
// what the graph draws: it shows where a need is, and alone meets nothing
const IN_GRAPH = new Set<Kind>(['state', 'node']);

const isMap = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v);
const text = (v: unknown): string | null => (typeof v === 'string' && v.trim() !== '' ? v : null);
const kindOf = (e: Record<string, unknown>): Kind | undefined => KINDS.find((k) => typeof e[k] === 'string');
const isMissing = (n: Record<string, unknown>): boolean => text(n.missing) !== null;

// the value of a graph file, or null when it cannot be read: the schema rule says why
function readDoc(file: string): { text: string; doc: Record<string, unknown> } | null {
  try {
    const raw = readFileSync(file, 'utf8');
    const doc = readYaml(raw);
    if (!isMap(doc)) return null;
    if (typeof doc.schema === 'string' && doc.schema !== FORMAT) return null;
    return { text: raw, doc };
  } catch {
    return null;
  }
}

// the needs.yaml beside a flow, read; null when there is none
export function readNeeds(dir: string): Needs | null {
  const file = join(dir, NEEDS_FILE);
  if (!existsSync(file)) return null;
  const doc = readYaml(readFileSync(file, 'utf8'));
  return isMap(doc) && Array.isArray(doc.needs) ? (doc as unknown as Needs) : null;
}

export interface Planned {
  states: Map<string, string[]>; // a state -> the missing needs drawn as it
  nodes: Map<string, string[]>; // a node of ui.yaml -> the missing needs drawn as it
}

// what a tool that walks the flow on the app leaves out (SPEC.md §13): the states and the nodes that missing needs are
// drawn as and that no met need names, each with the ids of those needs
export function planned(needs: Needs | null): Planned {
  const out: Planned = { states: new Map(), nodes: new Map() };
  const built = { states: new Set<string>(), nodes: new Set<string>() };
  for (const n of (needs?.needs ?? []) as unknown[]) {
    if (!isMap(n) || typeof n.id !== 'string') continue;
    for (const e of Array.isArray(n.evidence) ? n.evidence : []) {
      if (!isMap(e)) continue;
      for (const [key, into] of [['state', 'states'], ['node', 'nodes']] as const) {
        const name = e[key];
        if (typeof name !== 'string') continue;
        if (!isMissing(n)) built[into].add(name);
        else out[into].set(name, [...(out[into].get(name) ?? []), n.id]);
      }
    }
  }
  for (const into of ['states', 'nodes'] as const) for (const name of built[into]) out[into].delete(name);
  return out;
}

// the nearest gaps file at or above a folder, up to the repository's root (the first folder that holds .git)
function nearestGaps(dir: string): string | null {
  for (let d = dir; ; d = dirname(d)) {
    if (existsSync(join(d, GAPS_FILE))) return join(d, GAPS_FILE);
    if (existsSync(join(d, '.git')) || dirname(d) === d) return null;
  }
}

const escapeRe = (s: string): string => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
// the text holds the name, not as a part of a longer word: an identifier, or a test's name in quotes
const holds = (body: string, name: string): boolean => new RegExp(`(?<![\\w$])${escapeRe(name)}(?![\\w$])`).test(body);
// a line of the file sets the variable and is not commented out: NAME=, export NAME=, NAME: or - NAME=
const sets = (body: string, name: string): boolean => new RegExp(`^[ \\t]*(?:-[ \\t]*)?(?:export[ \\t]+)?${escapeRe(name)}[ \\t]*[=:]`, 'm').test(body);

interface Context {
  app: string;
  code: AppFiles; // the application's files, as a source: anchor reads them (SPEC.md §9)
  lookFor: boolean; // false under --no-app: the application's files are not here
  dir: string; // the flow's folder
  flowId: string | null;
  states: Set<string> | null; // null when flow.yaml cannot be read: its own check says why
  built: Map<string, string>; // the states of a captured flow that name their code, with their source: they are built
  nodes: Set<string> | null | 'none'; // 'none' when the flow has no ui.yaml
}

const fileAt = (ctx: Context, path: string): string | null => {
  const abs = isAbsolute(path) ? path : join(ctx.app, path);
  return existsSync(abs) && statSync(abs).isFile() ? abs : null;
};

// a binding's scenarios, those of the bindings it extends included (each named relative to the one that extends it)
function scenariosOf(file: string, seen = new Set<string>()): Set<string> {
  const out = new Set<string>();
  if (seen.has(file) || !existsSync(file)) return out;
  seen.add(file);
  const read = readDoc(file);
  if (read === null) return out;
  if (isMap(read.doc.scenarios)) for (const s of Object.keys(read.doc.scenarios)) out.add(s);
  if (typeof read.doc.extends === 'string') for (const s of scenariosOf(join(dirname(file), read.doc.extends), seen)) out.add(s);
  return out;
}

// whether a piece of evidence names a file of the application by its path from the repository's root: a source, a test,
// the file a setting is set in, a binding named by its path (SPEC.md §13)
const namesAppFile = (kind: Kind, e: Record<string, unknown>): boolean =>
  kind === 'source' || kind === 'test' || kind === 'setting' || (kind === 'scenario' && text(e.binding) !== null);

// why a piece of evidence does not resolve, or null when it does (or when what it names cannot be read, which is
// reported where it is)
function unresolved(ctx: Context, kind: Kind, e: Record<string, unknown>): string | null {
  const value = e[kind] as string;
  const name = text(e.name);
  switch (kind) {
    case 'source': {
      const why = ctx.code.unresolved(value);
      if (why !== null) return `source ${why.subject} ${why.reason}`;
      const path = (/^(.*?)(?::\d+)?$/.exec(value) as RegExpExecArray)[1];
      return name !== null && !holds(readFileSync(ctx.code.abs(path), 'utf8'), name) ? `source ${path} holds no ${name}` : null;
    }
    case 'test': {
      const abs = fileAt(ctx, value);
      if (abs === null) return `test ${value} does not exist`;
      return name !== null && !holds(readFileSync(abs, 'utf8'), name) ? `test ${value} holds no test named "${name}"` : null;
    }
    case 'scenario': {
      const given = text(e.binding);
      const abs = given === null ? join(ctx.dir, 'binding.yaml') : fileAt(ctx, given);
      const said = given === null ? "the flow's own binding.yaml" : `the binding ${given}`;
      if (abs === null || !existsSync(abs)) return `scenario ${value}: ${said} does not exist`;
      const read = readDoc(abs);
      if (read === null) return `scenario ${value}: ${said} cannot be read`;
      if (read.doc.kind !== 'binding') return `scenario ${value}: ${said} is not a binding (kind: binding)`;
      if (ctx.flowId !== null && read.doc.flow !== ctx.flowId) return `scenario ${value}: ${said} binds ${String(read.doc.flow ?? 'no flow')}, not ${ctx.flowId}`;
      const scenarios = scenariosOf(abs);
      return scenarios.has(value) ? null : `scenario ${value} is not a scenario of ${said}${scenarios.size > 0 ? ` (it has ${[...scenarios].sort().join(', ')})` : ''}`;
    }
    case 'state':
      return ctx.states === null || ctx.states.has(value) ? null : `state ${value} is not a state of ${ctx.flowId ?? 'the flow'}`;
    case 'node':
      if (ctx.nodes === 'none') return `node ${value}: the flow has no screens, as there is no ui.yaml beside it`;
      return ctx.nodes === null || ctx.nodes.has(value) ? null : `node ${value} is not a node of the flow's ui.yaml`;
    case 'setting': {
      const where = text(e.in);
      if (where === null) return null; // the schema says it is missing
      const abs = fileAt(ctx, where);
      if (abs === null) return `setting ${value}: ${where} does not exist`;
      return sets(readFileSync(abs, 'utf8'), value) ? null : `setting ${value} is not set in ${where}: no line sets it, or the one that does is commented out`;
    }
  }
}

// the states of a captured flow that name their code (SPEC.md §9), which are therefore built, each with its anchor
function builtStates(flow: Record<string, unknown> | null): Map<string, string> {
  const out = new Map<string, string>();
  if (flow?.status !== 'captured' || !isMap(flow.states)) return out;
  for (const [name, st] of Object.entries(flow.states)) if (isMap(st) && typeof st.source === 'string') out.set(name, st.source);
  return out;
}

// the nodes of a ui.yaml, or null when it cannot be read
function nodesOf(file: string): Set<string> | null {
  const read = readDoc(file);
  return read === null ? null : new Set(isMap(read.doc.nodes) ? Object.keys(read.doc.nodes) : []);
}

interface Listed {
  id: string;
  gaps: Map<string, string[]>; // a flow's id -> the ids of its needs listed
  text: string;
}

// a gaps file, read once per run; null when it cannot be read (the schema rule says why)
function gapsReader(): (file: string) => Listed | null {
  const seen = new Map<string, Listed | null>();
  return (file) => {
    if (!seen.has(file)) {
      const read = readDoc(file);
      const gaps = new Map<string, string[]>();
      if (read !== null && isMap(read.doc.gaps)) {
        for (const [flow, ids] of Object.entries(read.doc.gaps)) gaps.set(flow, Array.isArray(ids) ? ids.filter((x): x is string => typeof x === 'string') : []);
      }
      seen.set(file, read === null ? null : { id: typeof read.doc.id === 'string' ? read.doc.id : basename(dirname(file)), gaps, text: read.text });
    }
    return seen.get(file) ?? null;
  };
}

type State = 'met' | 'missing' | 'unproven';

// every needs file and gaps file among `files` (the graph files under the folders checked), with the application's
// repository root that evidence paths are relative to; with `app` null (--no-app), evidence that names a file of the
// application is counted and not looked for, and the rest is held as before
export function checkNeeds(files: string[], app: string | null): NeedsCheck {
  const out: NeedsCheck = { problems: [], files: 0, needs: 0, gaps: 0, notLookedFor: 0 };
  const gapsOf = gapsReader();
  const code = new AppFiles(app ?? '.');
  // each needs file's flow and the state of each of its needs, by the gaps file that covers it; null for a needs file
  // that cannot be read, whose entries are then not judged
  const covered = new Map<string, Map<string, Map<string, State> | null>>();
  const cover = (gapsFile: string | null, flow: string, states: Map<string, State> | null): void => {
    if (gapsFile === null) return;
    const byFlow = covered.get(gapsFile) ?? new Map<string, Map<string, State> | null>();
    byFlow.set(flow, states);
    covered.set(gapsFile, byFlow);
  };
  const problem = (rule: NeedRule, file: string, body: string, path: string[], message: string, graph: string, element: string): void => {
    out.problems.push({ rule, file, ...keyAt(body, path), message, graph, element });
  };

  for (const file of files.filter((f) => basename(f) === NEEDS_FILE)) {
    const read = readDoc(file);
    if (read === null || !Array.isArray(read.doc.needs)) {
      const beside = readDoc(join(dirname(file), 'flow.yaml'))?.doc.id;
      if (typeof beside === 'string') cover(nearestGaps(dirname(file)), beside, null);
      continue;
    }
    out.files++;
    const { doc, text: body } = read;
    const dir = dirname(file);
    const flowFile = join(dir, 'flow.yaml');
    const flow = existsSync(flowFile) ? readDoc(flowFile) : null;
    const flowId = typeof flow?.doc.id === 'string' ? flow.doc.id : null;
    const ownId = typeof doc.id === 'string' ? doc.id : null;
    const graph = flowId ?? ownId ?? basename(dir);
    if (!existsSync(flowFile)) {
      problem('needs-place', file, body, ['id'], `needs.yaml is beside no flow: it sits in a flow's folder, beside its flow.yaml, and lists that flow's needs (SPEC.md §13)`, graph, 'needs');
    } else if (flowId !== null && ownId !== null && ownId !== flowId) {
      problem('needs-place', file, body, ['id'], `needs.yaml is the needs of ${ownId}, and the flow beside it is ${flowId}: its id is the id of the flow beside it (SPEC.md §13)`, graph, 'needs/id');
    }
    const uiFile = join(dir, 'ui.yaml');
    const ctx: Context = {
      app: app ?? '.',
      code,
      lookFor: app !== null,
      dir,
      flowId,
      states: flow === null ? null : new Set(isMap(flow.doc.states) ? Object.keys(flow.doc.states) : []),
      built: builtStates(flow?.doc ?? null),
      nodes: !existsSync(flowFile) ? null : existsSync(uiFile) ? nodesOf(uiFile) : 'none',
    };
    const key = flowId ?? ownId ?? graph; // the flow, as the gaps name it
    const gapsFile = nearestGaps(dir);
    const listed = gapsFile === null ? null : gapsOf(gapsFile);
    const states = new Map<string, State>();
    const firstAt = new Map<string, number>();

    (read.doc.needs as unknown[]).forEach((item, i) => {
      if (!isMap(item) || typeof item.id !== 'string') return;
      const id = item.id;
      out.needs++;
      const at = ['needs', String(i)];
      const first = firstAt.get(id);
      if (first !== undefined) {
        problem('need-twice', file, body, [...at, 'id'], `need ${id} is given twice: it is given first at line ${keyAt(body, ['needs', String(first), 'id']).line}, and a need's id comes once in its flow (SPEC.md §13)`, graph, `needs/${id}/twice`);
      } else firstAt.set(id, i);
      const evidence = Array.isArray(item.evidence) ? item.evidence : [];
      let outside = false;
      let kinds = 0;
      evidence.forEach((e, j) => {
        if (!isMap(e)) return;
        const kind = kindOf(e);
        if (kind === undefined) return; // the schema says it names no kind
        kinds++;
        if (!IN_GRAPH.has(kind)) outside = true;
        if (!ctx.lookFor && namesAppFile(kind, e)) {
          out.notLookedFor++;
          return;
        }
        const why = unresolved(ctx, kind, e);
        if (why !== null) problem('evidence-unresolved', file, body, [...at, 'evidence', String(j), kind], `need ${id}: ${why} (SPEC.md §13)`, graph, `needs/${id}/evidence/${kind}:${String(e[kind])}`);
        // a missing need names only what is not built: a walk leaves out a state that only missing needs name
        const built = kind === 'state' && isMissing(item) ? ctx.built.get(e.state as string) : undefined;
        if (built !== undefined) {
          problem('need-names-built', file, body, [...at, 'evidence', String(j), kind], `need ${id} is missing, and names state ${String(e.state)}, which is built: it names its code (source: ${built}). A walk on the app leaves out a state that only missing needs name, so a missing need names only what is not built yet; say in its note that it is about ${String(e.state)} (SPEC.md §13)`, graph, `needs/${id}/built/state:${String(e.state)}`);
        }
      });
      const missing = isMissing(item);
      const state: State = missing ? 'missing' : outside ? 'met' : 'unproven';
      if (first === undefined) states.set(id, state);
      if (state === 'unproven' && kinds > 0 && kinds === evidence.length) {
        problem('need-no-evidence', file, body, [...at, 'evidence'], `need ${id} is not missing and names no evidence outside the graph: what it is drawn as shows nothing built; name a source, a test, a scenario or a setting, or say why it is missing (SPEC.md §13)`, graph, `needs/${id}`);
      }
      if (!missing) return;
      out.gaps++;
      const why = text(item.missing) as string;
      if (listed === null) {
        problem('need-unlisted', file, body, [...at, 'missing'], `need ${id} is missing, and there is no ${GAPS_FILE} above it: list it in the product's gaps, at the root of its graphs (SPEC.md §13)`, graph, `needs/${id}`);
      } else if (!(listed.gaps.get(key) ?? []).includes(id)) {
        problem('need-unlisted', file, body, [...at, 'missing'], `need ${id} is missing, and ${GAPS_FILE} does not list it under ${key}: list it there, or name its evidence and take out missing (SPEC.md §13)`, graph, `needs/${id}`);
      } else {
        problem('need-gap', file, body, [...at, 'missing'], `need ${id} is missing: ${why}`, graph, `needs/${id}`);
      }
    });

    cover(gapsFile, key, states);
  }

  // each gaps file among the files checked, held to the needs under its folder that it covers
  for (const file of files.filter((f) => basename(f) === GAPS_FILE)) {
    const listed = gapsOf(file);
    if (listed === null) continue;
    const byFlow = covered.get(file) ?? new Map<string, Map<string, State> | null>();
    for (const [flow, ids] of listed.gaps) {
      const states = byFlow.get(flow);
      if (states === null) continue;
      if (states === undefined) {
        problem('gap-unknown', file, listed.text, ['gaps', flow], `${GAPS_FILE} lists gaps of ${flow}, which is no flow with a needs.yaml under its folder: take them out, or name the flow by its id (SPEC.md §13)`, listed.id, `gaps/${flow}`);
        continue;
      }
      ids.forEach((id, k) => {
        const state = states.get(id);
        const at = ['gaps', flow, String(k)];
        if (state === undefined) problem('gap-unknown', file, listed.text, at, `${GAPS_FILE} lists ${id} of ${flow}, which is no need of its needs.yaml: take it out, or name the need by its id (SPEC.md §13)`, listed.id, `gaps/${flow}/${id}`);
        else if (state === 'met') problem('gap-met', file, listed.text, at, `${GAPS_FILE} lists ${id} of ${flow}, which is met now: take it out, as the list only shrinks (SPEC.md §13)`, listed.id, `gaps/${flow}/${id}`);
      });
    }
  }
  return out;
}
