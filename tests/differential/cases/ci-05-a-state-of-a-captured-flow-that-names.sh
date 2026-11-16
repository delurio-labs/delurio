# from the reference CI step: A state of a captured flow that names no code fails the check
cp -r examples/qauth-authorize "$TMPDIR_CASE/captured"
sed -i 's/^status: proposed.*/status: captured/' "$TMPDIR_CASE/captured/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/captured" --strict > "$TMPDIR_CASE/captured.txt" || code=$?
cat "$TMPDIR_CASE/captured.txt"
test "$code" -eq 2
grep -F '[source-missing]' "$TMPDIR_CASE/captured.txt"
! grep -F 'state left names no code' "$TMPDIR_CASE/captured.txt"
