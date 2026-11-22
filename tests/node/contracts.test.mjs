// What delurio's own code relies on in the package's modules (the inventory of its imports): real error classes,
// callbacks that throw, Sets and Maps where the TypeScript modules gave them, the keys of the YAML kept, copies of a
// graph handed back, the shapes of what the model gives, the entry points as Node resolves them, and the bin.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cpSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join, resolve, sep } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { edges, expr, graph, html, kit, model, nav, needs, spine, story, wireframe, yaml } from './modules.mjs';

// the package as this test resolves it: the checkout, or the copy npm installed
const pkgRoot = dirname(createRequire(import.meta.url).resolve('@delurio/cli/package.json'));
const shop = fileURLToPath(new URL('fixtures/shop', import.meta.url));
// what a child under the permission model may read: the package, the node_modules it is in, the addon given
const readable = [pkgRoot, pkgRoot.includes(`${sep}node_modules${sep}`) ? pkgRoot.slice(0, pkgRoot.indexOf(`${sep}node_modules${sep}`) + `${sep}node_modules`.length) : pkgRoot];
if (process.env.DELURIO_ADDON !== undefined) readable.push(dirname(resolve(process.env.DELURIO_ADDON)));

function copy(dir) {
  const to = mkdtempSync(join(tmpdir(), 'delurio-node-'));
  cpSync(dir, to, { recursive: true });
  return to;
}

function edit(file, from, to) {
  const text = readFileSync(file, 'utf8');
  assert.ok(text.includes(from), `${file} holds ${from}`);
  writeFileSync(file, text.replace(from, to));
}

class Refused extends Error {}

test('errors are the classes the TypeScript modules threw, with their place', () => {
  const broken = copy('examples/notes/edit');
  edit(join(broken, 'flow.yaml'), 'initial: editing', 'initial: nowhere');
  assert.throws(
    () => graph.load(broken),
    (e) => e instanceof graph.GraphError && e instanceof Error && e.message === 'initial state nowhere does not exist' && e.place.file === 'flow.yaml',
  );
  assert.throws(() => kit.loadKit(join(broken, 'no-kit')), (e) => e instanceof kit.KitError && /does not exist/.test(e.message));
  assert.throws(() => nav.loadNav('examples/notes/edit', undefined, () => 'kind: flow\n'), (e) => e instanceof graph.GraphError && /is not a navigation graph/.test(e.message));
  const g = graph.load('examples/notes/edit');
  const flow = { ...g.flow, states: { ...g.flow.states, editing: { ...g.flow.states.editing, always: [{ target: 'saving', guard: 'nope' }] } } };
  assert.throws(
    () => model.explore(new model.Model(flow)),
    (e) => e instanceof model.ModelError && e.message.startsWith('unknown name "nope"') && Array.isArray(e.place.path),
  );
  assert.throws(() => expr.parseExpr('a =='), (e) => e instanceof Error && e.message === 'unexpected end in: a ==');
  const bad = 'a: [1, 2\nb: 3\n';
  assert.throws(
    () => yaml.readYaml(bad),
    (e) => e.name === 'YAMLParseError' && typeof e.code === 'string' && Array.isArray(e.pos) && e.linePos[0].line >= 1,
  );
  assert.throws(
    () => yaml.readYaml('a: &x 1\nb: *x\n'),
    (e) => e.name === 'YAMLParseError' && e.code === 'BAD_ALIAS' && e.message.startsWith('unsupported YAML: an anchor') && e.linePos[0].line === 1,
  );
  assert.equal(yaml.unsupported('a: &x 1\n').code, 'BAD_ALIAS');
});

test('a callback that throws reaches the caller unchanged, and the call stops there', () => {
  const thrown = new Refused('refused path');
  let calls = 0;
  const read = (file) => {
    calls++;
    if (file === 'ui.yaml') throw thrown;
    return readFileSync(join('examples/notes/edit', file), 'utf8');
  };
  assert.throws(
    () => graph.load('examples/notes/edit', read, () => true),
    (e) => e === thrown && e.place.file === 'ui.yaml',
  );
  assert.equal(calls, 2);
  // a catalog that cannot be read is said in the graph's words, with what the reader said
  const catalogRead = (file) => {
    if (file.startsWith('catalog/')) throw new Error('missing file');
    return readFileSync(join('examples/notes/edit', file), 'utf8');
  };
  assert.throws(() => graph.load('examples/notes/edit', catalogRead), (e) => e instanceof graph.GraphError && e.message === 'the catalog catalog/en.json that ui.yaml names cannot be read: missing file');
  const absent = new Refused('exists refused');
  assert.throws(
    () => graph.load('examples/notes/edit', undefined, () => {
      throw absent;
    }),
    (e) => e === absent,
  );
  const navRead = new Refused('flows/x is not in the approved graphs');
  assert.throws(
    () => nav.loadNav(shop, undefined, () => {
      throw navRead;
    }),
    (e) => e === navRead,
  );
  // the default reader is Node's own: a locale without a catalog is ENOENT
  assert.throws(() => nav.loadNav(shop, 'de'), (e) => e.code === 'ENOENT');
  assert.equal(nav.loadNav(shop, 'tr').g.pages.start.title, 'Ana sayfa');

  const g = graph.load('examples/notes/edit');
  const warned = new Refused('warn');
  let warns = 0;
  const noKit = { ...g, ui: { ...g.ui, nodes: { ...g.ui.nodes, title: { ...g.ui.nodes.title, text: { key: 'No.such.key' } } } } };
  assert.throws(
    () => wireframe.renderScreens(noKit, () => {
      warns++;
      throw warned;
    }),
    (e) => e === warned,
  );
  assert.equal(warns, 1);
  const k = kit.loadKit('examples/notes/kit');
  const intent = new Refused('intent');
  assert.throws(
    () => kit.drawWith(k, 'Button', { id: 'b', text: 'Go', name: 'Go', intent: 'nonesuch', role: undefined, disabled: false, named: false, conditional: false, checked: false }, () => {
      throw intent;
    }),
    (e) => e === intent,
  );

  const m = new model.Model(g.flow);
  const stop = new Refused('keep');
  assert.throws(
    () => model.explore(m, undefined, () => {
      throw stop;
    }),
    (e) => e === stop,
  );
});

test('evaluate calls lookup again inside lookup, and what it throws reaches the caller', () => {
  const guards = { a: "b and c == 'x'", b: 'not d', d: 'false' };
  const values = { c: 'x' };
  const lookupOf = (depth) => (name) => {
    if (name in values) return values[name];
    if (name in guards) {
      if (depth > 8) throw new Error('too deep');
      return expr.evaluate(expr.parseExpr(guards[name]), lookupOf(depth + 1));
    }
    throw new Refused(`unknown ${name}`);
  };
  assert.equal(expr.evaluate(expr.parseExpr('a'), lookupOf(0)), true);
  assert.equal(expr.evaluate(expr.parseExpr("c in ['y', 'x']"), lookupOf(0)), true);
  assert.equal(expr.parseExpr('a and b'), expr.parseExpr('a and b'), 'the same expression object for the same text');
  assert.throws(
    () => expr.evaluate(expr.parseExpr('a or zzz'), lookupOf(0)) === false || expr.evaluate(expr.parseExpr('zzz'), lookupOf(0)),
    (e) => e instanceof Refused && e.message === 'unknown zzz',
  );
  guards.d = 'q';
  assert.throws(() => expr.evaluate(expr.parseExpr('a'), lookupOf(0)), (e) => e instanceof Refused && e.message === 'unknown q');
  assert.deepEqual([...expr.identifiers(expr.parseExpr("a and c == 'x' or a"))], ['a', 'c']);
});

test('Sets and Maps where the TypeScript modules gave them', () => {
  const g = graph.load('examples/notes/edit');
  const m = new model.Model(g.flow);
  const ex = model.explore(m);
  assert.ok(ex.taken instanceof Set && ex.taken.size > 0 && ex.taken.has([...ex.taken][0]));
  assert.ok(ex.atState instanceof Map && ex.atState.get(g.flow.initial) >= 1);
  assert.ok(ex.branchCounts instanceof Map);
  assert.equal(typeof ex.starts, 'number');
  assert.ok(expr.identifiers(expr.parseExpr('a')) instanceof Set);
  const { g: shopGraph } = nav.loadNav(shop);
  assert.deepEqual([...nav.guardCodes(shopGraph, shopGraph.guards.canWrite)].sort(), ['orders.write']);
  assert.ok(nav.guardCodes(shopGraph, 'isAdmin') instanceof Set);
  const admin = shopGraph.personas.find((p) => p.id === 'admin');
  const walked = nav.walk(shopGraph, admin);
  assert.ok(walked instanceof Map && walked.get('settings').clicks === 1);
  const v = nav.viewOf(shopGraph, admin);
  assert.ok(v.reach instanceof Map && v.persona === admin);
  assert.equal(v.reach.get('settings').via, shopGraph.entries[1], 'a reach names the graph’s own entry');
  assert.deepEqual(nav.pathTo(v.reach, 'settings'), [shopGraph.entries[1]]);
  const p = needs.planned(null);
  assert.ok(p.states instanceof Map && p.nodes instanceof Map);
});

test('the keys load does not know survive it, and a copy of a graph is read as the copy', () => {
  const dir = copy('examples/notes/edit');
  edit(join(dir, 'flow.yaml'), 'initial: editing', 'initial: editing\nx-own: { kept: true }');
  edit(join(dir, 'ui.yaml'), '    script: optional', '    script: optional\n    repeat: notes\n    x-own: kept');
  const g = graph.load(dir);
  assert.deepEqual(g.flow['x-own'], { kept: true });
  assert.ok(g.flow.failures !== undefined, 'flow.failures, which load does not read, is there');
  assert.equal(g.ui.nodes.page.repeat, 'notes');
  assert.equal(g.ui.nodes.page['x-own'], 'kept');
  assert.deepEqual(g.ui.focus, { saved: 'savedStatus' });
  assert.ok(Array.isArray(g.ui.edges));
  assert.equal(g.ui.nodes.bodyField.value.key, 'Edit.bodyValue');

  const texts = wireframe.nodeTexts(g, 'title', () => undefined);
  const catalog = Object.fromEntries(Object.entries(g.catalog).map(([k, s]) => [k, `«${s}»`]));
  const other = { ...g, catalog };
  assert.notDeepEqual(wireframe.nodeTexts(other, 'title', () => undefined), texts);
  assert.deepEqual(wireframe.nodeTexts(g, 'title', () => undefined), texts, 'the loaded graph is still read as it was');
  const samples = { ...g, ui: { ...g.ui, samples: { ...g.ui.samples, body: 'Tea' } } };
  assert.equal(edges.kindOf(samples, 'editing'), 'screen');
  const m = new model.Model(other.flow);
  const ex = model.explore(m);
  const steps = story.stepsTo(m, ex, ex.configs.length - 1);
  assert.equal(typeof story.narrate(other, m, steps).result, 'string');
});

test('what the model gives has the shapes the TypeScript model gave, and keep may call the model again', () => {
  const g = graph.load('examples/notes/edit');
  const m = new model.Model(g.flow);
  assert.equal(m.flow, g.flow);
  assert.deepEqual(m.vars, Object.keys(g.flow.context));
  assert.equal(m.domains[0], g.flow.context[m.vars[0]].values, 'the flow’s own lists of values');
  const starts = m.initials();
  assert.ok(Array.isArray(starts) && starts.every((c) => c.length === m.vars.length));
  for (const c of starts) assert.ok(c.every((v, i) => g.flow.context[m.vars[i]].values.includes(v) || g.flow.context[m.vars[i]].initial === v));
  const s = m.steps(g.flow.initial, starts[0]);
  assert.ok(s.length > 0);
  assert.deepEqual(Object.keys(s[0]), ['event', 'target', 'ctx', 'taken']);
  assert.equal(s[0].taken.split('|')[0], g.flow.initial);
  assert.equal(typeof m.test('state == "editing"', g.flow.initial, starts[0]), 'boolean');
  assert.equal(m.tester('state == "editing"')(g.flow.initial, starts[0]), m.test('state == "editing"', g.flow.initial, starts[0]));
  let calls = 0;
  const ex = model.explore(m, starts, (at, step) => {
    calls++;
    assert.deepEqual(Object.keys(at), ['state', 'ctx', 'parent', 'event']);
    return m.steps(at.state, at.ctx).some((x) => x.taken === step.taken) && m.test('true', at.state, at.ctx);
  });
  assert.ok(calls > 0);
  assert.deepEqual(ex.configs, model.explore(m).configs);
  assert.deepEqual(Object.keys(ex), ['configs', 'starts', 'taken', 'atState', 'branchCounts']);
  assert.deepEqual(Object.keys(ex.configs[1]), ['state', 'ctx', 'parent', 'event']);
  assert.equal(ex.configs[0].parent, -1);
  assert.equal(ex.configs[0].event, null);
  // a path the engine extends with steps of its own goes back in
  const path = [...story.stepsTo(m, ex, 1), { state: 'left', ctx: ex.configs[1].ctx, event: 'LEAVE', taken: null }];
  assert.equal(story.narrate(g, m, path).steps.length, path.length - 1);
  assert.deepEqual(Object.keys(path[0]), ['state', 'ctx', 'event', 'taken']);
  const main = story.stepsByEvents(m, g.flow.main ?? []);
  assert.ok(main === null || Array.isArray(main));
  // a value a context holds that the flow does not list is kept as it was given
  const odd = { odd: true };
  const ctx = starts[0].map((v, i) => (i === 0 ? odd : v));
  assert.equal(m.steps(g.flow.initial, ctx)[0]?.ctx[0] ?? odd, odd);
  assert.deepEqual(m.scenario(starts[0]), Object.fromEntries(m.vars.filter((v) => g.flow.context[v].initial === 'any').map((v) => [v, starts[0][m.vars.indexOf(v)]])));
});

test('drawWith takes a fill whose keys are set to undefined', () => {
  const k = kit.loadKit('examples/notes/kit');
  const html = kit.drawWith(k, 'Button', { id: 'b', text: 'Save', name: 'Save', intent: undefined, role: undefined, disabled: false, named: false, conditional: false, checked: false }, () => undefined);
  assert.match(html, /Save/);
  assert.equal(kit.drawWith(k, 'NoSuchComponent', { id: 'b', text: '', name: '', disabled: false, named: false, conditional: false, checked: false }, () => undefined), null);
  assert.equal(kit.drawWith({ ...k }, 'Button', { id: 'b', text: 'Save', name: 'Save', disabled: false, named: false, conditional: false, checked: false }, () => undefined), html, 'a copy of a kit draws as the kit');
  assert.match(kit.kitCss(k, 'page'), /\.wf\.kit/);
  assert.match(kit.kitStageScript(k), /--kit-zoom/);
});

test('a tone is drawn by the context a stage is given, and a kit carries its fonts (SPEC.md §4, §8)', () => {
  const dir = copy('examples/notes');
  edit(join(dir, 'edit/ui.yaml'), '    text: { key: Edit.saved }\n', '    text: { key: Edit.saved }\n    tone: { danger: "not pinned", success: pinned }\n');
  edit(join(dir, 'kit/kit.yaml'), "    template: '<p class=\"k-status\" role=\"status\">{text}</p>'", "    template: '<p class=\"k-status k-{tone}\" role=\"status\">{text}</p>'\n    tones: { danger: alarm, success: fine }");
  writeFileSync(join(dir, 'kit/brand.woff2'), 'wOF2font');
  edit(join(dir, 'kit/kit.yaml'), 'components:\n', 'fonts:\n  - { family: Brand Sans, file: brand.woff2, style: italic }\ncomponents:\n');
  const g = graph.load(join(dir, 'edit'));
  assert.deepEqual(g.ui.nodes.savedStatus.tone, { danger: 'not pinned', success: 'pinned' });
  const k = kit.resolveKit(g.dir, g.ui.kit, undefined);
  assert.deepEqual(k.components.Status.tones, { danger: 'alarm', success: 'fine' });
  assert.equal(Object.hasOwn(k.components.Text, 'tones'), false, 'a component without tones has no key for them');
  assert.deepEqual(k.fonts, [{ family: 'Brand Sans', file: 'brand.woff2', style: 'italic', src: 'data:font/woff2;base64,d09GMmZvbnQ=' }]);
  assert.deepEqual(kit.kitFamilies(k), ['Brand Sans']);
  assert.deepEqual(kit.kitFamilies({ ...k }), ['Brand Sans'], 'a copy of a kit carries its fonts');
  assert.equal(kit.kitCss({ ...k }, 'stage').match(/@font-face/g).length, 1);
  assert.equal(kit.kitCss({ ...k, fonts: [{ family: 'X', file: 'x', src: 'https://example.com/x.woff2' }] }, 'page').includes('@font-face'), false);
  const status = (ctx) => /<p class="k-status k-(\w*)" role="status">The note is saved/.exec(wireframe.renderStageScreen(g, 'saved', [], () => undefined, k, ctx))?.[1];
  assert.equal(status({ pinned: false }), 'alarm');
  assert.equal(status({ pinned: true }), 'fine');
  assert.equal(status(null), '');
  assert.equal(status(), '');
  // the fixed components, and the styles of a tone a stage adds
  assert.match(wireframe.renderStageScreen(g, 'saved', [], () => undefined, null, { pinned: false }), /class="wf-item wf-tone-danger" data-node="savedStatus"/);
  assert.match(wireframe.TONE_CSS, /\.wf-tone-danger \{ --wf-tone: var\(--bad, #b91c1c\); \}/);
  const toned = kit.drawWith(k, 'Status', { id: 's', text: 'Hi', name: 'Hi', tone: 'success', disabled: false, named: false, conditional: false, checked: false }, () => undefined);
  assert.equal(toned, '<p class="k-status k-fine" role="status">Hi</p>');
  const said = [];
  kit.drawWith(k, 'Status', { id: 's', text: 'Hi', name: 'Hi', tone: 'info', disabled: false, named: false, conditional: false, checked: false }, (m) => said.push(m));
  assert.deepEqual(said, ['kit example: Status has no entry for tone info (node s); drawn as no tone']);
});

test('the navigation rules take the views and goal checks they gave back', () => {
  const { g, warnings } = nav.loadNav(shop);
  assert.ok(Array.isArray(warnings));
  const views = g.personas.map((p) => nav.viewOf(g, p));
  const rows = nav.menuRows(g, views);
  assert.equal(rows[0].entry, g.entries[0]);
  assert.deepEqual(
    rows.map((r) => r.cells.map((c) => c.shown)),
    [
      [true, true],
      [false, true],
    ],
  );
  const goals = nav.checkGoals(g, views);
  assert.equal(goals[0].goal, g.goals[0]);
  assert.equal(goals[0].persona, views[0].persona);
  assert.deepEqual(
    goals.map((x) => [x.goal.id, x.persona.id, x.outcome]),
    [
      ['seeOrders', 'buyer', 'ok'],
      ['seeOrders', 'admin', 'ok'],
      ['changeSettings', 'admin', 'not built'],
      ['changeSettings', 'buyer', 'cannot reach'],
    ],
  );
  const found = nav.findings(g, views, goals);
  assert.ok(Array.isArray(found) && found.every((f) => typeof f.code === 'string' && Array.isArray(f.detail)));
  const buyer = g.personas[0];
  assert.equal(nav.holds(g, buyer, 'isAdmin'), false);
  assert.equal(nav.holds(g, buyer, null), true);
  assert.equal(nav.refusedFor(g, buyer, 'settings'), true);
  assert.equal(nav.frameReached(g, views[0], 'shop'), true);
  assert.equal(nav.frameLevel(g.entries[0]), true);
  // a graph changed in place after it was loaded is read as the copy it now is when its own keys change
  const moved = { ...g, pages: { ...g.pages, decks: { ...g.pages.orders, id: 'decks', source: 'app/decks/page.tsx' } } };
  assert.equal(nav.viewOf(moved, buyer).standing.decks, 'unreachable');
});

test('the page CSS keeps the lines the video stage reads out of it', () => {
  const lines = html.CSS.split('\n');
  assert.ok(lines.some((l) => l.startsWith(':root {')));
  assert.ok(lines.filter((l) => /^(svg)?\.spine[ .]/.test(l)).length > 5);
  assert.throws(() => html.renderPage({}), /delurio page/);
});

test('renderSpine draws the diagram the page draws', async () => {
  const g = graph.load('examples/notes/edit');
  const m = new model.Model(g.flow);
  const ex = model.explore(m);
  // the main story as the page finds it, the same steps cli.ts hands renderSpine: the declared one, else the found one
  const declared = g.flow.main === undefined ? null : story.stepsByEvents(m, g.flow.main);
  const index = story.mainStoryIndex(g, ex);
  const steps = declared ?? (index === null ? null : story.stepsTo(m, ex, index));
  const svg = await spine.renderSpine(g, ex, 'full', steps);
  assert.match(svg, /viewBox="0 0 (\d+) (\d+)"/);
  const page = mkdtempSync(join(tmpdir(), 'delurio-page-'));
  const r = spawnSync(process.execPath, [join(pkgRoot, 'js/cli.js'), 'page', 'examples/notes/edit', '--out', page], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(readFileSync(join(page, 'index.html'), 'utf8').includes(svg));
});

test('every entry point resolves for require and import, beside package.json and SPEC.md', async () => {
  const require = createRequire(import.meta.url);
  const pkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8'));
  assert.equal(typeof pkg.bin.delurio, 'string');
  for (const entry of Object.keys(pkg.exports)) {
    const file = require.resolve(`@delurio/cli${entry.slice(1)}`);
    assert.ok(file.startsWith(pkgRoot), `${entry} resolves inside the package`);
    if (entry !== './package.json') await import(file);
  }
  assert.ok(readFileSync(join(pkgRoot, 'SPEC.md'), 'utf8').length > 1000);
});

test('the bin is a JavaScript file node runs, under the permission model with --allow-addons', () => {
  const bin = join(pkgRoot, 'js/cli.js');
  const allow = readable.map((d) => `--allow-fs-read=${d}`);
  const help = spawnSync(process.execPath, [bin, '--help'], { encoding: 'utf8' });
  assert.equal(help.status, 0);
  assert.match(help.stderr, /^usage:/);
  const bad = spawnSync(process.execPath, [bin, 'nonesuch'], { encoding: 'utf8' });
  assert.equal(bad.status, 1);
  const allowed = spawnSync(process.execPath, ['--permission', ...allow, '--allow-addons', bin, '--help'], { encoding: 'utf8' });
  assert.equal(allowed.status, 0, allowed.stderr);
  const refused = spawnSync(process.execPath, ['--permission', ...allow, bin, '--help'], { encoding: 'utf8' });
  assert.notEqual(refused.status, 0);
  const binary = process.env.DELURIO_BIN;
  if (binary !== undefined) {
    for (const args of [['check', 'examples', '--strict'], ['check', 'examples', '--format', 'sarif'], ['check', 'nowhere']]) {
      const a = spawnSync(process.execPath, [bin, ...args], { encoding: 'utf8' });
      const b = spawnSync(binary, args, { encoding: 'utf8' });
      assert.deepEqual([a.status, a.stdout, a.stderr], [b.status, b.stdout, b.stderr], `delurio ${args.join(' ')}: the bin and the binary`);
    }
  }
});
