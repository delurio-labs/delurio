# from the reference CI step: Evidence is found under --app, and a walk leaves out what only missing needs draw
code=0
(cd examples && "$DELURIO" check notes --strict > "$TMPDIR_CASE/app.txt" 2>&1) || code=$?
cat "$TMPDIR_CASE/app.txt"
test "$code" -eq 2
grep -F 'test examples/notes/app/notes.test.js does not exist' "$TMPDIR_CASE/app.txt" | grep -F '[evidence-unresolved]'
(cd examples && "$DELURIO" check notes --strict --app ..)
code=0
"$DELURIO" check examples/notes --app "$TMPDIR_CASE/nowhere" > "$TMPDIR_CASE/app.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/app.txt"
test "$code" -eq 1
grep -F "$TMPDIR_CASE/nowhere is not a folder" "$TMPDIR_CASE/app.txt"
