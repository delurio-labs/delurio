// A flow's screens as wireframes, drawn with the fixed components or with a kit. The checker is the Rust one
// (crates/delurio-core/src/wireframe.rs).
import { Trap, call, native, nativeGraph, nativeKit } from './internal.js';

export function renderScreens(g, warn, kit = null) {
  const t = new Trap();
  return call(t, () => native.renderScreens(nativeGraph(g), t.warner(warn), nativeKit(kit)));
}

// One state's screen for the tutorial stage. Nodes that depend on a flag (`when`) appear only when named in `reveal`.
export function renderStageScreen(g, state, reveal, warn, kit = null) {
  const t = new Trap();
  return call(t, () => native.renderStageScreen(nativeGraph(g), String(state), [...(reveal ?? [])].map(String), t.warner(warn), nativeKit(kit)));
}

// For `kit check`: every screen with all its nodes (flags revealed, no annotations), and what each node must expose.
export function renderCheckScreens(g, kit, warn) {
  const t = new Trap();
  const out = JSON.parse(call(t, () => native.renderCheckScreens(nativeGraph(g), nativeKit(kit), t.warner(warn))));
  return { html: out.html, expected: out.expected.map((e) => ({ state: e.state, id: e.id, index: e.index, role: e.role, name: e.name, disabled: e.disabled })) };
}

export function nodeTexts(g, id, warn) {
  const t = new Trap();
  return call(t, () => native.nodeTexts(nativeGraph(g), String(id), t.warner(warn)));
}
