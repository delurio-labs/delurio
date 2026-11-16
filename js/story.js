// A flow's stories: the steps to a situation the exploration reached, the main story the graph declares, and either
// told in plain words. The checker is the Rust one (crates/delurio-core/src/story.rs).
import { call, json, native, nativeExploration, nativeGraph, nativeModel, pathSteps, pathStepsOf } from './internal.js';

export function stepsTo(m, ex, index) {
  return pathStepsOf(m, call(null, () => native.stepsTo(nativeModel(m), nativeExploration(ex), index)));
}

// The main story the graph declares: from a starting situation, send the events in order (`always` rules run by themselves).
export function stepsByEvents(m, main) {
  const p = call(null, () => native.stepsByEvents(nativeModel(m), main === undefined ? undefined : json(main)));
  return p === null || p === undefined ? null : pathStepsOf(m, p);
}

// The first success exit reachable through at least one screen: the story a person expects to read first.
export function mainStoryIndex(g, ex) {
  return native.mainStoryIndex(nativeGraph(g), nativeExploration(ex)) ?? null;
}

export function narrate(g, m, steps) {
  return JSON.parse(native.narrate(nativeGraph(g), nativeModel(m), pathSteps(m, steps)));
}
