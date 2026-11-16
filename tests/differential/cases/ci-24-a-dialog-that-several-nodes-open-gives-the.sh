# from the reference CI step: A dialog that several nodes open gives the focus back to the one pressed (a list, or opener)
for target in opener '[cancel, save]'; do
  rm -rf "$TMPDIR_CASE/openers"
  cp -r examples/notes "$TMPDIR_CASE/openers"
  sed -i "s/returnsFocus: \[discardDialog, cancel\]/returnsFocus: [discardDialog, $target]/" "$TMPDIR_CASE/openers/edit/ui.yaml"
  "$DELURIO" check "$TMPDIR_CASE/openers" --strict
done

sed -i 's/returnsFocus: \[discardDialog, \[cancel, save\]\]/returnsFocus: [discardDialog, [cancel, keep]]/' "$TMPDIR_CASE/openers/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/openers" --strict > "$TMPDIR_CASE/openers.txt" || code=$?
cat "$TMPDIR_CASE/openers.txt"
test "$code" -eq 2
grep -F 'returnsFocus gives the focus of discardDialog to keep, but no state discardDialog closes into shows it (editing and left)' "$TMPDIR_CASE/openers.txt" | grep -F '[focus-not-shown]'

sed -i 's/returnsFocus: \[discardDialog, \[cancel, keep\]\]/returnsFocus: [discardDialog, opener]/; /^    emits: CANCEL$/d' "$TMPDIR_CASE/openers/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/openers" --strict > "$TMPDIR_CASE/openers.txt" || code=$?
cat "$TMPDIR_CASE/openers.txt"
test "$code" -eq 2
grep -F 'returnsFocus gives the focus of discardDialog back to the node that opened it, but no node opens it' "$TMPDIR_CASE/openers.txt" | grep -F '[focus-not-shown]'

sed -i 's/^  working:$/  opener:/' "$TMPDIR_CASE/openers/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/openers" --strict > "$TMPDIR_CASE/openers.txt" || code=$?
cat "$TMPDIR_CASE/openers.txt"
grep -F 'the name "opener" is not allowed' "$TMPDIR_CASE/openers.txt" | grep -F '[schema]'
