# from the reference CI step: What a node holds is held to its flow's domain, as errors (SPEC.md §4)
cp -r examples/qauth-authorize "$TMPDIR_CASE/holds"
sed -i '/^  title:$/a\    holds: Client.name' "$TMPDIR_CASE/holds/ui.yaml"
sed -i '/^  account:$/a\    holds: AuthorizationCode.value' "$TMPDIR_CASE/holds/ui.yaml"
"$DELURIO" check "$TMPDIR_CASE/holds" --strict > "$TMPDIR_CASE/holds.txt" 2>&1
cat "$TMPDIR_CASE/holds.txt"
grep -Fx 'delurio check: 1 graph, 0 errors, 0 warnings; exit 0' "$TMPDIR_CASE/holds.txt"

sed -i 's/    holds: Client.name/    holds: Client.host/; s/    holds: AuthorizationCode.value/    holds: Session.email/' "$TMPDIR_CASE/holds/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/holds" --strict > "$TMPDIR_CASE/holds.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/holds.txt"
test "$code" -eq 2
grep -F 'ui.yaml:' "$TMPDIR_CASE/holds.txt" | grep -F 'error: node title holds Client.host, and host is not a field of Client in the domain qauth.authorize' | grep -F '[holds-unknown]'
grep -F 'error: node account holds Session.email, and Session is not an entity of the domain qauth.authorize' "$TMPDIR_CASE/holds.txt" | grep -F '[holds-unknown]'
grep -Fx 'delurio check: 1 graph, 2 errors, 0 warnings; exit 2' "$TMPDIR_CASE/holds.txt"

sed -i 's/^domain: qauth.authorize/domain: qauth.authorise/' "$TMPDIR_CASE/holds/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/holds" --strict > "$TMPDIR_CASE/holds.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/holds.txt"
test "$code" -eq 2
grep -F 'error: the flow names the domain qauth.authorise, which no domain beside it is (under the nearest folder called graphs), so what its nodes hold is not checked' "$TMPDIR_CASE/holds.txt" | grep -F '[holds-unknown]'

# a holds that is not <Entity>.<field> is the schema's
sed -i 's/^domain: qauth.authorise/domain: qauth.authorize/; s/    holds: Client.host/    holds: the client/' "$TMPDIR_CASE/holds/ui.yaml" "$TMPDIR_CASE/holds/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/holds" --strict > "$TMPDIR_CASE/holds.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/holds.txt"
test "$code" -ne 0
grep -F '[schema]' "$TMPDIR_CASE/holds.txt" | grep -F 'holds'
