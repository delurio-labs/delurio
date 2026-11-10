// YAML as delurio reads it (SPEC.md §1): YAML 1.2 without anchors, aliases, tags, or keys that are lists or maps.
// No graph needs them, and two of them keep the yaml library busy far beyond a file's size: on a 40 KB file, maps
// used as keys, or aliases inside keys, held `parse` for more than 30 seconds, which stops a server that reads the
// file. So the text is first read into the library's concrete syntax tree, which resolves nothing and takes time in
// proportion to the text, and walked once; a file that holds one of them is refused at its place, as a file that is not
// YAML is. Every reader of a graph file goes through `readYaml`.
import { isMap, isSeq, LineCounter, parse, parseDocument, Parser, YAMLParseError, type ErrorCode } from 'yaml';

export interface Unsupported {
  offset: number;
  line: number;
  column: number;
  code: ErrorCode;
  message: string; // `unsupported YAML: …`, with what to write instead
}

const WHY = 'delurio reads YAML without anchors, aliases, tags or keys that are lists or maps (SPEC.md §1)';
const PROPS: Record<string, [ErrorCode, string, string]> = {
  anchor: ['BAD_ALIAS', 'an anchor', 'write the value where it is used'],
  alias: ['BAD_ALIAS', 'an alias', 'write the value where it is used'],
  tag: ['TAG_RESOLVE_FAILED', 'a tag', 'write the value without it'],
};
const COLLECTIONS = new Set(['block-map', 'block-seq', 'flow-collection']);

interface Token {
  type?: unknown;
  offset?: unknown;
  source?: unknown;
  key?: Token | null;
  start?: unknown;
}

// the 1-based line and column of an offset into the text
export function lineAndColumn(text: string, offset: number): { line: number; column: number } {
  let line = 1;
  let from = 0;
  for (let i = text.indexOf('\n'); i >= 0 && i < offset; i = text.indexOf('\n', i + 1)) {
    line++;
    from = i + 1;
  }
  return { line, column: offset - from + 1 };
}

// the first place in the text, by offset, that delurio does not read; null when there is none. A text that is not
// YAML at all is left to the parser, which says why.
export function unsupported(text: string): Unsupported | null {
  let first: { offset: number; code: ErrorCode; what: string } | null = null;
  const found = (offset: number, code: ErrorCode, what: string): void => {
    if (first === null || offset < first.offset) first = { offset, code, what };
  };
  // walked with a stack of its own: a deeply nested text must not overflow the call stack here
  const stack: unknown[] = [...new Parser().parse(text)];
  while (stack.length > 0) {
    const v = stack.pop();
    if (Array.isArray(v)) {
      for (const x of v) if (x !== null && typeof x === 'object') stack.push(x);
      continue;
    }
    if (v === null || typeof v !== 'object') continue;
    const t = v as Token;
    const offset = typeof t.offset === 'number' ? t.offset : 0;
    const prop = typeof t.type === 'string' ? PROPS[t.type] : undefined;
    if (prop !== undefined) found(offset, prop[0], `${prop[1]} (${String(t.source)}): ${prop[2]}`);
    const key = t.key;
    if (key !== null && key !== undefined && typeof key.type === 'string' && COLLECTIONS.has(key.type)) {
      const list = key.type === 'block-seq' || (key.start as { source?: unknown } | undefined)?.source === '[';
      found(typeof key.offset === 'number' ? key.offset : offset, 'NON_STRING_KEY', `a key that is a ${list ? 'list' : 'map'}: name it with a string`);
    }
    for (const x of Object.values(v)) if (x !== null && typeof x === 'object') stack.push(x);
  }
  if (first === null) return null;
  const { offset, code, what } = first;
  return { offset, ...lineAndColumn(text, offset), code, message: `unsupported YAML: ${what}; ${WHY}` };
}

// the refusal as the error the yaml library throws for a text that is not YAML, so that every reader that already
// says "cannot be loaded" for one says it for this too, at the same place
function refusal(u: Unsupported): YAMLParseError {
  const err = new YAMLParseError([u.offset, u.offset + 1], u.code, `${u.message} at line ${u.line}, column ${u.column}`);
  err.linePos = [{ line: u.line, col: u.column }];
  return err;
}

// the value of a graph file's text: the yaml library's `parse`, once nothing unsupported is found
export function readYaml(text: string): unknown {
  const u = unsupported(text);
  if (u !== null) throw refusal(u);
  return parse(text);
}

// the 1-based line and column of the key at `path` in a YAML text (of the item, for a list index), else the file's
// start; for a text `readYaml` has read
export function keyAt(text: string, path: string[]): { line: number; column: number } {
  const lineCounter = new LineCounter();
  const doc = parseDocument(text, { lineCounter, prettyErrors: false });
  const keyOf = (pair: { key: unknown }): string => String((pair.key as { value?: unknown } | null)?.value ?? pair.key);
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
