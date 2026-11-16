# set: medaris-395
# Medaris's 179 graphs written anew on 0.9.0 (amel-tech/medaris#395 at a fixed commit), checked from the repository's
# root as its CI does, with the code beside them for the needs' evidence and the source anchors
cp -r "$SETS/medaris-395" "$TMPDIR_CASE/set" && cd "$TMPDIR_CASE/set" || exit 1
"$DELURIO" check graphs --strict --baseline graphs/delurio.baseline.sarif
"$DELURIO" check graphs
"$DELURIO" check graphs --strict --no-app
"$DELURIO" check graphs --format sarif
"$DELURIO" check graphs --format sarif --output base.sarif
"$DELURIO" check graphs/realm graphs/tedris --strict
"$DELURIO" page graphs --out viz
