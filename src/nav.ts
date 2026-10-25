import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { parse } from 'yaml';
import { evaluate, identifiers, parseExpr } from './expr.ts';
import type { Expr, Value } from './expr.ts';
import { GraphError } from './graph.ts';

// The navigation graph (kind: nav): pages, the entries that lead from one to another, and the people who use them.
// It reuses the guard language of the flow graph; a person is a fixed set of facts, so no search over contexts is needed.

export type PageKind = 'screen' | 'placeholder' | 'missing' | 'redirect' | 'external';
export type FrameId = string;
export type How = 'auto' | 'menu' | 'picker' | 'user' | 'link' | 'button';

const PAGE_KINDS: readonly string[] = ['screen', 'placeholder', 'missing', 'redirect', 'external'];
const HOWS: readonly string[] = ['auto', 'menu', 'picker', 'user', 'link', 'button'];
// an entry of the frame (menu, scope picker, person row) is drawn even around a page that refuses the person
const FRAME_LEVEL: readonly string[] = ['menu', 'picker', 'user'];

// A frame is the chrome drawn around a page. `menu` names the menu it carries (a frame without one has none);
// `shows: { fact }` makes its pages draw the menu of the frame the person's fact names (a page outside any scope shows the menu of the scope the person lands in).
export interface FrameSpec {
  title: string;
  menu?: string;
  shows?: { fact: string };
}

interface FactSpec {
  values?: Value[];
  of?: string[];
  say?: string;
}
interface PageSpec {
  route: string;
  title?: string;
  kind: string;
  frame: string;
  source?: string;
  opensFor?: string;
  pageCodes?: string;
  evidence?: string;
  refusal?: string;
}
interface EntrySpec {
  id?: string;
  from: string | string[];
  to: string;
  how: string;
  label?: string;
  when?: string;
  clicks?: number;
  data?: boolean;
  back?: boolean;
}
interface PersonaSpec {
  title: string;
  say: string;
  facts: Record<string, Value | string>;
}
interface GoalSpec {
  id: string;
  say: string;
  page: string;
  for: string[];
  within: number;
}
export interface ExtractCfg {
  adapter?: string;
  names?: Record<string, string>;
  titles?: Record<string, string>;
  locales?: string[];
}

interface NavDoc {
  extends?: string;
  extract?: ExtractCfg;
  frames: Record<string, FrameSpec>;
  kind: string;
  schema: string;
  id: string;
  title: string;
  status: string;
  locale: string;
  catalog: string;
  summary: string;
  source: Record<string, string>;
  facts: Record<string, FactSpec>;
  sets: Record<string, string[]>;
  guards: Record<string, string>;
  personas: Record<string, PersonaSpec>;
  pages: Record<string, PageSpec>;
  entries: EntrySpec[];
  goals: GoalSpec[];
  notModeled: string[];
}

export interface Page {
  id: string;
  route: string;
  title: string; // plain words (catalog applied)
  titleKey: string | null;
  kind: PageKind;
  frame: FrameId;
  source: string | null;
  opensFor: string | null;
  pageCodes: string | null;
  evidence: string | null;
  refusal: string | null; // how the page says no, in words
}

export interface Entry {
  id: string;
  spec: number; // index of the line it was written on
  from: string;
  ofFrame: FrameId | null; // the frame selector it was written with (frame:ders), null when it starts at one page
  to: string;
  how: How;
  label: string;
  labelKey: string | null;
  when: string | null;
  clicks: number;
  data: boolean;
  back: boolean;
  order: number;
}

export interface Persona {
  id: string;
  title: string;
  say: string;
  facts: Record<string, Value>;
}

export interface Goal {
  id: string;
  say: string;
  page: string;
  for: string[];
  within: number;
}

export interface NavGraph {
  dir: string;
  adapter: string | null;
  frames: Record<string, FrameSpec>;
  menuFrames: string[]; // frames that carry a menu, in the order they are declared
  id: string;
  title: string;
  status: string;
  summary: string;
  locale: string;
  source: Record<string, string>;
  facts: Record<string, FactSpec>;
  guards: Record<string, string>;
  pages: Record<string, Page>;
  entries: Entry[];
  specs: EntrySpec[];
  personas: Persona[];
  goals: Goal[];
  notModeled: string[];
  catalog: Record<string, string>;
}

export const frameLevel = (e: Entry): boolean => FRAME_LEVEL.includes(e.how);
const own = (o: object, k: string): boolean => Object.prototype.hasOwnProperty.call(o, k);

// nav.yaml may extend a generated file (nav.extracted.yaml): its pages, guards and entries come first, and the hand-written
// file adds to them or overrides single fields of a page. A guard may not be defined twice with different text.
function mergeDocs(base: Partial<NavDoc>, hand: NavDoc): NavDoc {
  const pages: Record<string, PageSpec> = {};
  for (const [id, p] of Object.entries(base.pages ?? {})) pages[id] = { ...p, ...(own(hand.pages ?? {}, id) ? hand.pages[id] : {}) };
  for (const [id, p] of Object.entries(hand.pages ?? {})) {
    if (own(pages, id)) continue;
    if (p.route === undefined) throw new GraphError(`page ${id} is not in the extracted file and gives no route (a misspelt override?)`);
    pages[id] = p;
  }
  const guards: Record<string, string> = { ...(base.guards ?? {}) };
  for (const [name, src] of Object.entries(hand.guards ?? {})) {
    if (own(guards, name) && guards[name] !== src) throw new GraphError(`guard ${name} is written in both the extracted file and nav.yaml, with different text`);
    guards[name] = src;
  }
  return { ...hand, pages, guards, entries: [...(base.entries ?? []), ...(hand.entries ?? [])], source: { ...(base.source ?? {}), ...(hand.source ?? {}) } };
}

// `read` takes a path relative to `dir`; the revision diff passes one that reads a git revision instead of the disk
export function loadNav(dir: string, locale?: string, read = (file: string): string => readFileSync(join(dir, file), 'utf8')): { g: NavGraph; warnings: string[] } {
  const raw = parse(read('nav.yaml')) as NavDoc;
  const doc = raw.extends === undefined ? raw : mergeDocs(parse(read(raw.extends)) as Partial<NavDoc>, raw);
  const warnings: string[] = [];
  if (doc.kind !== 'nav') throw new GraphError(`${join(dir, 'nav.yaml')} is not a navigation graph (kind: nav is missing)`);
  const catalogFile = locale === undefined ? doc.catalog : doc.catalog.replace(`${doc.locale}.json`, `${locale}.json`);
  const catalog = JSON.parse(read(catalogFile)) as Record<string, string>;
  const guards = doc.guards ?? {};
  const facts = doc.facts ?? {};
  const sets = doc.sets ?? {};

  const known = (name: string): boolean => own(facts, name) || own(guards, name);
  const checkGuard = (src: string, where: string): void => {
    let e: Expr;
    try {
      e = parseExpr(src);
    } catch (err) {
      throw new GraphError(`${where}: ${(err as Error).message}`);
    }
    for (const name of identifiers(e)) if (!known(name)) throw new GraphError(`${where}: "${name}" is neither a fact nor a named guard`);
  };
  for (const [name, src] of Object.entries(guards)) checkGuard(src, `guard ${name}`);

  const looksLikeKey = (s: string): boolean => /^[A-Za-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)+$/.test(s);
  const resolveLabel = (s: string, where: string): { text: string; key: string | null } => {
    if (own(catalog, s)) return { text: catalog[s], key: s };
    if (looksLikeKey(s)) warnings.push(`${where}: "${s}" looks like a catalog key but the catalog has no such key`);
    return { text: s, key: null };
  };

  const frames = doc.frames ?? {};
  if (Object.keys(frames).length === 0) throw new GraphError('a navigation graph declares its frames (frames:)');
  for (const [name, f] of Object.entries(frames)) {
    if (f.shows !== undefined && !own(facts, f.shows.fact)) throw new GraphError(`frame ${name} shows the menu named by ${f.shows.fact}, which is not a fact`);
  }
  const pages: Record<string, Page> = {};
  for (const [id, p] of Object.entries(doc.pages)) {
    if (!PAGE_KINDS.includes(p.kind)) throw new GraphError(`page ${id}: unknown kind ${p.kind}`);
    if (!own(frames, p.frame)) throw new GraphError(`page ${id}: unknown frame ${p.frame}`);
    if (p.opensFor !== undefined) checkGuard(p.opensFor, `page ${id} opensFor`);
    const title = p.title === undefined ? { text: id, key: null } : resolveLabel(p.title, `page ${id}`);
    pages[id] = {
      id,
      route: p.route,
      title: title.text,
      titleKey: title.key,
      kind: p.kind as PageKind,
      frame: p.frame,
      source: p.source ?? null,
      opensFor: p.opensFor ?? null,
      pageCodes: p.pageCodes ?? null,
      evidence: p.evidence ?? null,
      refusal: p.refusal ?? null,
    };
  }
  if (pages.start === undefined) throw new GraphError('a navigation graph needs a page called start');

  const entries: Entry[] = [];
  const specs = doc.entries ?? [];
  specs.forEach((s, spec) => {
    const where = `entry ${spec + 1} (${String(s.to)})`;
    if (!HOWS.includes(s.how)) throw new GraphError(`${where}: unknown how ${s.how}`);
    if (pages[s.to] === undefined) throw new GraphError(`${where}: goes to ${s.to}, which is not a page`);
    if (s.when !== undefined) checkGuard(s.when, `${where} when`);
    const label = s.label === undefined ? { text: '', key: null } : resolveLabel(s.label, where);
    if (label.text === '' && s.how !== 'auto') throw new GraphError(`${where}: needs a label`);
    for (const token of Array.isArray(s.from) ? s.from : [s.from]) {
      const sources: Array<{ page: string; implicit: string | null }> = [];
      let ofFrame: FrameId | null = null;
      if (token.startsWith('frame:')) {
        const frame = token.slice('frame:'.length);
        if (!own(frames, frame)) throw new GraphError(`${where}: unknown frame in ${token}`);
        ofFrame = frame;
        for (const p of Object.values(pages)) {
          if (p.frame === frame) sources.push({ page: p.id, implicit: null });
          // a page whose frame shows the menu of another frame is drawn with this frame's menu when the person's fact names it
          const shows = frames[p.frame].shows;
          if (shows !== undefined && p.frame !== frame && frames[frame].menu !== undefined) sources.push({ page: p.id, implicit: `${shows.fact} == '${frame}'` });
        }
        if (sources.length === 0) throw new GraphError(`${where}: no page is in ${token}`);
      } else {
        if (pages[token] === undefined) throw new GraphError(`${where}: starts at ${token}, which is not a page`);
        sources.push({ page: token, implicit: null });
      }
      for (const src of sources) {
        if (src.page === s.to && frameLevelHow(s.how)) continue; // the page you are on is not a way to itself
        const parts = [src.implicit, s.when ?? null].filter((x): x is string => x !== null);
        entries.push({
          id: s.id ?? `${src.page}>${s.to}#${spec + 1}`,
          spec,
          from: src.page,
          ofFrame,
          to: s.to,
          how: s.how as How,
          label: label.text,
          labelKey: label.key,
          when: parts.length === 0 ? null : parts.length === 1 ? parts[0] : parts.map((x) => `(${x})`).join(' and '),
          clicks: s.clicks ?? (s.how === 'auto' ? 0 : 1),
          data: s.data === true,
          back: s.back === true,
          order: entries.length,
        });
      }
    }
  });

  const personas: Persona[] = Object.entries(doc.personas).map(([id, p]) => {
    const f: Record<string, Value> = {};
    for (const [name, spec] of Object.entries(facts)) {
      if (!own(p.facts, name)) throw new GraphError(`persona ${id} gives no value for ${name}`);
      let v = p.facts[name];
      if (spec.of !== undefined) {
        if (typeof v === 'string') {
          if (!own(sets, v)) throw new GraphError(`persona ${id}: ${v} is not a named set`);
          v = sets[v];
        }
        if (!Array.isArray(v)) throw new GraphError(`persona ${id}: ${name} must be a list`);
        for (const item of v) if (!spec.of.includes(String(item))) throw new GraphError(`persona ${id}: ${String(item)} is not one of the values of ${name}`);
      } else if (spec.values !== undefined && !spec.values.includes(v as Value)) {
        throw new GraphError(`persona ${id}: ${String(v)} is not a value of ${name}`);
      }
      f[name] = v as Value;
    }
    for (const name of Object.keys(p.facts)) if (!own(facts, name)) throw new GraphError(`persona ${id} gives ${name}, which is not a declared fact`);
    return { id, title: p.title, say: p.say, facts: f };
  });

  const notModeled = doc.notModeled ?? [];
  notModeled.forEach((x, i) => {
    if (typeof x !== 'string') throw new GraphError(`notModeled item ${i + 1} is not text (a colon followed by a space turns a line into a map: quote it)`);
  });
  const goals = doc.goals ?? [];
  for (const goal of goals) {
    if (pages[goal.page] === undefined) throw new GraphError(`goal ${goal.id}: ${goal.page} is not a page`);
    for (const who of goal.for) if (!personas.some((p) => p.id === who)) throw new GraphError(`goal ${goal.id}: ${who} is not a persona`);
  }

  return {
    g: {
      dir,
      adapter: doc.extract?.adapter ?? null,
      frames,
      menuFrames: Object.entries(frames).filter(([, f]) => f.menu !== undefined).map(([name]) => name),
      id: doc.id,
      title: doc.title,
      status: doc.status,
      summary: (doc.summary ?? '').trim(),
      locale: locale ?? doc.locale,
      source: doc.source ?? {},
      facts,
      guards,
      pages,
      entries,
      specs,
      personas,
      goals,
      notModeled,
      catalog,
    },
    warnings,
  };
}

function frameLevelHow(how: string): boolean {
  return FRAME_LEVEL.includes(how);
}

function lookupFor(g: NavGraph, p: Persona, depth: number): (name: string) => Value {
  return (name) => {
    if (own(p.facts, name)) return p.facts[name];
    const guard = g.guards[name];
    if (guard === undefined) throw new GraphError(`unknown name "${name}"`);
    if (depth > 8) throw new GraphError(`guard ${name} nests too deep`);
    return evaluate(parseExpr(guard), lookupFor(g, p, depth + 1));
  };
}

export function holds(g: NavGraph, p: Persona, expr: string | null): boolean {
  return expr === null ? true : Boolean(evaluate(parseExpr(expr), lookupFor(g, p, 0)));
}

export const refusedFor = (g: NavGraph, p: Persona, page: string): boolean => {
  const guard = g.pages[page].opensFor;
  return guard !== null && !holds(g, p, guard);
};

export interface Reach {
  page: string;
  clicks: number;
  via: Entry | null;
  prev: string | null;
  usesData: boolean;
}

// Fewest clicks from "start"; ties go to the entry written first. Around a page that refuses the person only the frame's own entries work.
export function walk(g: NavGraph, p: Persona): Map<string, Reach> {
  const best = new Map<string, Reach>();
  const done = new Set<string>();
  const index = (id: string): number => Object.keys(g.pages).indexOf(id);
  best.set('start', { page: 'start', clicks: 0, via: null, prev: null, usesData: false });
  for (;;) {
    let pick: Reach | null = null;
    for (const r of best.values()) {
      if (done.has(r.page)) continue;
      if (pick === null || r.clicks < pick.clicks || (r.clicks === pick.clicks && index(r.page) < index(pick.page))) pick = r;
    }
    if (pick === null) break;
    done.add(pick.page);
    const refused = refusedFor(g, p, pick.page);
    for (const e of g.entries) {
      if (e.from !== pick.page) continue;
      if (refused && !frameLevel(e)) continue;
      if (!holds(g, p, e.when)) continue;
      const clicks = pick.clicks + e.clicks;
      const held = best.get(e.to);
      if (held === undefined || clicks < held.clicks) best.set(e.to, { page: e.to, clicks, via: e, prev: pick.page, usesData: pick.usesData || e.data });
    }
  }
  return best;
}

export function pathTo(reach: Map<string, Reach>, page: string): Entry[] {
  const out: Entry[] = [];
  let cur = reach.get(page);
  while (cur !== undefined && cur.via !== null) {
    out.unshift(cur.via);
    cur = cur.prev === null ? undefined : reach.get(cur.prev);
  }
  return out;
}

export type Standing = 'ok' | 'placeholder' | 'missing' | 'refused' | 'unreachable';

export interface PersonaView {
  persona: Persona;
  reach: Map<string, Reach>;
  standing: Record<string, Standing>;
  landing: string | null;
}

export function viewOf(g: NavGraph, p: Persona): PersonaView {
  const reach = walk(g, p);
  const standing: Record<string, Standing> = {};
  for (const id of Object.keys(g.pages)) {
    if (!reach.has(id)) standing[id] = 'unreachable';
    else if (refusedFor(g, p, id)) standing[id] = 'refused';
    else standing[id] = g.pages[id].kind === 'placeholder' ? 'placeholder' : g.pages[id].kind === 'missing' ? 'missing' : 'ok';
  }
  const auto = g.entries.find((e) => e.from === 'start' && e.how === 'auto' && holds(g, p, e.when));
  // a start page that is a screen (not a redirect) is where the person lands when no rule sends them on
  const landing = auto !== undefined ? auto.to : g.pages.start.kind === 'redirect' ? null : 'start';
  return { persona: p, reach, standing, landing };
}

export const frameReached = (g: NavGraph, v: PersonaView, frame: FrameId): boolean =>
  Object.values(g.pages).some((pg) => pg.frame === frame && v.standing[pg.id] !== 'unreachable');

export interface MenuCell {
  shown: boolean; // the persona's menu has the entry
  standing: Standing | null; // what its target is for this persona, when shown
}

// One row per menu entry written in the graph; a column per persona.
export interface MenuRow {
  spec: number;
  frame: FrameId;
  entry: Entry;
  cells: MenuCell[]; // by persona; shown is false when the persona never has that menu
  frameKnown: boolean[]; // by persona: has this menu at all
}

export function menuRows(g: NavGraph, views: PersonaView[]): MenuRow[] {
  const seen = new Set<number>();
  const rows: MenuRow[] = [];
  for (const e of g.entries) {
    if (e.how !== 'menu' || seen.has(e.spec)) continue;
    seen.add(e.spec);
    const frame = e.ofFrame ?? g.pages[e.from].frame;
    const frameKnown = views.map((v) => frameReached(g, v, frame));
    rows.push({
      spec: e.spec,
      frame,
      entry: e,
      frameKnown,
      cells: views.map((v, i) => {
        if (!frameKnown[i]) return { shown: false, standing: null };
        const shown = holds(g, v.persona, g.specs[e.spec].when ?? null);
        return { shown, standing: shown ? standingAfterMenu(g, v, e.to) : null };
      }),
    });
  }
  return rows;
}

function standingAfterMenu(g: NavGraph, v: PersonaView, page: string): Standing {
  if (refusedFor(g, v.persona, page)) return 'refused';
  return g.pages[page].kind === 'placeholder' ? 'placeholder' : g.pages[page].kind === 'missing' ? 'missing' : 'ok';
}

export interface Finding {
  level: 'problem' | 'note';
  code: string;
  say: string;
  detail: string[];
}

const lower = (s: string): string => s.toLocaleLowerCase('tr');
const word = (g: NavGraph, key: string): string => g.catalog[key] ?? key;
const quote = (s: string): string => `“${s}”`;
const joinWords = (xs: string[]): string => (xs.length <= 1 ? xs.join('') : `${xs.slice(0, -1).join(', ')} and ${xs[xs.length - 1]}`);

// the words on the way in and on arrival name the same thing: one contains the other, or they share a stem
// (Turkish suffixes: hesap, hesabım)
function similar(a: string, b: string): boolean {
  if (a.includes(b) || b.includes(a)) return true;
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i += 1;
  return i >= 4 && i >= 0.6 * Math.min(a.length, b.length);
}

export interface Goalcheck {
  goal: Goal;
  persona: Persona;
  outcome: 'ok' | 'too many clicks' | 'not built' | 'does not exist' | 'refused' | 'cannot reach';
  clicks: number | null;
}

export function checkGoals(g: NavGraph, views: PersonaView[]): Goalcheck[] {
  const out: Goalcheck[] = [];
  for (const goal of g.goals) {
    for (const who of goal.for) {
      const v = views.find((x) => x.persona.id === who);
      if (v === undefined) continue;
      const standing = v.standing[goal.page];
      const clicks = v.reach.get(goal.page)?.clicks ?? null;
      const outcome: Goalcheck['outcome'] =
        standing === 'unreachable' ? 'cannot reach' : standing === 'refused' ? 'refused' : standing === 'placeholder' ? 'not built' : standing === 'missing' ? 'does not exist' : (clicks ?? 0) > goal.within ? 'too many clicks' : 'ok';
      out.push({ goal, persona: v.persona, outcome, clicks });
    }
  }
  return out;
}

export function findings(g: NavGraph, views: PersonaView[], goals: Goalcheck[]): Finding[] {
  const out: Finding[] = [];
  const rows = menuRows(g, views);
  const personaTitle = (i: number): string => views[i].persona.title;

  const who = (xs: string[]): string => joinWords(xs.map(quote));
  const menuName = (frame: FrameId): string => (g.frames[frame].menu ?? frame).toLowerCase();

  // 1. the first page a person sees is not built
  const landing = views.filter((v) => v.landing !== null && g.pages[v.landing].kind === 'placeholder');
  if (landing.length > 0) {
    const targets = [...new Set(landing.map((v) => v.landing as string))];
    out.push({
      level: 'problem',
      code: 'landing-not-built',
      say: `${who(landing.map((v) => v.persona.title))} ${landing.length === 1 ? 'lands' : 'land'} on a page that says it is not ready.`,
      detail: targets.map((t) => {
        const people = landing.filter((v) => v.landing === t).map((v) => v.persona.title);
        return `“/” sends ${who(people)} to ${quote(g.pages[t].title)} (${g.pages[t].route}), which answers “${word(g, 'Shell.placeholder')}”`;
      }),
    });
  }

  // 2. the menu offers an entry and the page behind it refuses
  const gated = [...new Set(rows.filter((r) => /\bheld\b/.test(g.specs[r.spec].when ?? '')).map((r) => r.entry.label))];
  const refusedLines: string[] = [];
  views.forEach((v, i) => {
    for (const frame of g.menuFrames) {
      const inFrame = rows.filter((r) => r.frame === frame && r.cells[i].shown);
      const of = (s: Standing): MenuRow[] => inFrame.filter((r) => r.cells[i].standing === s);
      if (of('refused').length === 0) continue;
      const parts = [`${of('ok').length} open`, `${of('refused').length} refuse (${of('refused').map((r) => quote(r.entry.label)).join(', ')})`];
      if (of('placeholder').length > 0) parts.push(`${of('placeholder').length} are not built`);
      if (of('missing').length > 0) parts.push(`${of('missing').length} lead to a page that does not exist`);
      refusedLines.push(`${personaTitle(i)}: the ${menuName(frame)} has ${inFrame.length} ${inFrame.length === 1 ? 'entry' : 'entries'}; ${parts.join(', ')}.`);
    }
  });
  if (refusedLines.length > 0) {
    out.push({
      level: 'problem',
      code: 'menu-refused',
      say: `The menu offers entries whose page then refuses the person. ${gated.length === 0 ? 'No entry is drawn by permission' : `Only ${who(gated)} ${gated.length === 1 ? 'is' : 'are'} drawn by permission`}; every other entry is drawn for everyone and the page decides.`,
      detail: refusedLines,
    });
  }

  // 2b. a link or button drawn on a page the person can open leads to a page that refuses them
  const deadLinks = new Map<string, string>();
  views.forEach((v) => {
    for (const e of g.entries) {
      if (e.how !== 'link' && e.how !== 'button') continue;
      if (v.standing[e.from] !== 'ok' && v.standing[e.from] !== 'placeholder') continue;
      if (!holds(g, v.persona, e.when) || !refusedFor(g, v.persona, e.to)) continue;
      const to = g.pages[e.to];
      deadLinks.set(`${v.persona.id}|${e.from}|${e.to}|${e.label}`, `${v.persona.title}: ${quote(e.label)} on ${quote(g.pages[e.from].title)} leads to ${quote(to.title)}, which ${to.refusal ?? 'refuses them'}.`);
    }
  });
  if (deadLinks.size > 0) {
    out.push({ level: 'problem', code: 'link-refused', say: 'A link or button is drawn for a person whose destination then refuses them.', detail: [...deadLinks.values()] });
  }

  // 3. a menu entry opens a page that is not built
  const unbuilt = new Map<string, { labels: Set<string>; who: Set<string> }>();
  rows.forEach((r) => {
    if (g.pages[r.entry.to].kind !== 'placeholder') return;
    const hit = unbuilt.get(r.entry.to) ?? { labels: new Set<string>(), who: new Set<string>() };
    r.cells.forEach((c, i) => {
      if (c.shown) hit.who.add(personaTitle(i));
    });
    hit.labels.add(r.entry.label);
    unbuilt.set(r.entry.to, hit);
  });
  if (unbuilt.size > 0) {
    out.push({
      level: 'problem',
      code: 'menu-not-built',
      say: `${unbuilt.size} pages that the menus offer say “${word(g, 'Shell.placeholder')}”`,
      detail: [...unbuilt.entries()].map(([page, hit]) => `${[...hit.labels].map(quote).join(' / ')} (${g.pages[page].route}) is in the menu of ${who([...hit.who])}.`),
    });
  }

  // 3b. a menu entry leads to a page that does not exist
  const gone = new Map<string, { labels: Set<string>; who: Set<string> }>();
  rows.forEach((r) => {
    if (g.pages[r.entry.to].kind !== 'missing') return;
    const hit = gone.get(r.entry.to) ?? { labels: new Set<string>(), who: new Set<string>() };
    r.cells.forEach((c, i) => {
      if (c.shown) hit.who.add(personaTitle(i));
    });
    hit.labels.add(r.entry.label);
    gone.set(r.entry.to, hit);
  });
  if (gone.size > 0) {
    out.push({
      level: 'problem',
      code: 'menu-missing',
      say: `${gone.size} ${gone.size === 1 ? 'entry' : 'entries'} in the menus lead to a page that does not exist, so the server answers not found.`,
      detail: [...gone.entries()].map(([page, hit]) => `${[...hit.labels].map(quote).join(' / ')} (${g.pages[page].route}) is in the menu of ${who([...hit.who])}.`),
    });
  }

  // 4. goals that cannot be done
  const failed = goals.filter((c) => c.outcome !== 'ok');
  if (failed.length > 0) {
    out.push({
      level: 'problem',
      code: 'goals',
      say: `${failed.length} of ${goals.length} things people come to do fail.`,
      detail: failed.map((c) => `${c.persona.title}, ${lower(c.goal.say)}: ${c.outcome === 'too many clicks' ? `${c.clicks} clicks, the limit is ${c.goal.within}` : c.outcome === 'not built' ? 'the page is not built' : c.outcome === 'does not exist' ? 'the page does not exist' : c.outcome === 'refused' ? (g.pages[c.goal.page].refusal ?? 'the page refuses them') : 'there is no way there'}.`),
    });
  }

  // 5. pages nobody can reach
  const lost = Object.values(g.pages).filter((p) => p.kind !== 'external' && p.kind !== 'redirect' && views.every((v) => v.standing[p.id] === 'unreachable'));
  if (lost.length > 0) {
    out.push({ level: 'problem', code: 'unreachable', say: 'Some pages cannot be reached by any kind of user.', detail: lost.map((p) => `${p.title} (${p.route})`) });
  }

  // 6. two entries of one menu open the same page
  const dup: string[] = [];
  views.forEach((v, i) => {
    for (const frame of g.menuFrames) {
      const shown = rows.filter((r) => r.frame === frame && r.cells[i].shown);
      const byTarget = new Map<string, string[]>();
      for (const r of shown) byTarget.set(r.entry.to, [...(byTarget.get(r.entry.to) ?? []), r.entry.label]);
      for (const [page, labels] of byTarget) if (labels.length > 1) dup.push(`${personaTitle(i)}: ${joinWords(labels.map(quote))} both open ${g.pages[page].route}.`);
    }
  });
  if (dup.length > 0) out.push({ level: 'note', code: 'menu-twins', say: 'Two entries in one menu open the same page.', detail: dup });

  // 7. the words on the way in are not the words on arrival
  const mismatch = new Map<string, string>();
  for (const e of g.entries) {
    const target = g.pages[e.to];
    if (e.labelKey === null || e.back || e.how === 'auto' || e.how === 'picker' || target.titleKey === null) continue;
    const a = lower(e.label);
    const b = lower(target.title);
    if (similar(a, b)) continue;
    const where = e.how === 'menu' ? `in the ${menuName(e.ofFrame ?? g.pages[e.from].frame)}` : `${e.how} on ${g.pages[e.from].title}`;
    mismatch.set(`${e.label}|${e.to}|${e.how === 'menu'}`, `${quote(e.label)} (${where}) opens a page titled ${quote(target.title)}.`);
  }
  if (mismatch.size > 0) out.push({ level: 'note', code: 'words-differ', say: 'A link or button is named differently from the page it opens.', detail: [...mismatch.values()].sort() });

  // 8. different pages that carry the same title
  const titles = new Map<string, Page[]>();
  for (const p of Object.values(g.pages)) {
    if (p.kind === 'redirect' || p.kind === 'external' || p.frame === 'none') continue;
    titles.set(lower(p.title), [...(titles.get(lower(p.title)) ?? []), p]);
  }
  const same = [...titles.values()].filter((ps) => ps.length > 1);
  if (same.length > 0) {
    out.push({
      level: 'note',
      code: 'same-title',
      say: 'Different pages share a title, so two tabs or two history entries cannot be told apart.',
      detail: same.map((ps) => `${quote(ps[0].title)}: ${ps.map((p) => p.route).join(' and ')}`),
    });
  }
  return out;
}

export interface RepoCheck {
  ok: boolean;
  what: string;
  detail: string;
}

// the permission codes a guard asks `'code' in held` about, named guards followed
export function guardCodes(g: NavGraph, src: string, depth = 0): Set<string> {
  const out = new Set<string>();
  const visit = (e: Expr): void => {
    switch (e.k) {
      case 'cmp':
        if (e.op === 'in' && e.l.k === 'lit' && typeof e.l.v === 'string' && e.r.k === 'id' && e.r.name === 'held') out.add(e.l.v);
        visit(e.l);
        visit(e.r);
        break;
      case 'and':
      case 'or':
        visit(e.l);
        visit(e.r);
        break;
      case 'not':
        visit(e.e);
        break;
      case 'id': {
        const guard = g.guards[e.name];
        if (guard !== undefined && depth < 8) for (const c of guardCodes(g, guard, depth + 1)) out.add(c);
        break;
      }
      case 'list':
        e.items.forEach(visit);
        break;
      case 'lit':
        break;
    }
  };
  visit(parseExpr(src));
  return out;
}
