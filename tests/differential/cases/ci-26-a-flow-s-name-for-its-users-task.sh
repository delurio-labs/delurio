# from the reference CI step: A flow's name for its users (task) is in every locale's catalog
cp -r examples/notes "$TMPDIR_CASE/task"
sed -i 's/^changes: { key: Changes.note }$/&\ntask: { key: Edit.task }/; s/^locales: \[en\]$/locales: [en, tr]/' "$TMPDIR_CASE/task/edit/ui.yaml"
node -e '
  const fs = require("fs");
  const dir = process.argv[1];
  const words = JSON.parse(fs.readFileSync(`${dir}/en.json`, "utf8"));
  words["Edit.task"] = "Edit a note";
  fs.writeFileSync(`${dir}/en.json`, JSON.stringify(words, null, 2));
  fs.writeFileSync(`${dir}/tr.json`, JSON.stringify({ ...words, "Edit.task": "Notu düzenle" }, null, 2));
' "$TMPDIR_CASE/task/edit/catalog"
"$DELURIO" check "$TMPDIR_CASE/task" --strict
node -e '
  const fs = require("fs");
  const file = `${process.argv[1]}/tr.json`;
  const words = JSON.parse(fs.readFileSync(file, "utf8"));
  delete words["Edit.task"];
  fs.writeFileSync(file, JSON.stringify(words, null, 2));
' "$TMPDIR_CASE/task/edit/catalog"
code=0
"$DELURIO" check "$TMPDIR_CASE/task" --strict > "$TMPDIR_CASE/task.txt" || code=$?
cat "$TMPDIR_CASE/task.txt"
test "$code" -eq 2
grep -E "edit/ui\.yaml:[0-9]+:1: error: the flow's name for its users, Edit\.task, is not in the tr catalog \(SPEC\.md §4\) \[task-locale\]" "$TMPDIR_CASE/task.txt"
