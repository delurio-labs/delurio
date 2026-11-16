# from the reference CI step: A GET that a writing policy lists writes only when it does more than record its call
cp -r examples/notes "$TMPDIR_CASE/audited"
sed -i 's/read: \[Note\], do: \[readNote\]/read: [Note], write: [Note], do: [readNote]/; s/^    serverFn: GET$/    serverFn: GET\n    sideEffects: [audit:note.read]/' "$TMPDIR_CASE/audited/domain/note.yaml"
sed -i '/^changes:/d' "$TMPDIR_CASE/audited/restore/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/audited" --strict > "$TMPDIR_CASE/audited.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/audited.txt"
test "$code" -eq 2
grep -F 'error: its steps write (restoreNote), but ui.yaml has no changes note' "$TMPDIR_CASE/audited.txt" | grep -F '[changes-note-missing]'

sed -i 's/^    sideEffects: \[audit:note.read\]$/    sideEffects: [audit:note.read, the owner is told the note was opened]/' "$TMPDIR_CASE/audited/domain/note.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/audited" --strict > "$TMPDIR_CASE/audited.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/audited.txt"
test "$code" -eq 2
grep -F 'error: its steps write (readNote, restoreNote), but ui.yaml has no changes note' "$TMPDIR_CASE/audited.txt"

sed -i 's/^    sideEffects: \[audit:note.read, the owner is told the note was opened\]$/    sideEffects: [audit:note.read]/' "$TMPDIR_CASE/audited/domain/note.yaml"
sed -i 's/invoke: { action: restoreNote }/invoke: { external: notes-archive }/' "$TMPDIR_CASE/audited/restore/flow.yaml"
"$DELURIO" check "$TMPDIR_CASE/audited" --strict > "$TMPDIR_CASE/audited.txt" 2>&1
cat "$TMPDIR_CASE/audited.txt"
! grep -F '[changes-note-missing]' "$TMPDIR_CASE/audited.txt"
