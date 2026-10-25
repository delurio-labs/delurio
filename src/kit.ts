// A kit says how the components of ui.yaml look in one product: design tokens (W3C design tokens JSON, written out as
// CSS custom properties), a template per component and optional CSS. Templates are data: a few slots and flags are
// filled in, nothing in a kit is ever run, and anything that could reach the network or run a script is refused when
// the kit loads.
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { parse } from 'yaml';

export class KitError extends Error {}

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const SLOTS = ['text', 'name', 'id', 'intent', 'children'] as const;
const FLAGS = ['disabled', 'named', 'conditional'] as const;
type Slot = (typeof SLOTS)[number];
type Flag = (typeof FLAGS)[number];

// `ctx` is where the slot stands: escaping differs only in a single-quoted attribute value
type Part = { lit: string } | { slot: Slot; ctx: 'text' | 'dq' | 'sq' } | { flag: Flag; negate: boolean; parts: Part[] };

export interface Template {
  source: string;
  parts: Part[];
}

export interface KitComponent {
  template: Template;
  roles: Record<string, Template>; // a node whose role has its own template is drawn with it
  intents?: Record<string, string>; // node intent -> the word {intent} gives; `default` for a node without one
}

export interface Kit {
  id: string;
  title: string;
  file: string;
  components: Record<string, KitComponent>;
  light: Array<[string, string]>; // custom properties from `tokens`
  dark: Array<[string, string]>; // the same names, re-pointed for a dark page
  css: string; // the kit's own CSS and each component's, checked, not yet scoped
  zoom: number; // at most how much larger the screens are drawn on the video stage
}

// What a template is filled with, for one node in one state.
export interface Fill {
  id: string;
  text: string;
  name: string; // the accessible name the graph expects; the visible text when it gives none
  intent: string | undefined;
  role: string | undefined;
  children?: string; // List only: the drawn list items
  disabled: boolean;
  named: boolean; // the graph gives an accessible name that differs from the visible text
  conditional: boolean; // the node appears only when a flag is set (`when`)
}

// Lists are drawn without a wrapper around each item, so their templates must be one element the renderer can mark.
const SINGLE = new Set(['List', 'ListItem']);

const VOID = new Set(['area', 'br', 'col', 'hr', 'img', 'input', 'source', 'track', 'wbr']);
const DENIED_TAGS = new Set(['script', 'style', 'iframe', 'frame', 'frameset', 'object', 'embed', 'applet', 'link', 'meta', 'base', 'form', 'noscript', 'template', 'portal', 'audio', 'video']);
// a quoted address in CSS, which image-set() fetches without url()
const FETCHED_STRING = /["'](https?:)?\/\//i;
const URL_ATTRS = new Set(['href', 'src', 'srcset', 'action', 'formaction', 'xlink:href', 'poster', 'data', 'background', 'ping', 'codebase', 'cite', 'longdesc', 'manifest', 'srcdoc', 'lowsrc']);

function checkAttr(where: string, tag: string, attr: string, value: string): void {
  const no = (why: string): never => {
    throw new KitError(`${where}: <${tag} ${attr}>: ${why}`);
  };
  if (!/^[a-z_:][-a-z0-9_:.]*$/.test(attr)) no('not an attribute name');
  if (attr.startsWith('on')) no('event handlers are not allowed: a kit is data, never code');
  if (/javascript:/i.test(value)) no('javascript: is not allowed');
  if (URL_ATTRS.has(attr) && value !== '' && !value.startsWith('#') && !(attr === 'src' && value.startsWith('data:image/'))) {
    no('may only point inside the page ("#…") or, for src, hold a data:image; nothing is fetched');
  }
  if (attr === 'style' && (/url\(\s*(?!['"]?(data:|#))/i.test(value) || FETCHED_STRING.test(value.replace(/url\([^)]*\)/gi, '')) || /expression\(|@import/i.test(value))) no('a style may only use data: URLs');
}

// Reads a template, checks that it is well-formed HTML with slots only where a value can stand, and splits it into parts.
function parseTemplate(source: string, where: string, component: string): Template {
  const src = source.trim();
  const fail = (why: string): never => {
    throw new KitError(`${where}: ${why}`);
  };
  const root: Part[] = [];
  let parts = root;
  const frames: Array<{ flag: Flag; outer: Part[]; key: string }> = [];
  let lit = '';
  const flush = (): void => {
    if (lit !== '') parts.push({ lit });
    lit = '';
  };
  let mode: 'text' | 'tag' | 'dq' | 'sq' | 'comment' = 'text';
  const stack: string[] = [];
  let svg = 0;
  let tag = '';
  let attr = '';
  let value = '';
  let opened = 0; // tags opened so far: a section that opens inside a tag must close inside the same one
  let topElements = 0;
  let topText = false;
  let rootEnd = -1;
  const key = (): string => `${mode}|${stack.join('>')}|${mode === 'text' ? '' : opened}|${mode === 'dq' || mode === 'sq' ? attr : ''}`;
  let i = 0;
  while (i < src.length) {
    const c = src[i];
    const marker = c === '{' && mode !== 'comment' ? /^\{([?!/]?)([a-z]+)\}/.exec(src.slice(i)) : null;
    if (marker !== null) {
      const [whole, sigil, word] = marker;
      if (sigil === '') {
        if (!(SLOTS as readonly string[]).includes(word)) fail(`unknown slot {${word}}; the slots are ${SLOTS.map((s) => `{${s}}`).join(' ')}`);
        if (word === 'children' && component !== 'List') fail('{children} is only for List');
        if (mode !== 'text' && mode !== 'dq' && mode !== 'sq') fail(`{${word}} must stand in text or inside a quoted attribute value`);
        if (word === 'children' && mode !== 'text') fail('{children} must stand in text');
        if (mode === 'text' && stack.length === 0) topText = true;
        if (mode !== 'text') value += whole;
        flush();
        parts.push({ slot: word as Slot, ctx: mode === 'dq' || mode === 'sq' ? mode : 'text' });
      } else if (sigil === '/') {
        const f = frames.pop();
        if (f === undefined || f.flag !== word) fail(`{/${word}} does not close the section that is open`);
        else {
          if (f.key !== key()) fail(`{?${word}}…{/${word}} must close in the same place it opens (same element, tag or attribute)`);
          flush();
          parts = f.outer;
        }
      } else {
        if (!(FLAGS as readonly string[]).includes(word)) fail(`unknown flag ${word}; the flags are ${FLAGS.join(', ')}`);
        if (frames.some((f) => f.flag === word)) fail(`{${sigil}${word}} inside a section on the same flag`);
        flush();
        const sec: Part = { flag: word as Flag, negate: sigil === '!', parts: [] };
        parts.push(sec);
        frames.push({ flag: word as Flag, outer: parts, key: key() });
        parts = sec.parts;
      }
      i += whole.length;
      continue;
    }
    if (mode === 'comment') {
      if (src.startsWith('-->', i)) {
        mode = 'text';
        lit += '-->';
        i += 3;
      } else {
        lit += c;
        i += 1;
      }
      continue;
    }
    if (mode === 'dq' || mode === 'sq') {
      if (c === (mode === 'dq' ? '"' : "'")) {
        checkAttr(where, tag, attr, value);
        mode = 'tag';
      } else value += c;
      lit += c;
      i += 1;
      continue;
    }
    if (mode === 'tag') {
      if (c === '>' || src.startsWith('/>', i)) {
        const self = c === '/';
        if (self && svg === 0 && !VOID.has(tag)) fail(`<${tag} />: only void elements and SVG may close themselves`);
        if (!self && !VOID.has(tag)) {
          stack.push(tag);
          if (tag === 'svg') svg += 1;
        }
        lit += self ? '/>' : '>';
        i += self ? 2 : 1;
        mode = 'text';
        attr = '';
        if (stack.length === 0) rootEnd = i;
        continue;
      }
      if (/\s/.test(c)) {
        lit += c;
        i += 1;
        continue;
      }
      const name = /^[^\s"'>/={]+/.exec(src.slice(i));
      if (name === null) fail(`unexpected "${c}" in <${tag}>`);
      else {
        attr = name[0].toLowerCase();
        lit += name[0];
        i += name[0].length;
        const eq = /^\s*=\s*/.exec(src.slice(i));
        if (eq === null) {
          checkAttr(where, tag, attr, '');
          continue;
        }
        lit += eq[0];
        i += eq[0].length;
        const q = src[i];
        if (q === '"' || q === "'") {
          mode = q === '"' ? 'dq' : 'sq';
          value = '';
          lit += q;
          i += 1;
        } else {
          const bare = /^[^\s"'>`=<{]+/.exec(src.slice(i));
          if (bare === null) fail(`<${tag} ${attr}=…>: write the value in quotes`);
          else {
            checkAttr(where, tag, attr, bare[0]);
            lit += bare[0];
            i += bare[0].length;
          }
        }
      }
      continue;
    }
    // text
    if (c === '<') {
      if (src.startsWith('<!--', i)) {
        mode = 'comment';
        lit += '<!--';
        i += 4;
        continue;
      }
      const close = /^<\/([a-zA-Z][a-zA-Z0-9-]*)\s*>/.exec(src.slice(i));
      if (close !== null) {
        const name = close[1].toLowerCase();
        const top = stack.pop();
        if (top !== name) fail(`</${name}> closes ${top === undefined ? 'nothing' : `<${top}>`}`);
        if (name === 'svg') svg -= 1;
        lit += close[0];
        i += close[0].length;
        if (stack.length === 0) rootEnd = i;
        continue;
      }
      const open = /^<([a-zA-Z][a-zA-Z0-9-]*)/.exec(src.slice(i));
      if (open === null) fail('a bare "<" in text: write &lt;');
      else {
        tag = open[1].toLowerCase();
        if (DENIED_TAGS.has(tag)) fail(`<${tag}> is not allowed in a kit template`);
        if (stack.length === 0) topElements += 1;
        opened += 1;
        lit += open[0];
        i += open[0].length;
        mode = 'tag';
      }
      continue;
    }
    if (stack.length === 0 && !/\s/.test(c)) topText = true;
    lit += c;
    i += 1;
  }
  flush();
  if (mode !== 'text') fail('the template ends inside a tag, an attribute value or a comment');
  if (stack.length > 0) fail(`<${stack[stack.length - 1]}> is not closed`);
  if (frames.length > 0) fail(`{?${frames[frames.length - 1].flag}} is not closed`);
  if (component === 'List' && !src.includes('{children}')) fail('a List template needs {children}, where its items go');
  if (SINGLE.has(component) && (topElements !== 1 || topText || rootEnd !== src.length || !src.startsWith('<'))) {
    fail(`a ${component} template must be one element (the renderer marks it with the node it draws)`);
  }
  return { source: src, parts: root };
}

function fillParts(parts: Part[], f: Fill, intent: string): string {
  let out = '';
  for (const p of parts) {
    if ('lit' in p) out += p.lit;
    else if ('slot' in p) {
      if (p.slot === 'children') out += f.children ?? '';
      else {
        const v = p.slot === 'intent' ? intent : f[p.slot];
        out += p.ctx === 'sq' ? esc(v).replace(/'/g, '&#39;') : esc(v);
      }
    } else if (f[p.flag] !== p.negate) out += fillParts(p.parts, f, intent);
  }
  return out;
}

// The node drawn with the kit, or null when the kit has no template for the component.
export function drawWith(kit: Kit, component: string, f: Fill, warn: (m: string) => void): string | null {
  const c = kit.components[component];
  if (c === undefined) return null;
  let intent = f.intent ?? '';
  if (c.intents !== undefined) {
    const mapped = f.intent === undefined ? c.intents.default : c.intents[f.intent];
    if (f.intent !== undefined && mapped === undefined) warn(`kit ${kit.id}: ${component} has no entry for intent ${f.intent} (node ${f.id}); drawn as ${c.intents.default ?? 'no intent'}`);
    intent = mapped ?? c.intents.default ?? '';
  }
  const t = (f.role === undefined ? undefined : c.roles[f.role]) ?? c.template;
  return fillParts(t.parts, f, intent);
}

// A List or ListItem is drawn without a wrapper: the node id goes on the template's one element, and on the page the
// annotation chips go inside it, last.
export function mark(html: string, id: string, chips: string): string {
  let q = '';
  let end = -1;
  for (let i = 0; i < html.length && end < 0; i++) {
    const c = html[i];
    if (q !== '') {
      if (c === q) q = '';
    } else if (c === '"' || c === "'") q = c;
    else if (c === '>') end = html[i - 1] === '/' ? i - 1 : i;
  }
  const close = html.lastIndexOf('</');
  return `${html.slice(0, end)} data-node="${esc(id)}"${html.slice(end, close)}${chips}${html.slice(close)}`;
}

// ---- CSS

// Checks kit CSS before it is scoped to the wireframe: nothing fetched, no way out of the wireframe's rule, nothing
// that cannot sit inside it. Throws on the first problem.
function checkCss(css: string, where: string): void {
  const fail = (why: string): never => {
    throw new KitError(`${where}: ${why}`);
  };
  if (css.includes('<')) fail('"<" is not allowed in kit CSS (percent-encode it in a data: URL)');
  if (/expression\(|javascript:|-moz-binding|behavior\s*:/i.test(css)) fail('script-like CSS is not allowed');
  let depth = 0;
  let prelude = '';
  let i = 0;
  while (i < css.length) {
    const c = css[i];
    if (css.startsWith('/*', i)) {
      const end = css.indexOf('*/', i + 2);
      if (end < 0) fail('a comment is not closed');
      i = end + 2;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < css.length && css[j] !== c && css[j] !== '\n') j += css[j] === '\\' ? 2 : 1;
      if (css[j] !== c) fail('a string is not closed');
      prelude += css.slice(i, j + 1);
      i = j + 1;
      continue;
    }
    if (c === '{') {
      const p = prelude.trim();
      const at = /^@([a-z-]+)/i.exec(p);
      if (at !== null && !['media', 'supports', 'container'].includes(at[1].toLowerCase())) {
        fail(`@${at[1]} is not allowed in kit CSS (only @media, @supports and @container, which can sit inside the wireframe's rule)`);
      }
      if (at === null) {
        for (const sel of p.split(',')) {
          if (/^(:root|html|body)\b/i.test(sel.trim())) fail(`"${sel.trim()}" never matches inside the wireframe: style the screen itself with &`);
        }
      }
      depth += 1;
      prelude = '';
    } else if (c === '}') {
      depth -= 1;
      if (depth < 0) fail('a "}" closes nothing');
      prelude = '';
    } else if (c === ';') {
      if (/^\s*@/.test(prelude)) fail(`${prelude.trim()} is not allowed in kit CSS`);
      prelude = '';
    } else prelude += c;
    i += 1;
  }
  if (depth !== 0) fail('a "{" is not closed');
  if (/^\s*@/.test(prelude)) fail(`${prelude.trim()} is not allowed in kit CSS`);
  for (const m of css.matchAll(/url\(\s*(['"]?)([^'")]*)/gi)) {
    if (!m[2].startsWith('data:') && !m[2].startsWith('#')) fail(`url(${m[2]}) would be fetched; only data: URLs are allowed`);
  }
  if (FETCHED_STRING.test(css.replace(/url\([^)]*\)/gi, ''))) fail('a quoted address would be fetched; only data: URLs are allowed');
}

// ---- design tokens

const GENERIC_FAMILIES = new Set(['serif', 'sans-serif', 'monospace', 'cursive', 'fantasy', 'system-ui', 'ui-serif', 'ui-sans-serif', 'ui-monospace', 'ui-rounded', 'math', 'emoji', 'fangsong']);
const UNITS = new Set(['px', 'rem', 'em', '%', 'ms', 's']);

function cssName(path: string[], where: string): string {
  for (const seg of path) if (/[{}.]/.test(seg) || seg === '') throw new KitError(`${where}: token name "${seg}" may not be empty or contain {, } or .`);
  return `--${path.map((s) => s.replace(/[^A-Za-z0-9_-]+/g, '-')).join('-')}`;
}

interface TokenFile {
  decls: Array<[string, string]>;
  refs: Array<{ from: string; to: string }>;
}

// Reads one W3C design tokens file (the Design Tokens Community Group format): groups nest, a token has `$value`,
// `$type` is inherited from its group, and "{group.token}" is an alias, written as var(--group-token).
function readTokens(file: string, all: Set<string>): TokenFile {
  let doc: unknown;
  try {
    doc = JSON.parse(readFileSync(file, 'utf8'));
  } catch (err) {
    throw new KitError(`${file}: ${err instanceof Error ? err.message : String(err)}`);
  }
  const decls: Array<[string, string]> = [];
  const refs: Array<{ from: string; to: string }> = [];
  const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

  const walk = (node: Record<string, unknown>, path: string[], type: string | undefined): void => {
    if ('$extends' in node || '$ref' in node) throw new KitError(`${file}: ${path.join('.') || 'the root'}: $extends and $ref are not supported; write an alias "{group.token}"`);
    for (const [k, v] of Object.entries(node)) {
      if (k.startsWith('$')) continue;
      const here = [...path, k];
      if (!isObj(v)) throw new KitError(`${file}: ${here.join('.')} is neither a group nor a token (a token has $value)`);
      const t = typeof v.$type === 'string' ? v.$type : type;
      if ('$value' in v) token(here, t, v.$value);
      else walk(v, here, t);
    }
  };

  const token = (path: string[], type: string | undefined, value: unknown): void => {
    const where = `${file}: ${path.join('.')}`;
    const name = cssName(path, file);
    const alias = (v: unknown): string | null => {
      if (typeof v !== 'string') return null;
      const m = /^\{([^{}]+)\}$/.exec(v);
      if (m === null) return null;
      const to = cssName(m[1].split('.'), where);
      refs.push({ from: name, to });
      return `var(${to})`;
    };
    const plain = (v: unknown): string => {
      const a = alias(v);
      if (a !== null) return a;
      if (typeof v === 'number') return String(v);
      if (typeof v !== 'string') throw new KitError(`${where}: expected a string or a number, got ${JSON.stringify(v)}`);
      if (/[;{}<]|url\(|@|\\/i.test(v)) throw new KitError(`${where}: ${JSON.stringify(v)} is not a plain CSS value`);
      return v;
    };
    const dimension = (v: unknown): string => {
      if (isObj(v)) {
        if (typeof v.value !== 'number' || typeof v.unit !== 'string' || !UNITS.has(v.unit)) throw new KitError(`${where}: a dimension needs a number value and one of the units ${[...UNITS].join(' ')}`);
        return `${v.value}${v.unit}`;
      }
      return plain(v);
    };
    const color = (v: unknown): string => {
      if (!isObj(v)) return plain(v);
      const a = typeof v.alpha === 'number' ? v.alpha : 1;
      if (typeof v.hex === 'string' && /^#[0-9a-fA-F]{6}$/.test(v.hex)) {
        if (a === 1) return v.hex;
        const n = parseInt(v.hex.slice(1), 16);
        return `rgb(${n >> 16} ${(n >> 8) & 255} ${n & 255} / ${a})`;
      }
      const comps = Array.isArray(v.components) ? v.components.map((x) => (typeof x === 'number' ? String(x) : 'none')).join(' ') : '';
      const space = String(v.colorSpace);
      if (comps === '') throw new KitError(`${where}: a colour needs components or a hex`);
      const tail = a === 1 ? '' : ` / ${a}`;
      if (['oklch', 'oklab', 'lab', 'lch'].includes(space)) return `${space}(${comps}${tail})`;
      if (['srgb', 'srgb-linear', 'display-p3', 'a98-rgb', 'prophoto-rgb', 'rec2020', 'xyz-d50', 'xyz-d65'].includes(space)) return `color(${space} ${comps}${tail})`;
      throw new KitError(`${where}: colour space ${space} is not supported (give a hex as well)`);
    };
    const family = (v: unknown): string => {
      if (!Array.isArray(v)) return plain(v);
      return v.map((f) => {
        const s = plain(f);
        return GENERIC_FAMILIES.has(s) || s.startsWith('var(') || s.startsWith('"') ? s : `"${s.replace(/"/g, '')}"`;
      }).join(', ');
    };
    const shadow = (v: unknown): string => {
      if (Array.isArray(v)) return v.map(shadow).join(', ');
      if (!isObj(v)) return plain(v);
      return [v.inset === true ? 'inset' : '', dimension(v.offsetX), dimension(v.offsetY), dimension(v.blur), dimension(v.spread ?? { value: 0, unit: 'px' }), color(v.color)].filter((s) => s !== '').join(' ');
    };

    const out = (n: string, v: string): void => {
      if (all.has(n)) throw new KitError(`${where}: ${n} is defined twice`);
      all.add(n);
      decls.push([n, v]);
    };
    const a = alias(value);
    if (a !== null) return out(name, a);
    switch (type) {
      case 'color':
        return out(name, color(value));
      case 'dimension':
      case 'duration':
        return out(name, dimension(value));
      case 'fontFamily':
        return out(name, family(value));
      case 'fontWeight':
      case 'number':
        return out(name, plain(value));
      case 'cubicBezier':
        if (!Array.isArray(value) || value.length !== 4) throw new KitError(`${where}: a cubicBezier is four numbers`);
        return out(name, `cubic-bezier(${value.map(plain).join(', ')})`);
      case 'shadow':
        return out(name, shadow(value));
      case 'border':
        if (!isObj(value)) return out(name, plain(value));
        return out(name, `${dimension(value.width)} ${plain(value.style)} ${color(value.color)}`);
      case 'typography': {
        if (!isObj(value)) throw new KitError(`${where}: a typography token is an object`);
        // one custom property per field: --name-font-family, --name-font-size, ...
        const fields: Array<[string, (v: unknown) => string]> = [['fontFamily', family], ['fontSize', dimension], ['fontWeight', plain], ['letterSpacing', dimension], ['lineHeight', plain]];
        for (const [f, conv] of fields) if (value[f] !== undefined) out(`${name}-${f.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`)}`, conv(value[f]));
        return;
      }
      case undefined:
        if (isObj(value) && 'unit' in value) return out(name, dimension(value));
        if (isObj(value) && ('colorSpace' in value || 'hex' in value)) return out(name, color(value));
        return out(name, plain(value));
      default:
        throw new KitError(`${where}: $type ${type} is not supported yet`);
    }
  };

  if (!isObj(doc)) throw new KitError(`${file}: a tokens file is a JSON object`);
  walk(doc, [], typeof doc.$type === 'string' ? doc.$type : undefined);
  return { decls, refs };
}

function readTokenSet(dir: string, files: string[], where: string, known?: Set<string>): Array<[string, string]> {
  const names = new Set<string>();
  const decls: Array<[string, string]> = [];
  const refs: Array<{ from: string; to: string }> = [];
  for (const f of files) {
    const r = readTokens(join(dir, f), names);
    decls.push(...r.decls);
    refs.push(...r.refs);
  }
  const visible = new Set([...names, ...(known ?? [])]);
  for (const r of refs) if (!visible.has(r.to)) throw new KitError(`${where}: ${r.from} refers to ${r.to}, which no tokens file defines`);
  if (known !== undefined) for (const [n] of decls) if (!known.has(n)) throw new KitError(`${where}: dark token ${n} has no light value`);
  // an alias cycle would leave every token on it without a value
  const edges = new Map<string, string[]>();
  for (const r of refs) edges.set(r.from, [...(edges.get(r.from) ?? []), r.to]);
  const state = new Map<string, number>();
  const visit = (n: string, trail: string[]): void => {
    if (state.get(n) === 2) return;
    if (state.get(n) === 1) throw new KitError(`${where}: alias cycle ${[...trail, n].join(' -> ')}`);
    state.set(n, 1);
    for (const t of edges.get(n) ?? []) visit(t, [...trail, n]);
    state.set(n, 2);
  };
  for (const n of edges.keys()) visit(n, []);
  return decls;
}

// ---- loading

const list = (v: unknown, where: string): string[] => {
  if (v === undefined) return [];
  const xs = Array.isArray(v) ? v : [v];
  for (const x of xs) if (typeof x !== 'string') throw new KitError(`${where}: expected a file name or a list of them`);
  return xs as string[];
};

function only(obj: Record<string, unknown>, keys: string[], where: string): void {
  for (const k of Object.keys(obj)) if (!keys.includes(k)) throw new KitError(`${where}: unknown key ${k} (known: ${keys.join(', ')})`);
}

export function loadKit(path: string): Kit {
  if (!existsSync(path)) throw new KitError(`kit ${path} does not exist`);
  const file = statSync(path).isDirectory() ? join(path, 'kit.yaml') : path;
  if (!existsSync(file)) throw new KitError(`kit ${path} has no kit.yaml`);
  const dir = dirname(file);
  const raw = parse(readFileSync(file, 'utf8')) as Record<string, unknown> | null;
  if (raw === null || typeof raw !== 'object') throw new KitError(`${file}: empty`);
  if (raw.kind !== 'kit') throw new KitError(`${file} is not a kit (kind: kit is missing)`);
  only(raw, ['schema', 'kind', 'id', 'title', 'source', 'tokens', 'dark', 'css', 'stage', 'components'], file);
  if (typeof raw.id !== 'string' || !/^[a-z0-9][a-z0-9-]*$/.test(raw.id)) throw new KitError(`${file}: id must be lowercase letters, digits and dashes`);
  const id = raw.id;
  const comps = raw.components as Record<string, Record<string, unknown>> | undefined;
  if (comps === undefined || typeof comps !== 'object' || Object.keys(comps).length === 0) throw new KitError(`${file}: components is missing`);

  const components: Record<string, KitComponent> = {};
  const css: string[] = [];
  for (const f of list(raw.css, `${file}: css`)) {
    const text = readFileSync(join(dir, f), 'utf8');
    checkCss(text, join(dir, f));
    css.push(text.trim());
  }
  for (const [name, spec] of Object.entries(comps)) {
    const where = `${file}: ${name}`;
    if (name === 'Page') throw new KitError(`${where}: Page is the screen itself and has no template; style the screen with & in the kit CSS`);
    if (spec === null || typeof spec !== 'object') throw new KitError(`${where}: needs a template`);
    only(spec, ['template', 'roles', 'intents', 'css'], where);
    if (typeof spec.template !== 'string') throw new KitError(`${where}: needs a template`);
    const roles: Record<string, Template> = {};
    for (const [role, t] of Object.entries((spec.roles ?? {}) as Record<string, unknown>)) {
      if (typeof t !== 'string') throw new KitError(`${where}: roles.${role} must be a template`);
      roles[role] = parseTemplate(t, `${where}: roles.${role}`, name);
    }
    let intents: Record<string, string> | undefined;
    if (spec.intents !== undefined) {
      intents = spec.intents as Record<string, string>;
      for (const [k, v] of Object.entries(intents)) if (typeof v !== 'string') throw new KitError(`${where}: intents.${k} must be a word`);
    }
    if (spec.css !== undefined) {
      if (typeof spec.css !== 'string') throw new KitError(`${where}: css must be CSS text`);
      checkCss(spec.css, `${where}: css`);
      css.push(spec.css.trim());
    }
    components[name] = { template: parseTemplate(spec.template, `${where}: template`, name), roles, intents };
  }

  const light = readTokenSet(dir, list(raw.tokens, `${file}: tokens`), `${file}: tokens`);
  const dark = readTokenSet(dir, list(raw.dark, `${file}: dark`), `${file}: dark`, new Set(light.map(([n]) => n)));
  const stage = (raw.stage ?? {}) as Record<string, unknown>;
  only(stage, ['zoom'], `${file}: stage`);
  const zoom = stage.zoom ?? 1.5;
  if (typeof zoom !== 'number' || zoom < 0.5 || zoom > 4) throw new KitError(`${file}: stage.zoom must be a number from 0.5 to 4`);
  return { id, title: typeof raw.title === 'string' ? raw.title : id, file, components, light, dark, css: css.join('\n\n'), zoom };
}

// `--kit <path>` wins over `kit:` in ui.yaml (a path relative to the graph); `--kit none` draws the fixed components.
export function resolveKit(graphDir: string, uiKit: string | undefined, flagValue: string | undefined): Kit | null {
  if (flagValue === 'none') return null;
  if (flagValue !== undefined) return loadKit(resolve(flagValue));
  if (uiKit !== undefined) return loadKit(resolve(graphDir, uiKit));
  return null;
}

// The CSS a kit adds: its tokens and its CSS, scoped to the wireframe (`.wf.kit`, with native CSS nesting), and the
// few rules that keep the page's own marks readable or fit the screens to the video stage.
export function kitCss(kit: Kit, target: 'page' | 'stage'): string {
  const decls = (ds: Array<[string, string]>): string => ds.map(([n, v]) => `${n}: ${v};`).join(' ');
  const out: string[] = [`/* kit ${kit.id} */`];
  if (kit.light.length > 0) out.push(`.wf.kit { ${decls(kit.light)} }`);
  if (target === 'page' && kit.dark.length > 0) out.push(`@media (prefers-color-scheme: dark) { .wf.kit { ${decls(kit.dark)} } }`);
  out.push(`.wf.kit {\n${kit.css}\n}`);
  // a frame is a pure function of time, and a screen is a still picture: nothing in it moves
  out.push('.wf.kit *, .wf.kit *::before, .wf.kit *::after { animation: none !important; transition: none !important; }');
  if (target === 'page') {
    // the chips and outlines are the page's marks, so they keep the page's colours whatever the kit names its tokens
    out.push('.screen { --page-muted: var(--muted); --page-line: var(--line); --page-focus: var(--focus); }');
    out.push('.wf.kit .chip { color: var(--page-muted); border-color: var(--page-line); background: none; }');
    out.push('.wf.kit .wf-cond { outline-color: var(--page-muted); } .wf.kit .wf-focus { outline-color: var(--page-focus); }');
    out.push(':where(.wf.kit) button:disabled:not(.wf-btn, .wf-link) { opacity: 1; }');
  } else {
    out.push(`:root { --kit-zoom: ${kit.zoom}; } .page { padding: 0; } .view .appcard { padding: 38px 46px 30px; }`);
    out.push('.view .wf.kit { zoom: var(--kit-zoom); padding: calc(26px / var(--kit-zoom)) calc(46px / var(--kit-zoom)) calc(32px / var(--kit-zoom)); }');
    out.push('.view .wf.kit .pressed { transform: scale(.95); }');
  }
  return `\n${out.join('\n')}\n`;
}

// Runs once in the video stage, before the first frame: a kit draws at its own sizes, so its screens are drawn at the
// largest zoom (at most the kit's) at which every one of them fits between the window's top and the caption. One zoom
// for all, so a screen does not change size from step to step.
export function kitStageScript(kit: Kit): string {
  return `(() => {
  const room = 480; // the window's page starts at y 146; captions start near y 627
  const win = document.getElementById('win');
  const views = [...document.querySelectorAll('.view')];
  const fits = (z) => {
    document.documentElement.style.setProperty('--kit-zoom', String(z));
    return views.every((v) => {
      const wf = v.querySelector('.wf.kit');
      if (wf === null) return true;
      v.style.display = 'block';
      const h = wf.getBoundingClientRect().height;
      v.style.display = '';
      return h <= room;
    });
  };
  win.style.display = 'block';
  let z = ${kit.zoom};
  while (z > 0.5 && !fits(z)) z = Math.round((z - 0.05) * 100) / 100;
  win.style.display = '';
})();`;
}
