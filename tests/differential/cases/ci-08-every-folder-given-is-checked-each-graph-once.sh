# from the reference CI step: Every folder given is checked, each graph once, and a path that is not a folder is refused
cp -r examples/qauth-authorize "$TMPDIR_CASE/second"
sed -i 's/left: { type: final, title: The user leaves, leavesPage: true }/left: { type: final, title: The user leaves, and nothing is sent, leavesPage: true }/' "$TMPDIR_CASE/second/flow.yaml"
code=0
"$DELURIO" check examples/notes "$TMPDIR_CASE/second" --strict > "$TMPDIR_CASE/second.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/second.txt"
test "$code" -eq 2
grep -F 'second/flow.yaml' "$TMPDIR_CASE/second.txt" | grep -F '[schema]'
grep -F 'delurio check: 4 graphs' "$TMPDIR_CASE/second.txt"

"$DELURIO" check examples examples/notes --strict > "$TMPDIR_CASE/twice.txt" 2>&1
cat "$TMPDIR_CASE/twice.txt"
grep -F 'delurio check: 5 graphs, 0 errors' "$TMPDIR_CASE/twice.txt"

code=0
"$DELURIO" check examples "$TMPDIR_CASE/nowhere" --strict > "$TMPDIR_CASE/nowhere.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/nowhere.txt"
test "$code" -eq 1
grep -F "$TMPDIR_CASE/nowhere is not a folder" "$TMPDIR_CASE/nowhere.txt"
