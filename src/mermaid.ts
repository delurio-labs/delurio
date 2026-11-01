import { collectEdges, exitLine, kindOf } from './edges.ts';
import { titleOf } from './graph.ts';
import type { Graph } from './graph.ts';
import type { Exploration } from './model.ts';

const clean = (s: string): string => s.replace(/"/g, "'").replace(/:/g, ' -');

// Mermaid's ids, which take no dot: an exit named `<error>.<cause>` is written `<error>__<cause>`
function mermaidIds(names: string[]): (name: string) => string {
  const ids = new Map<string, string>();
  const taken = new Set(names.filter((n) => !n.includes('.')));
  for (const n of names) {
    if (!n.includes('.')) {
      ids.set(n, n);
      continue;
    }
    let id = n.replace(/\./g, '__');
    for (let i = 2; taken.has(id); i++) id = `${n.replace(/\./g, '__')}_${i}`;
    taken.add(id);
    ids.set(n, id);
  }
  return (name) => ids.get(name) ?? name;
}

// Mermaid stateDiagram-v2 text, for PRs and READMEs. The full page is html.ts.
export function renderMermaid(g: Graph, ex: Exploration): string {
  const lines: string[] = ['---', `title: ${clean(g.flow.title ?? g.flow.id)}`, '---', 'stateDiagram-v2', '  direction TB'];
  const names = Object.keys(g.flow.states);
  const id = mermaidIds(names);
  for (const name of names) {
    const st = g.flow.states[name];
    const detail = st.type === 'final' && st.leavesPage !== true ? ` (${exitLine(st)})` : '';
    lines.push(`  state "${clean(titleOf(g.flow, name))}${clean(detail)}" as ${id(name)}`);
  }
  lines.push(`  [*] --> ${id(g.flow.initial)}`);
  for (const e of collectEdges(g, ex)) {
    const label = e.labels.map(clean).join(', ');
    lines.push(`  ${id(e.from)} --> ${id(e.to)}${label === '' ? '' : `: ${label}`}${e.unused ? ' (never taken)' : ''}`);
  }
  lines.push('  classDef server fill:#eeeeec,stroke:#6b6560,stroke-dasharray:4 3');
  lines.push('  classDef screen fill:#ffffff,stroke:#1c1917,stroke-width:2px');
  lines.push('  classDef subflow fill:#ffffff,stroke:#1d4ed8,stroke-width:2px');
  lines.push('  classDef exit fill:#ffffff,stroke:#6b6560,stroke-width:2px');
  for (const kind of ['server', 'screen', 'subflow', 'exit'] as const) {
    const members = names.filter((n) => kindOf(g, n) === kind);
    if (members.length > 0) lines.push(`  class ${members.map(id).join(',')} ${kind}`);
  }
  return lines.join('\n') + '\n';
}
