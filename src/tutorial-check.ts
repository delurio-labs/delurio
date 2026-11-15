// A flow's tutorial.yaml held to the flow and its ui (SPEC.md §7). First every name a tutorial uses: the flow it is
// written against, the variables and values of its `scenario` and of each step's `then`, the states of `show`, `cut`
// and `end`, the nodes of `press`, `reveal` and `focus`, the events of `after`, and each `{word}` its words fill. Then,
// when every name is one, its steps are replayed on the flow from the scenario as the video plays them, and the first
// step that no longer holds is where it stops: a tutorial that no longer matches the graph stops at that step.
import { kindOf } from './edges.ts';
import { shownIn } from './graph.ts';
import type { Graph, Scalar } from './graph.ts';
import type { Exploration, Model, Step } from './model.ts';
import { readYaml } from './yaml.ts';

export interface TutorialProblem {
  path: string[]; // where in tutorial.yaml
  line?: number; // where the YAML parser gave it, for a file that cannot be read
  column?: number;
  message: string;
}

interface StepSpec {
  intro?: unknown;
  chapter?: unknown;
  show?: unknown;
  cut?: unknown;
  end?: unknown;
  reveal?: unknown;
  focus?: unknown;
  press?: unknown;
  after?: unknown;
  then?: unknown;
  card?: unknown;
  say?: unknown;
}

interface Tutorial {
  kind?: unknown;
  flow?: unknown;
  stage?: unknown;
  scenario?: unknown;
  samples?: unknown;
  title?: unknown;
  steps?: unknown;
}

interface Cursor {
  state: string;
  ctx: Scalar[];
}

const isMap = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v);
const names = (v: unknown): unknown[] => (Array.isArray(v) ? v : v === undefined ? [] : [v]);
const shown = (v: Scalar): string => JSON.stringify(v);

class Stop extends Error {
  readonly path: string[];
  constructor(path: string[], message: string) {
    super(message);
    this.path = path;
  }
}

export function checkTutorial(g: Graph, m: Model, ex: Exploration, text: string): TutorialProblem[] {
  let t: Tutorial;
  try {
    t = readYaml(text) as Tutorial;
  } catch (err) {
    const at = (err as { linePos?: Array<{ line: number; col: number }> }).linePos?.[0];
    return [{ path: [], line: at?.line ?? 1, column: at?.col ?? 1, message: `tutorial.yaml cannot be read: ${(err as Error).message.split('\n')[0]}` }];
  }
  if (!isMap(t) || t.kind !== 'tutorial') return [{ path: isMap(t) ? ['kind'] : [], message: 'tutorial.yaml is not a tutorial: it says kind: tutorial (SPEC.md §1, §7)' }];
  if (t.flow !== g.flow.id) return [{ path: ['flow'], message: `the tutorial is written against ${String(t.flow)}, and the flow beside it is ${g.flow.id} (SPEC.md §7)` }];
  const stage = t.stage ?? 'screen';
  if (stage !== 'screen' && stage !== 'diagram') return [{ path: ['stage'], message: `stage is screen or diagram, not ${String(stage)} (SPEC.md §7)` }];
  const steps = Array.isArray(t.steps) ? (t.steps as unknown[]) : [];

  const out: TutorialProblem[] = [];
  const flow = g.flow;
  const nodes = g.ui.nodes;
  const events = new Set(Object.values(flow.states).flatMap((st) => Object.keys(st.on ?? {})));
  const samples = new Set([...Object.keys(g.ui.samples), ...(isMap(t.samples) ? Object.keys(t.samples) : [])]);
  const problem = (path: string[], message: string): void => {
    out.push({ path, message });
  };
  const values = (where: string[], given: unknown, what: string): void => {
    if (given === undefined) return;
    if (!isMap(given)) return problem(where, `${what} is a map of variables to values`);
    for (const [name, v] of Object.entries(given)) {
      const cv = flow.context[name];
      if (cv === undefined) problem([...where, name], `${what} names ${name}, which is not a variable of ${flow.id}`);
      else if (!cv.values.includes(v as Scalar)) problem([...where, name], `${what}: ${shown(v as Scalar)} is not a value of ${name} (${cv.values.map(shown).join(', ')})`);
    }
  };
  // the start: a value of the scenario's for each variable that starts from any value, and the fixed one of the others
  values(['scenario'], t.scenario, 'scenario');
  const scenario = isMap(t.scenario) ? t.scenario : {};
  for (const v of m.vars) {
    const cv = flow.context[v];
    const given = scenario[v] as Scalar | undefined;
    if (cv.initial === 'any' && given === undefined) problem(['scenario'], `scenario gives no value for ${v}, and the flow starts from any value of it`);
    else if (cv.initial !== 'any' && given !== undefined && given !== cv.initial) problem(['scenario', v], `scenario gives ${v} = ${shown(given)}, and ${flow.id} always starts with ${v} = ${shown(cv.initial)}`);
  }
  const words = (where: string[], s: unknown, step: string): void => {
    if (typeof s !== 'string') return;
    for (const [, name] of s.matchAll(/\{(\w+)\}/g)) {
      if (samples.has(name)) continue;
      const n = nodes[name];
      if (n === undefined) problem(where, `${step}{${name}} is neither a sample value nor a node of ui.yaml`);
      else if (n.text === undefined) problem(where, `${step}{${name}} names the node ${name}, which has no visible text to say`);
    }
  };
  words(['title'], t.title, 'title: ');
  steps.forEach((s0, i) => {
    if (!isMap(s0)) return;
    const s = s0 as StepSpec;
    const at = (...rest: string[]): string[] => ['steps', String(i), ...rest];
    const step = `step ${i + 1}`;
    for (const key of ['show', 'cut', 'end'] as const) {
      const v = s[key];
      if (typeof v === 'string' && flow.states[v] === undefined) problem(at(key), `${step}: ${key} ${v}, which is not a state of ${flow.id}`);
    }
    for (const key of ['press', 'reveal', 'focus'] as const) {
      names(s[key]).forEach((id, j) => {
        const where = Array.isArray(s[key]) ? at(key, String(j)) : at(key);
        if (stage === 'diagram') problem(where, `${step}: ${key} points at a node, and a tutorial on the diagram has none (SPEC.md §7)`);
        else if (typeof id !== 'string' || nodes[id] === undefined) problem(where, `${step}: ${key} ${String(id)}, which is not a node of ui.yaml`);
      });
    }
    names(s.after).forEach((e, j) => {
      if (typeof e !== 'string' || !events.has(e)) problem(Array.isArray(s.after) ? at('after', String(j)) : at('after'), `${step}: after ${String(e)}, which no state of ${flow.id} handles`);
    });
    values(at('then'), s.then, `${step}: then`);
    words(at('say'), s.say, `${step}: `);
    words(at('chapter'), s.chapter, `${step}: `);
    if (isMap(s.card)) {
      words(at('card', 'title'), s.card.title, `${step}: `);
      words(at('card', 'body'), s.card.body, `${step}: `);
    }
  });
  if (out.length > 0) return out;

  try {
    if (stage === 'diagram') replayDiagram(g, m, ex, steps, scenario);
    else replayScreens(g, m, ex, steps, scenario);
  } catch (err) {
    if (!(err instanceof Stop)) throw err;
    return [{ path: err.path, message: err.message }];
  }
  return [];
}

function start(g: Graph, m: Model, scenario: Record<string, unknown>): Cursor {
  return { state: g.flow.initial, ctx: m.vars.map((v) => (g.flow.context[v].initial === 'any' ? (scenario[v] as Scalar) : (g.flow.context[v].initial as Scalar))) };
}

// the step's `then`: of the cases its transitions can take, the one with these values; without it, the first
function thenOf(m: Model, s: StepSpec): Array<[number, Scalar]> {
  return isMap(s.then) ? Object.entries(s.then).map(([name, v]): [number, Scalar] => [m.vars.indexOf(name), v as Scalar]) : [];
}
const pick = (cases: Step[], then: Array<[number, Scalar]>): Step | undefined => cases.find((c) => then.every(([i, v]) => c.ctx[i] === v)) ?? cases[0];

function thenHolds(m: Model, s: StepSpec, cur: Cursor, path: string[], step: string): void {
  for (const [i, v] of thenOf(m, s)) {
    if (cur.ctx[i] !== v) throw new Stop([...path, 'then', m.vars[i]], `${step}: no case of this step has ${m.vars[i]} = ${shown(v)}`);
  }
}

function replayScreens(g: Graph, m: Model, ex: Exploration, steps: unknown[], scenario: Record<string, unknown>): void {
  const flow = g.flow;
  const nodes = g.ui.nodes;
  const hasScreen = (state: string): boolean => Object.values(nodes).some((n) => shownIn(n, state));
  let then: Array<[number, Scalar]> = [];
  const take = (cur: Cursor, event: string, path: string[], step: string): Cursor => {
    const hit = pick(m.steps(cur.state, cur.ctx).filter((x) => x.event === event), then);
    if (hit === undefined) throw new Stop(path, `${step}: the flow is in ${cur.state}, which does not handle ${event}`);
    return settle({ state: hit.target, ctx: hit.ctx }, path, step);
  };
  // a step with no screen passes on its own along its normal way (a transition with no event, then DONE); one with no
  // such way waits for an event the tutorial gives with `after`
  const settle = (cur: Cursor, path: string[], step: string): Cursor => {
    let c = cur;
    for (let hops = 0; hops < 50 && kindOf(g, c.state) === 'server'; hops++) {
      const ways = m.steps(c.state, c.ctx);
      const next = pick(ways.filter((x) => x.event === null || x.event === 'DONE'), then);
      if (next === undefined && ways.length > 0) return c;
      if (next === undefined) throw new Stop(path, `${step}: the flow stops in ${c.state}, which has no screen and no way on`);
      c = { state: next.target, ctx: next.ctx };
    }
    return c;
  };
  let cur = settle(start(g, m, scenario), ['scenario'], 'the start');
  steps.forEach((s0, i) => {
    if (!isMap(s0)) return;
    const s = s0 as StepSpec;
    const path = ['steps', String(i)];
    const step = `step ${i + 1}`;
    then = thenOf(m, s);
    if (s.intro === true) {
      names(s.after).forEach((e, j) => {
        cur = take(cur, e as string, Array.isArray(s.after) ? [...path, 'after', String(j)] : [...path, 'after'], step);
      });
      thenHolds(m, s, cur, path, step);
      return;
    }
    const reveal = [...names(s.reveal)] as string[];
    if (typeof s.press === 'string' && nodes[s.press].when !== undefined && !reveal.includes(s.press)) reveal.push(s.press);
    let state: string | null = cur.state; // the state whose screen the step shows; null for a card
    if (typeof s.cut === 'string') {
      const wanted = s.cut;
      let hit;
      try {
        hit = ex.configs.find((c) => c.state === wanted && reveal.every((id) => nodes[id].when === undefined || m.test(nodes[id].when as string, c.state, c.ctx)));
      } catch (err) {
        throw new Stop([...path, 'reveal'], `${step}: ${(err as Error).message}`);
      }
      if (hit === undefined) throw new Stop([...path, 'cut'], `${step}: no situation the flow reaches is in ${wanted}${reveal.length > 0 ? ` and shows ${reveal.join(', ')}` : ''}`);
      cur = { state: hit.state, ctx: hit.ctx };
      state = cur.state;
    } else if (s.end !== undefined) {
      if (typeof s.end === 'string' && s.end !== cur.state) throw new Stop([...path, 'end'], `${step}: the tutorial ends in ${s.end}, and the flow is in ${cur.state} here`);
      if (flow.states[cur.state].type === 'final') state = null;
      else if (typeof s.end !== 'string') throw new Stop([...path, 'end'], `${step}: the flow is in ${cur.state} here, which is no exit`);
      if (isMap(s.card)) state = null;
    } else if (typeof s.show === 'string') {
      if (s.show !== cur.state) throw new Stop([...path, 'show'], `${step}: shows ${s.show}, and the flow is in ${cur.state} here`);
    } else if (s.card !== undefined) {
      state = null;
    }
    if (state !== null && !hasScreen(state)) throw new Stop([...path, typeof s.cut === 'string' ? 'cut' : typeof s.show === 'string' ? 'show' : 'end'], `${step}: ${state} has no screen to show`);
    for (const key of ['reveal', 'focus'] as const) {
      names(s[key]).forEach((id, j) => {
        const where = Array.isArray(s[key]) ? [...path, key, String(j)] : [...path, key];
        const n = nodes[id as string];
        if (state === null) throw new Stop(where, `${step}: ${key} ${String(id)} on a card, which shows no node`);
        if (!shownIn(n, state)) throw new Stop(where, `${step}: ${key} ${String(id)}, which ${state} does not show`);
        if (key === 'focus' && n.when !== undefined && !reveal.includes(id as string)) throw new Stop(where, `${step}: ${String(id)} is shown only when ${n.when}: list it under reveal`);
      });
    }
    if (typeof s.press === 'string') {
      const n = nodes[s.press];
      const where = [...path, 'press'];
      if (!shownIn(n, cur.state)) throw new Stop(where, `${step}: press ${s.press}, which ${cur.state} does not show`);
      if ((n.disabledIn ?? []).includes(cur.state)) throw new Stop(where, `${step}: press ${s.press}, which is disabled in ${cur.state}`);
      if (n.emits === undefined) throw new Stop(where, `${step}: press ${s.press}, which sends no event`);
      cur = take(cur, n.emits, where, step);
    }
    names(s.after).forEach((e, j) => {
      cur = take(cur, e as string, Array.isArray(s.after) ? [...path, 'after', String(j)] : [...path, 'after'], step);
    });
    thenHolds(m, s, cur, path, step);
  });
}

// On the diagram no state passes on its own: `show` and `end` walk the flow to their state along its normal way (a
// transition with no event, then DONE), and every other event is given with `after`
function replayDiagram(g: Graph, m: Model, ex: Exploration, steps: unknown[], scenario: Record<string, unknown>): void {
  let cur = start(g, m, scenario);
  let then: Array<[number, Scalar]> = [];
  const walkTo = (wanted: string, path: string[], step: string): void => {
    const from = cur.state;
    for (let hops = 0; cur.state !== wanted; hops++) {
      const next = hops < 50 ? pick(m.steps(cur.state, cur.ctx).filter((x) => x.event === null || x.event === 'DONE'), then) : undefined;
      if (next === undefined) throw new Stop(path, `${step}: ${wanted} is not reached from ${from} without an event${cur.state === from ? '' : ` (the way stops in ${cur.state})`}`);
      cur = { state: next.target, ctx: next.ctx };
    }
  };
  steps.forEach((s0, i) => {
    if (!isMap(s0)) return;
    const s = s0 as StepSpec;
    const path = ['steps', String(i)];
    const step = `step ${i + 1}`;
    then = thenOf(m, s);
    if (s.intro === true) return;
    if (typeof s.cut === 'string') {
      const wanted = s.cut;
      const hit = ex.configs.find((c) => c.state === wanted);
      if (hit === undefined) throw new Stop([...path, 'cut'], `${step}: no situation the flow reaches is in ${wanted}`);
      cur = { state: hit.state, ctx: hit.ctx };
    } else if (typeof s.end === 'string') {
      walkTo(s.end, [...path, 'end'], step);
      if (g.flow.states[s.end].type !== 'final') throw new Stop([...path, 'end'], `${step}: ends in ${s.end}, which is no exit`);
    } else if (typeof s.show === 'string') {
      walkTo(s.show, [...path, 'show'], step);
    }
    names(s.after).forEach((e, j) => {
      const where = Array.isArray(s.after) ? [...path, 'after', String(j)] : [...path, 'after'];
      const hit = pick(m.steps(cur.state, cur.ctx).filter((x) => x.event === e), then);
      if (hit === undefined) throw new Stop(where, `${step}: the flow is in ${cur.state}, which does not handle ${String(e)}`);
      cur = { state: hit.target, ctx: hit.ctx };
    });
    thenHolds(m, s, cur, path, step);
  });
}
