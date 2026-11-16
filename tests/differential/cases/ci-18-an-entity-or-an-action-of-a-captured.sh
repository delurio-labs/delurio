# from the reference CI step: An entity or an action of a captured domain may be planned, and a planned one names no code
cp -r examples/notes "$TMPDIR_CASE/planned"
d="$TMPDIR_CASE/planned/domain"
sed -i 's/^status: proposed$/status: captured/' "$d/domain.yaml"
printf '%s\n' 'schema: delurio-graph/0' 'kind: domain' 'id: example.notes' 'entities:' '  Reminder:' '    planned: the second phase builds reminders' '    fields: { id: { type: string }, noteId: { type: string, references: Note } }' \
  'actions:' '  remindAbout:' '    route: POST /notes/{id}/reminders' '    planned: the second phase builds reminders' '    result: { returns: Reminder }' > "$d/planned.yaml"
"$DELURIO" check "$TMPDIR_CASE/planned" --strict 2>&1 | grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0'
sed -i 's|^    planned: the second phase builds reminders$|&\n    source: examples/notes/app/notes.js:12|' "$d/planned.yaml"
sed -i 's/^    route: POST \/notes\/{id}\/archive$/&\n    planned: ""/' "$d/note.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/planned" --strict > "$TMPDIR_CASE/planned.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/planned.txt"
test "$code" -eq 2
grep -F 'domain/planned.yaml:6:5: error: the entity Reminder says it is planned (the second phase builds reminders), and names its code, examples/notes/app/notes.js:12' "$TMPDIR_CASE/planned.txt" | grep -F '[planned-built]'
grep -F 'domain/planned.yaml:12:5: error: the action remindAbout says it is planned' "$TMPDIR_CASE/planned.txt" | grep -F '[planned-built]'
grep -F 'domain/planned.yaml:13:5: note: its source is given here' "$TMPDIR_CASE/planned.txt"
grep -F 'actions.archiveNote.planned: must NOT have fewer than 1 characters' "$TMPDIR_CASE/planned.txt" | grep -F '[schema]'
