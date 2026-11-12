// `changes:` in ui.yaml (SPEC.md §4): a flow whose steps write something tells its users, in its own note, what it
// changes and what follows. An action writes when its route or its server function is not a GET, when a policy lets
// someone `write` with it, or when it is a step inside the service (neither a route nor a server function) that returns
// an entity; all three are read from the flow's domain, looked up by id among the graphs beside this one, all its files
// read as one (SPEC.md §5). A flow with no screens, or whose domain is not found, is not checked.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import type { Graph } from './graph.ts';
import { inReadingOrder, isDomainText, mayBeDomain, mergeDomain } from './domain.ts';

export interface DomainAction {
  route?: string;
  serverFn?: string;
  result?: { returns?: string };
}

export interface Domain {
  id?: string;
  actions?: Record<string, DomainAction | null>;
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
  const names = readdirSync(root);
  const parts = inReadingOrder(names.filter((name) => mayBeDomain(name) && statSync(join(root, name)).isFile()))
    .map((name) => ({ file: join(root, name), text: readFileSync(join(root, name), 'utf8') }))
    .filter((p) => isDomainText(p.text));
  if (parts.length > 0) {
    const merged = mergeDomain(parts);
    if (merged.id === id) return merged.doc as Domain;
  }
  if (depth === 0) return null;
  for (const name of names) {
    if (SKIP.has(name)) continue;
    const sub = join(root, name);
    if (!statSync(sub).isDirectory()) continue;
    const found = domainUnder(sub, id, depth - 1);
    if (found !== null) return found;
  }
  return null;
}

// the domain the flow names, looked up by id among the graphs beside it; null when it names none or it is not found
export function flowDomain(g: Graph): Domain | null {
  return g.flow.domain === undefined ? null : domainUnder(rootOf(g.dir), g.flow.domain);
}

// the HTTP method of an action's route or server function; undefined for a step inside the service
const methodOf = (a: DomainAction | null | undefined): string | undefined => (a?.route ?? a?.serverFn)?.trim().split(/\s+/)[0]?.toUpperCase();

export function checkChanges(g: Graph): ChangesCheck | null {
  if (g.ui.absent === true) return null;
  const domain = flowDomain(g);
  if (domain === null) return null;
  const writing = new Set<string>();
  for (const [name, a] of Object.entries(domain.actions ?? {})) {
    const method = methodOf(a);
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
