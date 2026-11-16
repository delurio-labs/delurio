# set: medaris-main
# Medaris's graphs as its main branch has them (github.com/amel-tech/medaris at a fixed commit), with their baseline
cp -r "$SETS/medaris-main" "$TMPDIR_CASE/set" && cd "$TMPDIR_CASE/set" || exit 1
"$DELURIO" check graphs
"$DELURIO" check graphs --strict --baseline graphs/delurio.baseline.sarif
"$DELURIO" check graphs --strict --no-app
"$DELURIO" check graphs --format sarif
"$DELURIO" check graphs --format sarif --output base.sarif
"$DELURIO" page graphs --out viz
