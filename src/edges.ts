import { alternatives, humanize } from './graph.ts';
import type { Graph, State } from './graph.ts';
import type { Exploration } from './model.ts';

export type EdgeKind = 'user' | 'system' | 'route' | 'fail';

export interface VizEdge {
  from: string;
  to: string;
  labels: string[]; // plain words, one entry per merged transition
  kind: EdgeKind;
  leave: boolean; // ends in a state where the user leaves the page
  unused: boolean; // no reachable configuration ever takes it
}

export type NodeKind = 'server' | 'screen' | 'subflow' | 'exit';

export const BY: Record<string, string> = { human: 'person', ai: 'AI', agent: 'agent', ci: 'CI' };

export function kindOf(g: Graph, name: string): NodeKind {
  const st = g.flow.states[name];
  if (st.type === 'final') return 'exit';
  if (st.invoke?.flow !== undefined) return 'subflow';
  const shown = Object.values(g.ui.nodes).some((n) => (n.in ?? []).includes(name) && !(n.unless ?? []).includes(name));
  return shown ? 'screen' : 'server';
}

export function exitLine(st: State): string {
  if (st.leavesPage === true) return 'user leaves the page';
  if (st.redirect === 'never') return 'no redirect, page shown';
  if (st.redirect === 'success') return `redirect with ${(st.carries ?? []).join(' + ')}`;
  if (st.redirect === undefined && st.code === undefined) return 'ends here';
  const codes = (st.code ?? '').split('|').map((c) => c.trim()).filter((c) => c !== '');
  return codes.length === 1 ? `redirect, error=${codes[0]}` : `redirect, error (${codes.length} codes)`;
}

// a step's `response` in one line: the status, the code and the redirect first, then the other fields
export function responseLine(r: Record<string, unknown>): string {
  const first = ['status', 'code', 'redirect'].filter((k) => r[k] !== undefined).map((k) => (k === 'redirect' ? `redirect ${String(r[k])}` : String(r[k])));
  const rest = Object.entries(r)
    .filter(([k]) => !['status', 'code', 'redirect'].includes(k))
    .map(([k, v]) => `${k}: ${typeof v === 'string' ? v : JSON.stringify(v)}`);
  return [...first, ...rest].join(' · ');
}

export function nodeLine(g: Graph, name: string): string {
  const st = g.flow.states[name];
  const kind = kindOf(g, name);
  if (kind === 'exit') return exitLine(st);
  if (kind === 'subflow') return `sign-in flow · ${st.invoke?.flow}`;
  const by = st.by === undefined ? null : BY[st.by];
  if (st.goal === true) return `${name} · goal`;
  return `${name} · ${by ?? (kind === 'screen' ? 'screen' : 'server step')}`;
}

// visible text for a catalog key, with {placeholders} filled from the samples
export function plain(g: Graph, key: string): string {
  const raw = g.catalog[key] ?? key;
  return raw.replace(/\{(\w+)\}/g, (_m, p: string) => g.ui.samples[p] ?? p);
}

// the button or link text a person presses to send `event` while in `state`, if there is one
export function pressLabel(g: Graph, state: string, event: string): string | null {
  for (const n of Object.values(g.ui.nodes)) {
    if (n.emits !== event || !(n.in ?? []).includes(state)) continue;
    if (n.text?.key !== undefined) return plain(g, n.text.key);
  }
  return null;
}

export function wrap(text: string, max: number): string[] {
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(' ')) {
    if (line !== '' && line.length + 1 + word.length > max) {
      lines.push(line);
      line = word;
    } else {
      line = line === '' ? word : `${line} ${word}`;
    }
  }
  if (line !== '') lines.push(line);
  return lines;
}

interface Group {
  labels: string[];
  kinds: Set<EdgeKind>;
  used: boolean;
}

export function collectEdges(g: Graph, ex: Exploration): VizEdge[] {
  const groups = new Map<string, Group & { from: string; to: string }>();
  const add = (from: string, to: string, label: string | null, kind: EdgeKind, used: boolean): void => {
    const key = `${from}>${to}`;
    const grp = groups.get(key) ?? { from, to, labels: [], kinds: new Set<EdgeKind>(), used: false };
    if (label !== null && !grp.labels.includes(label)) grp.labels.push(label);
    grp.kinds.add(kind);
    grp.used = grp.used || used;
    groups.set(key, grp);
  };
  for (const [name, st] of Object.entries(g.flow.states)) {
    (st.always ?? []).forEach((a, i) => add(name, a.target, a.say ?? a.guard ?? 'otherwise', 'route', ex.taken.has(`${name}|always|${i}`)));
    for (const [event, spec] of Object.entries(st.on ?? {})) {
      alternatives(spec).forEach((a, i) => {
        const press = pressLabel(g, name, event);
        let label: string | null;
        let kind: EdgeKind;
        if (press !== null) {
          label = `“${press}”`;
          kind = 'user';
        } else if (event.startsWith('FAIL')) {
          label = a.say !== undefined ? `fails: ${a.say}` : humanize(event);
          kind = 'fail';
        } else {
          label = a.say ?? null;
          kind = 'system';
        }
        add(name, a.target, label, kind, ex.taken.has(`${name}|${event}|${i}`));
        if (a.starts !== undefined) add(name, a.target, `starts ${a.starts.flow}`, kind, ex.taken.has(`${name}|${event}|${i}`));
      });
    }
  }
  const order: EdgeKind[] = ['user', 'system', 'route', 'fail'];
  return [...groups.values()].map((grp) => ({
    from: grp.from,
    to: grp.to,
    labels: grp.labels,
    kind: order.find((k) => grp.kinds.has(k)) ?? 'system',
    leave: g.flow.states[grp.to].leavesPage === true,
    unused: !grp.used,
  }));
}

