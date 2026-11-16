# from the reference CI step: A writing flow whose app has no changes message says why, a gap that fails nothing
cp -r examples/notes "$TMPDIR_CASE/no-note"
sed -i 's/^changes: { key: Changes.note }$/changes: { missing: the app does not say yet what restoring changes }/' "$TMPDIR_CASE/no-note/restore/ui.yaml"
"$DELURIO" check "$TMPDIR_CASE/no-note" --strict --format sarif --output "$TMPDIR_CASE/no-note.sarif" > "$TMPDIR_CASE/no-note.txt" 2> "$TMPDIR_CASE/no-note.err"
cat "$TMPDIR_CASE/no-note.txt" "$TMPDIR_CASE/no-note.err"
grep -F 'no-note/restore/ui.yaml:11:12: note: its steps write (restoreNote), and its changes note is missing: the app does not say yet what restoring changes; Rehber does not guide its writing steps' "$TMPDIR_CASE/no-note.txt" | grep -F '[changes-note-gap]'
! grep -F '[changes-note-missing]' "$TMPDIR_CASE/no-note.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs, 1 missing changes note; exit 0' "$TMPDIR_CASE/no-note.err"
node -e '
  const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
  const rule = run.tool.driver.rules.find((r) => r.id === "changes-note-gap");
  if (rule?.defaultConfiguration.level !== "note") throw new Error("changes-note-gap is not a note");
  const r = run.results.find((x) => x.ruleId === "changes-note-gap");
  if (r?.level !== "note" || r.locations[0].logicalLocations[0].fullyQualifiedName !== "example.restoreNote/changes/missing") throw new Error(JSON.stringify(r));
' "$TMPDIR_CASE/no-note.sarif"

sed -i 's/^changes: { key: Changes.note }$/changes: { missing: no message yet }/' "$TMPDIR_CASE/no-note/edit/ui.yaml"
"$DELURIO" check "$TMPDIR_CASE/no-note" --strict --baseline "$TMPDIR_CASE/no-note.sarif" 2>&1 | grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs, 2 missing changes notes (1 new); exit 0'

for bad in 'changes: { key: Changes.note, missing: no message yet }' 'changes: { missing: "" }' 'changes: {}'; do
  sed -i "s/^changes: .*$/$bad/" "$TMPDIR_CASE/no-note/edit/ui.yaml"
  code=0
  "$DELURIO" check "$TMPDIR_CASE/no-note" --strict > "$TMPDIR_CASE/no-note.txt" 2>&1 || code=$?
  cat "$TMPDIR_CASE/no-note.txt"
  test "$code" -eq 2
  grep -F 'no-note/edit/ui.yaml:13:' "$TMPDIR_CASE/no-note.txt" | grep -F '[schema]'
done
grep -F 'error: its steps write (editNote), but ui.yaml has no changes note' "$TMPDIR_CASE/no-note.txt" | grep -F '[changes-note-missing]'
