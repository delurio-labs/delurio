import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'yaml';
import { identifiers, parseExpr } from './expr.ts';

export type Scalar = string | number | boolean;

export interface Transition {
  target: string;
  guard?: string;
  effect?: string[];
  say?: string; // the transition in plain words, for people
  starts?: Starts;
}
// another flow this transition starts and does not wait for; `given` is what it begins with, each value an expression
// over this flow's context, read where the transition is taken
export interface Starts {
  flow: string;
  given?: Record<string, string | number | boolean>;
}
export type TransitionSpec = string | Transition | Array<string | Transition>;

export interface State {
  title?: string; // the state in plain words; the id is used when missing
  description?: string;
  type?: string;
  invoke?: { action?: string; flow?: string; external?: string };
  on?: Record<string, TransitionSpec>;
  always?: Transition[];
  redirect?: string;
  code?: string;
  carries?: string[];
  leavesPage?: boolean;
  by?: 'human' | 'ai' | 'agent' | 'ci'; // who does the step: drives colour and shape
  goal?: boolean; // a state the flow is for, drawn green; for flows that never end
  includes?: string[]; // what the step covers beyond its title
  response?: Record<string, unknown>; // what the outside world sees from a step with no screen: status, code, redirect, headers, body
  states?: unknown;
}

export interface ContextVar {
  values: Scalar[];
  initial: 'any' | Scalar;
  say?: Record<string, string>; // value -> plain words, used to describe a starting scenario
}

export interface Effect {
  set?: Record<string, Scalar>;
  havoc?: string[];
}

export interface Decision {
  question: string;
  recommend: string;
  affects: string[];
}

export interface Flow {
  id: string;
  title?: string;
  status?: string;
  summary?: string;
  decisions?: Record<string, Decision>;
  context: Record<string, ContextVar>;
  guards: Record<string, string>;
  effects: Record<string, Effect>;
  initial: string;
  states: Record<string, State>;
  assertions?: Array<{ id: string; never: string; say?: string }>;
  main?: string[] | { given?: Record<string, Scalar>; events: string[] }; // the main story as events, when no success exit gives one
  scope?: { shows?: string; world?: 'open' | 'closed'; outOfView?: string[] };
  domain?: string; // the id of the domain.yaml its actions are in
}

export interface Label {
  key?: string;
  params?: Record<string, string>;
  labelledBy?: string;
  literal?: string;
  oneOf?: string[];
}

export interface UiNode {
  type: string;
  component?: string;
  role?: string;
  intent?: string;
  name?: Label;
  text?: Label;
  in?: string[];
  unless?: string[];
  when?: string;
  disabledIn?: string[];
  emits?: string;
  initialFocus?: string;
  frame?: string;
}

export interface Ui {
  absent?: boolean; // the flow has no screens and no ui.yaml
  id: string;
  catalog: string;
  kit?: string; // the kit the screens are drawn with, relative to the graph; --kit wins
  samples: Record<string, string>;
  nodes: Record<string, UiNode>;
  locales?: string[]; // each has a catalog beside `catalog`, named <locale>.json
  changes?: { key: string }; // what the flow changes and what follows, in its users' words (SPEC.md §4)
}

export interface Graph {
  dir: string;
  flow: Flow;
  ui: Ui;
  catalog: Record<string, string>;
}

export class GraphError extends Error {}

// 'silentRedirect' -> 'silent redirect'; used only when the graph gives no title
export function humanize(id: string): string {
  const words = id.replace(/([a-z0-9])([A-Z])/g, '$1 $2').replace(/[_.-]+/g, ' ').toLowerCase();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function titleOf(flow: Flow, state: string): string {
  return flow.states[state].title ?? humanize(state);
}

export function alternatives(spec: TransitionSpec): Transition[] {
  const list = Array.isArray(spec) ? spec : [spec];
  return list.map((a) => (typeof a === 'string' ? { target: a } : a));
}

// `read` and `exists` take a path relative to `dir`; the revision diff passes ones that read a git revision instead of the disk
export function load(
  dir: string,
  read = (file: string): string => readFileSync(join(dir, file), 'utf8'),
  exists = (file: string): boolean => existsSync(join(dir, file)),
): Graph {
  const flow = parse(read('flow.yaml')) as Flow;
  // a flow with no context, guards or effects leaves the key out; every reader expects an object
  flow.context ??= {};
  flow.guards ??= {};
  flow.effects ??= {};
  // a flow with no screens (an endpoint, a webhook, a CI job) has no ui.yaml and no catalog
  const ui: Ui = exists('ui.yaml') ? (parse(read('ui.yaml')) as Ui) : { absent: true, id: flow.id, catalog: '', samples: {}, nodes: {} };
  const catalog = ui.absent === true ? {} : (JSON.parse(read(ui.catalog)) as Record<string, string>);

  if (flow.states[flow.initial] === undefined) throw new GraphError(`initial state ${flow.initial} does not exist`);
  for (const [name, st] of Object.entries(flow.states)) {
    if (st.states !== undefined || st.type === 'parallel' || st.type === 'history') {
      throw new GraphError(`state ${name}: nested, parallel and history states are not supported by graph-viz yet`);
    }
    const targets = [
      ...(st.always ?? []).map((a) => a.target),
      ...Object.values(st.on ?? {}).flatMap((spec) => alternatives(spec).map((a) => a.target)),
    ];
    for (const t of targets) {
      if (flow.states[t] === undefined) throw new GraphError(`state ${name} goes to ${t}, which does not exist`);
    }
    const ways = [...(st.always ?? []), ...Object.values(st.on ?? {}).flatMap((spec) => alternatives(spec))];
    for (const w of ways) {
      if (w.starts === undefined) continue;
      if (typeof w.starts.flow !== 'string') throw new GraphError(`state ${name}: \`starts\` names no flow`);
      for (const [k, v] of Object.entries(w.starts.given ?? {})) {
        if (typeof v !== 'string') continue;
        for (const id of identifiers(parseExpr(v))) {
          if (id !== 'state' && flow.context[id] === undefined && flow.guards[id] === undefined) {
            throw new GraphError(`state ${name} starts ${w.starts.flow} with ${k} = ${v}: ${id} is not a variable or a guard of this flow (a value is quoted: '${id}')`);
          }
        }
      }
    }
  }
  for (const [id, node] of Object.entries(ui.nodes)) {
    for (const s of [...(node.in ?? []), ...(node.unless ?? []), ...(node.disabledIn ?? [])]) {
      if (flow.states[s] === undefined) throw new GraphError(`ui node ${id} names the unknown state ${s}`);
    }
  }
  return { dir, flow, ui, catalog };
}
