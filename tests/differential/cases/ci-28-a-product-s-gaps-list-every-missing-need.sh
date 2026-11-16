# from the reference CI step: A product's gaps list every missing need and only those, so they only shrink
broken() { rm -rf "$TMPDIR_CASE/gaps"; cp -r examples/notes "$TMPDIR_CASE/gaps"; sed -i "$2" "$TMPDIR_CASE/gaps/$1"; code=0; "$DELURIO" check "$TMPDIR_CASE/gaps" --strict > "$TMPDIR_CASE/gaps.txt" || code=$?; cat "$TMPDIR_CASE/gaps.txt"; test "$code" -eq 2; grep -F "$3" "$TMPDIR_CASE/gaps.txt" | grep -F "[$4]"; }
broken needs.gaps.yaml 's/^gaps:$/gaps: {}/; /^  example.restoreNote:/d' 'restore/needs.yaml:47:5: error: need says-when-it-is-erased is missing, and needs.gaps.yaml does not list it under example.restoreNote' need-unlisted
broken needs.gaps.yaml 's/\[says-when-it-is-erased\]/[says-when-it-is-erased, only-its-owner]/' 'needs.gaps.yaml:10:49: error: needs.gaps.yaml lists only-its-owner of example.restoreNote, which is met now: take it out' gap-met
broken restore/needs.yaml 's|^    missing: archived notes.*$|      - source: examples/notes/app/notes.js|' 'needs.gaps.yaml lists says-when-it-is-erased of example.restoreNote, which is met now' gap-met
broken needs.gaps.yaml 's/\[says-when-it-is-erased\]/[says-when-it-is-erased, restore-later]/' 'needs.gaps.yaml lists restore-later of example.restoreNote, which is no need of its needs.yaml' gap-unknown
broken needs.gaps.yaml 's/^  example.restoreNote: .*$/&\n  example.restorNote: [restore-later]/' 'needs.gaps.yaml lists gaps of example.restorNote, which is no flow with a needs.yaml under its folder' gap-unknown

rm -rf "$TMPDIR_CASE/gaps"
cp -r examples/notes "$TMPDIR_CASE/gaps"
rm "$TMPDIR_CASE/gaps/needs.gaps.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/gaps" --strict > "$TMPDIR_CASE/gaps.txt" || code=$?
cat "$TMPDIR_CASE/gaps.txt"
test "$code" -eq 2
grep -F 'need says-when-it-is-erased is missing, and there is no needs.gaps.yaml above it' "$TMPDIR_CASE/gaps.txt" | grep -F '[need-unlisted]'

rm -rf "$TMPDIR_CASE/gaps"
cp -r examples/notes "$TMPDIR_CASE/gaps"
"$DELURIO" check "$TMPDIR_CASE/gaps" --format sarif --output "$TMPDIR_CASE/gaps.sarif" 2> /dev/null
node -e '
  const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
  const [gap, ...rest] = run.results;
  if (rest.length !== 0 || gap.ruleId !== "need-gap" || gap.level !== "note") throw new Error(`one gap, at note: ${JSON.stringify(run.results)}`);
  if (gap.locations[0].physicalLocation.region.startLine !== 47) throw new Error("the gap is at its need");
  if (run.tool.driver.rules.find((r) => r.id === "need-gap").defaultConfiguration.level !== "note") throw new Error("need-gap is a note");
' "$TMPDIR_CASE/gaps.sarif"
sed -i 's/^    need: Only the note.s owner reads and restores it; to anyone else it is no note at all\.$/&\n    missing: the sessions are not signed yet/' "$TMPDIR_CASE/gaps/restore/needs.yaml"
sed -i 's/\[says-when-it-is-erased\]/[says-when-it-is-erased, only-its-owner]/' "$TMPDIR_CASE/gaps/needs.gaps.yaml"
"$DELURIO" check "$TMPDIR_CASE/gaps" --strict --baseline "$TMPDIR_CASE/gaps.sarif" > "$TMPDIR_CASE/gaps.txt" 2>&1
cat "$TMPDIR_CASE/gaps.txt"
grep -F 'note: need only-its-owner is missing: the sessions are not signed yet [need-gap]' "$TMPDIR_CASE/gaps.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 2 gaps of 4 needs (1 new); exit 0' "$TMPDIR_CASE/gaps.txt"
