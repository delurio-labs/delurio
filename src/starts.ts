// `starts:` on a transition: this flow starts another one and does not wait for it (SPEC.md §2, "Several flows").
// The started flow is looked up by id among the graphs beside this one. When it is found, every situation in which the
// transition is taken must give it a beginning it accepts; when it is not written yet, that is a note, not a failure.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { parse } from 'yaml';
import { alternatives } from './graph.ts';
import type { Flow, Graph, Scalar, Transition } from './graph.ts';
import type { Exploration, Model } from './model.ts';

export interface HandOver {
  from: string; // the state the transition leaves
  event: string | null; // null for an `always` rule
  flow: string;
  given: Record<string, string | number | boolean>;
  found: boolean;
  problems: string[]; // what the started flow cannot begin with; empty when it fits
}

const SKIP = new Set(['viz', 'video', 'node_modules', '.git']);

// the root to search: the nearest folder called `graphs` above the graph, else the graph's parent
function rootOf(dir: string): string {
  for (let d = dir, i = 0; i < 5; i++, d = dirname(d)) if (basename(d) === 'graphs') return d;
  return dirname(dir);
}

function flowsUnder(root: string, depth = 4, out = new Map<string, Flow>()): Map<string, Flow> {
  const file = join(root, 'flow.yaml');
  if (existsSync(file)) {
    const f = parse(readFileSync(file, 'utf8')) as Flow;
    if (typeof f?.id === 'string') out.set(f.id, { ...f, context: f.context ?? {} });
  }
  if (depth === 0) return out;
  for (const name of readdirSync(root)) {
    if (SKIP.has(name)) continue;
    const sub = join(root, name);
    if (statSync(sub).isDirectory()) flowsUnder(sub, depth - 1, out);
  }
  return out;
}

export function checkStarts(g: Graph, m: Model, ex: Exploration): HandOver[] {
  const out: HandOver[] = [];
  let flows: Map<string, Flow> | null = null;
  for (const [name, st] of Object.entries(g.flow.states)) {
    const ways: Array<[string | null, number, Transition]> = [
      ...(st.always ?? []).map((t, i): [null, number, Transition] => [null, i, t]),
      ...Object.entries(st.on ?? {}).flatMap(([event, spec]) => alternatives(spec).map((t, i): [string, number, Transition] => [event, i, t])),
    ];
    for (const [event, i, t] of ways) {
      if (t.starts === undefined) continue;
      flows ??= flowsUnder(rootOf(g.dir));
      const given = t.starts.given ?? {};
      const started = flows.get(t.starts.flow);
      const hand: HandOver = { from: name, event, flow: t.starts.flow, given, found: started !== undefined, problems: [] };
      out.push(hand);
      if (started === undefined) continue;
      const problem = (p: string): void => {
        if (!hand.problems.includes(p)) hand.problems.push(p);
      };
      for (const k of Object.keys(given)) if (started.context[k] === undefined) problem(`${k} is not a variable of ${started.id}`);
      const taken = `${name}|${event ?? 'always'}|${i}`;
      for (const c of ex.configs) {
        if (c.state !== name) continue;
        for (const s of m.steps(c.state, c.ctx)) {
          if (s.taken !== taken) continue;
          for (const [k, expr] of Object.entries(given)) {
            const cv = started.context[k];
            if (cv === undefined) continue;
            const v = typeof expr === 'string' ? m.value(expr, name, s.ctx) : expr;
            if (!cv.values.includes(v as Scalar)) problem(`${k} would be ${JSON.stringify(v)}, which is not a value of ${started.id}'s ${k}`);
            else if (cv.initial !== 'any' && cv.initial !== v) problem(`${started.id} begins with ${k} = ${String(cv.initial)}, and here it would be ${String(v)}`);
          }
        }
      }
    }
  }
  return out;
}
