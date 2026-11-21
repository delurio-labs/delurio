//! delurio: the open checker and page of delurio-graph/0 (SPEC.md). Reads graph files and nothing else: no network, no
//! account, no model. The port of cli.ts: `run` is the whole command line, for the binary and the npm package's bin.

pub mod affected;

use affected::affected_main;
use delurio_core::check::check_main;
use delurio_core::diff::diff_main;
use delurio_page::command::{page_main, usage};

/// `delurio <args>`: the exit code
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("check") => check_main(&args[1..]),
        Some("diff") => diff_main(&args[1..]),
        Some("affected") => affected_main(&args[1..]),
        Some("page") => page_main(&args[1..]),
        _ => {
            eprintln!("{}", usage());
            if args.is_empty() || args.iter().any(|a| a == "--help") { 0 } else { 1 }
        }
    }
}
