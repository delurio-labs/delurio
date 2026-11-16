# from the reference CI step: A tutorial that no longer matches its flow fails the check at its step
t=edit/tutorial.yaml
broken() { rm -rf "$TMPDIR_CASE/tutorial"; cp -r examples/notes "$TMPDIR_CASE/tutorial"; sed -i "$2" "$TMPDIR_CASE/tutorial/$1"; code=0; "$DELURIO" check "$TMPDIR_CASE/tutorial" --strict > "$TMPDIR_CASE/tutorial.txt" || code=$?; cat "$TMPDIR_CASE/tutorial.txt"; test "$code" -eq 2; grep -F "$3" "$TMPDIR_CASE/tutorial.txt" | grep -F '[tutorial-mismatch]'; }
broken $t 's/    press: save/    press: submit/' 'tutorial.yaml:28:5: error: step 4: press submit, which is not a node of ui.yaml'
broken $t 's/after: \[DONE\]/after: [FINISHED]/' 'step 5: after FINISHED, which no state of example.editNote handles'
broken $t 's/{pinBox}/{pinButton}/' 'step 3: {pinButton} is neither a sample value nor a node of ui.yaml'
broken $t 's/scenario: { pinned: false }/scenario: { pinned: maybe }/' 'scenario: "maybe" is not a value of pinned'
broken $t 's/  - show: saving/  - show: editing/' 'step 5: shows editing, and the flow is in saving here'
broken edit/ui.yaml 's/^    disabledIn: \[saving\]$/    disabledIn: [editing, saving]/' 'step 4: press save, which is disabled in editing'
broken edit/flow.yaml 's/^      DONE: saved$/      DONE: editing/' 'step 6: the tutorial ends in saved, and the flow is in editing here'

cp -r examples/sign-out "$TMPDIR_CASE/diagram"
printf 'schema: delurio-graph/0\nkind: tutorial\nid: example.signOut.tutorial\nflow: example.signOut\ntitle: Signing out\nstage: diagram\nscenario: { token: valid, hasCookie: true }\nsteps:\n  - show: checking\n    say: "The app sends the token it holds."\n  - end: signed_out.cookie_cleared\n    say: "The session ends, and the cookie with it."\n' > "$TMPDIR_CASE/diagram/tutorial.yaml"
"$DELURIO" check "$TMPDIR_CASE/diagram" --strict
sed -i 's/  - end: signed_out.cookie_cleared/  - end: signed_out/' "$TMPDIR_CASE/diagram/tutorial.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/diagram" --strict > "$TMPDIR_CASE/diagram.txt" || code=$?
cat "$TMPDIR_CASE/diagram.txt"
test "$code" -eq 2
grep -F 'step 2: signed_out is not reached from checking without an event (the way stops in signed_out.cookie_cleared) [tutorial-mismatch]' "$TMPDIR_CASE/diagram.txt"
