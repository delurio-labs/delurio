# from the reference CI step: A state no situation reaches, or one that is no exit and has no way out, fails the check
mkdir -p "$TMPDIR_CASE/reach"
printf 'schema: delurio-graph/0\nkind: flow\nid: example.checkout\nstatus: proposed\ntitle: Check out\ninitial: cart\nstates:\n  cart: { by: human, on: { CONTINUE: done } }\n  pay: { by: human }\n  done: { type: final }\n' > "$TMPDIR_CASE/reach/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/reach" --strict > "$TMPDIR_CASE/reach.txt" || code=$?
cat "$TMPDIR_CASE/reach.txt"
test "$code" -eq 2
grep -F 'reach/flow.yaml:9:3: error: state pay is not an exit and has no way out' "$TMPDIR_CASE/reach.txt" | grep -F '[state-no-way-out]'
grep -F 'reach/flow.yaml:9:3: error: state pay is reached by no situation the flow starts in: no transition leads to it' "$TMPDIR_CASE/reach.txt" | grep -F '[state-unreachable]'
"$DELURIO" check "$TMPDIR_CASE/reach"

cp -r examples/notes "$TMPDIR_CASE/guarded"
sed -i 's/^      CANCEL: discarding$/      CANCEL: { target: discarding, guard: "pinned and not pinned" }/' "$TMPDIR_CASE/guarded/edit/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/guarded" --strict > "$TMPDIR_CASE/guarded.txt" || code=$?
cat "$TMPDIR_CASE/guarded.txt"
test "$code" -eq 2
test "$(grep -c '\[state-unreachable\]' "$TMPDIR_CASE/guarded.txt")" -eq 2
grep -F 'state discarding is reached by no situation the flow starts in: the transitions that lead to it are never taken' "$TMPDIR_CASE/guarded.txt"
! grep -F '[state-no-way-out]' "$TMPDIR_CASE/guarded.txt"
