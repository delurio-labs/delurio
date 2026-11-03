import { alternatives } from './graph.ts';
import type { Flow, Scalar, Transition } from './graph.ts';
import { parseExpr } from './expr.ts';
import type { Expr, Value } from './expr.ts';

export class ModelError extends Error {}

export interface Step {
  event: string | null;
  target: string;
  ctx: Scalar[];
  taken: string; // `${state}|${event or "always"}|${alternative index}`
}

export interface Config {
  state: string;
  ctx: Scalar[];
  parent: number;
  event: string | null;
}

// an expression made ready to run against one (state, context) pair
type Compiled = (state: string, ctx: Scalar[]) => Value;

// an effect made ready to apply: the variables it sets, and the ones it lets take any value, by position
interface Ready {
  set: Array<[number, Scalar]>;
  havoc: Array<[number, Scalar[]]>;
}

// one way out of a state: an event's alternative, or an `always` branch
interface Way {
  event: string | null;
  target: string;
  guard: string | undefined; // read the first time it is tried, as before: a guard never tried is never parsed
  effect: string[] | undefined;
  taken: string;
}

// The model explores millions of (state, context) pairs on a large flow (QAuth's consent: 5.4 million), so every guard,
// assertion and effect is read once and kept ready, instead of being looked up by name at every pair.
export class Model {
  readonly flow: Flow;
  readonly vars: string[];
  readonly domains: Scalar[][];
  private readonly at: Map<string, number>; // a variable's position in a context
  private readonly compiled = new Map<string, Compiled>();
  private readonly ready = new Map<string, Ready>();
  private readonly ways = new Map<string, { always: boolean; groups: Way[][] }>();

  constructor(flow: Flow) {
    this.flow = flow;
    this.vars = Object.keys(flow.context);
    this.domains = this.vars.map((v) => flow.context[v].values);
    this.at = new Map(this.vars.map((v, i) => [v, i]));
  }

  // what evaluate(parseExpr(src), lookup) gave, with the same errors, raised only when the name is reached
  private compile(e: Expr, depth: number): Compiled {
    switch (e.k) {
      case 'lit': {
        const v = e.v;
        return () => v;
      }
      case 'id': {
        const name = e.name;
        if (name === 'state') return (state) => state;
        const i = this.at.get(name);
        if (i !== undefined) return (_, ctx) => ctx[i];
        const guard = this.flow.guards[name];
        if (guard === undefined)
          return (state) => {
            throw new ModelError(`unknown name "${name}" (state ${state})`);
          };
        if (depth > 8)
          return () => {
            throw new ModelError(`guard ${name} nests too deep`);
          };
        let inner: Compiled | null = null; // compiled when first reached, as the lookup read it
        return (state, ctx) => (inner ??= this.compile(parseExpr(guard), depth + 1))(state, ctx);
      }
      case 'list': {
        const items = e.items.map((x) => this.compile(x, depth));
        return (state, ctx) => items.map((f) => f(state, ctx));
      }
      case 'not': {
        const f = this.compile(e.e, depth);
        return (state, ctx) => !f(state, ctx);
      }
      case 'and': {
        const l = this.compile(e.l, depth);
        const r = this.compile(e.r, depth);
        return (state, ctx) => Boolean(l(state, ctx)) && Boolean(r(state, ctx));
      }
      case 'or': {
        const l = this.compile(e.l, depth);
        const r = this.compile(e.r, depth);
        return (state, ctx) => Boolean(l(state, ctx)) || Boolean(r(state, ctx));
      }
      case 'cmp': {
        const l = this.compile(e.l, depth);
        const r = this.compile(e.r, depth);
        if (e.op === '==') return (state, ctx) => l(state, ctx) === r(state, ctx);
        if (e.op === '!=') return (state, ctx) => l(state, ctx) !== r(state, ctx);
        const negate = e.op === 'not in';
        // a list of literals, the usual right side, is the same list every time: made once
        const fixed = e.r.k === 'list' && e.r.items.every((x) => x.k === 'lit') ? e.r.items.map((x) => (x as { v: Value }).v) : null;
        if (fixed !== null)
          return (state, ctx) => {
            const x = l(state, ctx);
            let inList = false;
            for (let j = 0; j < fixed.length && !inList; j++) inList = fixed[j] === x;
            return negate ? !inList : inList;
          };
        return (state, ctx) => {
          const x = l(state, ctx);
          const list = r(state, ctx);
          const inList = Array.isArray(list) && list.some((y) => y === x);
          return negate ? !inList : inList;
        };
      }
    }
  }

  private expr(src: string): Compiled {
    let f = this.compiled.get(src);
    if (f === undefined) {
      f = this.compile(parseExpr(src), 0);
      this.compiled.set(src, f);
    }
    return f;
  }

  test(expr: string, state: string, ctx: Scalar[]): boolean {
    return Boolean(this.expr(expr)(state, ctx));
  }

  /**
   * Whether evaluating `src` can never throw: it names only the state, variables, and guards that parse and nest no
   * deeper than evaluation allows. Only then may its pairs be tested in another order than the exploration's.
   */
  pure(src: string): boolean {
    const ok = (e: Expr, depth: number, seen: Set<string>): boolean => {
      switch (e.k) {
        case 'lit':
          return true;
        case 'id': {
          if (e.name === 'state' || this.at.has(e.name)) return true;
          const guard = this.flow.guards[e.name];
          if (guard === undefined || depth > 8 || seen.has(e.name)) return false;
          try {
            return ok(parseExpr(guard), depth + 1, new Set([...seen, e.name]));
          } catch {
            return false;
          }
        }
        case 'list':
          return e.items.every((x) => ok(x, depth, seen));
        case 'not':
          return ok(e.e, depth, seen);
        default:
          return ok(e.l, depth, seen) && ok(e.r, depth, seen);
      }
    };
    try {
      return ok(parseExpr(src), 0, new Set());
    } catch {
      return false;
    }
  }

  /**
   * The value of `src` in `state` whatever the context holds, when the state alone decides it (`state in [...] and
   * ...` outside those states); undefined when the context matters. Folds only what evaluation would compute the same
   * way: literals, the state, and the side of an `and` / `or` that decides it before the other is read.
   */
  inState(src: string, state: string): Value | undefined {
    const fold = (e: Expr): { v: Value } | undefined => {
      switch (e.k) {
        case 'lit':
          return { v: e.v };
        case 'id':
          return e.name === 'state' ? { v: state } : undefined;
        case 'list': {
          const items = e.items.map(fold);
          return items.every((x) => x !== undefined) ? { v: items.map((x) => (x as { v: Value }).v) } : undefined;
        }
        case 'not': {
          const x = fold(e.e);
          return x === undefined ? undefined : { v: !x.v };
        }
        case 'and':
        case 'or': {
          const l = fold(e.l);
          if (l === undefined) return undefined;
          if (e.k === 'and' ? !l.v : Boolean(l.v)) return { v: e.k === 'or' };
          const r = fold(e.r);
          return r === undefined ? undefined : { v: Boolean(r.v) };
        }
        case 'cmp': {
          const l = fold(e.l);
          const r = fold(e.r);
          if (l === undefined || r === undefined) return undefined;
          if (e.op === '==') return { v: l.v === r.v };
          if (e.op === '!=') return { v: l.v !== r.v };
          const inList = Array.isArray(r.v) && r.v.some((y) => y === l.v);
          return { v: e.op === 'in' ? inList : !inList };
        }
      }
    };
    return fold(parseExpr(src))?.v;
  }

  /** `test` for one expression at many pairs: read once, then run at each. */
  tester(expr: string): (state: string, ctx: Scalar[]) => boolean {
    const f = this.expr(expr);
    return (state, ctx) => Boolean(f(state, ctx));
  }

  value(expr: string, state: string, ctx: Scalar[]): Value {
    return this.expr(expr)(state, ctx);
  }

  private effect(name: string): Ready {
    let r = this.ready.get(name);
    if (r === undefined) {
      const effect = this.flow.effects[name];
      if (effect === undefined) throw new ModelError(`unknown effect ${name}`);
      r = {
        // a name that is not a variable set nothing a context holds
        set: Object.entries(effect.set ?? {}).flatMap(([k, v]): Array<[number, Scalar]> => (this.at.has(k) ? [[this.at.get(k) as number, v]] : [])),
        havoc: (effect.havoc ?? []).map((h): [number, Scalar[]] => {
          const i = this.vars.indexOf(h);
          return [i, this.domains[i]];
        }),
      };
      this.ready.set(name, r);
    }
    return r;
  }

  private apply(names: string[] | undefined, ctx: Scalar[]): Scalar[][] {
    let outs: Scalar[][] = [ctx];
    for (const name of names ?? []) {
      const { set, havoc } = this.effect(name);
      const next: Scalar[][] = [];
      for (const base of outs) {
        const c = base.slice();
        for (const [i, v] of set) c[i] = v;
        if (havoc.length === 0) {
          next.push(c);
          continue;
        }
        // every combination of the havoc variables' values, the first variable slowest, as the nested lists gave them
        let combos: Scalar[][] = [[]];
        for (const [, values] of havoc) combos = combos.flatMap((p) => values.map((v) => [...p, v]));
        for (const combo of combos) {
          const c2 = c.slice();
          for (let j = 0; j < havoc.length; j++) c2[havoc[j][0]] = combo[j];
          next.push(c2);
        }
      }
      outs = next;
    }
    return outs;
  }

  // a state's ways out, read once: `always` as one group tried in order, else one group per event
  private waysOf(state: string): { always: boolean; groups: Way[][] } {
    let w = this.ways.get(state);
    if (w === undefined) {
      const st = this.flow.states[state];
      const way = (event: string | null, a: Transition, i: number): Way => ({
        event,
        target: a.target,
        guard: a.guard,
        effect: a.effect,
        taken: `${state}|${event ?? 'always'}|${i}`,
      });
      w =
        st.type === 'final'
          ? { always: false, groups: [] }
          : st.always !== undefined
            ? { always: true, groups: [st.always.map((a, i) => way(null, a, i))] }
            : { always: false, groups: Object.entries(st.on ?? {}).map(([event, spec]) => alternatives(spec).map((a, i) => way(event, a, i))) };
      this.ways.set(state, w);
    }
    return w;
  }

  steps(state: string, ctx: Scalar[]): Step[] {
    const { always, groups } = this.waysOf(state);
    const out: Step[] = [];
    for (const alts of groups) {
      let chosen = false;
      for (const a of alts) {
        if (a.guard === undefined || this.test(a.guard, state, ctx)) {
          for (const c of this.apply(a.effect, ctx)) out.push({ event: a.event, target: a.target, ctx: c, taken: a.taken });
          chosen = true;
          break;
        }
      }
      if (always && !chosen) throw new ModelError(`\`always\` in ${state} is not exhaustive for ${JSON.stringify(Object.fromEntries(this.vars.map((v, i) => [v, ctx[i]])))}`);
    }
    return out;
  }

  initials(): Scalar[][] {
    // every combination of the starting values, the first variable slowest: the order the nested lists gave
    const choices = this.vars.map((v, i) => {
      const initial = this.flow.context[v].initial;
      return initial === 'any' ? this.domains[i] : [initial];
    });
    const total = choices.reduce((n, c) => n * c.length, 1);
    const out: Scalar[][] = new Array(total);
    for (let k = 0; k < total; k++) {
      const c: Scalar[] = new Array(choices.length);
      let rest = k;
      for (let i = choices.length - 1; i >= 0; i--) {
        c[i] = choices[i][rest % choices[i].length];
        rest = Math.floor(rest / choices[i].length);
      }
      out[k] = c;
    }
    return out;
  }

  scenario(ctx: Scalar[]): Record<string, Scalar> {
    const out: Record<string, Scalar> = {};
    this.vars.forEach((v, i) => {
      if (this.flow.context[v].initial === 'any') out[v] = ctx[i];
    });
    return out;
  }
}

export interface Exploration {
  configs: Config[];
  starts: number;
  taken: Set<string>;
  atState: Map<string, number>;
  branchCounts: Map<string, number[]>;
}

// A (state, context) pair as one number: the state's position, then each variable's value by its position in the
// variable's values, as digits of a mixed radix. A Set keeps numbers far faster than the strings they replace.
// Two pairs are the same exactly when their old keys, `${state}|${ctx.join(',')}`, are: a value is placed by its text,
// as join wrote it. A flow whose pairs do not fit below 2^53, a value or state name that would make the old key
// ambiguous, or a value outside its variable's values, keeps the old key.
function keyer(m: Model): (state: string, ctx: Scalar[]) => number | string {
  const text = (state: string, ctx: Scalar[]): string => `${state}|${ctx.join(',')}`;
  const written = (v: unknown): string => (v === null || v === undefined ? '' : String(v));
  const names = Object.keys(m.flow.states);
  const size = m.domains.reduce((n, values) => n * Math.max(values.length, 1), Math.max(names.length, 1));
  if (!Number.isSafeInteger(size) || names.some((s) => s.includes('|')) || m.domains.some((values) => values.some((v) => written(v).includes(',')))) return text;
  const states = new Map(names.map((s, i) => [s, i]));
  const byText = m.domains.map((values) => {
    const d = new Map<string, number>();
    values.forEach((v, i) => {
      if (!d.has(written(v))) d.set(written(v), i);
    });
    return d;
  });
  const byValue = m.domains.map((values, i) => new Map(values.map((v) => [v, byText[i].get(written(v)) as number])));
  const radix = m.domains.map((values) => Math.max(values.length, 1));
  return (state, ctx) => {
    let key = states.get(state);
    if (key === undefined || ctx.length !== radix.length) return text(state, ctx);
    for (let i = 0; i < radix.length; i++) {
      const d = byValue[i].get(ctx[i]) ?? byText[i].get(written(ctx[i]));
      if (d === undefined) return text(state, ctx);
      key = key * radix[i] + d;
    }
    return key;
  };
}

// Every reachable (state, context) pair, breadth first, so the first hit of a state is a shortest path to it.
// `from` and `keep` narrow the search: the replay starts only from situations the app is bound to and takes only steps it can drive.
export function explore(m: Model, from: Scalar[][] = m.initials(), keep?: (at: Config, s: Step) => boolean): Exploration {
  const index = new Set<number | string>();
  const key = keyer(m);
  const configs: Config[] = [];
  const taken = new Set<string>();
  const atState = new Map<string, number>();
  const branchCounts = new Map<string, number[]>();
  const push = (state: string, ctx: Scalar[], parent: number, event: string | null): void => {
    const k = key(state, ctx);
    if (index.has(k)) return;
    index.add(k);
    configs.push({ state, ctx, parent, event });
  };
  for (const c of from) push(m.flow.initial, c, -1, null);
  const starts = configs.length;
  for (let i = 0; i < configs.length; i++) {
    const { state, ctx } = configs[i];
    atState.set(state, (atState.get(state) ?? 0) + 1);
    const steps = m.steps(state, ctx);
    for (const s of steps) {
      if (keep !== undefined && !keep(configs[i], s)) continue;
      taken.add(s.taken);
      push(s.target, s.ctx, i, s.event);
    }
    const always = m.flow.states[state].always;
    if (always !== undefined && steps.length > 0) {
      const counts = branchCounts.get(state) ?? always.map(() => 0);
      const taken0 = steps[0].taken;
      counts[Number(taken0.slice(taken0.lastIndexOf('|') + 1))] += 1;
      branchCounts.set(state, counts);
    }
  }
  return { configs, starts, taken, atState, branchCounts };
}

export interface PathResult {
  events: string[];
  start: Record<string, Scalar>;
}

export function pathTo(m: Model, ex: Exploration, index: number): PathResult {
  const events: string[] = [];
  let c = ex.configs[index];
  while (c.parent >= 0) {
    if (c.event !== null) events.push(c.event);
    c = ex.configs[c.parent];
  }
  return { events: events.reverse(), start: m.scenario(c.ctx) };
}

export interface AssertionResult {
  id: string;
  never: string;
  say?: string;
  violation?: PathResult & { state: string };
}

// the first pair, in exploration order, where `src` holds; -1 when none does. An assertion that cannot throw is tested
// only in the states where the state alone does not rule it out: the first hit of each such state, the earliest of them.
function firstHit(m: Model, ex: Exploration, src: string, byState: () => Map<string, number[]>): number {
  // read where the exploration reads it, so an expression that throws throws at the same pair as before
  if (!m.pure(src)) return ex.configs.findIndex((c) => m.test(src, c.state, c.ctx));
  const test = m.tester(src);
  let hit = -1;
  for (const [state, at] of byState()) {
    const fixed = m.inState(src, state);
    if (fixed !== undefined && !fixed) continue;
    for (const i of at) {
      if (hit >= 0 && i > hit) break;
      if (fixed !== undefined || test(state, ex.configs[i].ctx)) {
        hit = i;
        break;
      }
    }
  }
  return hit;
}

export function checkAssertions(m: Model, ex: Exploration): AssertionResult[] {
  let groups: Map<string, number[]> | null = null;
  const byState = (): Map<string, number[]> => {
    if (groups === null) {
      groups = new Map();
      ex.configs.forEach((c, i) => {
        const list = groups?.get(c.state);
        if (list === undefined) groups?.set(c.state, [i]);
        else list.push(i);
      });
    }
    return groups;
  };
  return (m.flow.assertions ?? []).map((a) => {
    const hit = firstHit(m, ex, a.never, byState);
    if (hit < 0) return { id: a.id, never: a.never, say: a.say };
    return { id: a.id, never: a.never, say: a.say, violation: { ...pathTo(m, ex, hit), state: ex.configs[hit].state } };
  });
}

export interface FinalPath extends PathResult {
  state: string;
  index: number; // position in Exploration.configs, for building a narrative
}

export function finalPaths(m: Model, ex: Exploration): FinalPath[] {
  const out: FinalPath[] = [];
  for (const [name, st] of Object.entries(m.flow.states)) {
    if (st.type !== 'final') continue;
    const hit = ex.configs.findIndex((c) => c.state === name);
    if (hit >= 0) out.push({ state: name, index: hit, ...pathTo(m, ex, hit) });
  }
  return out;
}
