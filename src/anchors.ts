// Source anchors held to the code they name (SPEC.md §9). Every `source:` that a graph file's schema gives as an anchor
// names a file of the application's repository, from its root, and, with a line, a line within that file: one that
// does not is an error (`source-unresolved`), as a need's evidence is (SPEC.md §13). A `path:line` written in a graph's
// words (a description, a `say`, a note) is no anchor, but it cites the code all the same, so it is held to the code
// too, as a warning (`citation-unresolved`): words are not parsed, so only a token that reads as a path from the
// repository's root, a folder of the root first and a file with an extension last, is looked at. Under --no-app neither
// is looked for, and only counted.
import { existsSync, readFileSync, statSync } from 'node:fs';
import { basename, dirname, isAbsolute, join } from 'node:path';
import { anchorsIn, schemaKindOf } from './schemas.ts';
import type { SchemaKind } from './schemas.ts';
import { keyAt, readYaml } from './yaml.ts';

export type AnchorRule = 'source-unresolved' | 'citation-unresolved';

export interface AnchorProblem {
  rule: AnchorRule;
  file: string; // as found: absolute when the check was given absolute folders
  line: number;
  column: number;
  message: string;
  graph: string; // the id of the graph the file belongs to
  element: string; // the file and the JSON pointer of the anchor or the words: flow.yaml#/states/consent/source
}

export interface AnchorsCheck {
  problems: AnchorProblem[];
  anchors: number; // the anchors found
  citations: number; // the citations in words that read as a path from the repository's root
}

const FORMAT = 'delurio-graph/0';
// the kinds whose files carry anchors; a need's evidence is held by its own rule, and a catalog holds the product's
// words, which cite nothing
const ANCHORED = new Set<SchemaKind>(['flow', 'ui', 'nav', 'domain']);
const WORDED = new Set<SchemaKind>(['flow', 'ui', 'nav', 'domain', 'needs']);
// what is data rather than words: sample values of the screens, records of an entity, the anchors themselves and a nav
// graph's source; and in a needs file a need's evidence, which its own rule holds (a nav page's evidence is words)
const NOT_WORDS = new Set(['samples', 'examples', 'source']);
const NOT_NEEDS_WORDS = new Set([...NOT_WORDS, 'evidence']);

const lineCount = (body: string): number => body.split('\n').length - (body.endsWith('\n') ? 1 : 0);
const pointer = (path: string[]): string => path.map((s) => `/${s.replace(/~/g, '~0').replace(/\//g, '~1')}`).join('');

// the files of the application, each looked up once per run: whether it is there and is a file, and its lines
export class AppFiles {
  private readonly kinds = new Map<string, 'file' | 'folder' | null>();
  private readonly lines = new Map<string, number>();
  readonly root: string;
  constructor(root: string) {
    this.root = root;
  }

  abs(path: string): string {
    return isAbsolute(path) ? path : join(this.root, path);
  }

  kind(path: string): 'file' | 'folder' | null {
    const abs = this.abs(path);
    if (!this.kinds.has(abs)) this.kinds.set(abs, existsSync(abs) ? (statSync(abs).isFile() ? 'file' : 'folder') : null);
    return this.kinds.get(abs) ?? null;
  }

  lineCount(path: string): number {
    const abs = this.abs(path);
    if (!this.lines.has(abs)) this.lines.set(abs, lineCount(readFileSync(abs, 'utf8')));
    return this.lines.get(abs) ?? 0;
  }

  // why `<path>[:<line>]` names nothing under the root, or null when it names a file and a line within it: what is
  // wrong (the path, or the path and its line) and what is wrong with it
  unresolved(value: string): { subject: string; reason: string } | null {
    const m = /^(.*?)(?::(\d+))?$/.exec(value) as RegExpExecArray;
    const kind = this.kind(m[1]);
    if (kind === null) return { subject: m[1], reason: 'does not exist' };
    if (kind === 'folder') return { subject: m[1], reason: 'is a folder: an anchor names a file' };
    if (m[2] === undefined) return null;
    const line = Number(m[2]);
    const n = this.lineCount(m[1]);
    if (line < 1) return { subject: value, reason: `names line ${line}: a file's lines count from 1` };
    return line > n ? { subject: value, reason: `is past the file's end: it has ${n} ${n === 1 ? 'line' : 'lines'}` } : null;
  }
}

// a token of words that reads as `<path>:<line>`: a folder, then a file with an extension, then the line, and a range's
// last line after a dash; not inside an address, a longer path or a word
const CITATION = /(?<![\p{L}\p{N}_.\/:@~+\-])((?:[\p{L}\p{N}_@.][\p{L}\p{N}_@.()[\]+\-]*\/)+[\p{L}\p{N}_@()[\]+\-.]*?\.[A-Za-z][A-Za-z0-9]*):(\d+)(?:-(\d+))?(?!\d)/gu;

// the folder given as source.app by a nav graph, or by the nav.yaml beside a generated one, to say where an anchor
// would have been had it been written from there
function navApp(file: string, doc: Record<string, unknown>): string | null {
  const own = (doc.source as { app?: unknown } | undefined)?.app;
  if (typeof own === 'string') return own;
  const beside = join(dirname(file), 'nav.yaml');
  if (basename(file) === 'nav.yaml' || !existsSync(beside)) return null;
  try {
    const app = ((readYaml(readFileSync(beside, 'utf8')) as { source?: { app?: unknown } } | null)?.source)?.app;
    return typeof app === 'string' ? app : null;
  } catch {
    return null;
  }
}

// every anchor and every citation in words of the graph files given, against the application's files under `app`;
// with `app` null (--no-app), each is counted and none is looked for
export function checkAnchors(files: string[], app: string | null): AnchorsCheck {
  const out: AnchorsCheck = { problems: [], anchors: 0, citations: 0 };
  const code = app === null ? null : new AppFiles(app);
  for (const file of files) {
    const kind = schemaKindOf(file);
    if (kind === null || !WORDED.has(kind)) continue;
    let text: string;
    let doc: unknown;
    try {
      text = readFileSync(file, 'utf8');
      doc = readYaml(text);
    } catch {
      continue; // the schema rule, or the graph's load, says why
    }
    if (doc === null || typeof doc !== 'object' || Array.isArray(doc)) continue;
    const own = doc as Record<string, unknown>;
    if (typeof own.schema === 'string' && own.schema !== FORMAT) continue;
    const graph = typeof own.id === 'string' ? own.id : basename(dirname(file));
    const name = basename(file);
    const problem = (rule: AnchorRule, path: string[], message: string, element: string): void => {
      out.problems.push({ rule, file, ...keyAt(text, path), message, graph, element });
    };

    if (ANCHORED.has(kind)) {
      const navFrom = kind === 'nav' ? navApp(file, own) : null;
      for (const { path, value } of anchorsIn(kind, doc)) {
        if (!/^[^\s:]+(:[0-9]+)?$/.test(value)) continue; // the schema rule says why
        out.anchors++;
        const why = code?.unresolved(value) ?? null;
        if (why === null) continue;
        const where = path.slice(0, -1).join('.') || 'the graph';
        const from = navFrom === null || code === null ? null : join(navFrom, value);
        const there = from !== null && code?.unresolved(from) === null ? `; ${from} is there: an anchor is written from the repository's root, not from source.app` : '';
        problem('source-unresolved', path, `${where}: source ${why.subject} ${why.reason}${there} (SPEC.md §9)`, `${name}#${pointer(path)}`);
      }
    }

    const skip = kind === 'needs' ? NOT_NEEDS_WORDS : NOT_WORDS;
    const words = (value: unknown, path: string[]): void => {
      if (typeof value === 'string') {
        for (const m of value.matchAll(CITATION)) {
          const first = m[1].split('/')[0];
          if (first === '.' || first === '..') continue;
          if (code !== null && code.kind(first) !== 'folder') continue; // not a path from the repository's root
          out.citations++;
          if (code === null) continue;
          const line = Math.max(Number(m[2]), Number(m[3] ?? 0));
          const why = code.unresolved(`${m[1]}:${line}`);
          if (why === null) continue;
          const cited = m[0];
          const said = why.subject === m[1] ? `and ${why.subject} ${why.reason}` : `which ${why.reason}`;
          problem('citation-unresolved', path, `${path.join('.')} cites ${cited}, ${said} (SPEC.md §9)`, `${name}#${pointer(path)}/${cited}`);
        }
        return;
      }
      if (Array.isArray(value)) value.forEach((x, i) => words(x, [...path, String(i)]));
      else if (value !== null && typeof value === 'object') {
        for (const [k, x] of Object.entries(value)) if (!skip.has(k)) words(x, [...path, k]);
      }
    };
    words(doc, []);
  }
  return out;
}
