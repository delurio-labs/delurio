import { GraphError } from './graph.ts';
import { checkGoals, findings, menuRows, pathTo, viewOf } from './nav.ts';
import type { Finding, Goalcheck, NavGraph, Page, PersonaView, Standing } from './nav.ts';
import { deepDiff, fieldChanges, joinWords, listChanges, matchOrdered, moved, otherKeys, quote, same, union } from './compare.ts';
import type { Change, Op, Shift } from './compare.ts';

// The revision diff of a navigation graph: pages, entries, personas and goals keyed by id (nav.yaml and the file it extends
// are compared as one graph), and what changes for each kind of user, found by walking both revisions.

export interface NavSide {
  g: NavGraph;
  warnings: string[];
}

type Spec = NavGraph['specs'][number];

const raw = (x: object | undefined): Record<string, unknown> => (x ?? {}) as Record<string, unknown>;
const opOf = (b: unknown, a: unknown): Op => (b === undefined ? 'added' : a === undefined ? 'removed' : 'changed');
const capital = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

const KIND_WORDS: Record<string, string> = { screen: 'built', placeholder: 'not built', missing: 'a page that does not exist', redirect: 'a redirect', external: 'another application' };
const STANDING_WORDS: Record<Standing, string> = { ok: 'opens', placeholder: 'is not built', missing: 'does not exist', refused: 'refuses them', unreachable: 'cannot be reached' };
const STANDING_NOW: Record<Standing, string> = { ok: 'now opens', placeholder: 'is now not built', missing: 'now does not exist', refused: 'now refuses them', unreachable: 'can no longer be reached' };
const STANDING_PAST: Record<Standing, string> = { ok: 'opened', placeholder: 'was not built', missing: 'did not exist', refused: 'refused them', unreachable: 'could not be reached' };

const word = (g: NavGraph, key: string): string => g.catalog[key] ?? key;
const pageName = (g: NavGraph, id: string | null): string => (id === null ? 'no page' : g.pages[id] === undefined ? id : quote(g.pages[id].title));
const fromTokens = (s: Spec): string[] => (Array.isArray(s.from) ? s.from : [s.from]);
const menuName = (g: NavGraph, frame: string): string => (g.frames[frame]?.menu ?? frame).toLowerCase();

function fromWords(g: NavGraph, s: Spec): string {
  return joinWords(
    fromTokens(s).map((t) => {
      if (!t.startsWith('frame:')) return pageName(g, t);
      const frame = t.slice('frame:'.length);
      const f = g.frames[frame];
      if (f === undefined) return t;
      return s.how === 'menu' && f.menu !== undefined ? `the ${menuName(g, frame)}` : `every page of ${quote(f.title)}`;
    }),
  );
}

function entryWords(g: NavGraph, s: Spec): string {
  const label = s.label === undefined ? '' : quote(word(g, s.label));
  if (s.how === 'menu') return `menu entry ${label} in ${fromWords(g, s)}`;
  if (s.how === 'picker') return `scope picker entry ${label} on ${fromWords(g, s)}`;
  if (s.how === 'user') return `entry ${label} in the person’s corner of ${fromWords(g, s)}`;
  if (s.how === 'auto') return `automatic redirect from ${fromWords(g, s)}`;
  return `${s.how} ${label} on ${fromWords(g, s)}`;
}

const entryKey = (s: Spec): string => `entry ${s.id ?? `${fromTokens(s).join(', ')} → ${s.to} (${s.how})`}`;
const pathWords = (v: PersonaView, page: string): string => {
  const words = pathTo(v.reach, page).filter((e) => e.label !== '').map((e) => e.label);
  return words.length === 0 ? 'automatic' : words.join(' → ');
};

function pageFields(p: Page): Record<string, unknown> {
  return { route: p.route, title: p.titleKey ?? p.title, kind: p.kind, frame: p.frame, opensFor: p.opensFor ?? undefined, refusal: p.refusal ?? undefined, pageCodes: p.pageCodes ?? undefined, source: p.source ?? undefined, evidence: p.evidence ?? undefined };
}

export interface NavStructure {
  changes: Change[];
  entries: Array<[number, number]>; // entries matched on both sides: [index in the old specs, index in the new specs]
}

export function navStructure(b: NavSide, a: NavSide): NavStructure {
  const out: Change[] = [];
  const gb = b.g;
  const ga = a.g;

  // the graph itself
  const meta = (g: NavGraph): Record<string, unknown> => ({ id: g.id, title: g.title, status: g.status, summary: g.summary, locale: g.locale, adapter: g.adapter ?? undefined });
  out.push(
    ...fieldChanges('graph', 'graph', meta(gb), meta(ga), ['title', 'summary', 'status', 'locale', 'id', 'adapter'], (f, x, y) => {
      if (f === 'title') return `The graph is now called ${quote(String(y))} (was ${quote(String(x))}).`;
      if (f === 'summary') return `New summary: ${quote(String(y))}`;
      if (f === 'status') return `Status: ${String(x)} → ${String(y)}.`;
      if (f === 'locale') return `The wording is now in ${String(y)} (was ${String(x)}).`;
      return null;
    }),
  );
  deepDiff('citations', 'graph source', gb.source, ga.source, out);

  // frames
  for (const id of union(gb.frames, ga.frames)) {
    const x = gb.frames[id];
    const y = ga.frames[id];
    if (same(x, y)) continue;
    const key = `frame ${id}`;
    if (x === undefined || y === undefined) {
      const f = (y ?? x) as NonNullable<typeof x>;
      out.push({ area: 'frames', key, op: opOf(x, y), say: y === undefined ? `The frame ${quote(f.title)} was removed.` : `New frame: ${quote(f.title)}${f.menu === undefined ? '' : `, with the ${f.menu.toLowerCase()}`}.` });
      continue;
    }
    out.push(
      ...fieldChanges('frames', key, raw(x), raw(y), ['title', 'menu', 'shows'], (f, p, q) => {
        if (f === 'title') return `The frame ${quote(x.title)} is now called ${quote(y.title)}.`;
        if (f === 'menu') return q === undefined ? `${quote(y.title)} no longer carries a menu.` : `${quote(y.title)} now carries the ${String(q).toLowerCase()}${p === undefined ? '' : ` (was the ${String(p).toLowerCase()})`}.`;
        return null;
      }),
    );
  }

  // facts and named conditions: formal, listed under the exact changes; who they affect shows under behaviour
  for (const id of union(gb.facts, ga.facts)) {
    if (!same(gb.facts[id], ga.facts[id])) out.push({ area: 'facts', key: `fact ${id}`, op: opOf(gb.facts[id], ga.facts[id]), before: gb.facts[id], after: ga.facts[id], say: null });
  }
  for (const id of union(gb.guards, ga.guards)) {
    if (!same(gb.guards[id], ga.guards[id])) out.push({ area: 'facts', key: `guard ${id}`, op: opOf(gb.guards[id], ga.guards[id]), before: gb.guards[id], after: ga.guards[id], say: null });
  }

  // kinds of user
  const pb = Object.fromEntries(gb.personas.map((p) => [p.id, p]));
  const pa = Object.fromEntries(ga.personas.map((p) => [p.id, p]));
  for (const id of union(pb, pa)) {
    const x = pb[id];
    const y = pa[id];
    if (same(x, y)) continue;
    const key = `persona ${id}`;
    if (x === undefined) {
      out.push({ area: 'people', key, op: 'added', say: `New kind of user: ${quote(y.title)}, ${y.say}.` });
      continue;
    }
    if (y === undefined) {
      out.push({ area: 'people', key, op: 'removed', say: `Kind of user removed: ${quote(x.title)}.` });
      continue;
    }
    if (x.title !== y.title) out.push({ area: 'people', key, op: 'changed', field: 'title', before: x.title, after: y.title, say: `${quote(x.title)} is now called ${quote(y.title)}.` });
    if (x.say !== y.say) out.push({ area: 'people', key, op: 'changed', field: 'say', before: x.say, after: y.say, say: `${quote(y.title)} is now described as: ${y.say}.` });
    for (const fact of union(x.facts, y.facts)) {
      const p = x.facts[fact];
      const q = y.facts[fact];
      if (same(p, q)) continue;
      let say: string;
      if (Array.isArray(p) && Array.isArray(q)) {
        const ch = listChanges(p.map(String), q.map(String));
        const parts = [ch.added.length > 0 ? `now also has ${ch.added.join(', ')}` : '', ch.removed.length > 0 ? `no longer has ${ch.removed.join(', ')}` : ''].filter((s) => s !== '');
        say = `${quote(y.title)}: ${fact} ${parts.length > 0 ? parts.join('; ') : 'is in a new order'}.`;
      } else say = `${quote(y.title)}: ${fact} is now ${String(q)} (was ${String(p)}).`;
      out.push({ area: 'people', key, op: opOf(p, q), field: `facts.${fact}`, before: p, after: q, say });
    }
  }

  // pages
  for (const id of union(gb.pages, ga.pages)) {
    const x = gb.pages[id];
    const y = ga.pages[id];
    const key = `page ${id}`;
    if (x === undefined) {
      out.push({ area: 'pages', key, op: 'added', say: `New page ${pageName(ga, id)} (${y.route})${y.kind === 'screen' ? '' : `, ${KIND_WORDS[y.kind]}`}.` });
      continue;
    }
    if (y === undefined) {
      out.push({ area: 'pages', key, op: 'removed', say: `Page ${pageName(gb, id)} (${x.route}) removed.` });
      continue;
    }
    const name = pageName(ga, id);
    out.push(
      ...fieldChanges('pages', key, pageFields(x), pageFields(y), ['route', 'title', 'kind', 'frame', 'opensFor', 'refusal', 'pageCodes', 'source', 'evidence'], (f, p, q) => {
        if (f === 'route') return `${name} moved to ${String(q)} (was ${String(p)}).`;
        if (f === 'title') return `${pageName(gb, id)} is now titled ${name}.`;
        if (f === 'kind') return `${name} is now ${KIND_WORDS[String(q)]} (was ${KIND_WORDS[String(p)]}).`;
        if (f === 'frame') return `${name} is now drawn in ${quote(ga.frames[String(q)]?.title ?? String(q))} (was ${quote(gb.frames[String(p)]?.title ?? String(p))}).`;
        if (f === 'opensFor') return q === undefined ? `${name} now opens for everyone.` : p === undefined ? `${name} now opens only for some people.` : `Who ${name} opens for changed.`;
        if (f === 'refusal') return q === undefined ? `${name} no longer says how it refuses.` : `${name} now refuses with: ${quote(String(q))}.`;
        return null;
      }),
    );
  }

  // menus, links and buttons: entries are paired by their id or by where they start, where they go, how and their label;
  // what is left by (start, how, label) when the target changed, then by (start, target, how) when the label changed
  const sb = gb.specs;
  const sa = ga.specs;
  const k1 = (s: Spec): string => s.id ?? `${fromTokens(s).join(',')}|${s.to}|${s.how}|${s.label ?? ''}`;
  const k2 = (s: Spec): string | null => (s.id !== undefined ? null : `${fromTokens(s).join(',')}|${s.how}|${s.label ?? ''}`);
  const k3 = (s: Spec): string | null => (s.id !== undefined ? null : `${fromTokens(s).join(',')}|${s.to}|${s.how}`);
  const pairs = matchOrdered(sb, sa, [k1, k2, k3]);
  const inB = new Set(pairs.map((p) => p[0]));
  const inA = new Set(pairs.map((p) => p[1]));
  sa.forEach((s, j) => {
    if (inA.has(j)) return;
    out.push({ area: 'entries', key: entryKey(s), op: 'added', after: s, say: `New ${entryWords(ga, s)} → ${pageName(ga, s.to)}.` });
  });
  sb.forEach((s, i) => {
    if (inB.has(i)) return;
    out.push({ area: 'entries', key: entryKey(s), op: 'removed', before: s, say: `Removed: the ${entryWords(gb, s)} → ${pageName(gb, s.to)}.` });
  });
  const ENTRY_KNOWN = ['id', 'from', 'to', 'how', 'label', 'when', 'clicks', 'data', 'back'];
  for (const [i, j] of pairs) {
    const x = sb[i];
    const y = sa[j];
    const what = `The ${entryWords(ga, y)}`;
    out.push(
      ...fieldChanges('entries', entryKey(y), raw(x), raw(y), ENTRY_KNOWN, (f, p, q) => {
        if (f === 'to') return `${what} now opens ${pageName(ga, y.to)} (was ${pageName(gb, x.to)}).`;
        if (f === 'label') return `The ${entryWords(gb, x)} now reads ${quote(word(ga, y.label ?? ''))}.`;
        if (f === 'when') return `Who sees the ${entryWords(ga, y)} changed.`;
        if (f === 'clicks') return `${what} now takes ${String(q ?? 1)} clicks (was ${String(p ?? 1)}).`;
        if (f === 'data') return q === true ? `${what} now needs an item in a list.` : `${what} no longer needs an item in a list.`;
        if (f === 'back') return q === true ? `${what} now goes back.` : `${what} no longer goes back.`;
        return null;
      }),
    );
    otherKeys(entryKey(y), raw(x), raw(y), ENTRY_KNOWN, out);
  }
  // the order of a menu is what people see; other entries are not ordered for them
  const groups = new Map<string, Array<[number, number]>>();
  for (const [i, j] of pairs) {
    if (sb[i].how !== 'menu' || sa[j].how !== 'menu') continue;
    const g = fromTokens(sa[j]).join(',');
    groups.set(g, [...(groups.get(g) ?? []), [i, j]]);
  }
  for (const [g, group] of groups) {
    const posB = sb.map((s, i) => [s, i] as const).filter(([s]) => s.how === 'menu' && fromTokens(s).join(',') === g).map(([, i]) => i);
    const posA = sa.map((s, j) => [s, j] as const).filter(([s]) => s.how === 'menu' && fromTokens(s).join(',') === g).map(([, j]) => j);
    for (const [i, j] of moved(group)) {
      const s = sa[j];
      out.push({ area: 'entries', key: entryKey(s), op: 'moved', before: posB.indexOf(i) + 1, after: posA.indexOf(j) + 1, say: `In ${fromWords(ga, s)}, ${quote(word(ga, s.label ?? ''))} is now entry ${posA.indexOf(j) + 1} (was ${posB.indexOf(i) + 1}).` });
    }
  }

  // goals
  const goalsB = Object.fromEntries(gb.goals.map((x) => [x.id, x]));
  const goalsA = Object.fromEntries(ga.goals.map((x) => [x.id, x]));
  if (Object.keys(goalsB).length !== gb.goals.length || Object.keys(goalsA).length !== ga.goals.length) throw new GraphError('a goal id is written twice');
  const who = (g: NavGraph, ids: string[]): string => joinWords(ids.map((id) => quote(g.personas.find((p) => p.id === id)?.title ?? id)));
  for (const id of union(goalsB, goalsA)) {
    const x = goalsB[id];
    const y = goalsA[id];
    if (same(x, y)) continue;
    const key = `goal ${id}`;
    if (x === undefined) out.push({ area: 'goals', key, op: 'added', say: `New goal: ${quote(y.say)}, for ${who(ga, y.for)}, within ${y.within} ${y.within === 1 ? 'click' : 'clicks'}.` });
    else if (y === undefined) out.push({ area: 'goals', key, op: 'removed', say: `Goal dropped: ${quote(x.say)}.` });
    else {
      out.push(
        ...fieldChanges('goals', key, raw(x), raw(y), ['say', 'page', 'for', 'within'], (f) => {
          if (f === 'say') return `Goal reworded: ${quote(y.say)} (was ${quote(x.say)}).`;
          if (f === 'page') return `The goal ${quote(y.say)} now means reaching ${pageName(ga, y.page)} (was ${pageName(gb, x.page)}).`;
          if (f === 'for') return `The goal ${quote(y.say)} is now checked for ${who(ga, y.for)} (was ${who(gb, x.for)}).`;
          return `The goal ${quote(y.say)} now allows ${y.within} ${y.within === 1 ? 'click' : 'clicks'} (was ${x.within}).`;
        }),
      );
      otherKeys(key, raw(x), raw(y), ['id', 'say', 'page', 'for', 'within'], out);
    }
  }

  const nm = listChanges(gb.notModeled, ga.notModeled);
  for (const x of nm.added) out.push({ area: 'notModeled', key: 'notModeled', op: 'added', after: x, say: `Now listed as not modeled: ${quote(x)}.` });
  for (const x of nm.removed) out.push({ area: 'notModeled', key: 'notModeled', op: 'removed', before: x, say: `No longer listed as not modeled: ${quote(x)}.` });

  for (const k of union(gb.catalog, ga.catalog)) {
    const p = gb.catalog[k];
    const q = ga.catalog[k];
    if (p === q) continue;
    out.push({ area: 'wording', key: `catalog ${k}`, op: opOf(p, q), before: p, after: q, say: p === undefined ? `New text: ${quote(q)}.` : q === undefined ? `Text removed: ${quote(p)}.` : `${quote(p)} now reads ${quote(q)}.` });
  }
  return { changes: out, entries: pairs };
}

export interface NavRun {
  g: NavGraph;
  views: PersonaView[];
  goals: Goalcheck[];
  found: Finding[];
}

export function runNav(g: NavGraph, side: string): NavRun {
  try {
    const views = g.personas.map((p) => viewOf(g, p));
    const goals = checkGoals(g, views);
    return { g, views, goals, found: findings(g, views, goals) };
  } catch (err) {
    throw new GraphError(`${side}: ${(err as Error).message}`);
  }
}

const outcomeWords = (c: Goalcheck | undefined): string => (c === undefined ? 'not checked' : c.outcome === 'ok' ? `ok, ${c.clicks} of ${c.goal.within} ${c.goal.within === 1 ? 'click' : 'clicks'}` : c.outcome === 'too many clicks' ? `${c.clicks} clicks, the limit is ${c.goal.within}` : c.outcome);

export function navBehaviour(rb: NavRun, ra: NavRun, s: NavStructure): Shift[] {
  const out: Shift[] = [];
  const gb = rb.g;
  const ga = ra.g;
  const rowsB = menuRows(gb, rb.views);
  const rowsA = menuRows(ga, ra.views);
  const pairedA = new Map(s.entries.map(([i, j]) => [j, i]));
  const pairedB = new Set(s.entries.map(([i]) => i));

  for (const [ia, va] of ra.views.entries()) {
    const id = va.persona.id;
    const who = va.persona.title;
    const ib = rb.views.findIndex((v) => v.persona.id === id);
    const add = (topic: string, key: string, before: unknown, after: unknown, say: string, worse = false): void => {
      out.push({ topic, key: `${id} ${key}`, persona: who, before, after, say, detail: [], worse });
    };
    const reached = (v: PersonaView, p: string): boolean => v.standing[p] !== undefined && v.standing[p] !== 'unreachable';
    if (ib < 0) {
      const n = Object.keys(ga.pages).filter((p) => reached(va, p)).length;
      add('persona', 'new', null, va.landing, `A new kind of user: lands on ${pageName(ga, va.landing)} and reaches ${n} of ${Object.keys(ga.pages).length} pages.`);
      continue;
    }
    const vb = rb.views[ib];
    if (vb.landing !== va.landing) add('landing', 'landing', vb.landing, va.landing, `Lands on ${pageName(ga, va.landing)} (was ${pageName(gb, vb.landing)}).`);
    for (const page of union(gb.pages, ga.pages)) {
      const inB = gb.pages[page] !== undefined && reached(vb, page);
      const inA = ga.pages[page] !== undefined && reached(va, page);
      const name = ga.pages[page] === undefined ? pageName(gb, page) : pageName(ga, page);
      if (!inB && inA) {
        const st = va.standing[page];
        add('page', page, null, { clicks: va.reach.get(page)?.clicks, standing: st }, `Can now reach ${name}${st === 'ok' ? '' : `, which ${STANDING_WORDS[st]}`} (${va.reach.get(page)?.clicks ?? 0} clicks: ${pathWords(va, page)}).`);
      } else if (inB && !inA) {
        add('page', page, { clicks: vb.reach.get(page)?.clicks, standing: vb.standing[page] }, null, `Can no longer reach ${name}${ga.pages[page] === undefined ? ' (the page was removed)' : ''}.`);
      } else if (inB && inA) {
        const stB = vb.standing[page];
        const stA = va.standing[page];
        if (stB !== stA) add('page', page, stB, stA, `${capital(name)} ${STANDING_NOW[stA]} (it ${STANDING_PAST[stB]}).`);
        const cb = vb.reach.get(page)?.clicks ?? 0;
        const ca = va.reach.get(page)?.clicks ?? 0;
        if (cb !== ca) add('clicks', page, cb, ca, `${capital(name)}: ${cb} → ${ca} ${ca === 1 ? 'click' : 'clicks'} (${pathWords(va, page)}).`);
      }
    }
    // the menu they see
    for (const row of rowsA) {
      const shownA = row.cells[ia].shown;
      const i = pairedA.get(row.spec);
      const rowB = i === undefined ? undefined : rowsB.find((r) => r.spec === i);
      const shownB = rowB === undefined ? false : rowB.cells[ib].shown;
      const label = quote(row.entry.label);
      if (shownA && !shownB) add('menu', entryKey(ga.specs[row.spec]), false, true, `Their ${menuName(ga, row.frame)} now shows ${label}${i === undefined ? ' (a new entry)' : ''}.`);
      else if (!shownA && shownB) add('menu', entryKey(ga.specs[row.spec]), true, false, `Their ${menuName(ga, row.frame)} no longer shows ${label}.`);
    }
    for (const row of rowsB) {
      if (pairedB.has(row.spec) || !row.cells[ib].shown) continue;
      add('menu', entryKey(gb.specs[row.spec]), true, false, `Their ${menuName(gb, row.frame)} no longer shows ${quote(row.entry.label)} (the entry was removed).`);
    }
    // what they come to do
    for (const c of ra.goals.filter((x) => x.persona.id === id)) {
      const before = rb.goals.find((x) => x.persona.id === id && x.goal.id === c.goal.id);
      if (before !== undefined && before.outcome === c.outcome && before.clicks === c.clicks) continue;
      if (before === undefined && c.outcome === 'ok') continue;
      const worse = c.outcome !== 'ok' && (before === undefined || before.outcome === 'ok');
      add('goal', `goal ${c.goal.id}`, before?.outcome ?? null, c.outcome, `${quote(c.goal.say)}: ${outcomeWords(c)} (was ${outcomeWords(before)}).`, worse);
    }
  }

  // findings, compared line by line: a finding's headline carries counts, so its detail lines are what is compared. A new
  // line and a gone line that start the same way ("Ders nazırı: …") and are the only such pair are one case that changed.
  const lines = (f: Finding): string[] => (f.detail.length > 0 ? f.detail : [f.say]);
  const codes = union(Object.fromEntries(rb.found.map((f) => [f.code, f])), Object.fromEntries(ra.found.map((f) => [f.code, f])));
  for (const code of codes) {
    const fb = rb.found.find((f) => f.code === code);
    const fa = ra.found.find((f) => f.code === code);
    const lb = fb === undefined ? [] : lines(fb);
    const la = fa === undefined ? [] : lines(fa);
    let added = la.filter((l) => !lb.includes(l));
    let resolved = lb.filter((l) => !la.includes(l));
    const head = (l: string): string | null => (l.includes(': ') ? l.slice(0, l.indexOf(': ')) : null);
    const changed: string[] = [];
    for (const l of [...added]) {
      const h = head(l);
      if (h === null || added.filter((x) => head(x) === h).length > 1) continue;
      const was = resolved.filter((x) => head(x) === h);
      if (was.length !== 1) continue;
      changed.push(`${l} (was: ${was[0].slice(h.length + 2)})`);
      added = added.filter((x) => x !== l);
      resolved = resolved.filter((x) => x !== was[0]);
    }
    const level = (fa ?? fb) as Finding;
    const push = (say: string, detail: string[], worse: boolean, before: unknown, after: unknown): void => {
      out.push({ topic: 'finding', key: code, before, after, say, detail: detail.map((d) => `- ${d}`), worse });
    };
    if (fb === undefined && fa !== undefined) push(`New ${fa.level}: ${fa.say}`, fa.detail, fa.level === 'problem', null, la);
    else if (fb !== undefined && fa === undefined) push(`Resolved ${fb.level}: ${fb.say}`, fb.detail, false, lb, null);
    else {
      if (added.length > 0) push(`${capital(level.level)}, new cases: ${(fa as Finding).say}`, added, level.level === 'problem', null, added);
      if (resolved.length > 0) push(`${capital(level.level)}, cases resolved: ${(fb as Finding).say}`, resolved, false, resolved, null);
      if (changed.length > 0) push(`${capital(level.level)}, cases that changed: ${(fa as Finding).say}`, changed, false, null, changed);
    }
  }
  return out;
}

export function navSameBehaviour(r: NavRun): string {
  return `Nothing: each of the ${r.views.length} kinds of user lands on the same page, reaches the same pages in the same number of clicks and sees the same menu; goals and findings are the same.`;
}
