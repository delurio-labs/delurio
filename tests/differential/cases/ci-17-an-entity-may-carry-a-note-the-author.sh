# from the reference CI step: An entity may carry a note, the author's own words
cp -r examples/notes "$TMPDIR_CASE/entity-note"
sed -i 's/^  Note:$/&\n    note: a note is kept until its owner erases it; archived notes are erased in the second phase/' "$TMPDIR_CASE/entity-note/domain/note.yaml"
sed -i 's/^    fields:$/    note: [not words]\n&/' "$TMPDIR_CASE/entity-note/domain/domain.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/entity-note" --strict > "$TMPDIR_CASE/entity-note.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/entity-note.txt"
test "$code" -eq 2
test "$(grep -c '\[schema\]' "$TMPDIR_CASE/entity-note.txt")" -eq 1
grep -F 'domain/domain.yaml' "$TMPDIR_CASE/entity-note.txt" | grep -F 'entities.Note.note: must be a string [schema]'
sed -i '/^    note: \[not words\]$/d' "$TMPDIR_CASE/entity-note/domain/domain.yaml"
"$DELURIO" check "$TMPDIR_CASE/entity-note" --strict 2>&1 | grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0'
