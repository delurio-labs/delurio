# from the reference CI step: A graph that breaks its schema fails the check
cp -r examples/qauth-authorize "$TMPDIR_CASE/broken"
sed -i 's/left: { type: final, title: The user leaves, leavesPage: true }/left: { type: final, title: The user leaves, and nothing is sent, leavesPage: true }/' "$TMPDIR_CASE/broken/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/broken" --strict > "$TMPDIR_CASE/broken.txt" || code=$?
cat "$TMPDIR_CASE/broken.txt"
test "$code" -eq 2
grep -F '[schema]' "$TMPDIR_CASE/broken.txt"
