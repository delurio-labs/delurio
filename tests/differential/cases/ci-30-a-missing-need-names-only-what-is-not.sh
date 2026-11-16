# from the reference CI step: A missing need names only what is not built, and a state that only missing needs name may name no code
cp -r examples/notes "$TMPDIR_CASE/built"
f="$TMPDIR_CASE/built/restore/flow.yaml"
sed -i 's/^status: proposed$/status: captured/' "$f"
sed -i 's#^    title: \(Read the note\|The archived note\)$#&\n    source: examples/notes/app/notes.js:12#' "$f"
sed -i 's|^      NAVIGATE: left$|&\n      ERASE: confirmingErase\n  confirmingErase:\n    title: Confirm that the note is erased\n    on: { CANCEL: archived }|' "$f"
sed -i 's|^      - node: keptFor$|&\n      - state: archived\n      - state: confirmingErase|' "$TMPDIR_CASE/built/restore/needs.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/built" --strict > "$TMPDIR_CASE/built.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/built.txt"
test "$code" -eq 2
grep -F 'restore/needs.yaml:47:9: warning: need says-when-it-is-erased is missing, and names state archived, which is built: it names its code (source: examples/notes/app/notes.js:12)' "$TMPDIR_CASE/built.txt" | grep -F '[need-names-built]'
test "$(grep -c '\[need-names-built\]' "$TMPDIR_CASE/built.txt")" -eq 1
grep -F 'state restoring names no code' "$TMPDIR_CASE/built.txt" | grep -F '[source-missing]'
test "$(grep -c '\[source-missing\]' "$TMPDIR_CASE/built.txt")" -eq 1
sed -i 's|^    title: Restore it$|&\n    source: examples/notes/app/notes.js:30|' "$f"
"$DELURIO" check "$TMPDIR_CASE/built" --strict 2>&1 | grep -Fx 'delurio check: 3 graphs, 0 errors, 1 warning, 1 gap of 4 needs; exit 0'
