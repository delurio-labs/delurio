// YAML as delurio reads it (SPEC.md §1): YAML 1.2 without anchors, aliases, tags, or keys that are lists or maps.
// The reader is the Rust port of the yaml library 2.9.1 (crates/delurio-core/src/yaml).
import { Trap, call, native, warnings } from './internal.js';

// the 1-based line and column of an offset into the text
export function lineAndColumn(text, offset) {
  return JSON.parse(native.lineAndColumn(String(text), Number(offset)));
}

// the first place in the text, by offset, that delurio does not read; null when there is none
export function unsupported(text) {
  return JSON.parse(native.unsupported(String(text)));
}

// `run`, with every text it reads through `readYaml` or `keyAt` parsed once; the same as `run` alone otherwise
export function parsingOnce(run) {
  const t = new Trap();
  let out;
  native.parsingOnce(
    t.runner(() => {
      out = run();
    }),
  );
  if (t.threw) throw t.error;
  return out;
}

// the value of a graph file's text: the yaml library's `parse`, once nothing unsupported is found
export function readYaml(text) {
  try {
    return JSON.parse(call(null, () => native.readYaml(String(text))));
  } finally {
    warnings();
  }
}

// the 1-based line and column of the key at `path` in a YAML text (of the item, for a list index), else the file's start
export function keyAt(text, path) {
  return JSON.parse(native.keyAt(String(text), [...path].map(String)));
}
