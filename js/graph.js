// A graph as `load` reads it from its folder: flow.yaml, ui.yaml and the catalog ui.yaml names (SPEC.md §2-§4), and
// the readers of its parts. The checker is the Rust one (crates/delurio-core/src/graph.rs), run in the addon.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { GraphError, Trap, call, json, native, nativeGraph, nativeValue, placed, rememberGraph, warnings } from './internal.js';

export { GraphError, placed };

const constants = JSON.parse(native.graphConstants());

// in a returnsFocus, the node that was pressed to open the dialog (SPEC.md §4)
export const OPENER = constants.OPENER;

// the roles whose node may say whether it is `checked`
export const CHECKABLE = constants.CHECKABLE;

export const isDialog = (n) => native.isDialogNode(n === undefined ? undefined : JSON.stringify(n));

// the place of a transition in flow.yaml: `on.<event>`, the item of a list of alternatives, or an item of `always`
export function transitionPath(state, st, event, i) {
  return native.transitionPathOf(String(state), json(st ?? {}), event === null || event === undefined ? undefined : String(event), i);
}

// 'silentRedirect' -> 'Silent redirect'; used only when the graph gives no title
export function humanize(id) {
  return native.humanize(String(id));
}

export function titleOf(flow, state) {
  return native.titleOfState(nativeValue(flow), String(state));
}

// a node is shown in a state when the state is in its `in` and not in its `unless`
export function shownIn(n, state) {
  return native.shownInState(json(n ?? {}), String(state));
}

// The node that has focus once the flow has moved from the state `from` (null when it starts) into another state `to`
// (SPEC.md §4, Focus). `ui` is read as it is at the call.
// the sensitivities a field may have (SPEC.md §5), which a binding's `mask` names
const SENSITIVITIES = ['credential', 'personal', 'internal'];

// the nodes a recording masks: each node that holds a field of the flow's domain whose sensitivity `mask` names, the
// binding's (SPEC.md §4, §7): `credential` unless it says, and nothing for []. In the order of the ui's nodes; none
// when the domain is not found beside the graph.
export function maskedNodes(g, mask = ['credential']) {
  if (!Array.isArray(mask)) throw new TypeError('maskedNodes: mask is a list of sensitivities');
  for (const s of mask)
    if (!SENSITIVITIES.includes(s))
      throw new TypeError(`maskedNodes: ${String(s)} is not a sensitivity (${SENSITIVITIES.join(', ')})`);
  return native.maskedNodesOf(nativeGraph(g), [...mask]);
}

export function focusAfter(ui, from, to, pressed) {
  return JSON.parse(native.focusAfterStep(json(ui), from ?? undefined, String(to), pressed ?? undefined));
}

export function alternatives(spec) {
  if (spec === undefined) return [undefined];
  return JSON.parse(native.alternativesOf(json(spec)));
}

// `read` and `exists` take a path relative to `dir`; the revision diff passes ones that read a git revision instead of the disk
export function load(dir, read = (file) => readFileSync(join(dir, file), 'utf8'), exists = (file) => existsSync(join(dir, file))) {
  const t = new Trap();
  try {
    const n = call(t, () => native.load(String(dir), t.reader(read), t.exister(exists)));
    return rememberGraph(JSON.parse(n.json()), n);
  } finally {
    warnings();
  }
}

