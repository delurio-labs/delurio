# from the reference CI step: A domain's names are held to what it has, an entity, a field or a policy's names as errors
cp -r examples/qauth-authorize "$TMPDIR_CASE/names"
sed -i '0,/references: Client }/s//references: Clinet }/; s/result: { returns: AuthorizationCode }/result: { returns: AuthCode }/; s/required: \[codeChallenge, codeChallengeMethod\]/required: [codeChallenge, challengeMethod]/; s/csrf: { type: string } }/csrf: { type: string, references: Session } }/; s/    do: \[decide\]/    do: [decide, revoke]/; s/    write: \[Consent\]/    write: [Consent, Grant]/' "$TMPDIR_CASE/names/domain.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/names" --strict > "$TMPDIR_CASE/names.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/names.txt"
test "$code" -eq 2
grep -F 'names/domain.yaml:76:15: error: the action issue returns AuthCode, which is not an entity of the domain qauth.authorize' "$TMPDIR_CASE/names.txt" | grep -F '[entity-unknown]'
grep -F 'names/domain.yaml:19:33: error: the field AuthorizationRequest.clientId references Clinet, which is not an entity' "$TMPDIR_CASE/names.txt" | grep -F '[entity-unknown]'
grep -F 'names/domain.yaml:69:126: error: the body of the action decide, at csrf, references Session, which is not an entity' "$TMPDIR_CASE/names.txt" | grep -F '[entity-unknown]'
grep -F 'names/domain.yaml:17:31: error: the entity AuthorizationRequest requires challengeMethod, which is not one of its fields' "$TMPDIR_CASE/names.txt" | grep -F '[field-unknown]'
grep -F 'names/domain.yaml:50:18: error: the policy for end-user names revoke in do, which is not an action of the domain qauth.authorize' "$TMPDIR_CASE/names.txt" | grep -F '[policy-unknown]'
grep -F 'names/domain.yaml:49:22: error: the policy for end-user names Grant in write, which is not an entity of the domain qauth.authorize' "$TMPDIR_CASE/names.txt" | grep -F '[policy-unknown]'
grep -F 'delurio check: 1 graph, 6 errors, 0 warnings; exit 2' "$TMPDIR_CASE/names.txt"
"$DELURIO" check "$TMPDIR_CASE/names" --format sarif --output "$TMPDIR_CASE/names.sarif"
node -e '
  const run = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).runs[0];
  const level = Object.fromEntries(run.tool.driver.rules.map((r) => [r.id, r.defaultConfiguration.level]));
  const want = { "entity-unknown": "error", "field-unknown": "error", "policy-unknown": "error" };
  for (const [id, l] of Object.entries(want)) if (level[id] !== l) throw new Error(`${id} is ${level[id]}, not ${l}`);
  const where = run.results.map((r) => `${r.ruleId} ${r.level} ${r.locations[0].physicalLocation.region.startLine} ${r.locations[0].logicalLocations[0].fullyQualifiedName}`);
  for (const w of ["entity-unknown error 76 qauth.authorize/actions/issue/result/returns", "policy-unknown error 50 qauth.authorize/policies/end-user/do/revoke"]) if (!where.includes(w)) throw new Error(`no ${w} in ${where.join("; ")}`);
' "$TMPDIR_CASE/names.sarif"

cp -r examples/notes "$TMPDIR_CASE/split-names"
sed -i 's/^  Note:$/  Note:\n    key: [id]/' "$TMPDIR_CASE/split-names/domain/note.yaml"
sed -i '0,/properties: { id: { type: string, format: uuid } }/s//properties: { id: { type: string, format: uuid, references: Note } }/' "$TMPDIR_CASE/split-names/domain/note.yaml"
"$DELURIO" check "$TMPDIR_CASE/split-names" --strict > "$TMPDIR_CASE/split-names.txt" 2>&1
cat "$TMPDIR_CASE/split-names.txt"
grep -F 'delurio check: 3 graphs, 0 errors, 0 warnings' "$TMPDIR_CASE/split-names.txt"
sed -i 's/^    key: \[id\]$/    key: [noteId]/; s/do: \[editNote\]/do: [editNote, pinNote]/' "$TMPDIR_CASE/split-names/domain/note.yaml"
sed -i 's/required: \[id, ownerId, body\]/required: [id, ownerId, body, title]/' "$TMPDIR_CASE/split-names/domain/domain.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/split-names" --strict > "$TMPDIR_CASE/split-names.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/split-names.txt"
test "$code" -eq 2
grep -F 'split-names/domain/domain.yaml:10:35: error: the entity Note requires title, which is not one of its fields' "$TMPDIR_CASE/split-names.txt" | grep -F '[field-unknown]'
grep -F 'split-names/domain/note.yaml:10:11: error: the key of the entity Note names noteId, which is not one of its fields' "$TMPDIR_CASE/split-names.txt" | grep -F '[field-unknown]'
grep -F 'split-names/domain/note.yaml:17:100: error: the policy for person names pinNote in do' "$TMPDIR_CASE/split-names.txt" | grep -F '[policy-unknown]'

mkdir -p "$TMPDIR_CASE/two/graphs/people" "$TMPDIR_CASE/two/graphs/notes"
printf 'schema: delurio-graph/0\nkind: domain\nid: example.people\nentities:\n  Person:\n    fields:\n      id: { type: string }\n' > "$TMPDIR_CASE/two/graphs/people/domain.yaml"
printf 'schema: delurio-graph/0\nkind: domain\nid: example.notes\nentities:\n  Note:\n    fields:\n      id: { type: string }\n      ownerId: { type: string, references: Person }\n' > "$TMPDIR_CASE/two/graphs/notes/domain.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/two" --strict > "$TMPDIR_CASE/two.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/two.txt"
test "$code" -eq 2
grep -F 'notes/domain.yaml:8:32: error: the field Note.ownerId references Person, which is not an entity of the domain example.notes' "$TMPDIR_CASE/two.txt" | grep -F '[entity-unknown]'
grep -F 'people/domain.yaml:5:3: note: Person is an entity of the domain example.people' "$TMPDIR_CASE/two.txt"
