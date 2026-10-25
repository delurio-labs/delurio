import { alternatives } from './graph.ts';
import type { Flow, Scalar } from './graph.ts';
import { evaluate, parseExpr } from './expr.ts';
import type { Value } from './expr.ts';

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

export class Model {
  readonly flow: Flow;
  readonly vars: string[];
  readonly domains: Scalar[][];

  constructor(flow: Flow) {
    this.flow = flow;
    this.vars = Object.keys(flow.context);
    this.domains = this.vars.map((v) => flow.context[v].values);
  }

  private lookup(state: string, ctx: Scalar[], depth: number): (name: string) => Value {
    return (name) => {
      if (name === 'state') return state;
      const i = this.vars.indexOf(name);
      if (i >= 0) return ctx[i];
      const guard = this.flow.guards[name];
      if (guard === undefined) throw new ModelError(`unknown name "${name}" (state ${state})`);
      if (depth > 8) throw new ModelError(`guard ${name} nests too deep`);
      return evaluate(parseExpr(guard), this.lookup(state, ctx, depth + 1));
    };
  }

  test(expr: string, state: string, ctx: Scalar[]): boolean {
    return Boolean(this.value(expr, state, ctx));
  }

  value(expr: string, state: string, ctx: Scalar[]): Value {
    return evaluate(parseExpr(expr), this.lookup(state, ctx, 0));
  }

  private apply(names: string[] | undefined, ctx: Scalar[]): Scalar[][] {
    let outs: Scalar[][] = [ctx];
    for (const name of names ?? []) {
      const effect = this.flow.effects[name];
      if (effect === undefined) throw new ModelError(`unknown effect ${name}`);
      const next: Scalar[][] = [];
      for (const base of outs) {
        const c = base.slice();
        for (const [k, v] of Object.entries(effect.set ?? {})) c[this.vars.indexOf(k)] = v;
        const havoc = effect.havoc ?? [];
        if (havoc.length === 0) {
          next.push(c);
          continue;
        }
        let combos: Scalar[][] = [[]];
        for (const h of havoc) combos = combos.flatMap((p) => this.domains[this.vars.indexOf(h)].map((v) => [...p, v]));
        for (const combo of combos) {
          const c2 = c.slice();
          havoc.forEach((h, j) => {
            c2[this.vars.indexOf(h)] = combo[j];
          });
          next.push(c2);
        }
      }
      outs = next;
    }
    return outs;
  }

  steps(state: string, ctx: Scalar[]): Step[] {
    const st = this.flow.states[state];
    const out: Step[] = [];
    if (st.type === 'final') return out;
    if (st.always !== undefined) {
      for (let i = 0; i < st.always.length; i++) {
        const a = st.always[i];
        if (a.guard === undefined || this.test(a.guard, state, ctx)) {
          for (const c of this.apply(a.effect, ctx)) out.push({ event: null, target: a.target, ctx: c, taken: `${state}|always|${i}` });
          return out;
        }
      }
      throw new ModelError(`\`always\` in ${state} is not exhaustive for ${JSON.stringify(Object.fromEntries(this.vars.map((v, i) => [v, ctx[i]])))}`);
    }
    for (const [event, spec] of Object.entries(st.on ?? {})) {
      const alts = alternatives(spec);
      for (let i = 0; i < alts.length; i++) {
        const a = alts[i];
        if (a.guard === undefined || this.test(a.guard, state, ctx)) {
          for (const c of this.apply(a.effect, ctx)) out.push({ event, target: a.target, ctx: c, taken: `${state}|${event}|${i}` });
          break;
        }
      }
    }
    return out;
  }

  initials(): Scalar[][] {
    let combos: Scalar[][] = [[]];
    this.vars.forEach((v, i) => {
      const initial = this.flow.context[v].initial;
      const values = initial === 'any' ? this.domains[i] : [initial];
      combos = combos.flatMap((p) => values.map((x) => [...p, x]));
    });
    return combos;
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

// Every reachable (state, context) pair, breadth first, so the first hit of a state is a shortest path to it.
// `from` and `keep` narrow the search: the replay starts only from situations the app is bound to and takes only steps it can drive.
export function explore(m: Model, from: Scalar[][] = m.initials(), keep?: (at: Config, s: Step) => boolean): Exploration {
  const index = new Set<string>();
  const configs: Config[] = [];
  const taken = new Set<string>();
  const atState = new Map<string, number>();
  const branchCounts = new Map<string, number[]>();
  const push = (state: string, ctx: Scalar[], parent: number, event: string | null): void => {
    const key = `${state}|${ctx.join(',')}`;
    if (index.has(key)) return;
    index.add(key);
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
      counts[Number(steps[0].taken.split('|')[2])] += 1;
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

export function checkAssertions(m: Model, ex: Exploration): AssertionResult[] {
  return (m.flow.assertions ?? []).map((a) => {
    const hit = ex.configs.findIndex((c) => m.test(a.never, c.state, c.ctx));
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
