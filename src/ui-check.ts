// What `check` holds in ui.yaml beyond its schema (SPEC.md §4): `focus` and `returnsFocus` give the focus to a node
// that is shown where the focus goes, `returnsFocus` starts at a dialog, the words a field's `value` takes from the
// catalog are there, as every other node's are, and the flow's name for its users (`task`) is in every locale. That `value`, `checked` and `script` sit only on the nodes they
// are for is the schema's; that a `focus` state exists and that `checked` reads the context is the loader's.
import { readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { alternatives, isDialog, OPENER, shownIn } from './graph.ts';
import type { Graph, Label } from './graph.ts';

export interface UiProblem {
  rule: 'focus-not-shown' | 'returns-focus-not-dialog' | 'task-locale';
  path: string[]; // where in ui.yaml
  message: string;
  element: string;
}

const listed = (xs: string[]): string => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} and ${xs[xs.length - 1]}`);

// the states the flow stops in after a transition out of `state` (on `event` only, when it is given): its targets, and
// past a state with `always` the targets of that state, as far as they go
function stopsAfter(g: Graph, state: string, event?: string): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  const ways = (s: string): string[] => {
    const st = g.flow.states[s];
    return [...(st.always ?? []).map((a) => a.target), ...Object.values(st.on ?? {}).flatMap((spec) => alternatives(spec).map((a) => a.target))];
  };
  const first = g.flow.states[state].on?.[event ?? ''];
  const todo = event === undefined ? ways(state) : first === undefined ? [] : alternatives(first).map((a) => a.target);
  while (todo.length > 0) {
    const t = todo.shift() as string;
    if (seen.has(t)) continue;
    seen.add(t);
    if (g.flow.states[t].always !== undefined) todo.push(...ways(t));
    else out.push(t);
  }
  return out;
}

// the states a dialog closes into: those the flow stops in after a state that shows it, and that do not show it
function closesInto(g: Graph, dialog: string): string[] {
  const d = g.ui.nodes[dialog];
  const out = new Set<string>();
  for (const s of Object.keys(g.flow.states)) {
    if (!shownIn(d, s)) continue;
    for (const t of stopsAfter(g, s)) if (!shownIn(d, t)) out.add(t);
  }
  return [...out];
}

// the nodes that open a dialog: each is shown in a state that does not show the dialog, and sends an event that leads
// from there to a state that shows it (SPEC.md §4, the node that opened a dialog)
function openersOf(g: Graph, dialog: string): string[] {
  const d = g.ui.nodes[dialog];
  const out: string[] = [];
  for (const [id, n] of Object.entries(g.ui.nodes)) {
    if (n.emits === undefined) continue;
    const opens = Object.keys(g.flow.states).some((s) => shownIn(n, s) && !shownIn(d, s) && stopsAfter(g, s, n.emits).some((t) => shownIn(d, t)));
    if (opens) out.push(id);
  }
  return out;
}

export function checkUi(g: Graph, warn: (message: string) => void): UiProblem[] {
  const nodes = g.ui.nodes;
  const out: UiProblem[] = [];
  for (const [state, node] of Object.entries(g.ui.focus ?? {})) {
    const path = ['focus', state];
    const element = `focus/${state}`;
    if (nodes[node] === undefined) out.push({ rule: 'focus-not-shown', path, element, message: `focus.${state} names ${node}, which is no node of ui.yaml` });
    else if (!shownIn(nodes[node], state)) out.push({ rule: 'focus-not-shown', path, element, message: `focus.${state} gives the focus to ${node}, which ${state} does not show: a node has focus only where it is shown (its in and unless, SPEC.md §4)` });
  }
  (g.ui.edges ?? []).forEach((e, i) => {
    if (e.returnsFocus === undefined) return;
    const [dialog, node] = e.returnsFocus;
    const path = ['edges', String(i)];
    const element = `edges/returnsFocus/${dialog}`;
    if (!isDialog(nodes[dialog])) {
      const what = nodes[dialog] === undefined ? 'no node of ui.yaml' : 'not a dialog';
      out.push({ rule: 'returns-focus-not-dialog', path, element, message: `returnsFocus starts at ${dialog}, which is ${what}: it starts at a page node whose role is dialog or alertdialog (SPEC.md §4)` });
      return;
    }
    // one node, the nodes that open the dialog, or OPENER: whichever node opens it
    const several = Array.isArray(node) || node === OPENER;
    const targets = node === OPENER ? openersOf(g, dialog) : Array.isArray(node) ? node : [node];
    if (node === OPENER && targets.length === 0) {
      out.push({ rule: 'focus-not-shown', path, element, message: `returnsFocus gives the focus of ${dialog} back to the node that opened it, but no node opens it: none sends an event that leads from a state without the dialog to one that shows it (SPEC.md §4)` });
      return;
    }
    const into = closesInto(g, dialog);
    for (const t of targets) {
      const at = several ? `${element}/${t}` : element;
      if (nodes[t] === undefined) {
        out.push({ rule: 'focus-not-shown', path, element: at, message: `returnsFocus gives the focus of ${dialog} to ${t}, which is no node of ui.yaml` });
        continue;
      }
      if (into.some((s) => shownIn(nodes[t], s))) continue;
      const where = into.length === 0 ? `the dialog ${dialog} never closes into a state the flow stops in` : `no state ${dialog} closes into shows it (${listed(into)})`;
      const who = node === OPENER ? `back to ${t}, which opens it,` : `to ${t},`;
      out.push({ rule: 'focus-not-shown', path, element: at, message: `returnsFocus gives the focus of ${dialog} ${who} but ${where} (SPEC.md §4)` });
    }
  });
  // a field's value in words of the catalog, held as every other node's words are
  const words = (v: Label): void => {
    for (const key of [...(v.key === undefined ? [] : [v.key]), ...(v.oneOf ?? [])]) {
      const message = g.catalog[key];
      if (message === undefined) {
        warn(`catalog key ${key} is missing`);
        continue;
      }
      for (const [, p] of message.matchAll(/\{(\w+)\}/g)) if (g.ui.samples[p] === undefined) warn(`no sample value for {${p}} in ${key}`);
    }
  };
  for (const n of Object.values(nodes)) if (n.value !== undefined) words(n.value);
  // the flow's name for its users, in the catalog of every locale, as the changes note is
  const task = g.ui.task?.key;
  if (g.ui.absent !== true && typeof task === 'string') {
    for (const locale of g.ui.locales ?? [basename(g.ui.catalog, '.json')]) {
      let catalog: unknown = null;
      try {
        catalog = JSON.parse(readFileSync(join(g.dir, dirname(g.ui.catalog), `${locale}.json`), 'utf8'));
      } catch {
        catalog = null;
      }
      const name = (catalog as Record<string, unknown> | null)?.[task];
      if (typeof name === 'string' && name.trim() !== '') continue;
      out.push({ rule: 'task-locale', path: ['task'], element: `task/${locale}`, message: `the flow's name for its users, ${task}, is not in the ${locale} catalog (SPEC.md §4)` });
    }
  }
  return out;
}
