// What the revision diff is made of, for both kinds of graph: structural changes keyed by id, and changes in behaviour.
// Nothing here knows what a flow or a page is; diff-flow.ts and diff-nav.ts do.

export type Op = 'added' | 'removed' | 'changed' | 'moved';

// One structural change. `say` is the sentence for people (titles and `say` texts, no ids); null when the change is only
// listed under the exact changes (a source citation, a key graph-viz does not read) and counted in the area's summary line.
export interface Change {
  area: string;
  key: string; // what changed, by id: "state consent", "consent on ALLOW", "page dersPlan"
  op: Op;
  field?: string;
  before?: unknown;
  after?: unknown;
  say: string | null;
}

// One change in what the graph does, found by running both revisions.
export interface Shift {
  topic: string; // exit, path, guarantee, situations, main, reach, taken; nav: landing, page, clicks, menu, goal, finding
  key: string;
  persona?: string; // nav: the kind of user it is about (title)
  before: unknown;
  after: unknown;
  say: string;
  detail: string[]; // the steps of a path, in words
  worse: boolean; // a regression: a guarantee that newly fails, a goal that newly fails, a problem that is new
}

const own = (o: object, k: string): boolean => Object.prototype.hasOwnProperty.call(o, k);
const isMap = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v);

// a value as text with sorted keys, so two values written in a different key order compare equal
export function canon(v: unknown): string {
  if (v === undefined) return '~';
  if (Array.isArray(v)) return `[${v.map(canon).join(',')}]`;
  if (isMap(v)) {
    const keys = Object.keys(v).filter((k) => v[k] !== undefined).sort();
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canon(v[k])}`).join(',')}}`;
  }
  return JSON.stringify(v);
}

export const same = (a: unknown, b: unknown): boolean => canon(a) === canon(b);

// the keys of both maps: the new order first, then the keys only the old one has, in its order
export function union(before: object | undefined, after: object | undefined): string[] {
  const out = Object.keys(after ?? {});
  for (const k of Object.keys(before ?? {})) if (!own(after ?? {}, k)) out.push(k);
  return out;
}

export const quote = (s: string): string => `“${s}”`;
export const joinWords = (xs: string[]): string => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} and ${xs[xs.length - 1]}`);

// Pairs the items of two ordered lists (the rules of an `always`, the alternatives of one event, the entries of a nav graph).
// The first key pairs items in the order they occur (the n-th with the n-th); a later key pairs what is left only when it is
// unique on both sides, so nothing is paired by guesswork. Returns [before index, after index] pairs, sorted by before index.
export function matchOrdered<T>(before: T[], after: T[], keys: Array<(x: T) => string | null>): Array<[number, number]> {
  const pairs: Array<[number, number]> = [];
  const usedB = new Set<number>();
  const usedA = new Set<number>();
  keys.forEach((key, round) => {
    const group = (xs: T[], used: Set<number>): Map<string, number[]> => {
      const m = new Map<string, number[]>();
      xs.forEach((x, i) => {
        if (used.has(i)) return;
        const k = key(x);
        if (k !== null) m.set(k, [...(m.get(k) ?? []), i]);
      });
      return m;
    };
    const gb = group(before, usedB);
    const ga = group(after, usedA);
    for (const [k, bs] of gb) {
      const as = ga.get(k);
      if (as === undefined) continue;
      if (round > 0 && (bs.length > 1 || as.length > 1)) continue;
      for (let j = 0; j < Math.min(bs.length, as.length); j++) {
        pairs.push([bs[j], as[j]]);
        usedB.add(bs[j]);
        usedA.add(as[j]);
      }
    }
  });
  return pairs.sort((x, y) => x[0] - y[0]);
}

// The pairs that changed place relative to the others: the pairs outside the longest run that keeps its order.
// An item that only shifted because something was added or removed before it is not moved.
export function moved(pairs: Array<[number, number]>): Array<[number, number]> {
  const sorted = [...pairs].sort((x, y) => x[0] - y[0]);
  const n = sorted.length;
  const len = new Array<number>(n).fill(1);
  const prev = new Array<number>(n).fill(-1);
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < i; j++) {
      if (sorted[j][1] < sorted[i][1] && len[j] + 1 > len[i]) {
        len[i] = len[j] + 1;
        prev[i] = j;
      }
    }
  }
  let best = -1;
  for (let i = 0; i < n; i++) if (best < 0 || len[i] > len[best]) best = i;
  const keep = new Set<number>();
  for (let i = best; i >= 0; i = prev[i]) keep.add(i);
  return sorted.filter((_, i) => !keep.has(i));
}

// A field-by-field comparison for data graph-viz does not interpret (domain.yaml, unknown keys): maps by key, lists whose
// items all carry an `id` by id, anything else as one value. A field called `source` or `evidence` is a citation.
export function deepDiff(area: string, key: string, before: unknown, after: unknown, out: Change[], path: string[] = []): void {
  if (same(before, after)) return;
  if (isMap(before) && isMap(after)) {
    for (const k of union(before, after)) deepDiff(area, key, before[k], after[k], out, [...path, k]);
    return;
  }
  const byId = (v: unknown): v is Array<{ id: string }> => Array.isArray(v) && v.every((x) => isMap(x) && typeof x.id === 'string');
  if (byId(before) && byId(after) && before.length + after.length > 0) {
    const mb = Object.fromEntries(before.map((x) => [x.id, x]));
    const ma = Object.fromEntries(after.map((x) => [x.id, x]));
    if (Object.keys(mb).length === before.length && Object.keys(ma).length === after.length) {
      for (const id of union(mb, ma)) deepDiff(area, key, mb[id], ma[id], out, [...path, id]);
      return;
    }
  }
  const last = path[path.length - 1];
  out.push({
    area: last === 'source' || last === 'evidence' ? 'citations' : area,
    key,
    op: before === undefined ? 'added' : after === undefined ? 'removed' : 'changed',
    field: path.length === 0 ? undefined : path.join('.'),
    before,
    after,
    say: null,
  });
}

// the plain old → new comparison of named fields of one item; `say` words the change for people, or gives null
export function fieldChanges(
  area: string,
  key: string,
  before: Record<string, unknown>,
  after: Record<string, unknown>,
  fields: string[],
  say: (field: string, b: unknown, a: unknown) => string | null,
): Change[] {
  const out: Change[] = [];
  for (const f of fields) {
    if (same(before[f], after[f])) continue;
    const citation = f === 'source' || f === 'evidence';
    out.push({ area: citation ? 'citations' : area, key, op: 'changed', field: f, before: before[f], after: after[f], say: citation ? null : say(f, before[f], after[f]) });
  }
  return out;
}

// the keys of an item that are not in `known`, compared as data
export function otherKeys(key: string, before: Record<string, unknown>, after: Record<string, unknown>, known: string[], out: Change[]): void {
  for (const k of union(before, after)) {
    if (known.includes(k)) continue;
    deepDiff('other', key, before[k], after[k], out, [k]);
  }
}

// added and removed items of two lists of plain words
export function listChanges(before: string[] | undefined, after: string[] | undefined): { added: string[]; removed: string[] } {
  const b = before ?? [];
  const a = after ?? [];
  return { added: a.filter((x) => !b.includes(x)), removed: b.filter((x) => !a.includes(x)) };
}
