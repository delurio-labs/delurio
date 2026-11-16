# from the reference CI step: The example kit draws the notes screens, and a kit that is not data is refused
page="$TMPDIR_CASE/pages/notes/edit/index.html"
grep -F '<div class="wf kit"' "$page"
grep -F '<button type="button" class="k-button k-primary">Save</button>' "$page"
grep -F '<li class="k-item" data-node="pinBox"><label class="k-check"><input type="checkbox"' "$page"
grep -F -- '--color-accent: #2a54d5;' "$page"

cp -r examples/notes "$TMPDIR_CASE/kit-script"
sed -i "s|    template: '<p class=\"k-text\">{text}</p>'|    template: '<p class=\"k-text\" onclick=\"go()\">{text}</p>'|" "$TMPDIR_CASE/kit-script/kit/kit.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/kit-script" --strict > "$TMPDIR_CASE/kit-script.txt" || code=$?
cat "$TMPDIR_CASE/kit-script.txt"
test "$code" -eq 1
grep -F 'event handlers are not allowed: a kit is data, never code [load]' "$TMPDIR_CASE/kit-script.txt"
