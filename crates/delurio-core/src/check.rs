//! `check`: every graph under a folder, as a project's CI runs it (SPEC.md §10). The port of check.ts.
//!
//! STUB on the integration branch rust/next: the check porter replaces this file with the port of check.ts (its draft
//! is on rust/check). `count` and `graph_dirs` are final already: the page command uses them.

use crate::js;
use crate::nodepath::join2;

pub const CHECK_USAGE: &str = "  node src/cli.ts check <graphs folder>... [--strict] [--baseline <sarif>] [--format sarif --output <file>] [--app <dir> | --no-app]
    checks every graph under the folders and writes nothing else: GNU error lines on stdout, or SARIF 2.1.0;
    exit 0 clean, 1 a graph cannot be loaded or needs a newer format, 2 an error under --strict (new since --baseline);
    the evidence of needs and the source: anchors are found under --app, the application's repository root
    (default: the working directory); --no-app checks the graphs without the code, and does not look for them";

const SKIP: [&str; 5] = ["viz", "video", "recording", "node_modules", ".git"];

/// `count(n, word)`: `1 graph`, `2 graphs`
pub fn count(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

fn is_dir(path: &str) -> bool {
    std::path::Path::new(path).is_dir()
}

/// the names in a folder, as `readdirSync(dir).sort()` gives them
fn sorted_names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    js::sort_utf16(&mut names);
    names
}

/// every graph folder at or under root, in path order
pub fn graph_dirs(root: &str) -> Vec<String> {
    let mut out = Vec::new();
    graph_dirs_into(root, &mut out);
    out
}

fn graph_dirs_into(root: &str, out: &mut Vec<String>) {
    if exists(&join2(root, "flow.yaml")) || exists(&join2(root, "nav.yaml")) {
        out.push(root.to_string());
    }
    for name in sorted_names(root) {
        if SKIP.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        let sub = join2(root, &name);
        if is_dir(&sub) {
            graph_dirs_into(&sub, out);
        }
    }
}

/// `delurio check <args>`
pub fn check_main(_args: &[String]) -> i32 {
    eprintln!("delurio check: not ported yet");
    1
}
