# A page may say the path it opens at (route, SPEC.md §4), written as a navigation graph writes a page's route (§6).
# With no navigation graph it is held to nothing; where the folders hold one, a route that no page of it has, its
# parameters' names aside, is a warning (page-route-unknown) at the route, which fails nothing. A dialog has none.
cp -r examples/notes "$TMPDIR_CASE/route"
ui="$TMPDIR_CASE/route/edit/ui.yaml"
sed -i 's/^    script: optional # .*$/&\n    route: \/notes\/:id\/edit/' "$ui"
"$DELURIO" check "$TMPDIR_CASE/route" --strict > "$TMPDIR_CASE/route.txt" 2>&1
cat "$TMPDIR_CASE/route.txt"
grep -Fx 'delurio check: 3 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/route.txt"

# a navigation graph whose page has the route, its parameter named otherwise: nothing
mkdir -p "$TMPDIR_CASE/route/nav/catalog"
echo '{}' > "$TMPDIR_CASE/route/nav/catalog/en.json"
printf '%s\n' 'schema: delurio-graph/0' 'kind: nav' 'id: example.notes' 'title: The notes app' 'status: proposed' 'locale: en' 'catalog: catalog/en.json' \
  'frames:' '  app: { title: The notes app }' \
  'personas:' '  owner: { title: Owner, say: someone who keeps notes, facts: {} }' \
  'pages:' \
  '  start: { route: /, title: Notes, kind: screen, frame: app }' \
  '  edit: { route: "/notes/{note}/edit", title: Edit a note, kind: screen, frame: app }' \
  'entries:' \
  '  - { from: start, to: edit, how: link, label: Edit, data: true }' > "$TMPDIR_CASE/route/nav/nav.yaml"
"$DELURIO" check "$TMPDIR_CASE/route" --strict > "$TMPDIR_CASE/route.txt" 2>&1
cat "$TMPDIR_CASE/route.txt"
grep -Fx 'delurio check: 4 graphs, 0 errors, 0 warnings, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/route.txt"

# a route that no page of it has: a warning at the route, which names it
sed -i 's|^    route: /notes/:id/edit$|    route: /notes/:id/change|' "$ui"
"$DELURIO" check "$TMPDIR_CASE/route" --strict > "$TMPDIR_CASE/route.txt" 2>&1
cat "$TMPDIR_CASE/route.txt"
line=$(grep -n '^    route: /notes/:id/change$' "$ui" | cut -d: -f1)
grep -F "edit/ui.yaml:$line:5: warning: page page opens at /notes/:id/change, and no page of the navigation graph example.notes has that route, so Rehber cannot take a person to it (SPEC.md §4, §6) [page-route-unknown]" "$TMPDIR_CASE/route.txt"
grep -Fx 'delurio check: 4 graphs, 0 errors, 1 warning, 1 gap of 4 needs; exit 0' "$TMPDIR_CASE/route.txt"

# a dialog is drawn over another page and opens at none: the schema refuses its route
sed -i 's|^    route: /notes/:id/change$|    route: /notes/:id/edit|' "$ui"
sed -i 's/^    initialFocus: discardTitle$/&\n    route: \/notes\/:id\/discard/' "$ui"
code=0
"$DELURIO" check "$TMPDIR_CASE/route" --strict > "$TMPDIR_CASE/route.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/route.txt"
test "$code" -eq 2
grep -F "nodes.discardDialog.route: is not allowed here (route is only on a page that is not a dialog: a dialog is drawn over another page, at that page's route) [schema]" "$TMPDIR_CASE/route.txt"
! grep -F '[page-route-unknown]' "$TMPDIR_CASE/route.txt"
