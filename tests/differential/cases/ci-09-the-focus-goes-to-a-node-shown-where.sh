# from the reference CI step: The focus goes to a node shown where it goes, and a returnsFocus starts at a dialog
cp -r examples/notes "$TMPDIR_CASE/focus"
sed -i 's/^  saved: savedStatus/  saved: working/' "$TMPDIR_CASE/focus/edit/ui.yaml"
sed -i 's/returnsFocus: \[discardDialog, cancel\]/returnsFocus: [discardDialog, keep]/' "$TMPDIR_CASE/focus/edit/ui.yaml"
sed -i 's/^  - { labelledBy: \[page, title\] }$/&\n  - { returnsFocus: [page, title] }/' "$TMPDIR_CASE/focus/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/focus" --strict > "$TMPDIR_CASE/focus.txt" || code=$?
cat "$TMPDIR_CASE/focus.txt"
test "$code" -eq 2
test "$(grep -c '\[focus-not-shown\]' "$TMPDIR_CASE/focus.txt")" -eq 2
grep -F 'focus.saved gives the focus to working, which saved does not show' "$TMPDIR_CASE/focus.txt"
grep -F 'returnsFocus gives the focus of discardDialog to keep, but no state discardDialog closes into shows it (editing and left)' "$TMPDIR_CASE/focus.txt"
grep -F 'returnsFocus starts at page, which is not a dialog' "$TMPDIR_CASE/focus.txt" | grep -F '[returns-focus-not-dialog]'

cp -r examples/notes "$TMPDIR_CASE/focus-state"
sed -i 's/^  saved: savedStatus/  savd: savedStatus/' "$TMPDIR_CASE/focus-state/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/focus-state" --strict > "$TMPDIR_CASE/focus-state.txt" || code=$?
cat "$TMPDIR_CASE/focus-state.txt"
test "$code" -eq 1
grep -F 'ui focus names the unknown state savd [load]' "$TMPDIR_CASE/focus-state.txt"
