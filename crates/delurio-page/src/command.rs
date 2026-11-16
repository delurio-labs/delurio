//! `delurio page`: the page of a graph, or of every graph under a folder and their index, and `--watch` / `--serve`.
//! The port of the page half of cli.ts (navPage, flowPage, treePages, writePages and the page arguments).
//!
//! STUB on the integration branch rust/next: the page porter replaces `page_main` with the port.

use delurio_core::check::CHECK_USAGE;
use delurio_core::diff::DIFF_USAGE;

/// the usage lines of `page`
pub const PAGE_USAGE: &str = "  delurio page <graph dir | graphs folder> [--out <dir>] [--kit <kit>] [--locale <code>] [--strict] [--watch] [--serve [--port <n>]]
    writes the graph's page: <out>/index.html and its diagram (default out: <graph dir>/viz); on a folder of graphs,
    the page of each at <out>/<its path> and an index of them all at <out>/index.html (default out: <folder>/viz);
    --watch writes them again on every change under the folder; --serve also shows them at http://127.0.0.1:<port>/
    (default 4600) and reloads the open page after each write";

/// the usage of the whole command line, as cli.ts writes it
pub fn usage() -> String {
    ["usage:", CHECK_USAGE, PAGE_USAGE, &DIFF_USAGE.replace("node src/cli.ts", "delurio")]
        .join("\n")
        .replace("node src/cli.ts", "delurio")
}

/// `delurio page <args>`
pub fn page_main(_args: &[String]) -> i32 {
    eprintln!("delurio page: not ported yet");
    1
}
