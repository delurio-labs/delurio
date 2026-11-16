# from the reference CI step: Unsupported YAML is refused at once, not read for minutes
node -e '
  const fs = require("fs");
  const dir = process.argv[1];
  const put = (name, text) => { fs.mkdirSync(`${dir}/${name}`, { recursive: true }); fs.writeFileSync(`${dir}/${name}/flow.yaml`, text); };
  let key = "x";
  for (let d = 0; d < 400; d++) key = `{${key}: ${d}}`;
  let keys = "";
  for (let i = 0; keys.length < 40000; i++) keys += `? ${key.replace("x", "x" + i)}\n: v\n`;
  put("map-keys", keys);
  let aliases = `e: &e []\na: &a [${Array(50).fill("*e").join(", ")}]\nu: [`;
  while (aliases.length < 43000) aliases += "*a, ";
  put("aliases", aliases + "x]\n");
  put("tag", "schema: delurio-graph/0\nkind: flow\nid: tagged\ntitle: !custom Tagged\n");
' "$TMPDIR_CASE/yaml"
code=0
timeout 60 "$DELURIO" check "$TMPDIR_CASE/yaml" > "$TMPDIR_CASE/yaml.txt" || code=$?
cat "$TMPDIR_CASE/yaml.txt"
test "$code" -eq 1
test "$(grep -c 'unsupported YAML.*\[load\]' "$TMPDIR_CASE/yaml.txt")" -eq 3
