// The index of `page` on a folder of graphs: one row per graph, grouped by the folder it sits in, with what its check
// found and a link to its page.
import { CSS } from './html.ts';
import { count } from './check.ts';

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

export interface IndexEntry {
  path: string; // the graph's folder relative to the indexed one, with forward slashes
  kind: 'flow' | 'nav';
  id: string | null; // null when the graph cannot be loaded
  title: string;
  status: string | null;
  holds: number; // the flow's guarantees that hold
  total: number; // the flow's guarantees; for a navigation graph, its problems
  warnings: string[];
  error: string | null; // why the graph cannot be loaded
}

const INDEX_CSS = `
.groups h2 { font-size: 18px; margin: 32px 0 10px; } .groups table { table-layout: fixed; } .groups col.check { width: 9em; }
.groups a { color: var(--accent); } .groups td { overflow-wrap: anywhere; }
.meta { margin-top: 2px; } .meta .tag { margin-left: 6px; } .warn { margin-top: 6px; color: var(--muted); font-size: 14px; } .warn ul { margin: 4px 0 0; padding-left: 18px; }
`;

function stateCell(e: IndexEntry): string {
  if (e.error !== null) return `<span class="bad">cannot be loaded</span>`;
  if (e.kind === 'nav') return `<span class="${e.total === 0 ? 'ok' : 'bad'}">${esc(count(e.total, 'problem'))}</span>`;
  if (e.total === 0) return '<span class="muted">no guarantees</span>';
  return e.holds === e.total ? `<span class="ok">${e.holds}/${e.total} hold</span>` : `<span class="bad">${e.total - e.holds} of ${e.total} broken</span>`;
}

// root: the indexed folder's name, which also heads the graphs that sit right in it
export function renderIndexPage(root: string, entries: IndexEntry[]): string {
  const out: string[] = [];
  const failing = entries.filter((e) => e.error !== null || (e.kind === 'flow' ? e.holds < e.total : e.total > 0));
  const flows = entries.filter((e) => e.kind === 'flow').length;
  const navs = entries.length - flows;
  out.push('<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">');
  out.push(`<title>All graphs</title><style>${CSS}${INDEX_CSS}</style></head><body><main>`);
  out.push(`<p class="eyebrow"><span class="mono">${esc(root)}/</span> · delurio-graph/0</p><h1>All graphs</h1>`);
  const kinds = [flows > 0 ? count(flows, 'flow') : '', navs > 0 ? count(navs, 'navigation graph') : ''].filter((s) => s !== '').join(' and ');
  out.push(
    `<p class="lede">${esc(kinds)}. ${
      failing.length === 0 ? 'Every guarantee holds and nothing blocks anyone.' : `${esc(count(failing.length, 'graph'))} need${failing.length === 1 ? 's' : ''} a look: marked in red below.`
    }</p>`,
  );
  const groups = new Map<string, IndexEntry[]>();
  for (const e of entries) {
    const slash = e.path.lastIndexOf('/');
    const folder = slash < 0 ? root : e.path.slice(0, slash);
    groups.set(folder, [...(groups.get(folder) ?? []), e]);
  }
  out.push('<div class="groups">');
  for (const [folder, rows] of groups) {
    out.push(`<h2>${esc(folder)}</h2><div class="scroll"><table><colgroup><col><col class="check"></colgroup><thead><tr><th>Graph</th><th>Check</th></tr></thead><tbody>`);
    for (const e of rows) {
      const name = e.error !== null ? esc(e.title) : `<a href="${esc(e.path)}/index.html">${esc(e.title)}</a>`;
      const meta = e.id === null ? '' : `<div class="meta"><span class="mono muted">${esc(e.id)}</span>${e.status === null ? '' : `<span class="tag">${esc(e.status)}</span>`}</div>`;
      const warn = e.error !== null ? `<div class="warn">${esc(e.error)}</div>` : e.warnings.length === 0 ? '' : `<div class="warn">${esc(count(e.warnings.length, 'warning'))}<ul>${e.warnings.map((w) => `<li>${esc(w)}</li>`).join('')}</ul></div>`;
      out.push(`<tr><td>${name}${meta}${warn}</td><td class="state">${stateCell(e)}</td></tr>`);
    }
    out.push('</tbody></table></div>');
  }
  out.push('</div></main></body></html>');
  return out.join('\n');
}
