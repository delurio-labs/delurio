# from the reference CI step: A graph that cannot be loaded is reported at its place, and the other graphs are still checked
mkdir -p "$TMPDIR_CASE/load/no-catalog" "$TMPDIR_CASE/load/no-catalog-file" "$TMPDIR_CASE/load/guard" "$TMPDIR_CASE/load/starter" "$TMPDIR_CASE/load/not-yaml"
printf 'schema: delurio-graph/0\nkind: flow\nid: example.form\ninitial: form\nstates:\n  form: { on: { SEND: done } }\n  done: { type: final }\n' > "$TMPDIR_CASE/load/no-catalog/flow.yaml"
printf 'schema: delurio-graph/0\nkind: ui\nid: example.form\nflow: example.form\nnodes:\n  send: { type: action, component: Button, role: button, text: { key: Form.send }, in: [form], emits: SEND }\n' > "$TMPDIR_CASE/load/no-catalog/ui.yaml"
cp "$TMPDIR_CASE/load/no-catalog/flow.yaml" "$TMPDIR_CASE/load/no-catalog-file/flow.yaml"
sed 's/^flow: example.form$/&\ncatalog: catalog\/en.json/' "$TMPDIR_CASE/load/no-catalog/ui.yaml" > "$TMPDIR_CASE/load/no-catalog-file/ui.yaml"
printf 'schema: delurio-graph/0\nkind: flow\nid: example.guard\ncontext: { a: { values: [x, y], initial: any } }\ninitial: s\nstates:\n  s: { on: { GO: { target: done, guard: "a ==" } } }\n  done: { type: final }\n' > "$TMPDIR_CASE/load/guard/flow.yaml"
printf 'schema: delurio-graph/0\nkind: flow\nid: example.starter\ninitial: s\nstates:\n  s: { on: { GO: { target: done, starts: { flow: example.signOut } } } }\n  done: { type: final }\n' > "$TMPDIR_CASE/load/starter/flow.yaml"
printf 'schema: delurio-graph/0\nkind: flow\nid: example.notYaml\nstates: [\n' > "$TMPDIR_CASE/load/not-yaml/flow.yaml"
cp -r examples/sign-out "$TMPDIR_CASE/load/sign-out"
cp -r examples/qauth-authorize "$TMPDIR_CASE/load/target"
sed -i 's/      DONE: codeIssued/      DONE: codeIsued/' "$TMPDIR_CASE/load/target/flow.yaml"
cp -r examples/notes "$TMPDIR_CASE/load/notes"
printf 'oops: [\n' >> "$TMPDIR_CASE/load/notes/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/load" > "$TMPDIR_CASE/load.txt" 2> "$TMPDIR_CASE/load.err" || code=$?
cat "$TMPDIR_CASE/load.txt" "$TMPDIR_CASE/load.err"
test "$code" -eq 1
grep -F 'delurio check: 10 graphs' "$TMPDIR_CASE/load.err"
! grep -E 'ERR_INVALID_ARG_TYPE|ENOENT|    at ' "$TMPDIR_CASE/load.err"
grep -F 'no-catalog/ui.yaml:1:1: error: ui.yaml names no catalog' "$TMPDIR_CASE/load.txt" | grep -F '[load]'
grep -F 'no-catalog-file/ui.yaml:5:1: error: the catalog catalog/en.json that ui.yaml names does not exist [load]' "$TMPDIR_CASE/load.txt"
grep -F 'guard/flow.yaml:7:14: error: unexpected end in: a == [load]' "$TMPDIR_CASE/load.txt"
grep -F 'target/flow.yaml:129:7: error: state issuing goes to codeIsued, which does not exist [load]' "$TMPDIR_CASE/load.txt"
grep -E 'notes/edit/ui\.yaml:[0-9]+:1: error: .*\[load\]' "$TMPDIR_CASE/load.txt"
! grep -F 'notes/edit/flow.yaml' "$TMPDIR_CASE/load.txt"
! grep -F 'starter/flow.yaml' "$TMPDIR_CASE/load.txt"
"$DELURIO" check "$TMPDIR_CASE/load" --format sarif --output "$TMPDIR_CASE/load.sarif" > /dev/null 2>&1 || true
node -e '
  const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
  const at = run.results.filter((r) => r.ruleId === "load").map((r) => `${r.locations[0].physicalLocation.artifactLocation.uri.split("/load/")[1]}:${r.locations[0].physicalLocation.region.startLine}`);
  for (const want of ["no-catalog/ui.yaml:1", "no-catalog-file/ui.yaml:5", "guard/flow.yaml:7", "target/flow.yaml:129"]) if (!at.includes(want)) throw new Error(`no load result at ${want}: ${at.join(", ")}`);
' "$TMPDIR_CASE/load.sarif"
