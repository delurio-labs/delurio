// The guard and assertion language: a small, fixed subset, interpreted without eval.
//   or / and / not, == / != / in / not in, parentheses, [lists], 'strings', numbers, true / false / null, names.
// The checker is the Rust one (crates/delurio-core/src/expr.rs).
import { Trap, call, native } from './internal.js';

const cache = new Map();

export function parseExpr(src) {
  const hit = cache.get(src);
  if (hit !== undefined) return hit;
  const e = JSON.parse(call(null, () => native.parseExpr(String(src))));
  cache.set(src, e);
  return e;
}

// `lookup` is called for each name the expression reads, as it reads them; what it throws reaches the caller
export function evaluate(e, lookup) {
  const t = new Trap();
  return JSON.parse(call(t, () => native.evaluate(JSON.stringify(e), t.lookup(lookup))));
}

export function identifiers(e, out = new Set()) {
  for (const name of native.identifiers(JSON.stringify(e))) out.add(name);
  return out;
}
