import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { readYaml } from './yaml.ts';
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
  source?: string; // the code that makes the state true (SPEC.md §9)
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
  domain?: string; // the id of the domain its actions are in
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
  disabledWhen?: string; // an expression over the context: where it holds, the node is shown but disabled (SPEC.md §4)
  emits?: string;
  initialFocus?: string;
  repeat?: string; // the node is drawn once per item of this data
  frame?: string;
  value?: Label; // on a field: what it holds when shown; `{ literal: "" }` is an empty field (SPEC.md §4)
  checked?: boolean | string; // on a checkable role (CHECKABLE): an expression over the context, or true / false (SPEC.md §4)
  script?: 'optional' | 'actions'; // on a page: it and its actions work without JavaScript, or it is read without and only its actions need it (SPEC.md §4)
  source?: string;
}

// a relation between nodes (SPEC.md §4): a node and the node that labels it, an action and the state it leads to, or
// a dialog and the node that has focus when it closes: one node, the nodes that open it, or OPENER
export interface UiEdge {
  labelledBy?: [string, string];
  triggers?: [string, string];
  returnsFocus?: [string, string | string[]];
}

// in a returnsFocus, the node that was pressed to open the dialog, whichever it is (SPEC.md §4); no node is called so
export const OPENER = 'opener';

// the roles whose node may say whether it is `checked` (WAI-ARIA 1.2: the roles that support aria-checked)
export const CHECKABLE = ['checkbox', 'menuitemcheckbox', 'menuitemradio', 'option', 'radio', 'switch'] as const;

// a page node drawn over another page, whose closing `returnsFocus` may give the focus back (SPEC.md §4)
export const isDialog = (n: UiNode | undefined): boolean => n?.type === 'page' && (n.role === 'dialog' || n.role === 'alertdialog');

export interface Ui {
  absent?: boolean; // the flow has no screens and no ui.yaml
  id: string;
  catalog: string;
  kit?: string; // the kit the screens are drawn with, relative to the graph; --kit wins
  samples: Record<string, string>;
  nodes: Record<string, UiNode>;
  locales?: string[]; // each has a catalog beside `catalog`, named <locale>.json
  changes?: { key: string }; // what the flow changes and what follows, in its users' words (SPEC.md §4)
  focus?: Record<string, string>; // state -> the node that has focus when the flow enters it from another state (SPEC.md §4)
  edges?: UiEdge[];
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

// a node is shown in a state when the state is in its `in` and not in its `unless`; `when` is read over the context
export function shownIn(n: UiNode, state: string): boolean {
  return (n.in ?? []).includes(state) && !(n.unless ?? []).includes(state);
}

export interface FocusAfter {
  node: string;
  by: 'returnsFocus' | 'initialFocus' | 'focus'; // which part of ui.yaml gives it
  from?: string; // the dialog that closed, or the page that opened
}

// The node that has focus once the flow has moved from the state `from` (null when it starts) into another state `to`,
// both states the flow stops in (SPEC.md §4, Focus): a dialog that closes gives it back to its `returnsFocus` node when
// `to` shows that node; else a page that opens gives it to its `initialFocus`, a dialog's before the page it covers;
// else `focus.<to>`. null when the graph does not say, and on a move from a state to itself. `pressed` is the node that
// was pressed to open the dialog, which a replay knows and the graph does not: a returnsFocus that lists several
// nodes, or says OPENER, gives the focus to it. Without it, a list gives the one node of it that `to` shows, and
// OPENER, or a list of which `to` shows several, gives null.
export function focusAfter(ui: Ui, from: string | null, to: string, pressed?: string): FocusAfter | null {
  if (from === to) return null;
  const nodes = ui.nodes;
  const shown = (id: string, state: string | null): boolean => state !== null && nodes[id] !== undefined && shownIn(nodes[id], state);
  for (const e of ui.edges ?? []) {
    const [dialog, target] = e.returnsFocus ?? [];
    if (dialog === undefined || target === undefined || !isDialog(nodes[dialog])) continue;
    if (!shown(dialog, from) || shown(dialog, to)) continue;
    if (typeof target === 'string' && target !== OPENER) {
      if (shown(target, to)) return { node: target, by: 'returnsFocus', from: dialog };
      continue;
    }
    // the node that opened the dialog: the one pressed, when the caller knows it, among those a list allows
    const allowed = (id: string): boolean => target === OPENER || (Array.isArray(target) && target.includes(id));
    if (pressed !== undefined) {
      if (allowed(pressed) && shown(pressed, to)) return { node: pressed, by: 'returnsFocus', from: dialog };
      continue;
    }
    const candidates = Array.isArray(target) ? target.filter((id) => shown(id, to)) : [];
    if (candidates.length === 1) return { node: candidates[0], by: 'returnsFocus', from: dialog };
    return null;
  }
  const opened = Object.entries(nodes).filter(([id, n]) => n.type === 'page' && n.initialFocus !== undefined && shown(id, to) && !shown(id, from));
  const page = opened.find(([, n]) => isDialog(n)) ?? opened[0];
  if (page !== undefined) return { node: page[1].initialFocus as string, by: 'initialFocus', from: page[0] };
  const node = from === null ? undefined : ui.focus?.[to];
  return node === undefined ? null : { node, by: 'focus' };
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
  const flow = readYaml(read('flow.yaml')) as Flow;
  // a flow with no context, guards or effects leaves the key out; every reader expects an object
  flow.context ??= {};
  flow.guards ??= {};
  flow.effects ??= {};
  // a flow with no screens (an endpoint, a webhook, a CI job) has no ui.yaml and no catalog
  const ui: Ui = exists('ui.yaml') ? (readYaml(read('ui.yaml')) as Ui) : { absent: true, id: flow.id, catalog: '', samples: {}, nodes: {} };
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
    // `checked` is an expression over the context (SPEC.md §3), read when the node is shown
    if (typeof node.checked === 'string') {
      let names: Set<string>;
      try {
        names = identifiers(parseExpr(node.checked));
      } catch (err) {
        throw new GraphError(`ui node ${id}: checked: ${(err as Error).message}`);
      }
      for (const n of names) {
        if (n !== 'state' && flow.context[n] === undefined && flow.guards[n] === undefined) {
          throw new GraphError(`ui node ${id}: checked: ${node.checked} names ${n}, which is not a variable or a guard of the flow`);
        }
      }
    }
  }
  for (const s of Object.keys(ui.focus ?? {})) {
    if (flow.states[s] === undefined) throw new GraphError(`ui focus names the unknown state ${s}`);
  }
  disabledWhenReads(flow, ui);
  return { dir, flow, ui, catalog };
}

// a node's `disabledWhen` is an expression over the context (SPEC.md §3, §4), read where the node is shown: one that
// does not parse, or names what is not a variable, a guard or `state`, keeps the graph from loading, as `checked` does
function disabledWhenReads(flow: Flow, ui: Ui): void {
  for (const [id, node] of Object.entries(ui.nodes)) {
    if (typeof node.disabledWhen !== 'string') continue;
    let names: Set<string>;
    try {
      names = identifiers(parseExpr(node.disabledWhen));
    } catch (err) {
      throw new GraphError(`ui node ${id}: disabledWhen: ${(err as Error).message}`);
    }
    for (const n of names) {
      if (n !== 'state' && flow.context[n] === undefined && flow.guards[n] === undefined) {
        throw new GraphError(`ui node ${id}: disabledWhen: ${node.disabledWhen} names ${n}, which is not a variable or a guard of the flow`);
      }
    }
  }
}
