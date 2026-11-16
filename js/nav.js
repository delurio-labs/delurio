// A navigation graph (SPEC.md §6): the pages of an app, the ways between them, who may open them, and the walks each
// persona takes. The checker is the Rust one (crates/delurio-core/src/nav.rs); a graph loadNav read stays in the
// addon for the object it gave, and what the addon gives back names the graph's own entries, goals and personas.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { Trap, call, json, native, nativeNav, rememberNav, warnings } from './internal.js';

export const frameLevel = (e) => native.frameLevel(json(e));

export function loadNav(dir, locale, read = (file) => readFileSync(join(dir, file), 'utf8')) {
  const t = new Trap();
  try {
    const n = call(t, () => native.loadNav(String(dir), locale ?? undefined, t.reader(read)));
    return { g: rememberNav(JSON.parse(n.json()), n), warnings: n.warnings() };
  } finally {
    warnings();
  }
}

export function holds(g, p, expr) {
  return call(null, () => native.holds(nativeNav(g), json(p), expr ?? undefined));
}

export const refusedFor = (g, p, page) => call(null, () => native.refusedFor(nativeNav(g), json(p), String(page)));

// a reach as the addon gives it, its entry the graph's own
const reachOf = (g, r) => ({ page: r.page, clicks: r.clicks, via: r.via === null ? null : (g.entries[r.via] ?? null), prev: r.prev, usesData: r.usesData, byAddress: r.byAddress });

// a view as the addon reads it: its reach as the entries of the Map
const viewJson = (v) => ({ persona: v.persona, reach: v.reach instanceof Map ? [...v.reach] : Object.entries(v.reach ?? {}), standing: v.standing, landing: v.landing });

// Fewest clicks from "start"; ties go to the entry written first.
export function walk(g, p) {
  const entries = JSON.parse(call(null, () => native.walk(nativeNav(g), json(p))));
  return new Map(entries.map(([k, r]) => [k, reachOf(g, r)]));
}

export function pathTo(reach, page) {
  const list = [...reach];
  const way = JSON.parse(native.navPathTo(JSON.stringify(list), String(page)));
  return way.map((k) => (k === null ? undefined : list[k][1].via));
}

export function viewOf(g, p) {
  const v = JSON.parse(call(null, () => native.viewOf(nativeNav(g), json(p))));
  return { persona: p, reach: new Map(v.reach.map(([k, r]) => [k, reachOf(g, r)])), standing: Object.fromEntries(v.standing), landing: v.landing };
}

export const frameReached = (g, v, frame) => native.frameReached(nativeNav(g), JSON.stringify(viewJson(v)), String(frame));

// One row per menu entry written in the graph; a column per persona.
export function menuRows(g, views) {
  const rows = JSON.parse(call(null, () => native.menuRows(nativeNav(g), JSON.stringify(views.map(viewJson)))));
  return rows.map((r) => ({ spec: r.spec, frame: r.frame, entry: r.entry === null ? undefined : g.entries[r.entry], frameKnown: r.frameKnown, cells: r.cells }));
}

export function checkGoals(g, views) {
  const checks = JSON.parse(call(null, () => native.checkGoals(nativeNav(g), JSON.stringify(views.map(viewJson)))));
  return checks.map((c) => ({ goal: c.goal === null ? undefined : g.goals[c.goal], persona: c.persona === null ? undefined : views[c.persona].persona, outcome: c.outcome, clicks: c.clicks }));
}

export function findings(g, views, goals) {
  const checks = goals.map((c) => ({ goal: c.goal, persona: c.persona, outcome: c.outcome, clicks: c.clicks }));
  return JSON.parse(call(null, () => native.findings(nativeNav(g), JSON.stringify(views.map(viewJson)), JSON.stringify(checks))));
}

// the permission codes a guard asks `'code' in held` about, named guards followed
export function guardCodes(g, src, depth = 0) {
  return new Set(call(null, () => native.guardCodes(nativeNav(g), String(src), depth)));
}
