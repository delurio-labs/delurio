// The tests that the needs of restoring a note name by their names (../restore/needs.yaml). node --test runs them.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { afterRestore, readOwnNote, restoreNote } from './notes.js';

const store = () => new Map([['n1', { id: 'n1', owner: 'ada', body: 'Milk', archivedAt: '2026-11-01' }]]);

test('restores an archived note', () => {
  const notes = store();
  assert.equal(restoreNote(notes, 'ada', 'n1').archivedAt, null);
  assert.equal(notes.get('n1').archivedAt, null);
});

test('a note restored in another window meanwhile is shown restored', () => {
  const notes = store();
  restoreNote(notes, 'ada', 'n1');
  assert.equal(afterRestore(notes, 'ada', 'n1'), 'restored');
});

test('a note of someone else is no note at all', () => {
  const notes = store();
  assert.throws(() => readOwnNote(notes, 'bob', 'n1'), { code: 'NOT_FOUND' });
  assert.equal(afterRestore(notes, 'bob', 'n1'), 'not_found');
  assert.notEqual(notes.get('n1').archivedAt, null);
});
