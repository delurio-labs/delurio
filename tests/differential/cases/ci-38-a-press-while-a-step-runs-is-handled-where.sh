# A press while a step runs is handled where the step's outcome leads (duringStep, SPEC.md §4): a node marked so stays
# usable in a state that runs a step, the state need not handle its event, and every situation the step's DONE and
# FAIL.<class> lead to, past states with `always`, handles it, shows the node disabled or not at all, is an exit, or
# runs a step of its own where the press waits again. A marker that can change nothing is a warning
# (during-step-unused), and on a node that sends no event the schema refuses it. And a `when` that cannot be read keeps
# the graph from loading, as a `checked` or a `disabledWhen` does
cp -r examples/notes "$TMPDIR_CASE/during"
ui="$TMPDIR_CASE/during/edit/ui.yaml"
flow="$TMPDIR_CASE/during/edit/flow.yaml"
# the line of a node in ui.yaml, as it is now
at() { grep -n "^  $1:\$" "$ui" | cut -d: -f1; }

# Cancel, no longer disabled while the note is saved: saving does not handle CANCEL, and the error says the marker
python3 - "$ui" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    disabledIn: [saving]\n    emits: CANCEL\n", "    emits: CANCEL\n")
open(p, "w").write(s)
EOF
code=0
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/during.txt"
test "$code" -eq 2
test "$(grep -c '\[event-unhandled\]' "$TMPDIR_CASE/during.txt")" -eq 1
grep -F "edit/ui.yaml:$(at cancel):3: error: node cancel sends CANCEL, and saving shows it enabled but has no on.CANCEL: a node shown and not disabled can be pressed, so the state handles its event, or the node is disabled there (disabledIn, disabledWhen) or not shown, or, as the state runs a step, marked duringStep, so that its event is handled where the step's outcome leads (SPEC.md §4) [event-unhandled]" "$TMPDIR_CASE/during.txt"

# marked: saving's outcomes lead to saved and not_found, both exits, so nothing is found
sed -i 's/^    emits: CANCEL$/&\n    duringStep: true/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1
cat "$TMPDIR_CASE/during.txt"
if grep -F -e '[event-unhandled]' -e '[during-step-unused]' "$TMPDIR_CASE/during.txt"; then exit 1; fi
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/during.txt"

# saved made a state the owner stays in, which shows Cancel and does not handle CANCEL: a press made while saving is
# left there, and saved itself is held to it as before
sed -i 's/^  saved: { type: final, title: Saved }$/  saved: { title: Saved, on: { EDIT: editing } }/' "$flow"
python3 - "$ui" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    in: [editing, saving]\n    emits: CANCEL\n", "    in: [editing, saving, saved]\n    emits: CANCEL\n")
open(p, "w").write(s)
EOF
code=0
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/during.txt"
test "$code" -eq 2
test "$(grep -c '\[event-unhandled\]' "$TMPDIR_CASE/during.txt")" -eq 2
grep -F "edit/ui.yaml:$(at cancel):3: error: node cancel sends CANCEL, and saving shows it enabled while its step runs; it is marked duringStep, so a press there is handled once the step ends, but after DONE the flow stops in saved, which shows it enabled and has no on.CANCEL: where a step's outcome leads, a node marked duringStep has its event handled, or is disabled (disabledIn, disabledWhen) or not shown (SPEC.md §4) [event-unhandled]" "$TMPDIR_CASE/during.txt"
grep -F "edit/ui.yaml:$(at cancel):3: error: node cancel sends CANCEL, and saved shows it enabled but has no on.CANCEL" "$TMPDIR_CASE/during.txt"
# its notes, and in SARIF its code flow, give the way to saved through the step
grep -F "edit/ui.yaml:$(at cancel):3: note: step 2: DONE" "$TMPDIR_CASE/during.txt"
grep -F "edit/ui.yaml:$(at cancel):3: note: reached: saved" "$TMPDIR_CASE/during.txt"
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app --format sarif --output "$TMPDIR_CASE/during.sarif" > /dev/null 2>&1 || true
node -e '
const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
const r = run.results.filter((x) => x.ruleId === "event-unhandled" && x.locations[0].logicalLocations[0].fullyQualifiedName === "example.editNote/nodes/cancel/emits/CANCEL/saving");
if (r.length !== 1) throw new Error(`${r.length} results for saving`);
const steps = r[0].codeFlows[0].threadFlows[0].locations.map((l) => l.location.message.text);
if (steps.join(" | ") !== "start: pinned = false | step 1: SAVE | step 2: DONE | reached: saved") throw new Error(steps.join(" | "));
' "$TMPDIR_CASE/during.sarif"
# once saved handles CANCEL, nothing; and once it hides Cancel, nothing either
sed -i 's/on: { EDIT: editing }/on: { EDIT: editing, CANCEL: discarding }/' "$flow"
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1
if grep -F '[event-unhandled]' "$TMPDIR_CASE/during.txt"; then exit 1; fi
sed -i 's/on: { EDIT: editing, CANCEL: discarding }/on: { EDIT: editing }/; ' "$flow"
sed -i 's/^    in: \[editing, saving, saved\]$/    in: [editing, saving]/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1
if grep -F '[event-unhandled]' "$TMPDIR_CASE/during.txt"; then exit 1; fi

# a marker on a node that no step shows (Keep it, in the dialog) is a warning, which --strict does not fail
sed -i 's/^    emits: KEEP$/&\n    duringStep: true/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1
cat "$TMPDIR_CASE/during.txt"
grep -F "edit/ui.yaml:$(grep -n '^    duringStep: true$' "$ui" | tail -n 1 | cut -d: -f1):5: warning: keep is marked duringStep, but no state that runs a step (invoke) shows it enabled, by its in, unless and disabledIn: the marker keeps a node usable while its state's step runs, and holds its event where the step's outcome leads, so here it changes nothing (SPEC.md §4) [during-step-unused]" "$TMPDIR_CASE/during.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 1 warning, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/during.txt"

# on a node that sends no event (the status that says the note is being saved), the schema refuses it
sed -i 's/^    text: { key: Edit.saving }$/&\n    duringStep: true/' "$ui"
code=0
"$DELURIO" check "$TMPDIR_CASE/during" --strict --no-app > "$TMPDIR_CASE/during.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/during.txt"
test "$code" -eq 2
grep -F 'nodes.working: "emits" is missing (duringStep is only on a node that sends an event: it says where that event is handled) [schema]' "$TMPDIR_CASE/during.txt"

# a when that does not parse, or names what is not a variable, a guard or `state`, keeps the graph from loading, at it
cp -r examples/notes "$TMPDIR_CASE/when"
ui="$TMPDIR_CASE/when/edit/ui.yaml"
sed -i 's/^    role: list$/&\n    when: "pinned =="/' "$ui"
code=0
"$DELURIO" check "$TMPDIR_CASE/when" --strict --no-app > "$TMPDIR_CASE/when.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/when.txt"
test "$code" -eq 1
grep -F "edit/ui.yaml:$(grep -n '^    when: ' "$ui" | cut -d: -f1):5: error: ui node options: when: " "$TMPDIR_CASE/when.txt" | grep -F '[load]'
sed -i 's/^    when: "pinned =="$/    when: pined/' "$ui"
code=0
"$DELURIO" check "$TMPDIR_CASE/when" --strict --no-app > "$TMPDIR_CASE/when.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/when.txt"
test "$code" -eq 1
grep -F 'ui node options: when: pined names pined, which is not a variable or a guard of the flow [load]' "$TMPDIR_CASE/when.txt"
sed -i 's/^    when: pined$/    when: "pinned and state == '"'"'saving'"'"'"/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/when" --strict --no-app > "$TMPDIR_CASE/when.txt" 2>&1
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/when.txt"
