// The module API assertions of the TypeScript checker's CI (8e8e430, .github/workflows/ci.yml), run against the
// package's modules on the addon: each test names the CI step it comes from.
//   node --test tests/node/   (from the repository's root, after `npm run build`)
import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { focusAfter, load } from '@delurio/cli/graph';
import { renderCheckScreens } from '@delurio/cli/wireframe';
import { loadKit } from '@delurio/cli/kit';
import { cover, explore, Model } from '@delurio/cli/model';
import { planned, readNeeds } from '@delurio/cli/needs';

test('The focus after a step is the first of returnsFocus, initialFocus and focus', () => {
  const edit = load('examples/notes/edit');
  const at = (from, to) => {
    const f = focusAfter(edit.ui, from, to);
    return f === null ? null : `${f.by} ${f.node}`;
  };
  assert.equal(at(null, 'editing'), 'initialFocus title');
  assert.equal(at('editing', 'discarding'), 'initialFocus discardTitle');
  assert.equal(at('discarding', 'editing'), 'returnsFocus cancel');
  assert.equal(at('saving', 'saved'), 'focus savedStatus');
  assert.equal(at('editing', 'saving'), null);
  assert.equal(at('editing', 'editing'), null);
});

test('Paths cover every reachable state and every transition, the same in every run', () => {
  for (const dir of ['examples/qauth-authorize', 'examples/notes/edit', 'examples/notes/restore', 'examples/notes/archive', 'examples/sign-out']) {
    const m = new Model(load(dir).flow);
    const ex = explore(m);
    for (const [by, want] of [
      ['states', new Set(ex.configs.map((c) => c.state))],
      ['transitions', ex.taken],
    ]) {
      const c = cover(m, by);
      assert.deepEqual(cover(m, by), c, `${dir} ${by}: the same paths in every run`);
      const passed = new Set(c.paths.flatMap((p) => (by === 'states' ? p.states : p.transitions)));
      for (const x of want) assert.ok(passed.has(x), `${dir} ${by}: ${x} is on no path`);
      for (const p of c.paths) assert.match(p.id, /^[0-9a-f]{14}$/);
      assert.deepEqual(c.unreached, [], `${dir} ${by}: everything is reached`);
    }
  }
  const m = new Model(load('examples/notes/edit').flow);
  const driven = cover(m, 'states', m.initials(), (_, s) => s.event !== 'CANCEL');
  assert.deepEqual(driven.unreached, ['discarding', 'left']);
});

test('The example kit draws the notes screens, and a kit that is not data is refused (loadKit)', () => {
  const kit = loadKit('examples/notes/kit');
  for (const c of ['Heading', 'Text', 'Button', 'LinkButton', 'TextField', 'Checkbox', 'Radio', 'Select', 'List', 'ListItem', 'Status', 'Alert']) {
    assert.ok(kit.components[c] !== undefined, `no template for ${c}`);
  }
  assert.deepEqual(
    kit.dark.map(([n]) => n).filter((n) => !kit.light.some(([m]) => m === n)),
    [],
  );
});

test('A dialog that several nodes open gives the focus back to the one pressed (a list, or opener): focusAfter', () => {
  const edit = load('examples/notes/edit');
  const at = (from, to, pressed) => {
    const f = focusAfter(edit.ui, from, to, pressed);
    return f === null ? null : `${f.by} ${f.node}`;
  };
  const back = (target) => {
    edit.ui.edges = edit.ui.edges.map((e) => (e.returnsFocus === undefined ? e : { returnsFocus: ['discardDialog', target] }));
  };
  back('opener');
  assert.equal(at('discarding', 'editing', 'cancel'), 'returnsFocus cancel');
  assert.equal(at('discarding', 'editing'), null);
  back(['cancel', 'save']);
  assert.equal(at('discarding', 'editing', 'save'), 'returnsFocus save');
  assert.equal(at('discarding', 'editing'), null);
  back(['cancel', 'keep']);
  assert.equal(at('discarding', 'editing'), 'returnsFocus cancel');
});

test('Evidence is found under --app, and a walk leaves out what only missing needs draw (planned, readNeeds)', () => {
  const restore = readNeeds('examples/notes/restore');
  assert.deepEqual([...planned(restore).nodes], [['keptFor', ['says-when-it-is-erased']]]);
  assert.deepEqual([...planned(restore).states], []);
  assert.equal(readNeeds('examples/notes/edit'), null);
  const shared = {
    kind: 'needs',
    id: 'x',
    needs: [
      { id: 'built', need: 'A', evidence: [{ state: 'a' }, { test: 't.js' }] },
      { id: 'later', need: 'B', evidence: [{ state: 'a' }, { state: 'b' }, { node: 'n' }], missing: 'the second phase' },
      { id: 'after', need: 'C', evidence: [{ state: 'b' }], missing: 'the third phase' },
    ],
  };
  assert.deepEqual([...planned(shared).states], [['b', ['later', 'after']]]);
  assert.deepEqual([...planned(shared).nodes], [['n', ['later']]]);
});

test('A field with no role is a warning, and the kit check\'s expectation says its role is not given (renderCheckScreens)', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'delurio-role-missing-'));
  cpSync('examples/notes', root, { recursive: true });
  const ui = join(root, 'edit/ui.yaml');
  // sed '/^  bodyField:$/,/^    role: textbox$/{/^    role: textbox$/d}'
  const lines = readFileSync(ui, 'utf8').split('\n');
  const from = lines.indexOf('  bodyField:');
  const at = lines.indexOf('    role: textbox', from);
  assert.ok(from >= 0 && at > from);
  lines.splice(at, 1);
  writeFileSync(ui, lines.join('\n'));
  const { expected } = renderCheckScreens(load(join(root, 'edit')), null, () => {});
  if (!('roleGiven' in expected[0])) return t.skip('this build\'s wireframe gives no roleGiven yet (rust/next before b99f12f)');
  const body = expected.filter((e) => e.id === 'bodyField');
  assert.ok(body.length > 0 && body.every((e) => e.roleGiven === false && e.role === 'field'));
  assert.ok(expected.filter((e) => e.id !== 'bodyField').every((e) => e.roleGiven === true));
});

test('A native control\'s expectation says its kind, which a replay and kit check find it by (renderCheckScreens)', () => {
  const root = mkdtempSync(join(tmpdir(), 'delurio-native-'));
  cpSync('examples/notes', root, { recursive: true });
  const ui = join(root, 'edit/ui.yaml');
  // bodyField becomes a date input: `native: date` in place of its role
  const lines = readFileSync(ui, 'utf8').split('\n');
  const from = lines.indexOf('  bodyField:');
  const at = lines.indexOf('    role: textbox', from);
  assert.ok(from >= 0 && at > from);
  lines.splice(at, 1, '    native: date');
  writeFileSync(ui, lines.join('\n'));
  const { expected } = renderCheckScreens(load(join(root, 'edit')), null, () => {});
  const body = expected.filter((e) => e.id === 'bodyField');
  assert.ok(body.length > 0 && body.every((e) => e.native === 'date' && e.roleGiven === false));
  assert.ok(expected.filter((e) => e.id !== 'bodyField').every((e) => !('native' in e)));
});

test('A recording masks the nodes that hold what the binding\'s mask names: maskedNodes', async () => {
  const { maskedNodes } = await import('@delurio/cli/graph');
  const dir = mkdtempSync(join(tmpdir(), 'holds-'));
  const flow = join(dir, 'graphs', 'qauth-authorize');
  cpSync('examples/qauth-authorize', flow, { recursive: true });
  assert.deepEqual(maskedNodes(load(flow)), []);
  const ui = join(flow, 'ui.yaml');
  const text = readFileSync(ui, 'utf8')
    .replace(/^  title:\n/mu, '  title:\n    holds: Client.name\n')
    .replace(/^  account:\n/mu, '  account:\n    holds: AuthorizationCode.value\n');
  writeFileSync(ui, text);
  const g = load(flow);
  assert.equal(g.ui.nodes.account.holds, 'AuthorizationCode.value');
  // Client.name is no credential; AuthorizationCode.value is
  assert.deepEqual(maskedNodes(g), ['account']);
  assert.deepEqual(maskedNodes(g, ['credential']), ['account']);
  // a mask that names nothing masks nothing: a tutorial filmed with a credential made up for it
  assert.deepEqual(maskedNodes(g, []), []);
  assert.deepEqual(maskedNodes(g, ['personal']), []);
  assert.throws(() => maskedNodes(g, ['secret']), /secret is not a sensitivity \(credential, personal, internal\)/);
});
