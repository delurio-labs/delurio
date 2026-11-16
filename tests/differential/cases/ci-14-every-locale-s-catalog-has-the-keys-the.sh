# from the reference CI step: Every locale's catalog has the keys the screens use
cp -r examples/notes "$TMPDIR_CASE/locales"
sed -i 's/^locales: \[en\]$/locales: [en, tr, de]/' "$TMPDIR_CASE/locales/edit/ui.yaml"
node -e '
  const fs = require("fs");
  const dir = process.argv[1];
  const words = JSON.parse(fs.readFileSync(`${dir}/en.json`, "utf8"));
  delete words["Edit.save"];
  fs.writeFileSync(`${dir}/tr.json`, JSON.stringify(words, null, 2));
' "$TMPDIR_CASE/locales/edit/catalog"
code=0
"$DELURIO" check "$TMPDIR_CASE/locales" --strict > "$TMPDIR_CASE/locales.txt" || code=$?
cat "$TMPDIR_CASE/locales.txt"
test "$code" -eq 2
grep -F 'error: catalog key Edit.save is missing from the tr catalog, catalog/tr.json [catalog-key-missing]' "$TMPDIR_CASE/locales.txt"
grep -F 'error: the de catalog, catalog/de.json, does not exist' "$TMPDIR_CASE/locales.txt" | grep -F '[catalog-key-missing]'
test "$(grep -c '\[catalog-key-missing\]' "$TMPDIR_CASE/locales.txt")" -eq 2
