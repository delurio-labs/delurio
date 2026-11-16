// The edges a flow's diagrams draw, merged by their ends, and the words of their nodes: what a person presses, where
// an exit leaves them, a step's response in one line. The checker is the Rust one (crates/delurio-core/src/edges.rs).
import { json, native, nativeExploration, nativeGraph } from './internal.js';

export const BY = JSON.parse(native.edgesConstants());

export function kindOf(g, name) {
  return native.kindOf(nativeGraph(g), String(name));
}

export function exitLine(st) {
  return native.exitLine(st === undefined ? undefined : json(st));
}

// a step's `response` in one line: the status, the code and the redirect first, then the other fields
export function responseLine(r) {
  return native.responseLine(json(r ?? {}));
}

export function nodeLine(g, name) {
  return native.nodeLine(nativeGraph(g), String(name));
}

// visible text for a catalog key, with {placeholders} filled from the samples
export function plain(g, key) {
  return native.plain(nativeGraph(g), String(key));
}

// the button or link text a person presses to send `event` while in `state`, if there is one
export function pressLabel(g, state, event) {
  return native.pressLabel(nativeGraph(g), String(state), String(event)) ?? null;
}

export function wrap(text, max) {
  return native.wrap(String(text), Number(max));
}

export function collectEdges(g, ex) {
  return JSON.parse(native.collectEdges(nativeGraph(g), nativeExploration(ex)));
}
