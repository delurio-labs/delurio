// The JSON Schemas of schemas/ (one per kind of graph file, SPEC.md §12) as `check` applies them: a file is validated
// against its kind's schema, and each failure is placed at its line and column through the YAML document and said in
// words a writer can act on: the key that is not one, the key it was probably meant to be, why an if/then rule exists
// (its `$comment`), and the comma that cut a value inside `{ }`. The schemas are read from the package's own folder,
// beside src/ and dist/ alike.
import { existsSync, readFileSync } from 'node:fs';
import { basename, dirname, join } from 'node:path';
import { Ajv2020 } from 'ajv/dist/2020.js';
import type { ErrorObject, ValidateFunction } from 'ajv/dist/2020.js';
import addFormatsModule from 'ajv-formats';
import { isMap, isSeq, LineCounter, parse, parseDocument } from 'yaml';
import type { Document, YAMLMap } from 'yaml';

export type SchemaKind = 'flow' | 'ui' | 'domain' | 'nav' | 'catalog';

export interface SchemaFinding {
  line: number;
  column: number;
  message: string;
  graph: string | null; // the graph's id, when the file or the graph it belongs to says it
  element: string; // the file within its graph and the JSON pointer of what failed: ui.yaml#/nodes/deny/onClick
}

const FORMAT = 'delurio-graph/0';
const KINDS: SchemaKind[] = ['flow', 'ui', 'domain', 'nav', 'catalog'];

// the kind of graph file, by its name: null for a file no schema covers (binding, tutorial, kit, anchors.lock)
export function schemaKindOf(file: string): SchemaKind | null {
  const name = basename(file);
  if (name === 'flow.yaml' || name === 'ui.yaml' || name === 'domain.yaml') return name.slice(0, -'.yaml'.length) as SchemaKind;
  if (name === 'nav.yaml' || name === 'nav.extracted.yaml') return 'nav';
  if (name.endsWith('.json') && basename(dirname(file)) === 'catalog') return 'catalog';
  return null;
}

type Schema = Record<string, unknown>;
interface Failure extends ErrorObject {
  because?: string; // why the rule exists, from the $comment of the if/then that gave the failure
  alternative?: boolean; // a `required` of one alternative of an anyOf or a oneOf
}

let compiled: { roots: Record<SchemaKind, Schema>; validators: Record<SchemaKind, ValidateFunction> } | null = null;

// compiled once, on first use; strictRequired is off because an if/then names keys its branch does not declare
function schemas(): NonNullable<typeof compiled> {
  if (compiled !== null) return compiled;
  const ajv = new Ajv2020({ allErrors: true, verbose: true, strict: true, strictRequired: false, allowUnionTypes: true });
  addFormatsModule.default(ajv);
  const roots = {} as Record<SchemaKind, Schema>;
  const validators = {} as Record<SchemaKind, ValidateFunction>;
  for (const kind of KINDS) roots[kind] = JSON.parse(readFileSync(new URL(`../schemas/${kind}.schema.json`, import.meta.url), 'utf8')) as Schema;
  for (const kind of KINDS) validators[kind] = ajv.compile(roots[kind]);
  compiled = { roots, validators };
  return compiled;
}

const steps = (pointer: string): string[] => (pointer === '' ? [] : pointer.slice(1).split('/').map((t) => t.replace(/~1/g, '/').replace(/~0/g, '~')));
const escape = (step: string): string => step.replace(/~/g, '~0').replace(/\//g, '~1');
const within = (path: string, base: string): boolean => path === base || path.startsWith(`${base}/`);
const keyOf = (pair: { key: unknown }): string => String((pair.key as { value?: unknown } | null)?.value ?? pair.key);

// the YAML node at a JSON pointer; with `name`, the key `name` of the map there, that map and the key's value
function place(doc: Document, pointer: string, name: string | undefined): { at: unknown; map?: YAMLMap; value?: unknown } {
  let node: unknown = doc.contents;
  for (const t of steps(pointer)) {
    const next = isMap(node) ? node.items.find((p) => keyOf(p) === t)?.value : isSeq(node) ? node.items[Number(t)] : undefined;
    if (next === undefined || next === null) break;
    node = next;
  }
  if (name === undefined || !isMap(node)) return { at: node };
  const pair = node.items.find((p) => keyOf(p) === name);
  return { at: pair?.key ?? node, map: node, value: pair?.value };
}

// a schema and the ones its `$ref`s name, inside the same file
function chain(root: Schema, schema: unknown): Schema[] {
  const out: Schema[] = [];
  for (let s = schema; s !== null && typeof s === 'object' && out.length < 8; ) {
    const here = s as Schema;
    out.push(here);
    const ref = here.$ref;
    s = typeof ref === 'string' && ref.startsWith('#/') ? ref.slice(2).split('/').reduce<unknown>((x, k) => (x as Schema | undefined)?.[k], root) : undefined;
  }
  return out;
}

// edits between two names, a swap of two neighbours counting as one (titel, title)
function distance(a: string, b: string): number {
  const d = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array<number>(b.length).fill(0)]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
    }
  }
  return d[a.length][b.length];
}

const KIND_WORDS: Record<string, string> = { object: 'an object', array: 'a list', string: 'a string', integer: 'an integer', number: 'a number', boolean: 'true or false', null: 'null' };
const either = (xs: string[]): string => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} or ${xs[xs.length - 1]}`);
const quoted = (xs: unknown[]): string => either(xs.map((x) => JSON.stringify(x)));

function message(e: Failure, root: Schema): string {
  const p = e.params as Record<string, unknown>;
  switch (e.keyword) {
    case 'additionalProperties':
    case 'unevaluatedProperties': {
      const name = String(p.additionalProperty ?? p.unevaluatedProperty);
      const known = chain(root, e.parentSchema).flatMap((x) => Object.keys((x.properties as Schema | undefined) ?? {}));
      const near = known.filter((k) => distance(k.toLowerCase(), name.toLowerCase()) <= Math.min(2, Math.max(1, Math.floor(k.length / 3))));
      return `"${name}" is not a key here${near.length > 0 ? `; did you mean ${quoted(near)}?` : ''}`;
    }
    case 'required':
      return `${(p.missingProperty as string[]).map((k) => JSON.stringify(k)).join(' or ')} is missing`;
    case 'enum': {
      const allowed = p.allowedValues as unknown[];
      return allowed.length <= 12 ? `must be ${quoted(allowed)}` : `${JSON.stringify(e.data)} is not one of its ${allowed.length} values`;
    }
    case 'const':
      return `must be ${JSON.stringify(p.allowedValue)}`;
    case 'type':
      return `must be ${either((p.type as string[]).map((t) => KIND_WORDS[t] ?? t))}`;
    case 'pattern':
      return `${JSON.stringify(e.data)} does not match ${String(p.pattern)}`;
    case 'propertyNames': {
      const said = chain(root, (e.parentSchema as Schema | undefined)?.propertyNames).find((x) => typeof x.description === 'string')?.description as string | undefined;
      return `the name "${String(p.propertyName)}" is not allowed${said === undefined ? '' : `: ${said.split('. ')[0].replace(/\.$/, '')}`}`;
    }
    case 'false schema':
      return 'is not allowed here';
    case 'oneOf':
      return 'matches more than one of its forms';
    case 'minItems':
    case 'maxItems':
    case 'minProperties':
    case 'maxProperties': {
      const n = Number(p.limit);
      const what = e.keyword.endsWith('Items') ? (n === 1 ? 'item' : 'items') : n === 1 ? 'key' : 'keys';
      return e.keyword.startsWith('min') ? `needs at least ${n} ${what}` : `has more than ${n} ${what}`;
    }
    default:
      return e.message ?? e.keyword;
  }
}

// Ajv reports every alternative of an anyOf and every rule around a then; keep what the writer can act on
function simplify(errors: Failure[]): Failure[] {
  // the reason for an if/then rule, from its $comment, goes onto the errors its then or else gave (just before it)
  errors.forEach((e, i) => {
    const comment = (e.parentSchema as Schema | undefined)?.$comment;
    if (e.keyword !== 'if' || typeof comment !== 'string') return;
    const base = e.schemaPath.slice(0, -'/if'.length);
    for (let j = i - 1; j >= 0; j--) {
      const x = errors[j];
      if (!within(x.instancePath, e.instancePath) || !(x.schemaPath.startsWith(`${base}/then/`) || x.schemaPath.startsWith(`${base}/else/`))) break;
      x.because = comment;
    }
  });
  const wrapper = (x: Failure): boolean => ['if', 'anyOf', 'oneOf'].includes(x.keyword);
  const explained = (e: Failure): boolean =>
    errors.some((x) => x !== e && !wrapper(x) && ((x.instancePath !== e.instancePath && within(x.instancePath, e.instancePath)) || (x.instancePath === e.instancePath && x.keyword !== 'type')));
  const kept = errors.filter((e) => {
    if (e.keyword === 'if') return false; // its then or else said it
    if (e.keyword === 'anyOf' || (e.keyword === 'oneOf' && (e.params as { passingSchemas?: unknown }).passingSchemas === null)) {
      return !explained(e) && !errors.some((x) => x !== e && x.keyword === 'type' && x.instancePath === e.instancePath);
    }
    if (e.keyword === 'type') return !explained(e); // an alternative of the right type is there, and failed further in
    if (e.propertyName !== undefined) return false; // the name's own failure; its propertyNames error says it
    return true;
  });
  // what is left at one place in one form is one line: the types the alternatives want, the keys of which one is needed
  const out: Failure[] = [];
  for (const e of kept) {
    const alternative = e.keyword === 'required' && /\/(anyOf|oneOf)\/\d+\/required$/.test(e.schemaPath);
    const twin = out.find((x) => x.instancePath === e.instancePath && x.keyword === e.keyword && (e.keyword === 'type' || (alternative && x.alternative === true)));
    const p = e.params as Record<string, unknown>;
    if (twin !== undefined && e.keyword === 'type') (twin.params as { type: string[] }).type = [...new Set([...(twin.params as { type: string[] }).type, ...[p.type as string | string[]].flat()])];
    else if (twin !== undefined) (twin.params as { missingProperty: string[] }).missingProperty.push(p.missingProperty as string);
    else {
      const params = e.keyword === 'type' ? { type: [p.type as string | string[]].flat() } : e.keyword === 'required' ? { missingProperty: [p.missingProperty as string] } : e.params;
      out.push({ ...e, alternative, params });
    }
  }
  return out;
}

// the id of the graph a file belongs to, when the file does not say it: the flow, nav or ui graph beside it
function graphIdBeside(dir: string): string | null {
  for (const name of ['flow.yaml', 'nav.yaml', 'ui.yaml']) {
    const file = join(dir, name);
    if (!existsSync(file)) continue;
    try {
      const id = (parse(readFileSync(file, 'utf8')) as { id?: unknown } | null)?.id;
      if (typeof id === 'string') return id;
    } catch {
      // a graph file that does not parse names no graph; its own check says why
    }
  }
  return null;
}

// every place where the file does not follow its kind's schema; a file written for another format is left to the
// format check
export function validateFile(file: string): SchemaFinding[] {
  const kind = schemaKindOf(file);
  if (kind === null) return [];
  const text = readFileSync(file, 'utf8');
  const lineCounter = new LineCounter();
  const doc = parseDocument(text, { lineCounter, prettyErrors: false });
  const at = (offset: number | undefined): { line: number; column: number } => {
    const { line, col } = lineCounter.linePos(offset ?? 0);
    return { line, column: col };
  };
  const ownFile = kind === 'catalog' ? `catalog/${basename(file)}` : basename(file);
  let graph: string | null = null;
  const failure = (where: { line: number; column: number }, said: string, pointer: string): SchemaFinding => {
    graph ??= graphIdBeside(kind === 'catalog' ? dirname(dirname(file)) : dirname(file));
    return { ...where, message: said, graph, element: `${ownFile}#${pointer}` };
  };

  // the first YAML error only: the ones after it mostly follow from it
  if (doc.errors.length > 0) return [failure(at(doc.errors[0].pos[0]), `not YAML: ${doc.errors[0].message.split('\n')[0]}`, '')];
  let data: unknown;
  if (kind === 'catalog') {
    try {
      data = JSON.parse(text);
    } catch (err) {
      return [failure({ line: 1, column: 1 }, `not JSON: ${(err as Error).message}`, '')];
    }
  } else data = doc.toJS();
  const own = (data as { schema?: unknown; id?: unknown } | null) ?? {};
  if (kind !== 'catalog' && typeof own.schema === 'string' && own.schema !== FORMAT) return [];
  if (kind !== 'catalog' && typeof own.id === 'string') graph = own.id;

  const { roots, validators } = schemas();
  const validate = validators[kind];
  if (validate(data)) return [];
  const out: SchemaFinding[] = [];
  for (const e of simplify((validate.errors ?? []) as Failure[])) {
    const p = e.params as Record<string, unknown>;
    const named = p.additionalProperty ?? p.unevaluatedProperty ?? p.propertyName;
    const name = named === undefined ? undefined : String(named);
    const where = place(doc, e.instancePath, name);
    const path = steps(e.instancePath).join('.');
    let said = `${path === '' ? 'the file' : path}: ${message(e, roots[kind])}`;
    const because = e.because ?? (e.keyword === 'false schema' ? undefined : (e.parentSchema as Schema | undefined)?.$comment);
    if (typeof because === 'string') said += ` (${because})`;
    // `{ title: Done, and paid }` is a title and a key "and paid": YAML ends a flow mapping's value at the comma
    const empty = where.value === null || (where.value as { value?: unknown } | undefined)?.value === null;
    if (name !== undefined && where.map?.flow === true && empty) said += '; a comma inside { } ends the value before it: quote that value';
    const missing = e.keyword === 'required' ? (p.missingProperty as string[]).join('|') : undefined;
    const pointer = name !== undefined ? `${e.instancePath}/${escape(name)}` : missing !== undefined ? `${e.instancePath}/${escape(missing)}` : e.instancePath;
    const range = (where.at as { range?: [number, number, number] } | undefined)?.range;
    const finding = failure(at(range?.[0]), said, pointer);
    if (!out.some((x) => x.line === finding.line && x.column === finding.column && x.message === finding.message)) out.push(finding);
  }
  return out;
}
