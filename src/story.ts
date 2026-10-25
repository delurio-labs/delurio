import { alternatives, titleOf } from './graph.ts';
import type { Graph, Scalar } from './graph.ts';
import { kindOf, pressLabel } from './edges.ts';
import type { Exploration, Model } from './model.ts';

export interface PathStep {
  state: string;
  ctx: Scalar[];
  event: string | null; // the event that led here
  taken: string | null; // the transition that led here: `${state}|${event or "always"}|${alternative}`
}

export interface Story {
  given: string;
  steps: string[];
  result: string;
}

export function stepsTo(m: Model, ex: Exploration, index: number): PathStep[] {
  const chain: number[] = [];
  for (let i = index; i >= 0; i = ex.configs[i].parent) chain.push(i);
  chain.reverse();
  return chain.map((ci, k) => {
    const c = ex.configs[ci];
    let taken: string | null = null;
    if (k > 0) {
      const prev = ex.configs[chain[k - 1]];
      const hit = m.steps(prev.state, prev.ctx).find((s) => s.target === c.state && s.event === c.event && s.ctx.join(',') === c.ctx.join(','));
      taken = hit?.taken ?? null;
    }
    return { state: c.state, ctx: c.ctx, event: c.event, taken };
  });
}

// The main story the graph declares: from a starting situation, send the events in order (`always` rules run by themselves).
export function stepsByEvents(m: Model, main: NonNullable<Graph['flow']['main']>): PathStep[] | null {
  const events = Array.isArray(main) ? main : main.events;
  const given = Array.isArray(main) ? {} : (main.given ?? {});
  for (const init of m.initials()) {
    if (Object.entries(given).some(([k, v]) => init[m.vars.indexOf(k)] !== v)) continue;
    const path = walk(m, [{ state: m.flow.initial, ctx: init, event: null, taken: null }], events, 0);
    if (path !== null) return path;
  }
  return null;
}

// Depth first, in document order, so the first walk that sends every event wins; when an effect leaves several
// situations (havoc), one that cannot go on gives way to the next.
function walk(m: Model, path: PathStep[], events: string[], i: number): PathStep[] | null {
  if (path.length > 500) return null;
  const cur = path[path.length - 1];
  const steps = m.steps(cur.state, cur.ctx);
  const autos = steps.filter((s) => s.event === null);
  const next = autos.length > 0 ? autos : i < events.length ? steps.filter((s) => s.event === events[i]) : [];
  if (next.length === 0) return i === events.length ? path : null;
  for (const s of next) {
    const found = walk(m, [...path, { state: s.target, ctx: s.ctx, event: s.event, taken: s.taken }], events, autos.length > 0 ? i : i + 1);
    if (found !== null) return found;
  }
  return null;
}

// The first success exit reachable through at least one screen: the story a person expects to read first.
export function mainStoryIndex(g: Graph, ex: Exploration): number | null {
  const viaScreen: boolean[] = [];
  ex.configs.forEach((c, i) => {
    viaScreen[i] = kindOf(g, c.state) === 'screen' || (c.parent >= 0 && viaScreen[c.parent]);
  });
  for (const [name, st] of Object.entries(g.flow.states)) {
    if (st.type !== 'final' || st.redirect !== 'success') continue;
    const i = ex.configs.findIndex((c, idx) => c.state === name && viaScreen[idx]);
    if (i >= 0) return i;
  }
  return null;
}

function sayOf(g: Graph, taken: string | null): string | null {
  if (taken === null) return null;
  const [state, event, idx] = taken.split('|');
  const st = g.flow.states[state];
  const a = event === 'always' ? st.always?.[Number(idx)] : alternatives(st.on?.[event] ?? []).at(Number(idx));
  return a?.say ?? null;
}

const capital = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

export function narrate(g: Graph, m: Model, steps: PathStep[]): Story {
  const start = steps[0];
  const parts: string[] = [];
  m.vars.forEach((v, i) => {
    const cv = g.flow.context[v];
    if (cv.initial !== 'any') return;
    const said = cv.say?.[String(start.ctx[i])];
    if (said === undefined) parts.push(`${v} is ${start.ctx[i]}`);
    else if (said !== '') parts.push(said);
  });
  const lines: string[] = [];
  steps.forEach((s, k) => {
    if (g.flow.states[s.state].type === 'final') return;
    const next = steps[k + 1];
    let line = titleOf(g.flow, s.state);
    if (next !== undefined) {
      const say = sayOf(g, next.taken);
      const press = next.event === null ? null : pressLabel(g, s.state, next.event);
      if (press !== null) line += `: the user presses “${press}”`;
      else if (say !== null) line += next.event?.startsWith('FAIL') === true ? `: it fails, because ${say}` : `: ${say}`;
    }
    lines.push(line);
  });
  const last = steps[steps.length - 1];
  return {
    given: parts.length > 0 ? capital(parts.join(', ')) : 'Any request',
    steps: lines,
    result: titleOf(g.flow, last.state),
  };
}
