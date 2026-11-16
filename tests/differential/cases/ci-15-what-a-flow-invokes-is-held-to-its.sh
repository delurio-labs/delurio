# from the reference CI step: What a flow invokes is held to its domain, as errors, and a failure class may list network, timeout and 5xx
cp -r examples/qauth-authorize "$TMPDIR_CASE/invokes"
sed -i 's/    server: \[server_error\]/    server: [server_error, network, timeout, 5xx]/' "$TMPDIR_CASE/invokes/flow.yaml"
"$DELURIO" check "$TMPDIR_CASE/invokes" --strict > "$TMPDIR_CASE/invokes.txt" 2>&1
cat "$TMPDIR_CASE/invokes.txt"
grep -Fx 'delurio check: 1 graph, 0 errors, 0 warnings; exit 0' "$TMPDIR_CASE/invokes.txt"

sed -i 's/invoke: { action: issue }/invoke: { action: issueCode }/; s/    unsafe: \[CSRF_MISMATCH, REQUEST_EXPIRED\]/    unsafe: [CSRF_MISMATCH, REQUEST_EXPIRED, TOKEN_REUSED, unavailable]/' "$TMPDIR_CASE/invokes/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/invokes" --strict > "$TMPDIR_CASE/invokes.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/invokes.txt"
test "$code" -eq 2
grep -F 'error: state issuing invokes issueCode, which is not an action of the domain qauth.authorize' "$TMPDIR_CASE/invokes.txt" | grep -F '[invoke-unknown]'
grep -F 'error: FAIL.unsafe of deciding stands for TOKEN_REUSED, which is not a refusal of decide in the domain qauth.authorize, nor network, timeout or 5xx' "$TMPDIR_CASE/invokes.txt" | grep -F '[refusal-unknown]'
grep -F 'error: FAIL.unsafe of deciding stands for unavailable,' "$TMPDIR_CASE/invokes.txt" | grep -F '[refusal-unknown]'
grep -Fx 'delurio check: 1 graph, 3 errors, 0 warnings; exit 2' "$TMPDIR_CASE/invokes.txt"

sed -i 's/^domain: qauth.authorize/domain: qauth.authorise/' "$TMPDIR_CASE/invokes/flow.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/invokes" --strict > "$TMPDIR_CASE/invokes.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/invokes.txt"
test "$code" -eq 2
grep -F 'error: the flow names the domain qauth.authorise, which no domain beside it is' "$TMPDIR_CASE/invokes.txt" | grep -F '[invoke-unknown]'
