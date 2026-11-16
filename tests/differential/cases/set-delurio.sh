# set: delurio
# delurio's own graphs (the private delurio-workhouse at a fixed commit): every way to run check, and the pages
cp -r "$SETS/delurio" "$TMPDIR_CASE/set" && cd "$TMPDIR_CASE/set" || exit 1
"$DELURIO" check graphs
"$DELURIO" check graphs --strict
"$DELURIO" check graphs --strict --no-app
"$DELURIO" check graphs --format sarif
"$DELURIO" check graphs --format sarif --output base.sarif
"$DELURIO" check graphs --strict --baseline base.sarif
"$DELURIO" page graphs --out viz --strict
