# from the reference CI step (10904b5): A kit's CSS may scroll smoothly, and old Internet Explorer's behavior is still refused
cp -r examples/notes "$TMPDIR_CASE/kit-scroll"
printf '\n.k-scroll { scroll-behavior: smooth; overscroll-behavior: contain; }\n' >> "$TMPDIR_CASE/kit-scroll/kit/kit.css"
"$DELURIO" check "$TMPDIR_CASE/kit-scroll" --strict
cp -r examples/notes "$TMPDIR_CASE/kit-behavior"
printf '\n.k-old { behavior: url(x.htc); }\n' >> "$TMPDIR_CASE/kit-behavior/kit/kit.css"
code=0
"$DELURIO" check "$TMPDIR_CASE/kit-behavior" --strict > "$TMPDIR_CASE/kit-behavior.txt" || code=$?
cat "$TMPDIR_CASE/kit-behavior.txt"
test "$code" -eq 1
grep -F 'script-like CSS is not allowed' "$TMPDIR_CASE/kit-behavior.txt"
