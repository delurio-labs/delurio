// The package's modules against the TypeScript checker's (the reference commit's dist/): the same calls on the same
// graphs give the same values, thrown errors included (their class name, message and place). It runs when
// DELURIO_TS_REF names the reference's dist folder (CI's differential job builds it); DELURIO_SETS adds folders of
// graphs to the examples (`:`-separated).
//   DELURIO_TS_REF=<ref>/dist node --test tests/node/differential.test.mjs
import assert from 'node:assert/strict';
import { appendFileSync, existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { test } from 'node:test';
import { pathToFileURL } from 'node:url';

const ref = process.env.DELURIO_TS_REF;
const NAMES = ['graph', 'model', 'story', 'wireframe', 'kit', 'edges', 'nav', 'expr', 'yaml', 'needs'];
// the most pairs an exploration may have for its configs to be compared one by one, and the most steps it may try for
// the flow to be explored on both sides at all (the TypeScript explorer holds every pair as objects: QAuth's consent
// flow has 5.4 million)
const MOST = Number(process.env.DELURIO_MOST_CONFIGS ?? 20000);
const LARGEST = Number(process.env.DELURIO_LARGEST_EXPLORATION ?? 250000);

async function modules(dir) {
  const out = {};
  for (const n of NAMES) out[n] = await import(pathToFileURL(join(dir, `${n}.js`)).href);
  return out;
}

// a value as JSON can compare it: Sets and Maps as their items, an error as its class, place and code (its wording is
// incidental: CONVENTIONS.md, ACCEPTANCE; an error a callback threw is the caller's own, compared by message too)
function plain(v, seen = new Set()) {
  if (v instanceof Error) return { error: v.constructor.name, name: v.name, place: v.place, code: v.code, line: v.linePos?.[0]?.line };
  if (v instanceof Set) return { set: [...v].map((x) => plain(x, seen)) };
  if (v instanceof Map) return { map: [...v].map(([k, x]) => [k, plain(x, seen)]) };
  if (Array.isArray(v)) return v.map((x) => plain(x, seen));
  if (v !== null && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, plain(x, seen)]));
  return v;
}

function attempt(run) {
  try {
    return { value: plain(run()) };
  } catch (e) {
    return { threw: plain(e) };
  }
}

function graphDirs(root, out = []) {
  if (!existsSync(root)) return out;
  if (existsSync(join(root, 'flow.yaml')) || existsSync(join(root, 'nav.yaml'))) out.push(root);
  for (const name of readdirSync(root).sort()) {
    if (['viz', 'video', 'recording', 'node_modules', '.git'].includes(name) || name.startsWith('.')) continue;
    const sub = join(root, name);
    if (statSync(sub).isDirectory()) graphDirs(sub, out);
  }
  return out;
}

// DELURIO_DIFF_LOG: a file each folder is written to as it starts and ends, with the process's memory
const log = (line) => {
  if (process.env.DELURIO_DIFF_LOG !== undefined) appendFileSync(process.env.DELURIO_DIFF_LOG, `${line} ${(process.memoryUsage().rss / 1e6).toFixed(0)}MB\n`);
};

const roots = ['examples', 'tests/node/fixtures', ...(process.env.DELURIO_SETS ?? '').split(':').filter((s) => s !== '')];
const dirs = roots.flatMap((r) => graphDirs(r));

// every call made on one side, by name: what it gave or threw
function flowCalls(M, dir, explorable) {
  const out = [];
  const record = (name, run) => out.push([name, attempt(run)]);
  let g;
  try {
    g = M.graph.load(dir);
  } catch (e) {
    out.push(['load', { threw: plain(e) }]);
    return out;
  }
  record('load', () => g);
  const states = Object.keys(g.flow.states);
  const nodes = Object.keys(g.ui.nodes);
  for (const s of states) {
    const st = g.flow.states[s];
    record(`titleOf ${s}`, () => M.graph.titleOf(g.flow, s));
    record(`kindOf ${s}`, () => M.edges.kindOf(g, s));
    record(`nodeLine ${s}`, () => M.edges.nodeLine(g, s));
    record(`exitLine ${s}`, () => M.edges.exitLine(st));
    if (st.response !== undefined) record(`responseLine ${s}`, () => M.edges.responseLine(st.response));
    for (const [event, spec] of Object.entries(st.on ?? {})) {
      record(`alternatives ${s} ${event}`, () => M.graph.alternatives(spec));
      record(`pressLabel ${s} ${event}`, () => M.edges.pressLabel(g, s, event));
      record(`transitionPath ${s} ${event}`, () => M.graph.transitionPath(s, st, event, 0));
    }
    for (const n of nodes) record(`shownIn ${n} ${s}`, () => M.graph.shownIn(g.ui.nodes[n], s));
    for (const t of states.slice(0, 12)) record(`focusAfter ${s} ${t}`, () => M.graph.focusAfter(g.ui, s, t));
  }
  for (const n of nodes) {
    record(`isDialog ${n}`, () => M.graph.isDialog(g.ui.nodes[n]));
    record(`nodeTexts ${n}`, () => {
      const warned = [];
      return [M.wireframe.nodeTexts(g, n, (w) => warned.push(w)), warned];
    });
  }
  for (const k of Object.keys(g.catalog).slice(0, 50)) record(`plain ${k}`, () => M.edges.plain(g, k));
  record('humanize', () => states.map((s) => M.graph.humanize(s)));
  let kit = null;
  record('resolveKit', () => (kit = M.kit.resolveKit(dir, g.ui.kit, undefined)));
  if (kit !== null) {
    record('kitCss page', () => M.kit.kitCss(kit, 'page'));
    record('kitCss stage', () => M.kit.kitCss(kit, 'stage'));
    record('kitStageScript', () => M.kit.kitStageScript(kit));
    for (const c of Object.keys(kit.components)) {
      record(`drawWith ${c}`, () => {
        const warned = [];
        const f = { id: 'n1', text: 'A <b>"text"</b>', name: 'A name', intent: 'danger', role: undefined, disabled: true, named: true, conditional: false, checked: true };
        return [M.kit.drawWith(kit, c, f, (w) => warned.push(w)), warned];
      });
    }
  }
  record('renderScreens', () => {
    const warned = [];
    return [M.wireframe.renderScreens(g, (w) => warned.push(w), kit), warned];
  });
  record('renderScreens without a kit', () => M.wireframe.renderScreens(g, () => undefined, null));
  record('renderCheckScreens', () => M.wireframe.renderCheckScreens(g, kit, () => undefined));
  for (const s of states) record(`renderStageScreen ${s}`, () => M.wireframe.renderStageScreen(g, s, nodes, () => undefined, kit));

  const m = new M.model.Model(g.flow);
  record('vars', () => [m.vars, m.domains]);
  if (!explorable) {
    out.push(['explore', { skipped: `more than ${LARGEST} steps` }]);
    return out;
  }
  record('initials', () => m.initials());
  record('transitionsOf', () => M.model.transitionsOf(g.flow));
  for (const name of Object.keys(g.flow.guards)) {
    record(`parseExpr ${name}`, () => M.expr.parseExpr(g.flow.guards[name]));
    record(`identifiers ${name}`, () => M.expr.identifiers(M.expr.parseExpr(g.flow.guards[name])));
    record(`pure ${name}`, () => m.pure(g.flow.guards[name]));
    for (const s of states.slice(0, 6)) record(`inState ${name} ${s}`, () => m.inState(g.flow.guards[name], s));
  }
  let ex;
  try {
    ex = M.model.explore(m);
  } catch (e) {
    out.push(['explore', { threw: plain(e) }]);
    return out;
  }
  record('explore', () => ({ starts: ex.starts, taken: ex.taken, atState: ex.atState, branchCounts: ex.branchCounts, size: ex.configs.length }));
  if (ex.configs.length <= MOST) record('configs', () => ex.configs);
  const some = ex.configs.slice(0, 40);
  for (const [i, c] of some.entries()) {
    record(`steps ${i}`, () => m.steps(c.state, c.ctx));
    record(`scenario ${i}`, () => m.scenario(c.ctx));
    record(`pathTo ${i}`, () => M.model.pathTo(m, ex, i));
    record(`stepsTo ${i}`, () => M.story.stepsTo(m, ex, i));
    for (const name of Object.keys(g.flow.guards)) {
      record(`test ${name} ${i}`, () => m.test(name, c.state, c.ctx));
      record(`value ${name} ${i}`, () => m.value(g.flow.guards[name], c.state, c.ctx));
    }
  }
  record('checkAssertions', () => M.model.checkAssertions(m, ex));
  const finals = M.model.finalPaths(m, ex);
  record('finalPaths', () => finals);
  record('collectEdges', () => M.edges.collectEdges(g, ex));
  record('mainStoryIndex', () => M.story.mainStoryIndex(g, ex));
  for (const f of finals) record(`narrate ${f.state}`, () => M.story.narrate(g, m, M.story.stepsTo(m, ex, f.index)));
  if (g.flow.main !== undefined) {
    record('stepsByEvents', () => M.story.stepsByEvents(m, g.flow.main));
    record('narrate main', () => {
      const steps = M.story.stepsByEvents(m, g.flow.main);
      return steps === null ? null : M.story.narrate(g, m, steps);
    });
  }
  if (ex.configs.length <= MOST) {
    for (const by of ['states', 'transitions']) record(`cover ${by}`, () => M.model.cover(m, by));
    record('cover kept', () => M.model.cover(m, 'states', m.initials(), (_, s) => s.event === null || !s.event.startsWith('CANCEL')));
    record('explore kept', () => {
      const kept = M.model.explore(m, m.initials(), (at, s) => m.steps(at.state, at.ctx).length > 0 && s.taken.endsWith('|0'));
      return { configs: kept.configs, taken: kept.taken };
    });
  }
  return out;
}

function navCalls(M, dir) {
  const out = [];
  const record = (name, run) => out.push([name, attempt(run)]);
  let g;
  try {
    ({ g } = M.nav.loadNav(dir));
  } catch (e) {
    out.push(['loadNav', { threw: plain(e) }]);
    return out;
  }
  record('loadNav', () => M.nav.loadNav(dir));
  const views = [];
  for (const p of g.personas) {
    record(`walk ${p.id}`, () => M.nav.walk(g, p));
    const v = M.nav.viewOf(g, p);
    views.push(v);
    record(`viewOf ${p.id}`, () => v);
    for (const page of Object.keys(g.pages)) {
      record(`refusedFor ${p.id} ${page}`, () => M.nav.refusedFor(g, p, page));
      record(`pathTo ${p.id} ${page}`, () => M.nav.pathTo(v.reach, page));
    }
    for (const frame of Object.keys(g.frames)) record(`frameReached ${p.id} ${frame}`, () => M.nav.frameReached(g, v, frame));
    for (const name of Object.keys(g.guards)) record(`holds ${p.id} ${name}`, () => M.nav.holds(g, p, g.guards[name]));
  }
  for (const name of Object.keys(g.guards)) record(`guardCodes ${name}`, () => M.nav.guardCodes(g, g.guards[name]));
  for (const e of g.entries) record(`frameLevel ${e.id}`, () => M.nav.frameLevel(e));
  record('menuRows', () => M.nav.menuRows(g, views));
  const goals = M.nav.checkGoals(g, views);
  record('checkGoals', () => goals);
  record('findings', () => M.nav.findings(g, views, goals));
  return out;
}

function fileCalls(M, root) {
  const out = [];
  const record = (name, run) => out.push([name, attempt(run)]);
  const files = [];
  const walk = (d) => {
    for (const n of readdirSync(d).sort()) {
      const p = join(d, n);
      if (statSync(p).isDirectory()) {
        if (!n.startsWith('.') && n !== 'node_modules') walk(p);
      } else if (n.endsWith('.yaml')) files.push(p);
    }
  };
  walk(root);
  for (const f of files) {
    const text = readFileSync(f, 'utf8');
    const name = relative(root, f);
    record(`unsupported ${name}`, () => M.yaml.unsupported(text));
    record(`readYaml ${name}`, () => M.yaml.readYaml(text));
    record(`keyAt ${name}`, () => M.yaml.keyAt(text, ['states', 'editing', 'on']));
  }
  record('checkNeeds', () => M.needs.checkNeeds(files.filter((f) => f.endsWith('needs.yaml') || f.endsWith('needs.gaps.yaml')), '.'));
  for (const d of graphDirs(root)) {
    const r = attempt(() => M.needs.readNeeds(d));
    out.push([`readNeeds ${d}`, r]);
    record(`planned ${d}`, () => M.needs.planned(M.needs.readNeeds(d)));
  }
  return out;
}

const compare = (a, b, what) => {
  const ka = new Map(a);
  for (const [name, x] of b) assert.deepEqual(x, ka.get(name), `${what}: ${name}`);
  assert.deepEqual(
    b.map(([n]) => n),
    a.map(([n]) => n),
    `${what}: the same calls`,
  );
};

if (ref === undefined) {
  test('differential against the TypeScript reference (DELURIO_TS_REF is not set)', { skip: true }, () => {});
} else {
  const T = await modules(ref);
  const R = await modules(new URL('../../js/', import.meta.url).pathname);
  for (const dir of dirs) {
    test(`the same values as the TypeScript checker: ${dir}`, () => {
      if (existsSync(join(dir, 'nav.yaml'))) return compare(navCalls(T, dir), navCalls(R, dir), dir);
      // whether the exploration is small enough, asked of the Rust explorer: its keep throws once LARGEST steps were
      // tried, which stops the exploration there (a keep that threw is not called again)
      let kept = 0;
      const stop = new Error('large');
      try {
        R.model.explore(new R.model.Model(R.graph.load(dir).flow), undefined, () => {
          if (++kept > LARGEST) throw stop;
          return true;
        });
      } catch (e) {
        if (e !== stop) kept = 0;
      }
      const explorable = kept <= LARGEST;
      log(`${dir} ${kept} steps`);
      const ts = flowCalls(T, dir, explorable);
      log(`${dir} TypeScript done`);
      const rust = flowCalls(R, dir, explorable);
      log(`${dir} Rust done`);
      compare(ts, rust, dir);
    });
  }
  for (const root of roots) {
    test(`the same YAML and needs as the TypeScript checker: ${root}`, () => compare(fileCalls(T, root), fileCalls(R, root), root));
  }
}
