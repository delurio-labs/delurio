# from the reference CI step: A control may be disabled by what the context holds (disabledWhen), read as checked is
cp -r examples/notes "$TMPDIR_CASE/disabled-when"
sed -i 's/^    emits: SAVE$/&\n    disabledWhen: "not pinned"/' "$TMPDIR_CASE/disabled-when/edit/ui.yaml"
"$DELURIO" check "$TMPDIR_CASE/disabled-when" --strict
"$DELURIO" page "$TMPDIR_CASE/disabled-when/edit" --out "$TMPDIR_CASE/disabled-when-page" --kit none
grep -F 'disabled when not pinned' "$TMPDIR_CASE/disabled-when-page/index.html"

sed -i 's/disabledWhen: "not pinned"/disabledWhen: "not pined"/' "$TMPDIR_CASE/disabled-when/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/disabled-when" --strict > "$TMPDIR_CASE/disabled-when.txt" || code=$?
cat "$TMPDIR_CASE/disabled-when.txt"
test "$code" -eq 1
grep -F 'ui node save: disabledWhen: not pined names pined, which is not a variable or a guard of the flow [load]' "$TMPDIR_CASE/disabled-when.txt"
