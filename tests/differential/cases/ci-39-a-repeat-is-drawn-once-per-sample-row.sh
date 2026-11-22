# A repeat's items given as rows in the samples (SPEC.md §4, Rows of a repeat): a node with that repeat is drawn once
# per row, its params reading the row's fields (row.<field>), and its `checked` may read the row; a table is drawn row
# by row, its header cells first. An effect that sets a variable from the row pressed (row.<field>) lets it take any of
# its values: the sample rows illustrate the screens and never decide what the flow reaches. A tutorial's press says
# which row it presses (row, SPEC.md §7) and takes the case that row gives
d="$TMPDIR_CASE/rows/exam"
rm -rf "$TMPDIR_CASE/rows"
mkdir -p "$d/catalog"
cat > "$d/flow.yaml" <<'EOF'
schema: delurio-graph/0
kind: flow
id: exam.take
status: proposed
title: Answer a question
summary: The person chooses one option of a question, finishes, and sees the class's results.
context:
  answer:
    values: [none, a, b, c]
    initial: none
    say: { none: nothing is chosen, a: a is chosen, b: b is chosen, c: c is chosen }
effects:
  choose: { set: { answer: row.id } }
initial: answering
states:
  answering:
    title: Answering
    on:
      CHOOSE: { target: answering, effect: [choose], say: the person chooses an option }
      FINISH: checking
  checking:
    title: Checking the answer
    always:
      - { target: right, guard: "answer == 'c'" }
      - { target: wrong }
  right: { type: final, title: Right }
  wrong: { type: final, title: Wrong }
EOF
cat > "$d/ui.yaml" <<'EOF'
schema: delurio-graph/0
kind: ui
id: exam.take
flow: exam.take
status: proposed
catalog: catalog/en.json
locales: [en]
samples:
  question: How many legs has a spider?
  question.options: [ { id: a, text: Six }, { id: b, text: Eight } ]
  results.rows: [ { name: Ada, score: "90" }, { name: Bo, score: "75" } ]
nodes:
  page: { type: page, component: Page, role: main, in: [answering, right, wrong] }
  question: { type: text, component: Heading, role: heading, text: { key: Exam.question }, in: [answering] }
  options: { type: list, component: List, role: radiogroup, name: { labelledBy: question }, in: [answering] }
  option: { type: field, component: Radio, role: radio, repeat: question.options, text: { key: Exam.option, params: { option: row.text } }, checked: "row.id == answer", in: [answering], emits: CHOOSE }
  finish: { type: action, component: Button, role: button, intent: confirm, text: { key: Exam.finish }, in: [answering], emits: FINISH }
  results: { type: list, component: List, role: table, name: { key: Exam.results }, in: [right, wrong] }
  colName: { type: text, component: Text, role: columnheader, text: { key: Exam.colName }, in: [right, wrong] }
  colScore: { type: text, component: Text, role: columnheader, text: { key: Exam.colScore }, in: [right, wrong] }
  rowName: { type: text, component: Text, role: rowheader, repeat: results.rows, text: { key: Exam.name, params: { name: row.name } }, in: [right, wrong] }
  rowScore: { type: text, component: Text, role: cell, repeat: results.rows, text: { key: Exam.score, params: { score: row.score } }, in: [right, wrong] }
EOF
cat > "$d/catalog/en.json" <<'EOF'
{ "Exam.question": "{question}", "Exam.option": "{option}", "Exam.finish": "Finish", "Exam.results": "Results", "Exam.colName": "Name", "Exam.colScore": "Score", "Exam.name": "{name}", "Exam.score": "{score}" }
EOF
cat > "$d/tutorial.yaml" <<'EOF'
schema: delurio-graph/0
kind: tutorial
flow: exam.take
title: Answer the question
audience: a student
locale: en
steps:
  - { show: answering, say: "Read the question." }
  - { press: option, row: 2, then: { answer: b }, say: "Choose {option}." }
  - { press: finish }
  - { end: wrong }
EOF

# it passes --strict: right is reached, though no sample row gives c, as the row pressed may be any option
"$DELURIO" check "$TMPDIR_CASE/rows" --strict --no-app > "$TMPDIR_CASE/rows.txt" 2>&1
cat "$TMPDIR_CASE/rows.txt"
grep -Fx 'delurio check: 1 graph, 0 errors, 0 warnings; exit 0' "$TMPDIR_CASE/rows.txt"

# the page draws an option for each row, and the table row by row, its header first; the tree gives the same rows
"$DELURIO" page "$d" --out "$TMPDIR_CASE/rows-page" --strict
page="$TMPDIR_CASE/rows-page/index.html"
grep -F '<div class="wf-item" data-node="option"><label class="wf-check"><input type="radio" tabindex="-1"><span>Six</span>' "$page"
grep -F '<div class="wf-item" data-node="option"><label class="wf-check"><input type="radio" tabindex="-1"><span>Eight</span>' "$page"
test "$(grep -o '<li class="wf-li"' "$page" | wc -l)" -eq 2
grep -F '<div class="wf-table" data-node="results" style="display:grid;gap:4px">' "$page"
grep -F 'font-weight:600"><div class="wf-item" data-node="colName"><p class="wf-t">Name</p>' "$page"
grep -F '<div class="wf-item" data-node="rowName"><p class="wf-t">Bo</p><span class="chip">rowheader</span></div><div class="wf-item" data-node="rowScore"><p class="wf-t">75</p>' "$page"
node -e '
const html = require("fs").readFileSync(process.argv[1], "utf8");
const trees = [...html.matchAll(/<pre class="ax">([^<]*)<\/pre>/g)].map((m) => m[1].replaceAll("&quot;", "\""));
const want = ["main", "  table \"Results\"", "    row", "      columnheader", "      columnheader", "    row", "      rowheader", "      cell", "    row", "      rowheader", "      cell"].join("\n");
if (!trees.includes(want)) throw new Error(trees.join("\n---\n"));
if (!trees.some((t) => t.includes("  radiogroup \"How many legs has a spider?\"\n    listitem\n      radio \"Six\"  [→ CHOOSE]\n    listitem\n      radio \"Eight\"  [→ CHOOSE]"))) throw new Error(trees.join("\n---\n"));
' "$page"

# the tutorial's row is held to the rows: one past them, and a then the row does not give, stop it there
cp "$d/tutorial.yaml" "$TMPDIR_CASE/rows-tutorial.yaml"
sed -i 's/row: 2, then: { answer: b }/row: 3/' "$d/tutorial.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/rows" --strict --no-app > "$TMPDIR_CASE/rows.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/rows.txt"
test "$code" -eq 2
grep -F 'step 2: press option with row 3, and the samples give 2 rows for question.options [tutorial-mismatch]' "$TMPDIR_CASE/rows.txt"
cp "$TMPDIR_CASE/rows-tutorial.yaml" "$d/tutorial.yaml"
sed -i 's/then: { answer: b }/then: { answer: a }/' "$d/tutorial.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/rows" --strict --no-app > "$TMPDIR_CASE/rows.txt" 2>&1 || code=$?
test "$code" -eq 2
grep -F 'step 2: no case of this step has answer = "a" [tutorial-mismatch]' "$TMPDIR_CASE/rows.txt"
# a row whose value the variable does not have
cp "$TMPDIR_CASE/rows-tutorial.yaml" "$d/tutorial.yaml"
sed -i 's/{ id: b, text: Eight }/{ id: e, text: Eight }/' "$d/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/rows" --strict --no-app > "$TMPDIR_CASE/rows.txt" 2>&1 || code=$?
test "$code" -eq 2
grep -F 'step 2: row 2 of option gives answer = "e", which is not a value of answer ("none", "a", "b", "c") [tutorial-mismatch]' "$TMPDIR_CASE/rows.txt"
sed -i 's/{ id: e, text: Eight }/{ id: b, text: Eight }/' "$d/ui.yaml"

# a sample's rows are maps, and a node that reads its row in checked has a repeat
sed -i 's/^  results.rows: .*$/  results.rows: [ Ada, Bo ]/' "$d/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/rows" --strict --no-app > "$TMPDIR_CASE/rows.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/rows.txt"
test "$code" -eq 2
grep -F 'samples.results.rows.0: must be an object [schema]' "$TMPDIR_CASE/rows.txt"
sed -i 's/^  results.rows: .*$/  results.rows: [ { name: Ada, score: "90" }, { name: Bo, score: "75" } ]/' "$d/ui.yaml"
sed -i 's/ repeat: question.options,//' "$d/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/rows" --strict --no-app > "$TMPDIR_CASE/rows.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/rows.txt"
test "$code" -eq 1
grep -F 'ui node option: checked: row.id == answer names row.id, which is not a variable or a guard of the flow: a node reads the row it is drawn for (row.<field>) only when it has repeat [load]' "$TMPDIR_CASE/rows.txt"
