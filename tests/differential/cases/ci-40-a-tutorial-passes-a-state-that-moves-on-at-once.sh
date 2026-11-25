# A tutorial passes a state that moves on at once (always) as it passes a server step, whatever the state shows; a cut
# may name its situation by the values it has; and a {name} in a flow's own words that a tutorial planned from the graph
# says (its title, a state's title, a transition's say), that neither a sample value nor a node gives, is a warning
# (prose-placeholder) that fails nothing: the tutorial says it as the bare name (SPEC.md §7)
cp -r examples/notes "$TMPDIR_CASE/prose"
flow="$TMPDIR_CASE/prose/edit/flow.yaml"
ui="$TMPDIR_CASE/prose/edit/ui.yaml"
t="$TMPDIR_CASE/prose/edit/tutorial.yaml"

# the text is checked once Save is pressed, in a state that moves on at once and still draws Save: the tutorial's press
# passes it on its way to saving, as the video does
python3 - "$flow" "$ui" <<'EOF'
import sys
def edit(p, old, new):
    s = open(p).read()
    assert s.count(old) == 1, old
    open(p, "w").write(s.replace(old, new))
flow, ui = sys.argv[1], sys.argv[2]
edit(flow, "      SAVE: saving\n", "      SAVE: checking\n")
edit(flow, "  saving:\n    title: Save it\n", "  checking: { title: Check the text, always: [ { target: saving } ] }\n  saving:\n    title: Save it\n")
edit(ui, "    in: [editing, saving]\n    disabledIn: [saving]\n    emits: SAVE\n", "    in: [editing, checking, saving]\n    disabledIn: [saving]\n    emits: SAVE\n")
EOF
"$DELURIO" check "$TMPDIR_CASE/prose" --strict --no-app > "$TMPDIR_CASE/prose.txt" 2>&1
cat "$TMPDIR_CASE/prose.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/prose.txt"

# a cut that names its situation by the values it has, and one whose value is not one
sed -i 's/^    cut: discarding$/    cut: { state: discarding, given: { pinned: true } }/' "$t"
"$DELURIO" check "$TMPDIR_CASE/prose" --strict --no-app > "$TMPDIR_CASE/prose.txt" 2>&1
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/prose.txt"
sed -i 's/given: { pinned: true }/given: { pinned: maybe }/' "$t"
code=0
"$DELURIO" check "$TMPDIR_CASE/prose" --strict --no-app > "$TMPDIR_CASE/prose.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/prose.txt"
test "$code" -eq 2
grep -F "step 7: cut's given: \"maybe\" is not a value of pinned (false, true) [tutorial-mismatch]" "$TMPDIR_CASE/prose.txt"
sed -i 's/given: { pinned: maybe }/given: { pinned: true }/' "$t"

# the title of saving says {noteId}, which nothing gives: a warning at the title, until a sample gives it
sed -i 's/^    title: Save it$/    title: Save the note {noteId}/' "$flow"
"$DELURIO" check "$TMPDIR_CASE/prose" --strict --no-app > "$TMPDIR_CASE/prose.txt" 2>&1
cat "$TMPDIR_CASE/prose.txt"
line=$(grep -n '^    title: Save the note {noteId}$' "$flow" | cut -d: -f1)
grep -F "edit/flow.yaml:$line:5: warning: the title of saving says {noteId}, which is neither a sample value nor a node of ui.yaml: a tutorial planned from the graph says it as the word noteId; give it a sample value, such as samples: { noteId: ... } (SPEC.md §7) [prose-placeholder]" "$TMPDIR_CASE/prose.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 1 warning, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/prose.txt"
sed -i 's/^samples: { body: "Milk, eggs, bread" }$/samples: { body: "Milk, eggs, bread", noteId: "42" }/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/prose" --strict --no-app > "$TMPDIR_CASE/prose.txt" 2>&1
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/prose.txt"
