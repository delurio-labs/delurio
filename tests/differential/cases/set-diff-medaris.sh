# set: medaris-395
# set: medaris-main
# the revision diff between Medaris's graphs on main and the same graphs written anew (two folders)
cp -r "$SETS/medaris-main/graphs" "$TMPDIR_CASE/old" && cp -r "$SETS/medaris-395/graphs" "$TMPDIR_CASE/new" && cd "$TMPDIR_CASE" || exit 1
"$DELURIO" diff old new
"$DELURIO" diff old new --json
"$DELURIO" diff old/auth-account new/realm --fail-on-behaviour
"$DELURIO" diff new old --fail-on-regression
