# from the reference CI step (b99f12f): A field with no role is a warning, and the kit check's expectation says its role is not given
cp -r examples/notes "$TMPDIR_CASE/role-missing"
sed -i '/^  bodyField:$/,/^    role: textbox$/{/^    role: textbox$/d}' "$TMPDIR_CASE/role-missing/edit/ui.yaml"
"$DELURIO" check "$TMPDIR_CASE/role-missing" --strict > "$TMPDIR_CASE/role-missing.txt"
cat "$TMPDIR_CASE/role-missing.txt"
grep -F 'edit/ui.yaml' "$TMPDIR_CASE/role-missing.txt" | grep -F 'warning: bodyField is a field with no role' | grep -F '[role-missing]'
