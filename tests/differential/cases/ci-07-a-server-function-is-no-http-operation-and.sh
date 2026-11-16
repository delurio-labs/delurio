# from the reference CI step: A server function is no HTTP operation, and its method says whether it writes
cp -r examples/notes "$TMPDIR_CASE/server-fn"
printf 'actions:\n  restoreNote:\n    route: POST /notes/{id}/restore\n' >> "$TMPDIR_CASE/server-fn/domain/domain.yaml"
sed -i 's/^    serverFn: GET$/    serverFn: GET\n    query: { type: object }/' "$TMPDIR_CASE/server-fn/domain/note.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/server-fn" --strict > "$TMPDIR_CASE/server-fn.txt" || code=$?
cat "$TMPDIR_CASE/server-fn.txt"
test "$code" -eq 2
test "$(grep -c '\[server-fn-http\]' "$TMPDIR_CASE/server-fn.txt")" -eq 2
grep -F 'domain.yaml' "$TMPDIR_CASE/server-fn.txt" | grep -F 'note: its route is given here'

cp -r examples/notes "$TMPDIR_CASE/server-fn-method"
sed -i 's/^    serverFn: POST$/    serverFn: PUT/' "$TMPDIR_CASE/server-fn-method/domain/note.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/server-fn-method" --strict > "$TMPDIR_CASE/server-fn-method.txt" || code=$?
cat "$TMPDIR_CASE/server-fn-method.txt"
test "$code" -eq 2
grep -F 'serverFn: must be "GET" or "POST" [schema]' "$TMPDIR_CASE/server-fn-method.txt"

cp -r examples/notes "$TMPDIR_CASE/server-fn-writes"
sed -i '/^changes:/d' "$TMPDIR_CASE/server-fn-writes/restore/ui.yaml"
sed -i 's/do: \[archiveNote, restoreNote\]/do: [archiveNote]/' "$TMPDIR_CASE/server-fn-writes/domain/note.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/server-fn-writes" --strict > "$TMPDIR_CASE/server-fn-writes.txt" || code=$?
cat "$TMPDIR_CASE/server-fn-writes.txt"
test "$code" -eq 2
grep -F 'its steps write (restoreNote), but ui.yaml has no changes note' "$TMPDIR_CASE/server-fn-writes.txt"
