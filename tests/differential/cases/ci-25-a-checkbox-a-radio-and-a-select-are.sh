# from the reference CI step: A checkbox, a radio and a select are components of their own, named by their label
cp -r examples/notes "$TMPDIR_CASE/controls-drawn"
python3 - "$TMPDIR_CASE/controls-drawn/edit" <<'PY'
import json, sys
d = sys.argv[1]
ui = open(f"{d}/ui.yaml").read()
ui = ui.replace("  pinBox:\n    type: field\n    component: ListItem\n", "  pinBox:\n    type: field\n    component: Checkbox\n")
ui = ui.replace("  save:\n    type: action\n", "  colour:\n    type: field\n    component: Select\n    role: combobox\n    text: { key: Edit.colour }\n    value: { key: Edit.yellow }\n    in: [editing, saving]\n  late:\n    type: field\n    component: Radio\n    role: radio\n    text: { key: Edit.later }\n    checked: true\n    in: [editing]\n  save:\n    type: action\n")
open(f"{d}/ui.yaml", "w").write(ui)
words = json.load(open(f"{d}/catalog/en.json"))
words.update({"Edit.colour": "Colour", "Edit.yellow": "Yellow", "Edit.later": "Remind me later"})
json.dump(words, open(f"{d}/catalog/en.json", "w"), indent=2)
PY
"$DELURIO" check "$TMPDIR_CASE/controls-drawn" --strict
"$DELURIO" page "$TMPDIR_CASE/controls-drawn/edit" --out "$TMPDIR_CASE/controls-drawn-page" --kit none --strict
page="$TMPDIR_CASE/controls-drawn-page/index.html"
grep -F '<input type="checkbox" tabindex="-1"><span>Pin it to the top of my notes</span>' "$page"
grep -F '<input type="radio" tabindex="-1" checked><span>Remind me later</span>' "$page"
grep -F '<select class="wf-input" tabindex="-1"><option>Yellow</option></select>' "$page"
grep -F 'checkbox &quot;Pin it to the top of my notes&quot;' "$page"
grep -F 'combobox &quot;Colour&quot;' "$page"
