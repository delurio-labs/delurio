# from the reference CI step: A change touches the graphs whose files, anchors, domain actions, bindings or kit it changes, each with why, and anything unknown touches every graph (SPEC.md §9)
r="$TMPDIR_CASE/affected"
rm -rf "$r"
mkdir -p "$r/graphs"
cp -r examples/notes/app "$r/app"
for d in archive domain edit kit restore; do cp -r "examples/notes/$d" "$r/graphs/$d"; done
sed -i 's|^    title: Restore it$|&\n    source: app/notes.js:30|' "$r/graphs/restore/flow.yaml"
g() { git -C "$r" -c user.name=t -c user.email=t@example.test -c commit.gpgsign=false "$@"; }
g init -q -b main
g add -A
g commit -q -m 'the notes'

# a line an anchor names, in the working tree: its flow, line-precise; a line no anchor names: nothing
sed -i '30s|.*|  // changed|' "$r/app/notes.js"
(cd "$r" && "$DELURIO" affected --base main) > "$TMPDIR_CASE/affected.txt"
cat "$TMPDIR_CASE/affected.txt"
grep -Eqx '1 graph touched since [0-9a-f]{12} by 1 changed file; 2 not touched' "$TMPDIR_CASE/affected.txt"
grep -Fx '  graphs/restore (example.restoreNote)' "$TMPDIR_CASE/affected.txt"
grep -Fx '    - line 30 of app/notes.js changed: the anchor of states.restoring in flow.yaml [line-precise]' "$TMPDIR_CASE/affected.txt"
g checkout -q -- app/notes.js
sed -i '10s|.*|  // changed|' "$r/app/notes.js"
"$DELURIO" affected --base main --repo "$r" > "$TMPDIR_CASE/affected.txt"
cat "$TMPDIR_CASE/affected.txt"
grep -Eqx '0 graphs touched since [0-9a-f]{12} by 1 changed file; 3 not touched' "$TMPDIR_CASE/affected.txt"
grep -Fx 'touching no graph: app/notes.js' "$TMPDIR_CASE/affected.txt"
g checkout -q -- app/notes.js

# a commit that changes the shared kit and an action of the domain: the flows drawn with the kit, the flow invoking it
g checkout -q -b change
sed -i 's|"#ffffff"|"#fefefe"|' "$r/graphs/kit/tokens.json"
sed -i '/^  editNote:/,/^invariants:/ s|^      NOT_FOUND: { status: 404 }$|&\n      TOO_LONG: { status: 413 }|' "$r/graphs/domain/note.yaml"
g commit -q -am 'a change'
"$DELURIO" affected --base main --head HEAD --repo "$r" > "$TMPDIR_CASE/affected.txt"
cat "$TMPDIR_CASE/affected.txt"
grep -Eqx '2 graphs touched since [0-9a-f]{12} by 2 changed files; 1 not touched' "$TMPDIR_CASE/affected.txt"
grep -Fx '  graphs/edit (example.editNote), not bound' "$TMPDIR_CASE/affected.txt"
grep -Fx '    - graphs/domain/note.yaml changes the action editNote, which it invokes' "$TMPDIR_CASE/affected.txt"
test "$(grep -cFx '    - graphs/kit/tokens.json is changed: its kit' "$TMPDIR_CASE/affected.txt")" -eq 2
"$DELURIO" affected --base main --head HEAD --repo "$r" --json > "$TMPDIR_CASE/affected.json"
node -e '
const a = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
const want = { schema: "delurio-affected/0", all: null, untouched: 1, changed: 2, unmapped: [] };
for (const [k, v] of Object.entries(want)) if (JSON.stringify(a[k]) !== JSON.stringify(v)) throw new Error(`${k}: ${JSON.stringify(a[k])}`);
if (a.graphs.map((g) => g.id).join() !== "example.editNote,example.restoreNote") throw new Error(a.graphs.map((g) => g.id).join());
if (a.graphs[1].bindings.join() !== "graphs/restore/binding.yaml") throw new Error(a.graphs[1].bindings.join());
const r = a.graphs[1].reasons[0];
if (r.rule !== "shared" || r.file !== "graphs/kit/tokens.json" || r.precision !== undefined) throw new Error(JSON.stringify(r));
' "$TMPDIR_CASE/affected.json"

# what cannot be known touches every graph, and says why
"$DELURIO" affected --base no-such-branch --repo "$r" > "$TMPDIR_CASE/affected.txt"
cat "$TMPDIR_CASE/affected.txt"
grep -Fx "every graph (3): the change cannot be read: no-such-branch is not a commit of $(git -C "$r" rev-parse --show-toplevel)" "$TMPDIR_CASE/affected.txt"
printf 'a: 1\n' > "$r/graphs/stray.yaml"
"$DELURIO" affected --base main --repo "$r" > "$TMPDIR_CASE/affected.txt"
cat "$TMPDIR_CASE/affected.txt"
grep -Fx 'every graph (3) since '"$(git -C "$r" rev-parse main | cut -c1-12)"': graphs/stray.yaml is in the graphs folder and belongs to no graph' "$TMPDIR_CASE/affected.txt"
"$DELURIO" affected --base main --head HEAD --repo "$r" --all > "$TMPDIR_CASE/affected.txt"
grep -F ': every graph was asked for (--all)' "$TMPDIR_CASE/affected.txt"

# no base is a usage error
code=0
"$DELURIO" affected --repo "$r" 2> "$TMPDIR_CASE/affected.txt" || code=$?
test "$code" -eq 1
grep -F 'delurio affected --base <ref>' "$TMPDIR_CASE/affected.txt"
