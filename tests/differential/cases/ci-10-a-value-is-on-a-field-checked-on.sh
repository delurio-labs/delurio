# from the reference CI step: A value is on a field, checked on a checkable role and script (optional or actions) on a page; checked reads the context
cp -r examples/notes "$TMPDIR_CASE/controls"
sed -i 's/^    emits: SAVE$/&\n    value: { literal: "" }\n    checked: true/; s/^    text: { key: Edit.title }$/&\n    script: optional/; s/value: { key: Edit.bodyValue,/value: { key: Edit.bodyText,/' "$TMPDIR_CASE/controls/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/controls" --strict > "$TMPDIR_CASE/controls.txt" || code=$?
cat "$TMPDIR_CASE/controls.txt"
test "$code" -eq 2
grep -F 'nodes.save.value: is not allowed here (value is only on a field' "$TMPDIR_CASE/controls.txt"
grep -F 'nodes.save.checked: is not allowed here (checked is only on a node whose role is checkbox' "$TMPDIR_CASE/controls.txt"
grep -F 'nodes.title.script: is not allowed here (initialFocus, frame and script are only on a page)' "$TMPDIR_CASE/controls.txt"
grep -F 'catalog key Edit.bodyText is missing [catalog-key-missing]' "$TMPDIR_CASE/controls.txt"

cp -r examples/notes "$TMPDIR_CASE/script"
sed -i 's/^    script: optional # .*$/    script: readable/' "$TMPDIR_CASE/script/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/script" --strict > "$TMPDIR_CASE/script.txt" || code=$?
cat "$TMPDIR_CASE/script.txt"
test "$code" -eq 2
grep -F 'nodes.page.script: must be "optional" or "actions" [schema]' "$TMPDIR_CASE/script.txt"
! grep -F 'restore/ui.yaml' "$TMPDIR_CASE/script.txt"

cp -r examples/notes "$TMPDIR_CASE/checked"
sed -i 's/^    checked: pinned$/    checked: "pined == true"/' "$TMPDIR_CASE/checked/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/checked" --strict > "$TMPDIR_CASE/checked.txt" || code=$?
cat "$TMPDIR_CASE/checked.txt"
test "$code" -eq 1
grep -F 'ui node pinBox: checked: pined == true names pined, which is not a variable or a guard of the flow [load]' "$TMPDIR_CASE/checked.txt"
