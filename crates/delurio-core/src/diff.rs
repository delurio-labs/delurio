//! `delurio diff`: what a revision of a graph changes (SPEC.md §10). The port of diff.ts.
//!
//! STUB on the integration branch rust/next: the diff porter replaces this file with the port of diff.ts.

pub const DIFF_USAGE: &str = "  node src/cli.ts diff <old dir> <new dir> [--base <rev>] [--head <rev>] [--json] [--fail-on-behaviour] [--fail-on-regression]
  node src/cli.ts diff <graph dir> --base <rev> [--head <rev>] ...
    what the new revision changes, as markdown for a PR description (--json: for machines); --base reads the old side
    from a git revision, --head the new one (default: the directory on disk). Works for flow and navigation graphs.
    --fail-on-behaviour: exit 2 when what the graph does changed;  --fail-on-regression: exit 2 when a guarantee newly
    fails, a goal newly fails or a problem is new";

/// `delurio diff <args>`
pub fn diff_main(_args: &[String]) -> i32 {
    eprintln!("delurio diff: not ported yet");
    1
}
