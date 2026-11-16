// `changes:` in ui.yaml (SPEC.md §4): a flow whose steps write something tells its users, in its own note, what it
// changes and what follows. An action writes when its route or its server function is not a GET, when a policy lets
// someone `write` with it, or when it is a step inside the service (neither a route nor a server function) that returns
// an entity; all three are read from the flow's domain, looked up by id among the graphs beside this one, all its files
// read as one (SPEC.md §5). A GET that a policy lists is held to what it does: it writes only when one of its
// `sideEffects` is more than an audit record of the call (`audit:<event>`), as a policy names reads and writes together
// (SPEC.md §4, decided on 16 Nov 2026, d-1116-12). A flow with no screens, or whose domain is not found, is not checked.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import type { Graph } from './graph.ts';
import { inReadingOrder, isDomainText, mayBeDomain, mergeDomain } from './domain.ts';

export interface DomainAction {
  route?: string;
  serverFn?: string;
  result?: { returns?: string };
  sideEffects?: unknown[];
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

// the domain the flow names, looked up by id among the graphs beside it; null when it names none or it is not found.
// `seen` keeps what a run has found, so that the flows that name one domain look it up once
export function flowDomain(g: Graph, seen?: Map<string, Domain | null>): Domain | null {
  if (g.flow.domain === undefined) return null;
  const root = rootOf(g.dir);
  const key = `${root}\0${g.flow.domain}`;
  if (seen?.has(key) === true) return structuredClone(seen.get(key) as Domain | null);
  const found = domainUnder(root, g.flow.domain);
  seen?.set(key, found);
  return seen === undefined ? found : structuredClone(found);
}

// the HTTP method of an action's route or server function; undefined for a step inside the service
const methodOf = (a: DomainAction | null | undefined): string | undefined => (a?.route ?? a?.serverFn)?.trim().split(/\s+/)[0]?.toUpperCase();

// a side effect that records the call and changes nothing the flow's users are told of: `audit:<event>` (SPEC.md §5)
const isAudit = (s: unknown): boolean => typeof s === 'string' && s.trimStart().startsWith('audit:');

// a GET that a writing policy lists writes only when it does more than record that it was called: a policy names its
// actor's reads and writes together, and a GET reads (SPEC.md §4)
const writesThroughPolicy = (a: DomainAction | null | undefined): boolean => methodOf(a) !== 'GET' || (Array.isArray(a?.sideEffects) && a.sideEffects.some((s) => !isAudit(s)));

// `domainOf` finds the flow's domain, called at most once: a caller that reads the domain for another rule as well
// hands over the one it found
export function checkChanges(g: Graph, domainOf: () => Domain | null = () => flowDomain(g)): ChangesCheck | null {
  if (g.ui.absent === true) return null;
  const domain = domainOf();
  if (domain === null) return null;
  const writing = new Set<string>();
  for (const [name, a] of Object.entries(domain.actions ?? {})) {
    const method = methodOf(a);
    if ((method !== undefined && method !== 'GET') || (method === undefined && a?.result?.returns !== undefined)) writing.add(name);
  }
  for (const p of domain.policies ?? []) if ((p.write ?? []).length > 0) for (const name of p.do ?? []) if (writesThroughPolicy(domain.actions?.[name])) writing.add(name);
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
