// A flow's needs held to their evidence, and a product's gaps to its needs (SPEC.md §13). The checker is the Rust one
// (crates/delurio-core/src/needs.rs).
import { call, json, native, warnings } from './internal.js';

const constants = JSON.parse(native.needsConstants());

export const NEEDS_FILE = constants.NEEDS_FILE;
export const GAPS_FILE = constants.GAPS_FILE;

export function readNeeds(dir) {
  try {
    return JSON.parse(call(null, () => native.readNeeds(String(dir))));
  } finally {
    warnings();
  }
}

// what only missing needs name: the states and the nodes a walk leaves out, each with the needs that name it
export function planned(needs) {
  const p = JSON.parse(native.planned(needs === null || needs === undefined ? undefined : json(needs)));
  return { states: new Map(p.states), nodes: new Map(p.nodes) };
}

export function checkNeeds(files, app) {
  try {
    return JSON.parse(native.checkNeeds([...files].map(String), app ?? undefined));
  } finally {
    warnings();
  }
}
