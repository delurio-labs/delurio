// The model checker (SPEC.md §2, §10): every (state, context) pair a flow can reach, found breadth first, and the
// guards, effects and assertions it is held to. The checker is the Rust one (crates/delurio-core/src/model.rs): a
// Model holds the addon's model of its flow, and an exploration stays in the addon until its pairs are read.
import {
  ModelError,
  Trap,
  call,
  decodeAt,
  encode,
  encodeAll,
  exploration,
  inner,
  native,
  nativeExploration,
  nativeModel,
  nativeValue,
  placed,
  registerModel,
} from './internal.js';

export { ModelError };

export class Model {
  flow;
  vars;
  domains;
  #any;

  constructor(flow) {
    const n = call(null, () => new native.NativeModel(nativeValue(flow)));
    this.flow = flow;
    this.vars = n.vars();
    this.domains = this.vars.map((v) => flow.context[v].values);
    this.#any = n.anyVars();
    registerModel(this, n, this.domains);
  }

  test(expr, state, ctx) {
    return call(null, () => nativeModel(this).test(String(expr), String(state), encode(this, ctx)));
  }

  /**
   * Whether evaluating `src` can never throw: it names only the state, variables, and guards that parse and nest no
   * deeper than evaluation allows.
   */
  pure(src) {
    return nativeModel(this).pure(String(src));
  }

  /** The value of `src` in `state` whatever the context holds, when the state alone decides it; undefined when the context matters. */
  inState(src, state) {
    const j = call(null, () => nativeModel(this).inState(String(src), String(state)));
    return j === null || j === undefined ? undefined : JSON.parse(j);
  }

  /** `test` for one expression at many pairs: read once, then run at each. */
  tester(expr) {
    call(null, () => nativeModel(this).tester(String(expr)));
    return (state, ctx) => this.test(expr, state, ctx);
  }

  value(expr, state, ctx) {
    return JSON.parse(call(null, () => nativeModel(this).value(String(expr), String(state), encode(this, ctx))));
  }

  steps(state, ctx) {
    const x = inner(this);
    const out = call(null, () => x.n.steps(String(state), encode(this, ctx)));
    return out.target.map((target, k) => ({ event: out.event[k] ?? null, target, ctx: decodeAt(this, out.ctx, k * x.size), taken: out.taken[k] }));
  }

  /**
   * `err`, thrown by `steps(state, ctx)`, given the place in flow.yaml of the transition that threw it when it has
   * none; else the state.
   */
  placeError(err, state, ctx) {
    if (!(err instanceof Error) || err.place !== undefined) return err;
    const p = nativeModel(this).placeOf(String(err.message), String(state), encode(this, ctx));
    return p === null || p === undefined ? err : placed(err, JSON.parse(p));
  }

  initials() {
    const x = inner(this);
    const { count, ctx } = nativeModel(this).initials();
    const out = new Array(count);
    for (let k = 0; k < count; k++) out[k] = decodeAt(this, ctx, k * x.size);
    return out;
  }

  scenario(ctx) {
    const out = {};
    for (const i of this.#any) out[this.vars[i]] = ctx[i];
    return out;
  }
}

function starts(m, from) {
  if (from === undefined) return [undefined, undefined];
  return [encodeAll(m, from), from.length];
}

// Every reachable (state, context) pair, breadth first, so the first hit of a state is a shortest path to it.
// `from` and `keep` narrow the search: the replay starts only from situations the app is bound to and takes only steps it can drive.
export function explore(m, from, keep) {
  const t = new Trap();
  const [codes, count] = starts(m, from);
  const nx = call(t, () => native.explore(nativeModel(m), codes, count, keep === undefined || keep === null ? undefined : t.keeper(keep, m)));
  return exploration(m, nx);
}

export function pathTo(m, ex, index) {
  return JSON.parse(native.pathTo(nativeModel(m), nativeExploration(ex), index));
}

export function checkAssertions(m, ex) {
  return JSON.parse(call(null, () => native.checkAssertions(nativeModel(m), nativeExploration(ex))));
}

export function finalPaths(m, ex) {
  return JSON.parse(native.finalPaths(nativeModel(m), nativeExploration(ex)));
}

export function transitionsOf(flow) {
  return native.transitionsOf(nativeValue(flow));
}

/**
 * Event paths that together pass every reachable state (`states`) or take every reachable transition (`transitions`)
 * of the flow (#4), from the same starts and through the same steps as `explore(m, from, keep)`.
 */
export function cover(m, by, from, keep) {
  const t = new Trap();
  const [codes, count] = starts(m, from);
  return JSON.parse(call(t, () => native.cover(nativeModel(m), String(by), codes, count, keep === undefined || keep === null ? undefined : t.keeper(keep, m))));
}

