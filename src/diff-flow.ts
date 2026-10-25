import { alternatives, GraphError, humanize, titleOf } from './graph.ts';
import type { ContextVar, Graph, Label, Transition, UiNode } from './graph.ts';
import { exitLine, kindOf, plain, pressLabel, responseLine } from './edges.ts';
import { explore, finalPaths, Model } from './model.ts';
import type { Exploration } from './model.ts';
import { mainStoryIndex, narrate, stepsByEvents, stepsTo } from './story.ts';
import type { PathStep } from './story.ts';
import { deepDiff, fieldChanges, joinWords, listChanges, matchOrdered, moved, otherKeys, quote, same, union } from './compare.ts';
import type { Change, Op, Shift } from './compare.ts';

// The revision diff of a flow graph: what changed in flow.yaml, ui.yaml, the catalog and domain.yaml, keyed by id,
// and what the flow does differently, found by exploring both revisions.

export interface FlowSide {
  g: Graph;
  domain: { file: string; doc: unknown } | null;
}

const raw = (x: object | undefined): Record<string, unknown> => (x ?? {}) as Record<string, unknown>;
const opOf = (b: unknown, a: unknown): Op => (b === undefined ? 'added' : a === undefined ? 'removed' : 'changed');
const BY_WORDS: Record<string, string> = { human: 'a person', ai: 'AI', agent: 'an agent', ci: 'CI' };

const stateName = (g: Graph, id: string): string => (g.flow.states[id] === undefined ? id : quote(titleOf(g.flow, id)));

function stepKind(g: Graph, id: string): string {
  const st = g.flow.states[id];
  if (st.type === 'final') return `an ending: ${exitLine(st)}`;
  if (st.goal === true) return 'a goal';
  if (st.by !== undefined) return `done by ${BY_WORDS[st.by] ?? st.by}`;
  const k = kindOf(g, id);
  return k === 'screen' ? 'a screen' : k === 'subflow' ? 'another flow' : 'a server step';
}

function invokeWords(v: unknown): string {
  const inv = raw(v as object);
  if (typeof inv.action === 'string') return `the action ${inv.action}`;
  if (typeof inv.flow === 'string') return `the flow ${inv.flow}`;
  if (typeof inv.external === 'string') return inv.external;
  return 'nothing';
}

// how people know a transition: the button that sends it, its `say`, or the event in words
function wayWords(g: Graph, state: string, event: string, t: Transition, index: number): string {
  if (event === 'always') return `rule ${index + 1} (${quote(t.say ?? t.guard ?? 'otherwise')})`;
  const press = pressLabel(g, state, event);
  if (press !== null) return `pressing ${quote(press)}`;
  if (t.say !== undefined) return quote(t.say);
  return event.startsWith('FAIL.') ? `the failure ${event.slice('FAIL.'.length)}` : humanize(event).toLowerCase();
}

const wayKey = (state: string, event: string, index: number, count: number): string =>
  event === 'always' ? `state ${state}, rule ${index + 1}` : `state ${state}, on ${event}${count > 1 ? ` #${index + 1}` : ''}`;

function labelWords(g: Graph, label: Label | undefined): string | null {
  if (label === undefined) return null;
  if (label.key !== undefined) return plain(g, label.key);
  if (label.literal !== undefined) return label.literal;
  if (label.oneOf !== undefined) return label.oneOf.map((k) => plain(g, k)).join(' / ');
  return null;
}

function nodeWords(g: Graph, id: string, n: UiNode): string {
  const what = humanize(n.component ?? n.type).toLowerCase();
  const text = labelWords(g, n.text) ?? labelWords(g, n.name);
  return text === null ? `${what} ${id}` : `${what} ${quote(text)}`;
}

const shownOn = (n: UiNode): string[] => (n.in ?? []).filter((s) => !(n.unless ?? []).includes(s));

function valueWords(name: string, cv: ContextVar, v: unknown): string {
  const said = cv.say?.[String(v)];
  return said !== undefined && said !== '' ? quote(said) : `${name} = ${String(v)}`;
}

export interface FlowStructure {
  changes: Change[];
  ways: Array<[string, string]>; // transitions matched on both sides, as the `taken` keys of the explorations: [before, after]
  added: string[]; // `taken` keys of transitions that only the new revision has
}

export function flowStructure(b: FlowSide, a: FlowSide): FlowStructure {
  const out: Change[] = [];
  const fb = b.g.flow;
  const fa = a.g.flow;
  const rb = raw(fb);
  const ra = raw(fa);

  // the flow itself
  out.push(
    ...fieldChanges('flow', 'flow', rb, ra, ['title', 'summary', 'status', 'initial', 'id', 'ui', 'domain', 'schema', 'kind'], (f, x, y) => {
      if (f === 'title') return `The flow is now called ${quote(fa.title ?? humanize(fa.id))} (was ${quote(fb.title ?? humanize(fb.id))}).`;
      if (f === 'summary') return y === undefined ? 'The summary was removed.' : `New summary: ${quote(String(y).trim())}`;
      if (f === 'status') return `Status: ${String(x ?? 'none')} → ${String(y ?? 'none')}.`;
      if (f === 'initial') return `It now starts at ${stateName(a.g, fa.initial)} (was ${stateName(b.g, fb.initial)}).`;
      return null;
    }),
  );
  for (const id of union(fb.decisions, fa.decisions)) {
    const x = fb.decisions?.[id];
    const y = fa.decisions?.[id];
    if (same(x, y)) continue;
    const key = `decision ${id}`;
    if (x === undefined || y === undefined) {
      const d = (y ?? x) as NonNullable<typeof x>;
      out.push({ area: 'flow', key, op: opOf(x, y), before: x, after: y, say: y === undefined ? `No longer open: ${quote(d.question)}.` : `New open decision: ${quote(d.question)}; recommended: ${d.recommend}.` });
      continue;
    }
    out.push(
      ...fieldChanges('flow', key, raw(x), raw(y), ['question', 'recommend', 'affects'], (f) => {
        if (f === 'question') return `An open decision is now worded ${quote(y.question)} (was ${quote(x.question)}).`;
        if (f === 'recommend') return `The recommendation for ${quote(y.question)} is now: ${y.recommend}.`;
        return `The decision ${quote(y.question)} now affects ${joinWords((y.affects ?? []).map((s) => stateName(a.g, s)))}.`;
      }),
    );
    otherKeys(key, raw(x), raw(y), ['question', 'recommend', 'affects'], out);
  }

  // what this view shows
  const sb = fb.scope ?? {};
  const sa = fa.scope ?? {};
  if (!same(sb.shows, sa.shows)) {
    out.push({ area: 'scope', key: 'scope', op: opOf(sb.shows, sa.shows), field: 'shows', before: sb.shows, after: sa.shows, say: sa.shows === undefined ? 'The view no longer says what it shows.' : `What this view shows: ${quote(sa.shows)}${sb.shows === undefined ? '' : ` (was ${quote(sb.shows)})`}.` });
  }
  if (!same(sb.world, sa.world)) {
    out.push({ area: 'scope', key: 'scope', op: opOf(sb.world, sa.world), field: 'world', before: sb.world, after: sa.world, say: sa.world === 'open' ? 'The view is now an open world: what is not drawn is not ruled out.' : 'The view is no longer an open world.' });
  }
  const oov = listChanges(sb.outOfView, sa.outOfView);
  for (const x of oov.added) out.push({ area: 'scope', key: 'scope', op: 'added', field: 'outOfView', after: x, say: `Now left out of this view: ${quote(x)}.` });
  for (const x of oov.removed) out.push({ area: 'scope', key: 'scope', op: 'removed', field: 'outOfView', before: x, say: `No longer left out of this view: ${quote(x)}.` });
  otherKeys('scope', raw(sb), raw(sa), ['shows', 'world', 'outOfView'], out);

  // the declared main story
  const norm = (m: typeof fa.main): unknown => (m === undefined ? undefined : Array.isArray(m) ? { given: {}, events: m } : { given: m.given ?? {}, events: m.events });
  if (!same(norm(fb.main), norm(fa.main))) {
    out.push({
      area: 'main',
      key: 'main',
      op: opOf(fb.main, fa.main),
      before: norm(fb.main),
      after: norm(fa.main),
      say: fa.main === undefined ? 'The flow no longer declares its main story; the first success exit through a screen is used.' : fb.main === undefined ? 'The flow now declares its main story.' : 'The declared main story sends other events, or starts from another situation.',
    });
  }

  // what it keeps track of
  for (const v of union(fb.context, fa.context)) {
    const x = fb.context[v];
    const y = fa.context[v];
    if (same(x, y)) continue;
    const key = `context ${v}`;
    if (x === undefined || y === undefined) {
      const cv = (y ?? x) as ContextVar;
      const any = cv.initial === 'any';
      const values = joinWords(cv.values.map((val) => valueWords(v, cv, val)));
      const say =
        y !== undefined
          ? any
            ? `New in the starting situation: ${values}.`
            : `The flow now keeps track of ${v} (it starts as ${String(cv.initial)}).`
          : any
            ? `No longer part of the starting situation: ${values}.`
            : `The flow no longer keeps track of ${v}.`;
      out.push({ area: 'situations', key, op: opOf(x, y), before: x, after: y, say });
      continue;
    }
    if (!same(x.values, y.values)) {
      const vals = listChanges(x.values.map(String), y.values.map(String));
      const parts: string[] = [];
      if (vals.added.length > 0) parts.push(`can now also be ${joinWords(vals.added.map((val) => valueWords(v, y, val)))}`);
      if (vals.removed.length > 0) parts.push(`can no longer be ${joinWords(vals.removed.map((val) => valueWords(v, x, val)))}`);
      if (parts.length === 0) parts.push('lists its values in a new order (the order in which starting situations are tried)');
      out.push({ area: 'situations', key, op: 'changed', field: 'values', before: x.values, after: y.values, say: `${v} ${parts.join('; ')}.` });
    }
    if (!same(x.initial, y.initial)) {
      const w = (i: unknown): string => (i === 'any' ? 'any of its values' : String(i));
      out.push({ area: 'situations', key, op: 'changed', field: 'initial', before: x.initial, after: y.initial, say: `${v} now starts as ${w(y.initial)} (was ${w(x.initial)}).` });
    }
    for (const val of union(x.say, y.say)) {
      const p = x.say?.[val];
      const q = y.say?.[val];
      if (p === q) continue;
      // an empty `say` leaves the value out of the starting sentence
      const words = (t: string): string => (t === '' ? 'left out of the starting sentence' : `described as ${quote(t)}`);
      const say = q === undefined ? `${v} = ${val} is no longer described in words.` : p === undefined || p === '' || q === '' ? `${v} = ${val} is now ${words(q)}.` : `${quote(p)} now reads ${quote(q)}.`;
      out.push({ area: 'situations', key, op: opOf(p, q), field: `say.${val}`, before: p, after: q, say });
    }
    otherKeys(key, raw(x), raw(y), ['values', 'initial', 'say'], out);
  }

  // named conditions and effects: formal, so only listed under the exact changes; their consequences show in behaviour
  for (const n of union(fb.guards, fa.guards)) {
    if (!same(fb.guards[n], fa.guards[n])) out.push({ area: 'conditions', key: `guard ${n}`, op: opOf(fb.guards[n], fa.guards[n]), before: fb.guards[n], after: fa.guards[n], say: null });
  }
  for (const n of union(fb.effects, fa.effects)) {
    if (!same(fb.effects[n], fa.effects[n])) out.push({ area: 'conditions', key: `effect ${n}`, op: opOf(fb.effects[n], fa.effects[n]), before: fb.effects[n], after: fa.effects[n], say: null });
  }

  // steps
  const STATE_KNOWN = ['title', 'description', 'type', 'invoke', 'on', 'always', 'redirect', 'code', 'carries', 'leavesPage', 'by', 'goal', 'includes', 'response', 'source'];
  const ENDING = ['type', 'redirect', 'code', 'carries', 'leavesPage'];
  for (const id of union(fb.states, fa.states)) {
    const x = fb.states[id];
    const y = fa.states[id];
    const key = `state ${id}`;
    if (x === undefined) {
      out.push({ area: 'steps', key, op: 'added', say: `New step ${stateName(a.g, id)} (${stepKind(a.g, id)}).` });
      continue;
    }
    if (y === undefined) {
      out.push({ area: 'steps', key, op: 'removed', say: `Step ${stateName(b.g, id)} removed.` });
      continue;
    }
    const name = stateName(a.g, id);
    out.push(
      ...fieldChanges('steps', key, raw(x), raw(y), ['title', 'by', 'goal', 'description', 'invoke', 'includes', 'response', 'source'], (f, p, q) => {
        if (f === 'title') return `${stateName(b.g, id)} is now called ${name}.`;
        if (f === 'by') return q === undefined ? `${name} no longer says who does it.` : `${name} is now done by ${BY_WORDS[String(q)] ?? String(q)}${p === undefined ? '' : ` (was ${BY_WORDS[String(p)] ?? String(p)})`}.`;
        if (f === 'goal') return q === true ? `${name} is now a goal.` : `${name} is no longer a goal.`;
        if (f === 'description') return q === undefined ? `${name} has no description any more.` : `${name} has a new description: ${quote(String(q))}.`;
        if (f === 'invoke') return `${name} now calls ${invokeWords(q)} (was ${invokeWords(p)}).`;
        if (f === 'response') return q === undefined ? `${name} no longer says what it answers.` : `${name} now answers ${quote(responseLine(q as Record<string, unknown>))}${p === undefined ? '' : ` (was ${quote(responseLine(p as Record<string, unknown>))})`}.`;
        const inc = listChanges(p as string[] | undefined, q as string[] | undefined);
        const parts = [inc.added.length > 0 ? `now also covers ${joinWords(inc.added.map(quote))}` : '', inc.removed.length > 0 ? `no longer covers ${joinWords(inc.removed.map(quote))}` : ''].filter((s) => s !== '');
        return `${name} ${parts.length > 0 ? parts.join('; ') : 'lists what it covers in a new order'}.`;
      }),
    );
    const eb = Object.fromEntries(ENDING.map((f) => [f, raw(x)[f]]));
    const ea = Object.fromEntries(ENDING.map((f) => [f, raw(y)[f]]));
    if (!same(eb, ea)) {
      const say = y.type === 'final' && x.type === 'final' ? `${name} now ends with: ${exitLine(y)} (was ${exitLine(x)}).` : y.type === 'final' ? `${name} is now an ending: ${exitLine(y)}.` : x.type === 'final' ? `${name} is no longer an ending.` : `${name} changed its type.`;
      out.push({ area: 'steps', key, op: 'changed', field: 'ending', before: eb, after: ea, say });
    }
    otherKeys(key, raw(x), raw(y), STATE_KNOWN, out);
  }

  // how it moves on: the alternatives of one event, and the rules of an `always`, are ordered (the first that matches wins).
  // They are paired by their condition, then by their `say`, then by their target, so an inserted rule does not make the
  // rules after it look rewritten; a pair that changed place relative to the other pairs is reported as moved.
  const ways: Array<[string, string]> = [];
  const added: string[] = [];
  const TRANSITION_KNOWN = ['target', 'guard', 'effect', 'say', 'starts', 'source'];
  for (const id of union(fb.states, fa.states)) {
    const x = fb.states[id];
    const y = fa.states[id];
    const lists: Array<[string, Transition[], Transition[]]> = [['always', x?.always ?? [], y?.always ?? []]];
    for (const event of union(x?.on, y?.on)) lists.push([event, alternatives(x?.on?.[event] ?? []), alternatives(y?.on?.[event] ?? [])]);
    for (const [event, bl, al] of lists) {
      const pairs = matchOrdered(bl, al, [(t) => t.guard ?? '', (t) => t.say ?? null, (t) => t.target]);
      const from = (g: Graph, i: number, t: Transition): string => (event === 'always' ? `in ${stateName(g, id)}, ${wayWords(g, id, event, t, i)}` : `from ${stateName(g, id)}, ${wayWords(g, id, event, t, i)}`);
      const pb = new Set(pairs.map((p) => p[0]));
      const pa = new Set(pairs.map((p) => p[1]));
      al.forEach((t, i) => {
        if (pa.has(i)) return;
        added.push(`${id}|${event}|${i}`);
        out.push({ area: 'ways', key: wayKey(id, event, i, al.length), op: 'added', after: t, say: `New: ${from(a.g, i, t)} leads to ${stateName(a.g, t.target)}.` });
      });
      bl.forEach((t, i) => {
        if (pb.has(i)) return;
        out.push({ area: 'ways', key: wayKey(id, event, i, bl.length), op: 'removed', before: t, say: y === undefined ? null : `Removed: ${from(b.g, i, t)} led to ${stateName(b.g, t.target)}.` });
      });
      for (const [i, j] of pairs) {
        ways.push([`${id}|${event}|${i}`, `${id}|${event}|${j}`]);
        const p = bl[i];
        const q = al[j];
        const lead = from(a.g, j, q);
        const cap = lead.charAt(0).toUpperCase() + lead.slice(1);
        out.push(
          ...fieldChanges('ways', wayKey(id, event, j, al.length), raw(p), raw(q), TRANSITION_KNOWN, (f) => {
            if (f === 'target') return `${cap} now leads to ${stateName(a.g, q.target)} (was ${stateName(b.g, p.target)}).`;
            if (f === 'guard') return `${cap}: the condition changed.`;
            if (f === 'effect') return `${cap}: what it records changed.`;
            if (f === 'starts') return q.starts === undefined ? `${cap} no longer starts ${p.starts?.flow}.` : p.starts === undefined ? `${cap} now starts ${q.starts.flow}.` : `${cap}: the flow it starts, or what that flow begins with, changed.`;
            const where = event === 'always' ? `In ${stateName(a.g, id)}, rule ${j + 1}` : `From ${stateName(a.g, id)}, ${wayWords(a.g, id, event, { target: q.target }, j)}`;
            return q.say === undefined ? `${where} is no longer described in words (was ${quote(p.say ?? '')}).` : p.say === undefined ? `${where} is now described as ${quote(q.say)}.` : `${where}: ${quote(p.say)} now reads ${quote(q.say)}.`;
          }),
        );
        otherKeys(wayKey(id, event, j, al.length), raw(p), raw(q), TRANSITION_KNOWN, out);
      }
      for (const [i, j] of moved(pairs)) {
        const t = al[j];
        const say = event === 'always' ? `In ${stateName(a.g, id)}, ${quote(t.say ?? t.guard ?? 'otherwise')} is now rule ${j + 1} (was ${i + 1}); the first rule that matches wins.` : `From ${stateName(a.g, id)}, the alternatives of ${wayWords(a.g, id, event, t, j)} are checked in a new order.`;
        out.push({ area: 'ways', key: wayKey(id, event, j, al.length), op: 'moved', before: i + 1, after: j + 1, say });
      }
    }
  }

  // guarantees
  const byId = (list: Graph['flow']['assertions'], side: string): Record<string, { id: string; never: string; say?: string }> => {
    const m: Record<string, { id: string; never: string; say?: string }> = {};
    for (const x of list ?? []) {
      if (m[x.id] !== undefined) throw new GraphError(`${side}: the assertion ${x.id} is written twice`);
      m[x.id] = x;
    }
    return m;
  };
  const asb = byId(fb.assertions, 'before');
  const asa = byId(fa.assertions, 'after');
  for (const id of union(asb, asa)) {
    const x = asb[id];
    const y = asa[id];
    if (same(x, y)) continue;
    const key = `assertion ${id}`;
    if (x === undefined) out.push({ area: 'guarantees', key, op: 'added', after: y.never, say: `New guarantee: ${quote(y.say ?? humanize(id))}.` });
    else if (y === undefined) out.push({ area: 'guarantees', key, op: 'removed', before: x.never, say: `Guarantee dropped: ${quote(x.say ?? humanize(id))}.` });
    else {
      out.push(
        ...fieldChanges('guarantees', key, raw(x), raw(y), ['never', 'say'], (f) =>
          f === 'never' ? `The guarantee ${quote(y.say ?? humanize(id))} now checks a different condition.` : `Guarantee reworded: ${quote(y.say ?? humanize(id))} (was ${quote(x.say ?? humanize(id))}).`,
        ),
      );
      otherKeys(key, raw(x), raw(y), ['id', 'never', 'say'], out);
    }
  }
  otherKeys('flow', rb, ra, ['schema', 'kind', 'id', 'status', 'ui', 'title', 'summary', 'scope', 'context', 'guards', 'effects', 'main', 'initial', 'states', 'assertions', 'decisions', 'domain'], out);

  // what the user sees
  const ub = b.g.ui;
  const ua = a.g.ui;
  for (const k of union(ub.samples, ua.samples)) {
    const p = ub.samples?.[k];
    const q = ua.samples?.[k];
    if (p === q) continue;
    out.push({ area: 'screens', key: 'ui samples', op: opOf(p, q), field: k, before: p, after: q, say: q === undefined ? `The sample ${k} is gone.` : `The sample ${k} is now ${quote(q)}${p === undefined ? '' : ` (was ${quote(p)})`}.` });
  }
  const NODE_KNOWN = ['type', 'component', 'role', 'intent', 'name', 'text', 'in', 'unless', 'when', 'disabledIn', 'emits', 'source'];
  for (const id of union(ub.nodes, ua.nodes)) {
    const x = ub.nodes[id];
    const y = ua.nodes[id];
    if (same(x, y)) continue;
    const key = `ui node ${id}`;
    const on = (g: Graph, n: UiNode): string => (shownOn(n).length === 0 ? 'no screen' : joinWords(shownOn(n).map((s) => stateName(g, s))));
    if (x === undefined) {
      out.push({ area: 'screens', key, op: 'added', say: `New on ${on(a.g, y)}: ${nodeWords(a.g, id, y)}.` });
      continue;
    }
    if (y === undefined) {
      out.push({ area: 'screens', key, op: 'removed', say: `Removed from ${on(b.g, x)}: ${nodeWords(b.g, id, x)}.` });
      continue;
    }
    const what = nodeWords(a.g, id, y);
    const cap = what.charAt(0).toUpperCase() + what.slice(1);
    out.push(
      ...fieldChanges('screens', key, raw(x), raw(y), NODE_KNOWN, (f, p, q) => {
        if (f === 'in' || f === 'unless' || f === 'disabledIn') {
          const before = f === 'disabledIn' ? (x.disabledIn ?? []) : shownOn(x);
          const after = f === 'disabledIn' ? (y.disabledIn ?? []) : shownOn(y);
          const ch = listChanges(before, after);
          const verb = f === 'disabledIn' ? 'disabled' : 'shown';
          const parts = [ch.added.length > 0 ? `is now ${verb} on ${joinWords(ch.added.map((s) => stateName(a.g, s)))}` : '', ch.removed.length > 0 ? `is no longer ${verb} on ${joinWords(ch.removed.map((s) => stateName(b.g, s)))}` : ''].filter((s) => s !== '');
          return parts.length === 0 ? null : `${cap} ${parts.join('; ')}.`;
        }
        if (f === 'text') return `${quote(labelWords(b.g, x.text) ?? '')} now reads ${quote(labelWords(a.g, y.text) ?? '')}.`;
        if (f === 'name') return `The accessible name of the ${what} is now ${quote(labelWords(a.g, y.name) ?? '(from another node)')} (was ${quote(labelWords(b.g, x.name) ?? '(from another node)')}).`;
        if (f === 'when') return q === undefined ? `${cap} no longer depends on a flag.` : `${cap} is now shown only when ${String(q)}.`;
        if (f === 'emits') return q === undefined ? `${cap} no longer sends an event.` : `${cap} now sends ${String(q)}${p === undefined ? '' : ` (was ${String(p)})`}.`;
        if (f === 'component' || f === 'type') return `${cap} is now drawn as ${humanize(String(q ?? 'nothing')).toLowerCase()} (was ${humanize(String(p ?? 'nothing')).toLowerCase()}).`;
        return `${cap} now has the ${f} ${String(q ?? 'none')} (was ${String(p ?? 'none')}).`;
      }),
    );
    otherKeys(key, raw(x), raw(y), NODE_KNOWN, out);
  }
  otherKeys('ui', raw(ub), raw(ua), ['schema', 'kind', 'id', 'samples', 'nodes'], out);

  // wording
  for (const k of union(b.g.catalog, a.g.catalog)) {
    const p = b.g.catalog[k];
    const q = a.g.catalog[k];
    if (p === q) continue;
    const say = p === undefined ? `New text: ${quote(plain(a.g, k))}.` : q === undefined ? `Text removed: ${quote(plain(b.g, k))}.` : `${quote(plain(b.g, k))} now reads ${quote(plain(a.g, k))}.`;
    out.push({ area: 'wording', key: `catalog ${k}`, op: opOf(p, q), before: p, after: q, say });
  }

  // the API it calls
  if (b.domain === null && a.domain !== null) out.push({ area: 'domain', key: 'domain', op: 'added', after: a.domain.file, say: `The graph now has an API description (${a.domain.file}).` });
  else if (b.domain !== null && a.domain === null) out.push({ area: 'domain', key: 'domain', op: 'removed', before: b.domain.file, say: `The API description (${b.domain.file}) is no longer next to the graph.` });
  else if (b.domain !== null && a.domain !== null) deepDiff('domain', 'domain', b.domain.doc, a.domain.doc, out);

  return { changes: out, ways, added };
}

interface Run {
  g: Graph;
  m: Model;
  ex: Exploration;
  violations: Map<string, number>; // assertion id -> the first (shortest) situation that breaks it
  exits: Map<string, number>; // final state -> the first (shortest) situation that reaches it
  moves: number; // ways from one reachable situation to another
  main: PathStep[] | null;
  mainBroken: boolean; // a declared main story whose events cannot be walked
  reached: Set<string>;
}

function run(g: Graph): Run {
  const m = new Model(g.flow);
  const ex = explore(m);
  const violations = new Map<string, number>();
  for (const a of g.flow.assertions ?? []) {
    const hit = ex.configs.findIndex((c) => m.test(a.never, c.state, c.ctx));
    if (hit >= 0) violations.set(a.id, hit);
  }
  const exits = new Map(finalPaths(m, ex).map((f) => [f.state, f.index]));
  let moves = 0;
  for (const c of ex.configs) moves += m.steps(c.state, c.ctx).length;
  const declared = g.flow.main === undefined ? null : stepsByEvents(m, g.flow.main);
  const index = mainStoryIndex(g, ex);
  const main = declared ?? (index === null ? null : stepsTo(m, ex, index));
  return { g, m, ex, violations, exits, moves, main, mainBroken: g.flow.main !== undefined && declared === null, reached: new Set(ex.configs.map((c) => c.state)) };
}

// Runs one side; an `always` that is not exhaustive, an unknown name in a guard and the like stop the diff with the side named.
export function runSide(g: Graph, side: string): Run {
  try {
    return run(g);
  } catch (err) {
    throw new GraphError(`${side}: ${(err as Error).message}`);
  }
}

const capital = (s: string): string => s.charAt(0).toUpperCase() + s.slice(1);

// a path as people read it: the starting situation, then one numbered line per step, the ending last
function told(r: Run, steps: PathStep[]): string[] {
  const story = narrate(r.g, r.m, steps);
  const lines = r.g.flow.states[steps[steps.length - 1].state].type === 'final' ? [...story.steps, story.result] : story.steps;
  return [`When: ${story.given}.`, ...lines.map((s, i) => `${i + 1}. ${s}`)];
}

const chain = (r: Run, steps: PathStep[]): string => steps.map((s) => titleOf(r.g.flow, s.state)).join(' → ');

// the same path in both revisions: the same steps taken by the same events, from the same values of what both start with
function samePath(rb: Run, ra: Run, b: PathStep[], a: PathStep[]): boolean {
  const walk = (steps: PathStep[]): string => steps.map((s) => `${s.event ?? ''}>${s.state}`).join(' ');
  if (walk(b) !== walk(a)) return false;
  const common = rb.m.vars.filter((v) => rb.g.flow.context[v].initial === 'any' && ra.g.flow.context[v]?.initial === 'any');
  return common.every((v) => b[0].ctx[rb.m.vars.indexOf(v)] === a[0].ctx[ra.m.vars.indexOf(v)]);
}

function transitionAt(g: Graph, taken: string): { state: string; event: string; index: number; t: Transition } {
  const [state, event, idx] = taken.split('|');
  const st = g.flow.states[state];
  const index = Number(idx);
  const t = event === 'always' ? (st.always ?? [])[index] : alternatives(st.on?.[event] ?? [])[index];
  return { state, event, index, t };
}

export function flowBehaviour(rb: Run, ra: Run, s: FlowStructure): Shift[] {
  const out: Shift[] = [];
  const fb = rb.g.flow;
  const fa = ra.g.flow;
  const shift = (topic: string, key: string, before: unknown, after: unknown, say: string, detail: string[] = [], worse = false): void => {
    out.push({ topic, key, before, after, say, detail, worse });
  };

  // guarantees first: a broken one is what a reviewer must see
  const said = (id: string): string => quote(fa.assertions?.find((x) => x.id === id)?.say ?? fb.assertions?.find((x) => x.id === id)?.say ?? humanize(id));
  for (const id of union(Object.fromEntries((fb.assertions ?? []).map((x) => [x.id, x])), Object.fromEntries((fa.assertions ?? []).map((x) => [x.id, x])))) {
    const inB = (fb.assertions ?? []).some((x) => x.id === id);
    const inA = (fa.assertions ?? []).some((x) => x.id === id);
    const vb = rb.violations.get(id);
    const va = ra.violations.get(id);
    const at = (r: Run, i: number): { state: string; start: Record<string, unknown>; events: string[] } => {
      const steps = stepsTo(r.m, r.ex, i);
      return { state: r.ex.configs[i].state, start: r.m.scenario(steps[0].ctx), events: steps.flatMap((x) => (x.event === null ? [] : [x.event])) };
    };
    if (inA && va !== undefined && vb === undefined) {
      const lead = inB ? 'A guarantee no longer holds' : 'A new guarantee does not hold';
      shift('guarantee', id, inB ? 'holds' : 'absent', { violated: at(ra, va) }, `${lead}: ${said(id)}. It breaks in ${stateName(ra.g, ra.ex.configs[va].state)}:`, told(ra, stepsTo(ra.m, ra.ex, va)), true);
    } else if (inA && inB && vb !== undefined && va === undefined) {
      shift('guarantee', id, { violated: at(rb, vb) }, 'holds', `A guarantee now holds: ${said(id)} (it was broken).`);
    } else if (inA && inB && vb !== undefined && va !== undefined && !samePath(rb, ra, stepsTo(rb.m, rb.ex, vb), stepsTo(ra.m, ra.ex, va))) {
      shift('guarantee', id, { violated: at(rb, vb) }, { violated: at(ra, va) }, `Still broken, now first in ${stateName(ra.g, ra.ex.configs[va].state)}: ${said(id)}.`, told(ra, stepsTo(ra.m, ra.ex, va)));
    } else if (!inA && inB && vb !== undefined) {
      shift('guarantee', id, { violated: at(rb, vb) }, 'absent', `No longer checked, and it was broken: ${said(id)}.`);
    }
  }

  // the main story
  if (rb.mainBroken !== ra.mainBroken) {
    if (ra.mainBroken) shift('main', 'main', 'walks', 'broken', 'The declared main story can no longer be walked: an event is not possible where it is sent.', [], true);
    else shift('main', 'main', 'broken', 'walks', 'The declared main story can be walked again.');
  }
  if (rb.main !== null && ra.main === null) shift('main', 'main', chain(rb, rb.main), null, `There is no main story any more (it was: ${chain(rb, rb.main)}).`);
  else if (rb.main === null && ra.main !== null) shift('main', 'main', null, chain(ra, ra.main), 'There is now a main story:', told(ra, ra.main));
  else if (rb.main !== null && ra.main !== null && !samePath(rb, ra, rb.main, ra.main)) {
    shift('main', 'main', chain(rb, rb.main), chain(ra, ra.main), `The main story changed. It was: ${chain(rb, rb.main)}. Now:`, told(ra, ra.main));
  }

  // endings
  const finals = union(fb.states, fa.states).filter((id) => fa.states[id]?.type === 'final' || fb.states[id]?.type === 'final');
  for (const id of finals) {
    const ib = rb.exits.get(id);
    const ia = ra.exits.get(id);
    if (ib === undefined && ia !== undefined) {
      shift('exit', id, 'unreachable', 'reachable', `New way to end: ${stateName(ra.g, id)} (${exitLine(fa.states[id])}).`, told(ra, stepsTo(ra.m, ra.ex, ia)));
    } else if (ib !== undefined && ia === undefined) {
      shift('exit', id, 'reachable', 'unreachable', `No longer reachable: ${stateName(rb.g, id)}${fa.states[id] === undefined ? ' (the ending was removed)' : ''}.`);
    } else if (ib !== undefined && ia !== undefined) {
      const pb = stepsTo(rb.m, rb.ex, ib);
      const pa = stepsTo(ra.m, ra.ex, ia);
      if (samePath(rb, ra, pb, pa)) continue;
      const nb = pb.length - 1;
      const na = pa.length - 1;
      const how = na === nb ? 'is now reached another way' : `is now reached in ${na} steps (was ${nb})`;
      shift('path', id, chain(rb, pb), chain(ra, pa), `${capital(stateName(ra.g, id))} ${how}. It was: ${chain(rb, pb)}. Now:`, told(ra, pa));
    }
  }

  // steps no situation reaches, and transitions no situation takes
  for (const id of Object.keys(fa.states)) {
    if (fa.states[id].type === 'final') continue;
    const before = fb.states[id] === undefined ? null : rb.reached.has(id);
    const after = ra.reached.has(id);
    if (before === true && !after) shift('reach', id, true, false, `${capital(stateName(ra.g, id))} can no longer be reached.`);
    else if (before === false && after) shift('reach', id, false, true, `${capital(stateName(ra.g, id))} can now be reached.`);
    else if (before === null && !after) shift('reach', id, null, false, `The new step ${stateName(ra.g, id)} is never reached.`);
  }
  const wayName = (g: Graph, taken: string): string => {
    const w = transitionAt(g, taken);
    return w.event === 'always' ? `in ${stateName(g, w.state)}, ${wayWords(g, w.state, w.event, w.t, w.index)}` : `from ${stateName(g, w.state)}, ${wayWords(g, w.state, w.event, w.t, w.index)}`;
  };
  for (const [kb, ka] of s.ways) {
    const st = ka.split('|')[0];
    if (!rb.reached.has(st) || !ra.reached.has(st)) continue;
    const tb = rb.ex.taken.has(kb);
    const ta = ra.ex.taken.has(ka);
    if (tb && !ta) shift('taken', ka, true, false, `${capital(wayName(ra.g, ka))} is never taken any more.`);
    else if (!tb && ta) shift('taken', ka, false, true, `${capital(wayName(ra.g, ka))} is now taken (it never was).`);
  }
  for (const ka of s.added) {
    if (ra.reached.has(ka.split('|')[0]) && !ra.ex.taken.has(ka)) shift('taken', ka, null, false, `The new way ${wayName(ra.g, ka)} is never taken.`);
  }

  const nb = rb.ex.configs.length;
  const na = ra.ex.configs.length;
  if (na !== nb || ra.ex.starts !== rb.ex.starts || ra.moves !== rb.moves) {
    const starts = ra.ex.starts === rb.ex.starts ? `${ra.ex.starts}` : `${rb.ex.starts} → ${ra.ex.starts}`;
    const say =
      na === nb && ra.ex.starts === rb.ex.starts
        ? `The same ${na} situations, with ${ra.moves} moves between them (was ${rb.moves}): what changed connects situations that were already reachable.`
        : `Situations the model can reach: ${nb} → ${na}, from ${starts} starting situations; moves between them: ${rb.moves} → ${ra.moves}.`;
    shift('situations', 'situations', { situations: nb, starts: rb.ex.starts, moves: rb.moves }, { situations: na, starts: ra.ex.starts, moves: ra.moves }, say);
  }
  return out;
}

// the sentence for a revision that changes nothing a person can observe
export function flowSameBehaviour(r: Run): string {
  const ends = r.exits.size;
  const guarantees = (r.g.flow.assertions ?? []).length;
  const parts = [ends > 0 ? `the same ${ends === 1 ? 'ending is' : `${ends} endings are`} reached the same way` : '', guarantees > 0 ? `the ${guarantees} guarantees give the same result` : '', r.main !== null ? 'the main story is the same' : ''].filter((x) => x !== '');
  return `Nothing: ${parts.length > 0 ? `${joinWords(parts)}, ` : ''}in the same ${r.ex.configs.length} situations.`;
}
