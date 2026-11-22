# A node's tone (SPEC.md §4): how it looks in a state, one word or the first of a map whose expression holds there. The
# fixed components draw it as wf-tone-<word>, and a kit with its {tone} slot and its tones (SPEC.md §8); the page shows
# a map in the node's chips, as it shows when. A kit carries its fonts as files of its folder, embedded once in each
# page as data: URLs: one that cannot be embedded is an error (kit-font), and a family its CSS draws in first that it
# does not carry is a warning (kit-font-unembedded), each once however many flows draw with the kit
cp -r examples/notes "$TMPDIR_CASE/tones"
kit="$TMPDIR_CASE/tones/kit"
ui="$TMPDIR_CASE/tones/edit/ui.yaml"
python3 - "$ui" "$kit/kit.yaml" <<'PY'
import sys
ui, kit = sys.argv[1], sys.argv[2]
s = open(ui).read()
s = s.replace("    text: { key: Edit.saved }\n", "    text: { key: Edit.saved }\n    tone: success\n")
s = s.replace("    text: { key: Discard.discard }\n", "    text: { key: Discard.discard }\n    tone: danger\n")
s = s.replace("    text: { key: Edit.title }\n", "    text: { key: Edit.title }\n    tone: { warning: \"not pinned\" }\n")
open(ui, "w").write(s)
k = open(kit).read()
k = k.replace("""    template: '<p class="k-status" role="status">{text}</p>'""", """    template: '<p class="k-status k-{tone}" role="status">{text}</p>'""")
k = k.replace("components:\n", "tones: { success: done, default: plain }\nfonts:\n  - { family: Delurio Test, file: fonts/test.ttf, weight: 400 }\ncomponents:\n")
open(kit, "w").write(k)
PY
mkdir -p "$kit/fonts"
# a TrueType font of 820 bytes, two glyphs (.notdef and a square for A), made for these tests
echo 'AAEAAAAKAIAAAwAgT1MvMlOLYDEAAACsAAAAYmNtYXAADABzAAABEAAAACxnbHlmA1oFUAAAATwAAAAiaGVhZGESQz0AAAFgAAAANmhoZWEFFgGTAAABmAAAACRobXR4BLAAZAAAAbwAAAAIbG9jYQARAAAAAAHEAAAABm1heHAABAAGAAABzAAAACBuYW1lGLAqJAAAAewAAAEocG9zdP+fADIAAAMUAAAAIAAEAfQBkAAFAAACigK8AAAAjAKKArwAAAHgADIA+gAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAERMUk8AQABBAEEDIP84AAADIADIAAAAAQAAAAAB9AJYAAAAIAAAAAAAAAABAAMAAQAAAAwABAAgAAAABAACAAAAAgBB//8AAABB////wAABAAAAAAABAGQAAAH0AlgAAwAAAQEBAQBkAAABkAAAAAACWAAA/agAAAABAAAAAQAA8TzvZ18PPPUACwPoAAAAAAAAAAAAAAAAAAAAAAAAAAAB9AJYAAAACAACAAAAAAAAAAEAAAMg/zgAAAJYAAAAAAH0AAEAAAAAAAAAAAAAAAAAAAACAlgAAAJYAGQAAAAAABEAAAABAAAAAgAEAAEAAAAAAAIAAAAAAAAAAAAAAAAAAAAAAAAABQBCAAMAAQQJAAAAcgAAAAMAAQQJAAEAGAByAAMAAQQJAAIADgCKAAMAAQQJAAQAKACYAAMAAQQJAAYAJgDAAE0AYQBkAGUAIABmAG8AcgAgAGQAZQBsAHUAcgBpAG8AJwBzACAAdABlAHMAdABzADsAIABmAHIAZQBlACAAdABvACAAdQBzAGUALAAgAG4AbwAgAHIAaQBnAGgAdABzACAAcgBlAHMAZQByAHYAZQBkAEQAZQBsAHUAcgBpAG8AIABUAGUAcwB0AFIAZQBnAHUAbABhAHIARABlAGwAdQByAGkAbwAgAFQAZQBzAHQAIABSAGUAZwB1AGwAYQByAEQAZQBsAHUAcgBpAG8AVABlAHMAdAAtAFIAZQBnAHUAbABhAHIAAwAAAAAAAP+cADIAAAAAAAAAAAAAAAAAAAAAAAAAAA==' | base64 -d > "$kit/fonts/test.ttf"
"$DELURIO" check "$TMPDIR_CASE/tones" --strict --no-app > "$TMPDIR_CASE/tones.txt" 2>&1
cat "$TMPDIR_CASE/tones.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/tones.txt"

# the page: the kit draws the tone with its template, the font is embedded once, and the map is in the chips
"$DELURIO" page "$TMPDIR_CASE/tones" --out "$TMPDIR_CASE/tones-pages" --strict
page="$TMPDIR_CASE/tones-pages/edit/index.html"
test "$(grep -o '@font-face' "$page" | wc -l)" -eq 1
grep -F '@font-face { font-family: "Delurio Test"; src: url("data:font/ttf;base64,AAEAAAAKAIAAAwAg' "$page"
grep -F 'format("truetype"); font-weight: 400; font-display: block; }' "$page"
grep -F '<p class="k-status k-done" role="status">The note is saved</p><span class="chip">status · tone success</span>' "$page"
grep -F '<p class="k-status k-plain" role="status">Saving the note</p>' "$page"
grep -F '<span class="chip">heading · first focus · tone warning when not pinned</span>' "$page"
if grep -F 'class="wf-item wf-tone-' "$page"; then exit 1; fi

# the fixed components draw it as wf-tone-<word>, with the styles of a tone; the danger is red
"$DELURIO" page "$TMPDIR_CASE/tones" --kit none --out "$TMPDIR_CASE/tones-fixed" --strict
fixed="$TMPDIR_CASE/tones-fixed/edit/index.html"
grep -F '<div class="wf-item wf-tone-success" data-node="savedStatus"><div class="wf-status" role="status">The note is saved</div>' "$fixed"
grep -F '<div class="wf-item wf-tone-danger" data-node="discard"><button type="button" class="wf-btn wf-confirm">' "$fixed"
grep -F '.wf-tone-danger { --wf-tone: var(--bad, #b91c1c); }' "$fixed"
if grep -F '@font-face' "$fixed"; then exit 1; fi

# the examples, with no tone and no font, carry neither
"$DELURIO" page examples --out "$TMPDIR_CASE/tones-plain" > /dev/null
if grep -rlF -e 'wf-tone-' -e '@font-face' "$TMPDIR_CASE/tones-plain"; then exit 1; fi

# a tone the flow cannot read keeps the graph from loading; a word that is no tone is the schema's
cp -r "$TMPDIR_CASE/tones" "$TMPDIR_CASE/tones-bad"
sed -i 's/^    tone: success$/    tone: { danger: "late ==" }/' "$TMPDIR_CASE/tones-bad/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/tones-bad" --strict --no-app > "$TMPDIR_CASE/tones-bad.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/tones-bad.txt"
test "$code" -eq 1
grep -F 'error: ui node savedStatus: tone danger: unexpected end in: late == [load]' "$TMPDIR_CASE/tones-bad.txt"
sed -i 's/^    tone: { danger: "late ==" }$/    tone: critical/' "$TMPDIR_CASE/tones-bad/edit/ui.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/tones-bad" --strict --no-app > "$TMPDIR_CASE/tones-bad.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/tones-bad.txt"
test "$code" -eq 2
grep -F '[schema]' "$TMPDIR_CASE/tones-bad.txt" | grep -F 'tone'

# a font file outside the kit's folder is not read: an error once, though three flows draw with the kit, and the
# page is drawn without it
echo 'wOF2' > "$TMPDIR_CASE/tones/outside.woff2"
sed -i 's|file: fonts/test.ttf|file: ../outside.woff2|' "$kit/kit.yaml"
line=$(grep -n 'file: ../outside.woff2' "$kit/kit.yaml" | cut -d: -f1)
code=0
"$DELURIO" check "$TMPDIR_CASE/tones" --strict --no-app > "$TMPDIR_CASE/tones.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/tones.txt"
test "$code" -eq 2
test "$(grep -c '\[kit-font\]' "$TMPDIR_CASE/tones.txt")" -eq 1
grep -F "kit/kit.yaml:$line:" "$TMPDIR_CASE/tones.txt" | grep -F "error: kit example: the font file ../outside.woff2 climbs out of the kit's folder (..): a kit's font is a file in the kit's folder; Delurio Test is not embedded" | grep -F '[kit-font]'
grep -Fx 'delurio check: 3 graphs, 1 error, 0 warnings, 1 gap of 4 needs; exit 2' "$TMPDIR_CASE/tones.txt"
code=0
"$DELURIO" page "$TMPDIR_CASE/tones" --out "$TMPDIR_CASE/tones-pages" --strict > "$TMPDIR_CASE/tones-page.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/tones-page.txt"
test "$code" -eq 2
grep -F "warning: kit example: the font file ../outside.woff2 climbs out" "$TMPDIR_CASE/tones-page.txt"
if grep -F '@font-face' "$page"; then exit 1; fi
# nor one that is no font by its bytes
printf 'not a font' > "$kit/fonts/fake.woff2"
sed -i 's|file: ../outside.woff2|file: fonts/fake.woff2|' "$kit/kit.yaml"
"$DELURIO" check "$TMPDIR_CASE/tones" --strict --no-app > "$TMPDIR_CASE/tones.txt" 2>&1 || true
grep -F 'the font file fonts/fake.woff2 is no woff2 file by its bytes' "$TMPDIR_CASE/tones.txt" | grep -F '[kit-font]'

# a family the kit's CSS draws in first, through its tokens, that the kit does not carry: a warning, once, where the
# tokens write it; carried, it is none
sed -i 's|file: fonts/fake.woff2|file: fonts/test.ttf|' "$kit/kit.yaml"
sed -i 's/\["system-ui", "Segoe UI", "Roboto", "sans-serif"\]/["Brand Sans", "Segoe UI", "sans-serif"]/' "$kit/tokens.json"
"$DELURIO" check "$TMPDIR_CASE/tones" --strict --no-app > "$TMPDIR_CASE/tones.txt" 2>&1
cat "$TMPDIR_CASE/tones.txt"
test "$(grep -c '\[kit-font-unembedded\]' "$TMPDIR_CASE/tones.txt")" -eq 1
grep -F "kit/tokens.json:$(grep -n 'Brand Sans' "$kit/tokens.json" | cut -d: -f1):" "$TMPDIR_CASE/tones.txt" | grep -F 'warning: kit example: its CSS draws in Brand Sans, which the kit does not carry in fonts' | grep -F '[kit-font-unembedded]'
grep -Fx 'delurio check: 3 graphs, 0 errors, 1 warning, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/tones.txt"
sed -i 's|family: Delurio Test, file: fonts/test.ttf|family: Brand Sans, file: fonts/test.ttf|' "$kit/kit.yaml"
"$DELURIO" check "$TMPDIR_CASE/tones" --strict --no-app > "$TMPDIR_CASE/tones.txt" 2>&1
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/tones.txt"
