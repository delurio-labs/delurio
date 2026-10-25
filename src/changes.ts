// `changes:` in ui.yaml (SPEC.md §4): a flow whose steps write something tells its users, in its own note, what it
// changes and what follows. An action writes when its route is not a GET, when a policy lets someone `write` with it, or
// when it is a step inside the service (no route) that returns an entity; all three are read from the flow's domain,
// looked up by id among the graphs beside this one. A flow with no screens, or whose domain is not found, is not checked.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { parse } from 'yaml';
import type { Graph } from './graph.ts';

interface Domain {
  id?: string;
  actions?: Record<string, { route?: string; result?: { returns?: string } } | null>;
  policies?: Array<{ do?: string[]; write?: string[] }>;
}

export interface ChangesCheck {
  writes: string[]; // the actions the flow invokes that write
  key: string | null;
  problems: string[];
}

const SKIP = new Set(['viz', 'video', 'node_modules', '.git']);

// the root to search: the nearest folder called `graphs` above the graph, else the graph's parent
function rootOf(dir: string): string {
  for (let d = dir, i = 0; i < 5; i++, d = dirname(d)) if (basename(d) === 'graphs') return d;
  return dirname(dir);
}

function domainUnder(root: string, id: string, depth = 4): Domain | null {
  const file = join(root, 'domain.yaml');
  if (existsSync(file)) {
    const d = parse(readFileSync(file, 'utf8')) as Domain;
    if (d?.id === id) return d;
  }
  if (depth === 0) return null;
  for (const name of readdirSync(root)) {
    if (SKIP.has(name)) continue;
    const sub = join(root, name);
    if (!statSync(sub).isDirectory()) continue;
    const found = domainUnder(sub, id, depth - 1);
    if (found !== null) return found;
  }
  return null;
}

export function checkChanges(g: Graph): ChangesCheck | null {
  if (g.ui.absent === true || g.flow.domain === undefined) return null;
  const domain = domainUnder(rootOf(g.dir), g.flow.domain);
  if (domain === null) return null;
  const writing = new Set<string>();
  for (const [name, a] of Object.entries(domain.actions ?? {})) {
    const method = a?.route?.trim().split(/\s+/)[0]?.toUpperCase();
    if ((method !== undefined && method !== 'GET') || (method === undefined && a?.result?.returns !== undefined)) writing.add(name);
  }
  for (const p of domain.policies ?? []) if ((p.write ?? []).length > 0) for (const name of p.do ?? []) writing.add(name);
  const invoked = Object.values(g.flow.states).map((s) => s.invoke?.action);
  const writes = [...new Set(invoked.filter((a): a is string => a !== undefined && writing.has(a)))].sort();
  const key = g.ui.changes?.key ?? null;
  const problems: string[] = [];
  if (writes.length > 0 && key === null) problems.push(`its steps write (${writes.join(', ')}), but ui.yaml has no changes note (SPEC.md §4)`);
  if (key !== null) {
    for (const locale of g.ui.locales ?? [basename(g.ui.catalog, '.json')]) {
      const file = join(g.dir, dirname(g.ui.catalog), `${locale}.json`);
      const catalog = existsSync(file) ? (JSON.parse(readFileSync(file, 'utf8')) as Record<string, unknown>) : null;
      const note = catalog?.[key];
      if (typeof note !== 'string' || note.trim() === '') problems.push(`the changes note ${key} is not in the ${locale} catalog`);
    }
  }
  return { writes, key, problems };
}
