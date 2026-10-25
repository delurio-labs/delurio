import { collectEdges, exitLine, kindOf } from './edges.ts';
import { titleOf } from './graph.ts';
import type { Graph } from './graph.ts';
import type { Exploration } from './model.ts';

const clean = (s: string): string => s.replace(/"/g, "'").replace(/:/g, ' -');

// Mermaid stateDiagram-v2 text, for PRs and READMEs. The full page is html.ts.
export function renderMermaid(g: Graph, ex: Exploration): string {
  const lines: string[] = ['---', `title: ${clean(g.flow.title ?? g.flow.id)}`, '---', 'stateDiagram-v2', '  direction TB'];
  const names = Object.keys(g.flow.states);
  for (const name of names) {
    const st = g.flow.states[name];
    const detail = st.type === 'final' && st.leavesPage !== true ? ` (${exitLine(st)})` : '';
    lines.push(`  state "${clean(titleOf(g.flow, name))}${clean(detail)}" as ${name}`);
  }
  lines.push(`  [*] --> ${g.flow.initial}`);
  for (const e of collectEdges(g, ex)) {
    const label = e.labels.map(clean).join(', ');
    lines.push(`  ${e.from} --> ${e.to}${label === '' ? '' : `: ${label}`}${e.unused ? ' (never taken)' : ''}`);
  }
  lines.push('  classDef server fill:#eeeeec,stroke:#6b6560,stroke-dasharray:4 3');
  lines.push('  classDef screen fill:#ffffff,stroke:#1c1917,stroke-width:2px');
  lines.push('  classDef subflow fill:#ffffff,stroke:#1d4ed8,stroke-width:2px');
  lines.push('  classDef exit fill:#ffffff,stroke:#6b6560,stroke-width:2px');
  for (const kind of ['server', 'screen', 'subflow', 'exit'] as const) {
    const members = names.filter((n) => kindOf(g, n) === kind);
    if (members.length > 0) lines.push(`  class ${members.join(',')} ${kind}`);
  }
  return lines.join('\n') + '\n';
}
