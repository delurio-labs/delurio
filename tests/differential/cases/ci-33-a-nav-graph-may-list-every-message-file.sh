# from the reference CI step: A nav graph may list every message file it reads and lay fields over generated entries
mkdir -p "$TMPDIR_CASE/read/site/catalog"
echo '{}' > "$TMPDIR_CASE/read/site/catalog/en.json"
printf '%s\n' 'schema: delurio-graph/0' 'kind: nav' 'id: example.read' 'title: Read from the app' 'status: captured' 'locale: en' 'catalog: catalog/en.json' \
  'source:' '  repo: acme/site' '  app: apps/web' '  locales:' '    - "libs/i18n/{locale}/web.json"' '    - "libs/i18n/{locale}/common.json"' \
  'extract:' '  adapter: next' '  locales: [en]' '  entries:' \
  '    start->about: { when: "not signedIn", label: About us }' \
  '    frame:site->start link: { skip: the logo links to the page it is on }' \
  'frames:' '  site: { title: The site }' \
  'facts:' '  signedIn: { values: [true, false] }' \
  'personas:' '  visitor: { title: Visitor, say: someone who is not signed in, facts: { signedIn: false } }' \
  'pages:' \
  '  start: { route: /, title: Home, kind: screen, frame: site }' \
  '  about: { route: /about, title: About us, kind: screen, frame: site }' \
  '  docs: { route: "/docs/{...slug}", title: Docs, kind: screen, frame: site, byAddress: true }' \
  'entries:' \
  '  - { from: start, to: about, how: link, label: About us }' > "$TMPDIR_CASE/read/site/nav.yaml"
"$DELURIO" check "$TMPDIR_CASE/read/site" --strict 2>&1 | grep -Fx 'delurio check: 1 graph, 0 errors, 0 warnings; exit 0'

sed -i 's/    start->about: /    start=>about: /; s/{ skip: the logo links to the page it is on }/{ skip: "", how: logo }/' "$TMPDIR_CASE/read/site/nav.yaml"
sed -i '/^    - "libs\/i18n/d; s/^  locales:$/  locales: []/' "$TMPDIR_CASE/read/site/nav.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/read/site" --strict > "$TMPDIR_CASE/read.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/read.txt"
test "$code" -eq 2
test "$(grep -c '\[schema\]' "$TMPDIR_CASE/read.txt")" -ge 4
grep -F 'start=>about' "$TMPDIR_CASE/read.txt" | grep -F '[schema]'
