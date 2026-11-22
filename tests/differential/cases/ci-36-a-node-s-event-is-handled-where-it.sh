# A node's event is handled where the node is shown and enabled (event-unhandled, SPEC.md §4): in every situation the
# checker reaches, a state that shows a node with `emits`, by `in`, `unless` and `when`, and does not disable it, by
# `disabledIn` and `disabledWhen`, has a transition on the event. Not held: NAVIGATE, the person leaving the page,
# which is always possible; an exit, whose page is outside the flow; and a state with `always`, passed at once
cp -r examples/notes "$TMPDIR_CASE/events"
ui="$TMPDIR_CASE/events/edit/ui.yaml"
# the line of a node in ui.yaml, as it is now
at() { grep -n "^  $1:\$" "$ui" | cut -d: -f1; }
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/events.txt"

# Save, shown in saving and no longer disabled there, sends SAVE where saving has no on.SAVE
python3 - "$ui" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    disabledIn: [saving]\n    emits: SAVE\n", "    emits: SAVE\n")
open(p, "w").write(s)
EOF
code=0
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/events.txt"
test "$code" -eq 2
test "$(grep -c '\[event-unhandled\]' "$TMPDIR_CASE/events.txt")" -eq 1
grep -F "edit/ui.yaml:$(at save):3: error: node save sends SAVE, and saving shows it enabled but has no on.SAVE: a node shown and not disabled can be pressed, so the state handles its event, or the node is disabled there (disabledIn, disabledWhen) or not shown, or, as the state runs a step, marked duringStep, so that its event is handled where the step's outcome leads (SPEC.md §4) [event-unhandled]" "$TMPDIR_CASE/events.txt"
grep -Fx 'delurio check: 3 graphs, 1 error, 0 warnings, 1 gap of 4 needs; exit 2' "$TMPDIR_CASE/events.txt"

# disabled by the context instead: where pinned is false only, so the situation that shows it enabled is given
python3 - "$ui" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    emits: SAVE\n", "    disabledWhen: \"not pinned\"\n    emits: SAVE\n")
open(p, "w").write(s)
EOF
code=0
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/events.txt"
test "$code" -eq 2
grep -F "edit/ui.yaml:$(at save):3: error: node save sends SAVE, and saving shows it enabled (where its disabledWhen, not pinned, does not hold) but has no on.SAVE" "$TMPDIR_CASE/events.txt" | grep -F '[event-unhandled]'
grep -F "edit/ui.yaml:$(at save):3: note: start: pinned = true" "$TMPDIR_CASE/events.txt"
grep -F "edit/ui.yaml:$(at save):3: note: step 1: SAVE" "$TMPDIR_CASE/events.txt"
grep -F "edit/ui.yaml:$(at save):3: note: reached: saving" "$TMPDIR_CASE/events.txt"
# in SARIF, the way there is the finding's code flow
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app --format sarif --output "$TMPDIR_CASE/events.sarif" > /dev/null 2>&1 || true
node -e '
const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
const r = run.results.filter((x) => x.ruleId === "event-unhandled");
if (r.length !== 1) throw new Error(`${r.length} event-unhandled results`);
if (r[0].level !== "error") throw new Error(r[0].level);
if (r[0].locations[0].logicalLocations[0].fullyQualifiedName !== "example.editNote/nodes/save/emits/SAVE/saving") throw new Error("element");
const steps = r[0].codeFlows[0].threadFlows[0].locations.map((l) => l.location.message.text);
if (steps.join(" | ") !== "start: pinned = true | step 1: SAVE | reached: saving") throw new Error(steps.join(" | "));
' "$TMPDIR_CASE/events.sarif"

# disabled where pinned is false or the step runs: nothing to find
sed -i 's/disabledWhen: "not pinned"/disabledWhen: "not pinned or state == '"'"'saving'"'"'"/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1
if grep -F '[event-unhandled]' "$TMPDIR_CASE/events.txt"; then exit 1; fi

# Cancel shown only when the note is pinned (when), and no longer disabled in saving: found where it is pinned
python3 - "$ui" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    disabledIn: [saving]\n    emits: CANCEL\n", "    when: pinned\n    emits: CANCEL\n")
open(p, "w").write(s)
EOF
code=0
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/events.txt"
test "$code" -eq 2
grep -F "edit/ui.yaml:$(at cancel):3: error: node cancel sends CANCEL, and saving shows it enabled (where its when, pinned, holds) but has no on.CANCEL" "$TMPDIR_CASE/events.txt"
sed -i 's/^    when: pinned$/    when: "not pinned"\n    unless: [saving]/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1
if grep -F '[event-unhandled]' "$TMPDIR_CASE/events.txt"; then exit 1; fi

# Keep it, shown on the exit too: an exit has no transitions, so what a node on its page does is outside the flow
python3 - "$ui" <<'EOF'
import sys
p = sys.argv[1]
s = open(p).read()
s = s.replace("    in: [discarding]\n    emits: KEEP\n", "    in: [discarding, saved]\n    emits: KEEP\n")
open(p, "w").write(s)
EOF
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1
if grep -F '[event-unhandled]' "$TMPDIR_CASE/events.txt"; then exit 1; fi

# saved made a state the owner stays in, which does not handle KEEP: found; once it does, nothing
sed -i 's/^  saved: { type: final, title: Saved }$/  saved: { title: Saved, on: { EDIT: editing } }/' "$TMPDIR_CASE/events/edit/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/events.txt"
test "$code" -eq 2
test "$(grep -c '\[event-unhandled\]' "$TMPDIR_CASE/events.txt")" -eq 1
grep -F "edit/ui.yaml:$(at keep):3: error: node keep sends KEEP, and saved shows it enabled but has no on.KEEP" "$TMPDIR_CASE/events.txt"
sed -i 's/on: { EDIT: editing }/on: { EDIT: editing, KEEP: editing }/' "$TMPDIR_CASE/events/edit/flow.yaml"
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1
if grep -F '[event-unhandled]' "$TMPDIR_CASE/events.txt"; then exit 1; fi

# a node that leaves the page (NAVIGATE) may be shown in any state: leaving is always possible (SPEC.md §2)
sed -i 's/on: { EDIT: editing, KEEP: editing }/on: { EDIT: editing }/' "$TMPDIR_CASE/events/edit/flow.yaml"
sed -i 's/^    emits: KEEP$/    emits: NAVIGATE/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/events" --strict --no-app > "$TMPDIR_CASE/events.txt" 2>&1
cat "$TMPDIR_CASE/events.txt"
if grep -F '[event-unhandled]' "$TMPDIR_CASE/events.txt"; then exit 1; fi
