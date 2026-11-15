// The notes app, as far as the needs of restoring a note name it (../restore/needs.yaml): the server functions behind
// the page of an archived note, on a store the caller gives. A sketch that runs (notes.test.js), not an app.

export class Refusal extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

// readNote: a note of someone else is no note at all, as a note that does not exist is
export function readOwnNote(store, owner, id) {
  const note = store.get(id);
  if (note === undefined || note.owner !== owner) throw new Refusal('NOT_FOUND');
  return note;
}

// restoreNote: an archived note of the owner's is active again
export function restoreNote(store, owner, id) {
  const note = readOwnNote(store, owner, id);
  if (note.archivedAt === null) throw new Refusal('NOT_ARCHIVED');
  const restored = { ...note, archivedAt: null };
  store.set(id, restored);
  return restored;
}

// what the page shows once Restore is pressed: a note restored meanwhile, in another window, is restored all the same
export function afterRestore(store, owner, id) {
  try {
    restoreNote(store, owner, id);
    return 'restored';
  } catch (err) {
    if (err instanceof Refusal && err.code === 'NOT_ARCHIVED') return 'restored';
    if (err instanceof Refusal && err.code === 'NOT_FOUND') return 'not_found';
    throw err;
  }
}
