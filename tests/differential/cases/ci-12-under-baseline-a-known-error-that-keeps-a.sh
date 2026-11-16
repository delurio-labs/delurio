# from the reference CI step: Under --baseline, a known error that keeps a graph from loading is still an error
mkdir -p "$TMPDIR_CASE/known/no-catalog-file"
printf 'schema: delurio-graph/0\nkind: flow\nid: example.form\ninitial: form\nstates:\n  form: { on: { SEND: done } }\n  done: { type: final }\n' > "$TMPDIR_CASE/known/no-catalog-file/flow.yaml"
printf 'schema: delurio-graph/0\nkind: ui\nid: example.form\nflow: example.form\ncatalog: catalog/en.json\nnodes:\n  send: { type: action, component: Button, role: button, text: { key: Form.send }, in: [form], emits: SEND }\n' > "$TMPDIR_CASE/known/no-catalog-file/ui.yaml"
cp -r examples/qauth-authorize "$TMPDIR_CASE/known/schema"
sed -i 's/left: { type: final, title: The user leaves, leavesPage: true }/left: { type: final, title: The user leaves, and nothing is sent, leavesPage: true }/' "$TMPDIR_CASE/known/schema/flow.yaml"
"$DELURIO" check "$TMPDIR_CASE/known" --format sarif --output "$TMPDIR_CASE/known.sarif" > /dev/null 2>&1 || true
code=0
"$DELURIO" check "$TMPDIR_CASE/known" --strict --baseline "$TMPDIR_CASE/known.sarif" > "$TMPDIR_CASE/known.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/known.txt"
test "$code" -eq 1
grep -F 'no-catalog-file/ui.yaml:5:1: error: the catalog catalog/en.json that ui.yaml names does not exist [load]' "$TMPDIR_CASE/known.txt"
! grep -F 'warning: the catalog' "$TMPDIR_CASE/known.txt"
grep -F 'schema/flow.yaml' "$TMPDIR_CASE/known.txt" | grep -F 'warning:' | grep -F '[schema]'
grep -E 'delurio check: 2 graphs, 1 error, [1-9][0-9]* warnings?; exit 1' "$TMPDIR_CASE/known.txt"
