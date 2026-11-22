// A kit: a design system's templates and styles, which the screens are drawn with (SPEC.md §4). The checker is the
// Rust one (crates/delurio-core/src/kit.rs); a kit loadKit read stays in the addon for the object it gave.
import { KitError, Trap, call, json, native, nativeKit, rememberKit, warnings } from './internal.js';

export { KitError };

// a kit as loadKit gave it: each component's `intents` is a key of its own, undefined when the kit gives none, and
// its `tones` one only when the kit gives them
function kitOf(n) {
  const k = JSON.parse(n.json());
  for (const [name, c] of Object.entries(k.components)) {
    k.components[name] = { template: c.template, roles: c.roles, intents: c.intents, ...(c.tones === undefined ? {} : { tones: c.tones }) };
  }
  return rememberKit(k, n);
}

// The node drawn with the kit, or null when the kit has no template for the component.
export function drawWith(kit, component, f, warn) {
  const t = new Trap();
  return call(t, () => native.drawWith(nativeKit(kit), String(component), json(f), t.warner(warn))) ?? null;
}

// A List or ListItem is drawn without a wrapper: the node id goes on the template's one element, and on the page the
// annotation chips go inside it, last.
export function mark(html, id, chips) {
  return native.mark(String(html), String(id), String(chips));
}

// the kit in a folder (its kit.yaml), or in a kit.yaml given itself
export function loadKit(path) {
  try {
    return kitOf(call(null, () => native.loadKit(String(path))));
  } finally {
    warnings();
  }
}

// `--kit <path>` wins over `kit:` in ui.yaml (a path relative to the graph); `--kit none` draws the fixed components.
export function resolveKit(graphDir, uiKit, flagValue) {
  try {
    const n = call(null, () => native.resolveKit(String(graphDir), uiKit ?? undefined, flagValue ?? undefined));
    return n === null || n === undefined ? null : kitOf(n);
  } finally {
    warnings();
  }
}

// The CSS a kit adds: its tokens and its CSS, scoped to the wireframe, and the few rules that keep the page's own
// marks readable or fit the screens to the video stage.
export function kitCss(kit, target) {
  return native.kitCss(nativeKit(kit), String(target));
}

// Runs once in the video stage, before the first frame: the screens are drawn at the largest zoom at which every one fits.
export function kitStageScript(kit) {
  return native.kitStageScript(nativeKit(kit));
}

// The families of the kit's own fonts (`fonts:`), each once, in the order of kit.yaml: what its screens should be
// drawn in, so that a tool can ask the browser whether it drew them.
export function kitFamilies(kit) {
  return native.kitFamilies(nativeKit(kit));
}
