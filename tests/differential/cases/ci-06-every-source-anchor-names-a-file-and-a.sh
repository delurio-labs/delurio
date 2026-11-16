# from the reference CI step: Every source anchor names a file and a line that are there, and words that cite the code warn where they do not
cp -r examples/notes "$TMPDIR_CASE/anchors"
f="$TMPDIR_CASE/anchors/restore/flow.yaml"
sed -i 's|^    title: Read the note$|&\n    source: examples/notes/app/notes.js:12|' "$f"
sed -i 's|^    title: The archived note$|&\n    source: examples/notes/app/note.js:3|' "$f"
sed -i 's|^    title: Restore it$|&\n    source: examples/notes/app/notes.js:99\n    description: restoreNote, at examples/notes/app/notes.js:30-40, app/notes.js:99 and https://example.com/examples/notes/app/notes.js:99|' "$f"
sed -i 's|^      DONE: restored$|      DONE: { target: restored, source: examples/notes/app }|' "$f"
sed -i 's|^    sideEffects: \[archivedAt = now\]$|&\n    source: examples/notes/app/notes.js:0|' "$TMPDIR_CASE/anchors/domain/note.yaml"
sed -i 's|^changes: { key: Changes.note }$|&\nsamples: { source: examples/notes/app/note.js:3 }|' "$TMPDIR_CASE/anchors/restore/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/anchors" --strict > "$TMPDIR_CASE/anchors.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/anchors.txt"
test "$code" -eq 2
test "$(grep -c '\[source-unresolved\]' "$TMPDIR_CASE/anchors.txt")" -eq 4
grep -F 'restore/flow.yaml:34:5: error: states.archived: source examples/notes/app/note.js does not exist (SPEC.md §9) [source-unresolved]' "$TMPDIR_CASE/anchors.txt"
grep -F "error: states.restoring: source examples/notes/app/notes.js:99 is past the file's end: it has 37 lines" "$TMPDIR_CASE/anchors.txt"
grep -F 'error: states.restoring.on.DONE: source examples/notes/app is a folder: an anchor names a file' "$TMPDIR_CASE/anchors.txt"
grep -F "domain/note.yaml:23:5: error: actions.archiveNote: source examples/notes/app/notes.js:0 names line 0: a file's lines count from 1" "$TMPDIR_CASE/anchors.txt"
test "$(grep -c '\[citation-unresolved\]' "$TMPDIR_CASE/anchors.txt")" -eq 1
grep -F "warning: states.restoring.description cites examples/notes/app/notes.js:30-40, which is past the file's end: it has 37 lines" "$TMPDIR_CASE/anchors.txt"
grep -Fx 'delurio check: 3 graphs, 4 errors, 1 warning, 1 gap of 4 needs; exit 2' "$TMPDIR_CASE/anchors.txt"
(cd examples && "$DELURIO" check "$TMPDIR_CASE/anchors" > "$TMPDIR_CASE/anchors.txt" 2>&1)
test "$(grep -c '\[source-unresolved\]' "$TMPDIR_CASE/anchors.txt")" -eq 5
grep -F 'states.reading: source examples/notes/app/notes.js does not exist' "$TMPDIR_CASE/anchors.txt"
(cd examples && "$DELURIO" check "$TMPDIR_CASE/anchors" --app .. > "$TMPDIR_CASE/anchors.txt" 2>&1)
test "$(grep -c '\[source-unresolved\]' "$TMPDIR_CASE/anchors.txt")" -eq 4
