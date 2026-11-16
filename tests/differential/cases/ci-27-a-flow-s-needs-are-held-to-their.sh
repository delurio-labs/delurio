# from the reference CI step: A flow's needs are held to their evidence, and the example app's tests pass
"$DELURIO" check examples/notes --strict > "$TMPDIR_CASE/needs.txt" 2> "$TMPDIR_CASE/needs.err"
cat "$TMPDIR_CASE/needs.txt" "$TMPDIR_CASE/needs.err"
grep -F 'restore/needs.yaml:47:5: note: need says-when-it-is-erased is missing: archived notes are not erased yet' "$TMPDIR_CASE/needs.txt" | grep -F '[need-gap]'
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/needs.err"
"$DELURIO" check examples/qauth-authorize --strict 2>&1 | grep -Fx 'delurio check: 1 graph, 0 errors, 0 warnings; exit 0'

n=restore/needs.yaml
broken() { rm -rf "$TMPDIR_CASE/needs"; cp -r examples/notes "$TMPDIR_CASE/needs"; sed -i "$2" "$TMPDIR_CASE/needs/$1"; code=0; "$DELURIO" check "$TMPDIR_CASE/needs" --strict > "$TMPDIR_CASE/needs.txt" || code=$?; cat "$TMPDIR_CASE/needs.txt"; test "$code" -eq 2; grep -F "$3" "$TMPDIR_CASE/needs.txt" | grep -F "[$4]"; }
broken $n '0,/notes\.js$/s//note.js/' 'restore/needs.yaml:17:9: error: need restore-from-its-page: source examples/notes/app/note.js does not exist' evidence-unresolved
broken $n '0,/notes\.js$/s//notes.js:99/' "source examples/notes/app/notes.js:99 is past the file's end: it has 37 lines" evidence-unresolved
broken $n 's/^        name: restoreNote$/        name: restoreNotes/' 'source examples/notes/app/notes.js holds no restoreNotes' evidence-unresolved
broken $n 's/name: restores an archived note$/name: restores a note/' 'test examples/notes/app/notes.test.js holds no test named "restores a note"' evidence-unresolved
broken $n 's/^      - scenario: owner$/      - scenario: stranger/' "scenario stranger is not a scenario of the flow's own binding.yaml (it has owner)" evidence-unresolved
broken restore/binding.yaml 's/^flow: example.restoreNote$/flow: example.editNote/' "scenario owner: the flow's own binding.yaml binds example.editNote, not example.restoreNote" evidence-unresolved
broken $n 's/^      - state: restoring$/      - state: restorng/' 'state restorng is not a state of example.restoreNote' evidence-unresolved
broken $n 's/^      - node: keptFor$/      - node: keepFor/' "node keepFor is not a node of the flow's ui.yaml" evidence-unresolved
broken $n 's/^      - setting: NOTES_SESSION_SECRET$/      - setting: NOTES_KEEP_ARCHIVED_DAYS/' 'setting NOTES_KEEP_ARCHIVED_DAYS is not set in examples/notes/app/.env.example: no line sets it, or the one that does is commented out' evidence-unresolved
broken $n '/^  - id: restore-from-its-page$/,/^  - id: restored-meanwhile$/{/^      - \(source\|test\|scenario\):/d;/^        name:/d}' 'need restore-from-its-page is not missing and names no evidence outside the graph' need-no-evidence
broken $n 's/^  - id: restored-meanwhile$/  - id: restore-from-its-page/' 'need restore-from-its-page is given twice: it is given first at line 12' need-twice
broken $n 's/^id: example.restoreNote$/id: example.editNote/' 'needs.yaml is the needs of example.editNote, and the flow beside it is example.restoreNote' needs-place
broken $n 's/^      - state: restoring$/&\n        node: restore/' 'evidence.0.node: is not allowed here (a piece of evidence is of one kind: give each kind an item of its own)' schema

mkdir "$TMPDIR_CASE/needs/stray"
cp "$TMPDIR_CASE/needs/$n" "$TMPDIR_CASE/needs/stray/needs.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/needs" --strict > "$TMPDIR_CASE/needs.txt" || code=$?
cat "$TMPDIR_CASE/needs.txt"
test "$code" -eq 2
grep -F 'stray/needs.yaml:8:1: error: needs.yaml is beside no flow' "$TMPDIR_CASE/needs.txt" | grep -F '[needs-place]'
