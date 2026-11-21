# A field that is a native control WAI-ARIA gives no role (a date input) says so with `native`: no role-missing
# warning; a role beside it is refused by the schema
cp -r examples/notes "$TMPDIR_CASE/native"
sed -i 's/^    role: textbox$/    native: date/' "$TMPDIR_CASE/native/edit/ui.yaml"
grep -q '^    native: date$' "$TMPDIR_CASE/native/edit/ui.yaml"
"$DELURIO" check "$TMPDIR_CASE/native" --strict > "$TMPDIR_CASE/native.txt"
cat "$TMPDIR_CASE/native.txt"
if grep -F '[role-missing]' "$TMPDIR_CASE/native.txt"; then exit 1; fi
cp -r examples/notes "$TMPDIR_CASE/native-role"
sed -i 's/^    role: textbox$/    role: textbox\n    native: date/' "$TMPDIR_CASE/native-role/edit/ui.yaml"
set +e
"$DELURIO" check "$TMPDIR_CASE/native-role" --strict > "$TMPDIR_CASE/native-role.txt"
code=$?
set -e
cat "$TMPDIR_CASE/native-role.txt"
test "$code" -ne 0
grep -F 'role: is not allowed here (a node that names its native control gives no role' "$TMPDIR_CASE/native-role.txt"
