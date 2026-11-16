// How the modules of @delurio/cli reach the checker, which runs in the addon (crates/delurio-node): not an entry
// point of the package. Values cross as JSON; the checker's errors come back as the classes the TypeScript modules
// threw; a caller's callback runs in a trap, so that what it throws never crosses into the addon and reaches the
// caller unchanged once the call is done, as it did when the TypeScript code called it.
import native from './native.cjs';

export { native };

const MARK = '\u0000delurio\u0000';

// a graph that cannot be loaded (SPEC.md §10)
export class GraphError extends Error {
  place;
  constructor(message, place) {
    super(message);
    this.place = place;
  }
}

// a flow that cannot be explored; `place` says where in flow.yaml, when it is known
export class ModelError extends Error {
  place;
  constructor(message, place) {
    super(message);
    this.place = place;
  }
}

// a kit that cannot be read or holds what a kit may not
export class KitError extends Error {}

// the yaml library's error for a text that is not YAML (or holds what delurio does not read), as readYaml threw it
export class YAMLParseError extends Error {
  constructor(pos, code, message) {
    super();
    this.name = 'YAMLParseError';
    this.code = code;
    this.message = message;
    this.pos = pos;
  }
}

// the place of an error that a reader of the graph's files threw, given to it when it has none yet
export function placed(err, place) {
  if (err instanceof Error && err.place === undefined) Object.assign(err, { place });
  return err;
}

// the error a call of the addon threw, as the caller meets it: the checker's own as its class, a callback's as the
// callback threw it
export function lift(e, t) {
  const text = e instanceof Error ? e.message : undefined;
  if (typeof text !== 'string' || !text.startsWith(MARK)) return e;
  const m = JSON.parse(text.slice(MARK.length));
  if (m.thrown === true) {
    if (t === undefined || t === null || !t.threw) return new Error('@delurio/cli: a callback failed');
    return m.place === undefined ? t.error : placed(t.error, m.place);
  }
  let err;
  switch (m.kind) {
    case 'graph':
      return new GraphError(m.message, m.place);
    case 'model':
      return new ModelError(m.message, m.place);
    case 'kit':
      err = new KitError(m.message);
      break;
    case 'yaml': {
      const at = m.line === undefined ? -1 : 0;
      err = new YAMLParseError(m.pos ?? [at, at], m.code, m.message);
      if (m.linePos !== undefined) err.linePos = m.linePos;
      else if (m.line !== undefined) err.linePos = [{ line: m.line, col: m.col }];
      break;
    }
    default:
      err = new Error(m.message);
  }
  return m.place === undefined ? err : placed(err, m.place);
}

// What a caller's callbacks threw during one call of the addon: the first throw is kept, and once one has thrown no
// callback of the call runs again (the TypeScript code had stopped there).
export class Trap {
  threw = false;
  error = undefined;

  catch(e) {
    if (this.threw) return;
    this.threw = true;
    this.error = e;
  }

  // read(file): { ok: text } or { thrown: message }
  reader(read) {
    return (file) => {
      if (this.threw) return { thrown: 'undefined' };
      try {
        return { ok: String(read(file)) };
      } catch (e) {
        this.catch(e);
        return { thrown: String(e?.message) };
      }
    };
  }

  // exists(file): 1, 0, or 2 when it threw
  exister(exists) {
    return (file) => {
      if (this.threw) return 2;
      try {
        return exists(file) ? 1 : 0;
      } catch (e) {
        this.catch(e);
        return 2;
      }
    };
  }

  warner(warn) {
    return (message) => {
      if (this.threw) return;
      try {
        warn(message);
      } catch (e) {
        this.catch(e);
      }
    };
  }

  // lookup(name): the value as JSON
  lookup(lookup) {
    return (name) => {
      if (this.threw) return 'null';
      try {
        const v = lookup(name);
        return (v === undefined ? undefined : JSON.stringify(v)) ?? 'null';
      } catch (e) {
        this.catch(e);
        return 'null';
      }
    };
  }

  // keep(at, s) of explore and cover: the addon hands the pair and the step as places in the model's tables
  keeper(keep, model) {
    let at = null;
    return (fresh, state, parent, event, atCtx, sEvent, target, sCtx, taken) => {
      if (this.threw) return 0;
      try {
        if (fresh || at === null) at = { state, ctx: decodeAll(model, atCtx), parent, event };
        const s = { event: sEvent, target, ctx: decodeAll(model, sCtx), taken };
        return keep(at, s) ? 1 : 0;
      } catch (e) {
        this.catch(e);
        return 0;
      }
    };
  }

  // run(): its result is kept by the caller
  runner(run) {
    return () => {
      if (this.threw) return;
      try {
        run();
      } catch (e) {
        this.catch(e);
      }
    };
  }
}

// `run()`, a call of the addon: its error lifted, and what a callback threw thrown again once it is done
export function call(t, run) {
  let out;
  try {
    out = run();
  } catch (e) {
    throw lift(e, t);
  }
  if (t !== null && t !== undefined && t.threw) throw t.error;
  return out;
}

// the yaml library hands each warning of a document it parsed to process.emitWarning; the addon keeps them
export function warnings() {
  for (const w of JSON.parse(native.takeYamlWarnings())) process.emitWarning(w.message, { type: 'YAMLWarning', code: w.code });
}

// a caller's value as JSON, its Sets as lists and its Maps as objects
const replacer = (_k, v) => (v instanceof Set ? [...v] : v instanceof Map ? Object.fromEntries(v) : v);
export const json = (v) => JSON.stringify(v, replacer);

function object(v, what) {
  if (v === null || typeof v !== 'object') throw new TypeError(`@delurio/cli: ${what} is not an object: ${String(v)}`);
  return v;
}

// ---- objects the addon holds for the caller's
//
// A graph, a navigation graph, a kit or a flow is read into the addon once for the object the caller holds, and read
// again when that object's own keys hold other values (a copy made with { ...g, catalog }, a key given another
// value). What lies deeper is read once: a caller that changes a loaded graph in place hands a copy instead, as
// delurio's own code does.

const graphs = new WeakMap();

export function rememberGraph(g, n) {
  graphs.set(g, { n, dir: g.dir, flow: g.flow, ui: g.ui, catalog: g.catalog });
  return g;
}

export function nativeGraph(g) {
  const hit = graphs.get(object(g, 'the graph'));
  if (hit !== undefined && hit.dir === g.dir && hit.flow === g.flow && hit.ui === g.ui && hit.catalog === g.catalog) return hit.n;
  const n = call(null, () => new native.NativeGraph(JSON.stringify(g)));
  rememberGraph(g, n);
  return n;
}

const values = new WeakMap();

export function nativeValue(v) {
  const hit = values.get(object(v, 'the value'));
  if (hit !== undefined) return hit;
  const n = call(null, () => new native.NativeValue(JSON.stringify(v)));
  values.set(v, n);
  return n;
}

const navs = new WeakMap();
const NAV_KEYS = ['dir', 'adapter', 'frames', 'menuFrames', 'id', 'title', 'status', 'summary', 'locale', 'source', 'facts', 'guards', 'pages', 'entries', 'specs', 'personas', 'goals', 'notModeled', 'catalog'];

export function rememberNav(g, n) {
  navs.set(g, { n, keys: NAV_KEYS.map((k) => g[k]) });
  return g;
}

export function nativeNav(g) {
  const hit = navs.get(object(g, 'the navigation graph'));
  if (hit !== undefined && NAV_KEYS.every((k, i) => hit.keys[i] === g[k])) return hit.n;
  const n = call(null, () => new native.NativeNav(JSON.stringify(g)));
  rememberNav(g, n);
  return n;
}

const kits = new WeakMap();

export function rememberKit(k, n) {
  kits.set(k, n);
  return k;
}

export function nativeKit(k) {
  if (k === null || k === undefined) return undefined;
  const hit = kits.get(object(k, 'the kit'));
  if (hit !== undefined) return hit;
  const n = call(null, () => new native.NativeKit(JSON.stringify(k)));
  kits.set(k, n);
  return n;
}

// ---- models and contexts
//
// A context crosses as the places of its values in the model's tables. Each Model keeps, per variable, which place
// holds which value and back: the values of flow.context[v].values are the flow's own, so a context the model gives
// holds the very values (objects included) the flow and the caller hold.

const models = new WeakMap();

export function registerModel(m, n, domains) {
  const codes = n.domainCodes();
  const dec = codes.map(() => new Map());
  const enc = codes.map(() => new Map());
  codes.forEach((list, i) => {
    const vs = Array.isArray(domains[i]) ? domains[i] : [];
    list.forEach((c, k) => {
      const v = vs[k];
      if (!dec[i].has(c)) dec[i].set(c, v);
      if (!enc[i].has(v)) enc[i].set(v, c);
    });
  });
  models.set(m, { n, dec, enc, size: codes.length });
}

export function inner(m) {
  const x = models.get(m);
  if (x === undefined) throw new TypeError('@delurio/cli: a Model made with new Model(flow) is needed');
  return x;
}

export function nativeModel(m) {
  return inner(m).n;
}

function decodeOne(x, i, c) {
  const d = x.dec[i];
  if (d.has(c)) return d.get(c);
  const j = x.n.valueAt(i, c);
  const v = j === null || j === undefined ? undefined : JSON.parse(j);
  d.set(c, v);
  if (!x.enc[i].has(v)) x.enc[i].set(v, c);
  return v;
}

// the values of the context at codes[at .. at + size]
export function decodeAt(m, codes, at) {
  const x = inner(m);
  const out = new Array(x.size);
  for (let i = 0; i < x.size; i++) out[i] = decodeOne(x, i, codes[at + i]);
  return out;
}

export function decodeAll(m, codes) {
  return decodeAt(m, codes, 0);
}

// a context's places, one per variable (a missing value is undefined, one past the variables is left out)
export function encode(m, ctx) {
  const x = inner(m);
  const out = new Uint16Array(x.size);
  for (let i = 0; i < x.size; i++) {
    const v = ctx?.[i];
    let c = x.enc[i].get(v);
    if (c === undefined) {
      c = call(null, () => x.n.codeOf(i, v === undefined ? undefined : JSON.stringify(v)));
      x.enc[i].set(v, c);
      if (!x.dec[i].has(c)) x.dec[i].set(c, v);
    }
    out[i] = c;
  }
  return out;
}

// many contexts, one after the other
export function encodeAll(m, ctxs) {
  const x = inner(m);
  const out = new Uint16Array(ctxs.length * x.size);
  ctxs.forEach((c, k) => out.set(encode(m, c), k * x.size));
  return out;
}

export function size(m) {
  return inner(m).size;
}

// ---- explorations

const explorations = new WeakMap();

export function exploration(m, nx) {
  const ex = {};
  const materialize = () => {
    const { names, state, parent, event, ctx } = nx.configs();
    const n = size(m);
    const out = new Array(state.length);
    for (let i = 0; i < state.length; i++) {
      out[i] = { state: names[state[i]], ctx: decodeAt(m, ctx, i * n), parent: parent[i], event: event[i] === 0 ? null : names[event[i] - 1] };
    }
    return out;
  };
  const settle = (value) => Object.defineProperty(ex, 'configs', { value, writable: true, enumerable: true, configurable: true });
  // the pairs are made into objects only when they are read: a caller that reads only `taken` or `starts` keeps none
  Object.defineProperty(ex, 'configs', {
    enumerable: true,
    configurable: true,
    get() {
      const v = materialize();
      settle(v);
      return v;
    },
    set(v) {
      settle(v);
    },
  });
  ex.starts = nx.starts();
  ex.taken = new Set(nx.taken());
  ex.atState = new Map(JSON.parse(nx.atState()));
  ex.branchCounts = new Map(JSON.parse(nx.branchCounts()));
  explorations.set(ex, nx);
  return ex;
}

export function nativeExploration(ex) {
  const nx = explorations.get(object(ex, 'the exploration'));
  if (nx === undefined) throw new TypeError('@delurio/cli: an Exploration that explore() gave is needed');
  return nx;
}

// path steps a caller hands in, as the addon reads them
export function pathSteps(m, steps) {
  const list = Array.isArray(steps) ? steps : [];
  return {
    state: list.map((s) => String(s.state)),
    event: list.map((s) => (s.event === null || s.event === undefined ? null : String(s.event))),
    taken: list.map((s) => (s.taken === null || s.taken === undefined ? null : String(s.taken))),
    ctx: encodeAll(
      m,
      list.map((s) => s.ctx ?? []),
    ),
  };
}

// path steps the addon gives, as objects
export function pathStepsOf(m, p) {
  const n = size(m);
  return p.state.map((state, k) => ({ state, ctx: decodeAt(m, p.ctx, k * n), event: p.event[k] ?? null, taken: p.taken[k] ?? null }));
}
