# from the reference CI step: A page opened by its address is reached so, not unreachable, and drawn with its own words
mkdir -p "$TMPDIR_CASE/typed/site/catalog"
echo '{}' > "$TMPDIR_CASE/typed/site/catalog/en.json"
printf '%s\n' 'schema: delurio-graph/0' 'kind: nav' 'id: example.site' 'title: Example site' 'status: proposed' 'locale: en' 'catalog: catalog/en.json' \
  'frames:' '  site: { title: The site }' \
  'personas:' '  visitor: { title: Visitor, say: someone who is not signed in, facts: {} }' \
  'pages:' \
  '  start: { route: /, title: Home, kind: screen, frame: site }' \
  '  about: { route: /about, title: About, kind: screen, frame: site }' \
  '  signIn: { route: /auth/sign-in, title: Sign in, kind: screen, frame: site, byAddress: true }' \
  '  forgot: { route: /auth/forgot, title: Forgot your password, kind: screen, frame: site }' \
  '  notFound: { route: "/{...path}", title: Not found, kind: screen, frame: site, byAddress: true }' \
  'entries:' \
  '  - { from: start, to: about, how: link, label: About }' \
  '  - { from: signIn, to: forgot, how: link, label: Forgot your password }' \
  '  - { from: notFound, to: start, how: link, label: Home }' > "$TMPDIR_CASE/typed/site/nav.yaml"
"$DELURIO" check "$TMPDIR_CASE/typed/site" --strict 2>&1 | grep -Fx 'delurio check: 1 graph, 0 errors, 0 warnings; exit 0'
"$DELURIO" page "$TMPDIR_CASE/typed/site" --out "$TMPDIR_CASE/typed/page" --strict
grep -F '<span class="path">by its address</span>' "$TMPDIR_CASE/typed/page/index.html"
grep -F '<span class="path">by the address of Sign in, then Forgot your password</span>' "$TMPDIR_CASE/typed/page/index.html"
grep -F '<span class="tag">by its address</span>' "$TMPDIR_CASE/typed/page/index.html"

cp -r "$TMPDIR_CASE/typed/site" "$TMPDIR_CASE/typed/old"
sed -i 's/, byAddress: true//' "$TMPDIR_CASE/typed/old/nav.yaml"
code=0
"$DELURIO" check "$TMPDIR_CASE/typed/old" --strict > "$TMPDIR_CASE/typed.txt" 2>&1 || code=$?
cat "$TMPDIR_CASE/typed.txt"
test "$code" -eq 2
grep -F 'error: Some pages cannot be reached by any kind of user. (unreachable) [nav-problem]' "$TMPDIR_CASE/typed.txt"
grep -F 'note: Sign in (/auth/sign-in)' "$TMPDIR_CASE/typed.txt"

"$DELURIO" diff "$TMPDIR_CASE/typed/old" "$TMPDIR_CASE/typed/site" > "$TMPDIR_CASE/typed.md"
cat "$TMPDIR_CASE/typed.md"
grep -Fx -e '- Can now reach “Sign in” (0 clicks: by its address).' "$TMPDIR_CASE/typed.md"
grep -Fx -e '- Can now reach “Forgot your password” (1 clicks: by the address of “Sign in”, then Forgot your password).' "$TMPDIR_CASE/typed.md"
grep -Fx -e '- “Not found” is now opened by its address.' "$TMPDIR_CASE/typed.md"
