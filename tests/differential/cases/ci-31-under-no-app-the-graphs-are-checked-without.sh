# from the reference CI step: Under --no-app the graphs are checked without the code, and the output says once what was not looked for
(cd examples && "$DELURIO" check notes --strict --no-app > "$TMPDIR_CASE/no-app.txt" 2>&1)
cat "$TMPDIR_CASE/no-app.txt"
grep -Fx 'delurio check: note: --no-app: 0 source anchors, 0 citations in words and 7 pieces of evidence name files of the application, which are not here, and were not looked for (SPEC.md §10)' "$TMPDIR_CASE/no-app.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/no-app.txt"

cp -r examples/notes "$TMPDIR_CASE/no-app"
sed -i 's|^    title: Read the note$|&\n    source: app/nowhere.js:99\n    description: readOwnNote, at examples/notes/app/notes.js:99|' "$TMPDIR_CASE/no-app/restore/flow.yaml"
sed -i 's/^      - state: restoring$/      - state: restorng/; s/^      - scenario: owner$/      - scenario: stranger/' "$TMPDIR_CASE/no-app/restore/needs.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/no-app" --strict --no-app --format sarif --output "$TMPDIR_CASE/no-app.sarif" > "$TMPDIR_CASE/no-app.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/no-app.txt"
test "$code" -eq 2
test "$(grep -c '\[evidence-unresolved\]' "$TMPDIR_CASE/no-app.txt")" -eq 2
grep -F 'state restorng is not a state of example.restoreNote' "$TMPDIR_CASE/no-app.txt"
grep -F "scenario stranger is not a scenario of the flow's own binding.yaml" "$TMPDIR_CASE/no-app.txt"
! grep -F -e '[source-unresolved]' -e '[citation-unresolved]' "$TMPDIR_CASE/no-app.txt"
grep -F 'note: --no-app: 1 source anchor, 1 citation in words and 7 pieces of evidence name files' "$TMPDIR_CASE/no-app.txt"
node -e '
  const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
  const [said, ...rest] = run.invocations[0].toolExecutionNotifications ?? [];
  if (said === undefined || rest.length !== 0 || said.level !== "note" || !said.message.text.startsWith("--no-app: 1 source anchor")) throw new Error(`one note of the run: ${JSON.stringify(run.invocations)}`);
  if (run.results.some((r) => r.ruleId === "source-unresolved" || r.ruleId === "citation-unresolved")) throw new Error("no anchor is looked for");
' "$TMPDIR_CASE/no-app.sarif"

code=0
"$DELURIO" check examples --app . --no-app > "$TMPDIR_CASE/no-app.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/no-app.txt"
test "$code" -eq 1
grep -F -- '--app names the application'"'"'s code and --no-app says it is not here: give one of them' "$TMPDIR_CASE/no-app.txt"
