// A domain (SPEC.md §1, §5) is every file in its directory that says `kind: domain`, read as one: `domain.yaml` first,
// then the others by name. `schema`, `kind` and `id` open every file and the id is the same in all; maps merge key by
// key at any depth; the top-level lists `policies` and `invariants` join in that order; any other value (a string, a
// number, a list) is given in one file only. A split lets a generated part and a hand-written part sit side by side.
import { isMap, isSeq, LineCounter, parse, parseDocument } from 'yaml';

export interface DomainPart {
  file: string; // as the caller names it; problems name it the same way
  text: string;
}

export interface DomainPlace {
  file: string;
  line: number;
  column: number;
}

export interface DomainProblem extends DomainPlace {
  rule: 'domain-twice' | 'domain-place';
  message: string;
  element: string; // the file's name and the JSON pointer of what it is about: user.yaml#/entities/User/fields/email/type
  first?: DomainPlace; // where the value was given first
}

export interface MergedDomain {
  id: string | null;
  doc: Record<string, unknown>;
  problems: DomainProblem[];
}

// the files whose name says another kind (SPEC.md §1); only the others are read to see whether they are a domain
export const OTHER_KINDS = new Set(['flow.yaml', 'ui.yaml', 'nav.yaml', 'nav.extracted.yaml', 'binding.yaml', 'tutorial.yaml', 'kit.yaml', 'anchors.lock.yaml']);
const OWN = new Set(['schema', 'kind', 'id']);
const JOINED = new Set(['policies', 'invariants']);

// a file that may be part of a domain, by its name
export const mayBeDomain = (name: string): boolean => name === 'domain.yaml' || (name.endsWith('.yaml') && !OTHER_KINDS.has(name));

// `kind: domain` at the top of the file; read from the text, so a file that is not YAML is still found and its own
// schema check says why
export const isDomainText = (text: string): boolean => /^kind:[ \t]*(domain|'domain'|"domain")[ \t]*(#.*)?$/m.test(text);

// the reading order of a domain's files: domain.yaml, then the others by name
export function inReadingOrder(names: string[]): string[] {
  const base = (n: string): string => n.slice(n.lastIndexOf('/') + 1);
  return [...names].sort((a, b) => (base(a) === 'domain.yaml' ? -1 : base(b) === 'domain.yaml' ? 1 : a < b ? -1 : a > b ? 1 : 0));
}

const isObject = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v);
const keyOf = (pair: { key: unknown }): string => String((pair.key as { value?: unknown } | null)?.value ?? pair.key);
const pointer = (path: string[]): string => path.map((s) => `/${s.replace(/~/g, '~0').replace(/\//g, '~1')}`).join('');
const nameOf = (file: string): string => file.slice(file.lastIndexOf('/') + 1);

// the 1-based line and column of the key at `path` in a YAML text (of the item, for a list index), else the file's start
function placeOf(text: string, path: string[]): { line: number; column: number } {
  const lineCounter = new LineCounter();
  const doc = parseDocument(text, { lineCounter, prettyErrors: false });
  let node: unknown = doc.contents;
  let at: number | undefined;
  for (const step of path) {
    if (isMap(node)) {
      const pair = node.items.find((p) => keyOf(p) === step);
      if (pair === undefined) break;
      at = (pair.key as { range?: [number, number, number] } | null)?.range?.[0] ?? at;
      node = pair.value;
    } else if (isSeq(node)) {
      node = node.items[Number(step)];
      at = (node as { range?: [number, number, number] } | null)?.range?.[0] ?? at;
    } else break;
  }
  const { line, col } = lineCounter.linePos(at ?? 0);
  return { line, column: col };
}

// the files of one directory, read as one domain; a file that is not YAML, or not a map, is left to its schema check
export function mergeDomain(parts: DomainPart[]): MergedDomain {
  const doc: Record<string, unknown> = {};
  const problems: DomainProblem[] = [];
  const origin = new Map<string, number>(); // the pointer of a value given -> the part that gave it
  const invariants = new Map<string, { part: number; index: number }>();
  let id: string | null = null;
  let idFrom = -1;

  const where = (i: number, path: string[]): DomainPlace => ({ file: parts[i].file, ...placeOf(parts[i].text, path) });
  // the part that gave the value at `path`, or the map it sits in
  const giver = (path: string[]): number => {
    for (let n = path.length; n > 0; n--) {
      const o = origin.get(pointer(path.slice(0, n)));
      if (o !== undefined) return o;
    }
    return 0;
  };
  const twice = (i: number, path: string[], first: number, said: string): void => {
    const at = where(i, path);
    const firstAt = where(first, path);
    problems.push({ rule: 'domain-twice', ...at, first: firstAt, message: `${said} is given in ${nameOf(parts[first].file)}:${firstAt.line} too: a domain's files give each value once (SPEC.md §5)`, element: `${nameOf(parts[i].file)}#${pointer(path)}` });
  };
  const put = (target: Record<string, unknown>, key: string, value: unknown, path: string[], i: number): void => {
    if (!(key in target)) {
      target[key] = value;
      origin.set(pointer(path), i);
      return;
    }
    const here = target[key];
    if (isObject(here) && isObject(value)) {
      for (const [k, v] of Object.entries(value)) put(here, k, v, [...path, k], i);
      return;
    }
    twice(i, path, giver(path), path.join('.'));
  };

  parts.forEach((part, i) => {
    let data: unknown;
    try {
      data = parse(part.text);
    } catch {
      return;
    }
    if (!isObject(data)) return;
    if (typeof data.id === 'string') {
      if (id === null) {
        id = data.id;
        idFrom = i;
      } else if (data.id !== id) {
        problems.push({ rule: 'domain-place', ...where(i, ['id']), message: `the id is ${data.id}, and ${nameOf(parts[idFrom].file)} beside it is ${id}: a directory holds one domain, every file of it with its id (SPEC.md §1)`, element: `${nameOf(part.file)}#/id` });
        return;
      }
    }
    for (const [key, value] of Object.entries(data)) {
      if (OWN.has(key)) {
        doc[key] ??= value;
        continue;
      }
      if (JOINED.has(key) && Array.isArray(value) && (doc[key] === undefined || Array.isArray(doc[key]))) {
        if (doc[key] === undefined) origin.set(pointer([key]), i);
        doc[key] = [...((doc[key] as unknown[] | undefined) ?? []), ...value];
        if (key !== 'invariants') continue;
        value.forEach((inv, index) => {
          const name = isObject(inv) && typeof inv.id === 'string' ? inv.id : null;
          if (name === null) return;
          const seen = invariants.get(name);
          if (seen === undefined) invariants.set(name, { part: i, index });
          else if (seen.part !== i) {
            const at = where(i, ['invariants', String(index), 'id']);
            const firstAt = where(seen.part, ['invariants', String(seen.index), 'id']);
            problems.push({ rule: 'domain-twice', ...at, first: firstAt, message: `the invariant ${name} is given in ${nameOf(parts[seen.part].file)}:${firstAt.line} too: a domain's files give each invariant once (SPEC.md §5)`, element: `${nameOf(part.file)}#/invariants/${index}` });
          }
        });
        continue;
      }
      put(doc, key, value, [key], i);
    }
  });
  return { id, doc, problems };
}
