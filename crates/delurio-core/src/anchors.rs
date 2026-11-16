//! Source anchors held to the code they name (SPEC.md §9). Every `source:` that a graph file's schema gives as an
//! anchor names a file of the application's repository, from its root, and, with a line, a line within that file: one
//! that does not is an error (`source-unresolved`), as a need's evidence is (SPEC.md §13). A `path:line` written in a
//! graph's words (a description, a `say`, a note) is no anchor, but it cites the code all the same, so it is held to the
//! code too, as a warning (`citation-unresolved`): words are not parsed, so only a token that reads as a path from the
//! repository's root, a folder of the root first and a file with an extension last, is looked at. Under --no-app
//! neither is looked for, and only counted. The port of anchors.ts.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::domain::pointer;
use crate::js::{self, Object, Value};
use crate::nodepath;
use crate::schemas::{SchemaKind, anchors_in, schema_kind_of};
use crate::yaml::{key_at, read_yaml};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorRule {
    SourceUnresolved,
    CitationUnresolved,
}

impl AnchorRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            AnchorRule::SourceUnresolved => "source-unresolved",
            AnchorRule::CitationUnresolved => "citation-unresolved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnchorProblem {
    pub rule: AnchorRule,
    pub file: String, // as found: absolute when the check was given absolute folders
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub graph: String,   // the id of the graph the file belongs to
    pub element: String, // the file and the JSON pointer of the anchor or the words: flow.yaml#/states/consent/source
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct AnchorsCheck {
    pub problems: Vec<AnchorProblem>,
    pub anchors: usize,   // the anchors found
    pub citations: usize, // the citations in words that read as a path from the repository's root
}

const FORMAT: &str = "delurio-graph/0";

/// the kinds whose files carry anchors; a need's evidence is held by its own rule, and a catalog holds the product's
/// words, which cite nothing
const ANCHORED: [SchemaKind; 4] = [SchemaKind::Flow, SchemaKind::Ui, SchemaKind::Nav, SchemaKind::Domain];
const WORDED: [SchemaKind; 5] =
    [SchemaKind::Flow, SchemaKind::Ui, SchemaKind::Nav, SchemaKind::Domain, SchemaKind::Needs];
/// what is data rather than words: sample values of the screens, records of an entity, the anchors themselves and a
/// nav graph's source; and in a needs file a need's evidence, which its own rule holds (a nav page's evidence is words)
const NOT_WORDS: [&str; 3] = ["samples", "examples", "source"];
const NOT_NEEDS_WORDS: [&str; 4] = ["samples", "examples", "source", "evidence"];

/// what a path is on the disk: a file, or a folder (anything else that is there)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    File,
    Folder,
}

/// why `<path>[:<line>]` names nothing under the root: what is wrong (the path, or the path and its line) and what is
/// wrong with it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Unresolved {
    pub subject: String,
    pub reason: String,
}

/// the lines of a text as anchors count them: a last line break ends the last line, and opens none
fn line_count(body: &[u8]) -> usize {
    let breaks = body.iter().filter(|&&b| b == b'\n').count();
    breaks + 1 - usize::from(body.last() == Some(&b'\n'))
}

/// `<path>:<line>` split as `/^(.*?)(?::(\d+))?$/` splits it: the line is the digits after the last colon
pub(crate) fn split_line(value: &str) -> (&str, Option<&str>) {
    if let Some(i) = value.rfind(':') {
        let digits = &value[i + 1..];
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return (&value[..i], Some(digits));
        }
    }
    (value, None)
}

/// `Number` of a run of digits
fn number_of(digits: &str) -> f64 {
    digits.parse::<f64>().unwrap_or(f64::NAN)
}

/// the files of the application, each looked up once per run: whether it is there and is a file, and its lines
#[derive(Debug)]
pub struct AppFiles {
    kinds: RefCell<HashMap<String, Option<AppKind>>>,
    lines: RefCell<HashMap<String, usize>>,
    pub root: String,
}

impl AppFiles {
    pub fn new(root: &str) -> Self {
        AppFiles { kinds: RefCell::new(HashMap::new()), lines: RefCell::new(HashMap::new()), root: root.to_string() }
    }

    pub fn abs(&self, path: &str) -> String {
        if path.starts_with('/') { path.to_string() } else { nodepath::join2(&self.root, path) }
    }

    pub fn kind(&self, path: &str) -> Option<AppKind> {
        let abs = self.abs(path);
        if let Some(k) = self.kinds.borrow().get(&abs) {
            return *k;
        }
        let k = std::fs::metadata(&abs).ok().map(|m| if m.is_file() { AppKind::File } else { AppKind::Folder });
        self.kinds.borrow_mut().insert(abs, k);
        k
    }

    /// the file's lines; a file that cannot be read counts as an empty one (the TypeScript checker threw there)
    pub fn line_count(&self, path: &str) -> usize {
        let abs = self.abs(path);
        if let Some(n) = self.lines.borrow().get(&abs) {
            return *n;
        }
        let n = line_count(&std::fs::read(&abs).unwrap_or_default());
        self.lines.borrow_mut().insert(abs, n);
        n
    }

    /// why `<path>[:<line>]` names nothing under the root, or None when it names a file and a line within it
    pub fn unresolved(&self, value: &str) -> Option<Unresolved> {
        let (path, line) = split_line(value);
        let why = |subject: &str, reason: String| Some(Unresolved { subject: subject.to_string(), reason });
        match self.kind(path) {
            None => return why(path, "does not exist".into()),
            Some(AppKind::Folder) => return why(path, "is a folder: an anchor names a file".into()),
            Some(AppKind::File) => {}
        }
        let line = number_of(line?);
        let n = self.line_count(path);
        if line < 1.0 {
            return why(value, format!("names line {}: a file's lines count from 1", js::number_to_string(line)));
        }
        if line > n as f64 {
            return why(value, format!("is past the file's end: it has {n} {}", if n == 1 { "line" } else { "lines" }));
        }
        None
    }
}

/// a token of words that reads as `<path>:<line>`: a folder, then a file with an extension, then the line, and a
/// range's last line after a dash; not inside an address, a longer path or a word. The TypeScript pattern's
/// `(?<![\p{L}\p{N}_.\/:@~+\-])` before it is `NOT_AFTER`, and its `(?!\d)` after it holds of every match the greedy
/// digits make.
static CITATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"((?:[\p{L}\p{N}_@.][\p{L}\p{N}_@.()\[\]+\-]*/)+[\p{L}\p{N}_@()\[\]+\-.]*?\.[A-Za-z][A-Za-z0-9]*):([0-9]+)(?:-([0-9]+))?",
    )
    .expect("the pattern compiles")
});
static NOT_AFTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_./:@~+\-]$").expect("the pattern compiles"));

/// a citation found in words: the whole token, its path, its line and a range's last line
struct Citation<'a> {
    cited: &'a str,
    path: &'a str,
    line: &'a str,
    to: Option<&'a str>,
}

/// `value.matchAll(CITATION)`: each match at the first place after the last one where the pattern matches and the
/// character before it may stand before a citation
fn citations(value: &str) -> Vec<Citation<'_>> {
    let mut out = Vec::new();
    let mut from = 0;
    while from <= value.len() {
        let Some(m) = CITATION.captures_at(value, from) else { break };
        let (Some(all), Some(path), Some(line)) = (m.get(0), m.get(1), m.get(2)) else { break };
        let before = value[..all.start()].chars().next_back();
        if before.is_some_and(|c| NOT_AFTER.is_match(c.encode_utf8(&mut [0; 4]))) {
            // the next place to try is the character after this match's start
            from = all.start() + value[all.start()..].chars().next().map_or(1, char::len_utf8);
            continue;
        }
        out.push(Citation {
            cited: all.as_str(),
            path: path.as_str(),
            line: line.as_str(),
            to: m.get(3).map(|x| x.as_str()),
        });
        from = all.end();
    }
    out
}

/// `/^[^\s:]+(:[0-9]+)?$/`: what reads as an anchor; the schema rule says why another value does not
fn reads_as_anchor(value: &str) -> bool {
    let (path, line) = match value.find(':') {
        None => (value, None),
        Some(i) => (&value[..i], Some(&value[i + 1..])),
    };
    !path.is_empty()
        && !path.chars().any(js::is_js_space)
        && line.is_none_or(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
}

/// the folder given as source.app by a nav graph, or by the nav.yaml beside a generated one, to say where an anchor
/// would have been had it been written from there
fn nav_app(file: &str, doc: &Object) -> Option<String> {
    if let Some(own) = doc.get("source").and_then(|s| s.get_str("app")) {
        return Some(own.to_string());
    }
    let beside = nodepath::join2(&nodepath::dirname(file), "nav.yaml");
    if nodepath::basename(file) == "nav.yaml" || !std::path::Path::new(&beside).exists() {
        return None;
    }
    let text = String::from_utf8_lossy(&std::fs::read(&beside).ok()?).into_owned();
    read_yaml(&text).ok()?.get("source")?.get_str("app").map(str::to_string)
}

/// every anchor and every citation in words of the graph files given, against the application's files under `app`;
/// with `app` None (--no-app), each is counted and none is looked for
pub fn check_anchors(files: &[String], app: Option<&str>) -> AnchorsCheck {
    let mut out = AnchorsCheck::default();
    let code = app.map(AppFiles::new);
    for file in files {
        let Some(kind) = schema_kind_of(file) else { continue };
        if !WORDED.contains(&kind) {
            continue;
        }
        // the schema rule, or the graph's load, says why a file cannot be read
        let Ok(bytes) = std::fs::read(file) else { continue };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let Ok(doc) = read_yaml(&text) else { continue };
        let Value::Object(own) = &doc else { continue };
        if own.get("schema").and_then(Value::as_str).is_some_and(|s| s != FORMAT) {
            continue;
        }
        let graph = own
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| nodepath::basename(&nodepath::dirname(file)), str::to_string);
        let name = nodepath::basename(file);
        let mut problem = |rule: AnchorRule, path: &[String], message: String, element: String| {
            let at = key_at(&text, path);
            out.problems.push(AnchorProblem {
                rule,
                file: file.clone(),
                line: at.line,
                column: at.column,
                message,
                graph: graph.clone(),
                element,
            });
        };
        let mut anchors = 0;
        let mut cited = 0;

        if ANCHORED.contains(&kind) {
            let nav_from = if kind == SchemaKind::Nav { nav_app(file, own) } else { None };
            for place in anchors_in(kind, &doc) {
                let (path, value) = (&place.path, &place.value);
                if !reads_as_anchor(value) {
                    continue; // the schema rule says why
                }
                anchors += 1;
                let Some(why) = code.as_ref().and_then(|c| c.unresolved(value)) else { continue };
                let parent = path[..path.len().saturating_sub(1)].join(".");
                let at = if parent.is_empty() { "the graph".to_string() } else { parent };
                let from = match (&nav_from, &code) {
                    (Some(n), Some(_)) => Some(nodepath::join2(n, value)),
                    _ => None,
                };
                let there = match (&from, &code) {
                    (Some(from), Some(c)) if c.unresolved(from).is_none() => format!(
                        "; {from} is there: an anchor is written from the repository's root, not from source.app"
                    ),
                    _ => String::new(),
                };
                problem(
                    AnchorRule::SourceUnresolved,
                    path,
                    format!("{at}: source {} {}{there} (SPEC.md §9)", why.subject, why.reason),
                    format!("{name}#{}", pointer(path)),
                );
            }
        }

        let skip: &[&str] = if kind == SchemaKind::Needs { &NOT_NEEDS_WORDS } else { &NOT_WORDS };
        let mut stack: Vec<(&Value, Vec<String>)> = vec![(&doc, Vec::new())];
        // depth first, in the order the TypeScript recursion visits the values
        while let Some((value, path)) = stack.pop() {
            match value {
                Value::String(s) => {
                    for m in citations(s) {
                        let first = m.path.split('/').next().unwrap_or("");
                        if first == "." || first == ".." {
                            continue;
                        }
                        if code.as_ref().is_some_and(|c| c.kind(first) != Some(AppKind::Folder)) {
                            continue; // not a path from the repository's root
                        }
                        cited += 1;
                        let Some(c) = &code else { continue };
                        let line = number_of(m.line).max(m.to.map_or(0.0, number_of));
                        let Some(why) = c.unresolved(&format!("{}:{}", m.path, js::number_to_string(line))) else {
                            continue;
                        };
                        let said = if why.subject == m.path {
                            format!("and {} {}", why.subject, why.reason)
                        } else {
                            format!("which {}", why.reason)
                        };
                        problem(
                            AnchorRule::CitationUnresolved,
                            &path,
                            format!("{} cites {}, {said} (SPEC.md §9)", path.join("."), m.cited),
                            format!("{name}#{}/{}", pointer(&path), m.cited),
                        );
                    }
                }
                Value::Array(items) => {
                    for (i, x) in items.iter().enumerate().rev() {
                        let mut p = path.clone();
                        p.push(i.to_string());
                        stack.push((x, p));
                    }
                }
                Value::Object(o) => {
                    for (k, x) in o.iter().rev() {
                        if skip.contains(&k.as_str()) {
                            continue;
                        }
                        let mut p = path.clone();
                        p.push(k.clone());
                        stack.push((x, p));
                    }
                }
                _ => {}
            }
        }
        out.anchors += anchors;
        out.citations += cited;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(String, String, Option<String>)> {
        citations(text)
            .into_iter()
            .map(|c| (c.path.to_string(), c.line.to_string(), c.to.map(str::to_string)))
            .collect()
    }

    #[test]
    fn citations_in_words() {
        // node -e "const re = /(?<![\p{L}\p{N}_.\/:@~+\-])((?:[\p{L}\p{N}_@.][\p{L}\p{N}_@.()[\]+\-]*\/)+[\p{L}\p{N}_@()[\]+\-.]*?\.[A-Za-z][A-Za-z0-9]*):(\d+)(?:-(\d+))?(?!\d)/gu;
        //   const t = 'restoreNote, at examples/notes/app/notes.js:30-40, app/notes.js:99 and https://example.com/examples/notes/app/notes.js:99 x/a.b.js:7 ../up/a.js:1 (src/x.ts:3) ab/c.d:1:2';
        //   console.log(JSON.stringify([...t.matchAll(re)].map(m => [m[1], m[2], m[3] ?? null])))"
        let t = "restoreNote, at examples/notes/app/notes.js:30-40, app/notes.js:99 and https://example.com/examples/notes/app/notes.js:99 x/a.b.js:7 ../up/a.js:1 (src/x.ts:3) ab/c.d:1:2";
        let s = |x: &str| x.to_string();
        assert_eq!(
            found(t),
            [
                (s("examples/notes/app/notes.js"), s("30"), Some(s("40"))),
                (s("app/notes.js"), s("99"), None),
                (s("x/a.b.js"), s("7"), None),
                (s("../up/a.js"), s("1"), None),
                (s("src/x.ts"), s("3"), None),
                (s("ab/c.d"), s("1"), None),
            ]
        );
    }

    #[test]
    fn what_reads_as_an_anchor() {
        assert!(reads_as_anchor("src/a.ts"));
        assert!(reads_as_anchor("src/a.ts:12"));
        assert!(!reads_as_anchor("src/a.ts:"));
        assert!(!reads_as_anchor("src/a.ts:1:2"));
        assert!(!reads_as_anchor("src/a b.ts"));
        assert!(!reads_as_anchor(":12"));
        assert_eq!(split_line("a:1:2"), ("a:1", Some("2")));
        assert_eq!(split_line("a:x"), ("a:x", None));
        assert_eq!(line_count(b"a\nb\n"), 2);
        assert_eq!(line_count(b"a\nb"), 2);
        assert_eq!(line_count(b""), 1);
    }

    #[test]
    fn anchors_that_name_nothing() {
        // node --input-type=module -e "import { AppFiles } from './dist/anchors.js'; const code = new AppFiles(process.cwd());
        //   console.log(JSON.stringify(values.map((v) => code.unresolved(v))))" from the repository's root
        let code = AppFiles::new(&nodepath::resolve(&[env!("CARGO_MANIFEST_DIR"), "../.."]));
        let why = |v: &str| code.unresolved(v).map(|u| format!("{} {}", u.subject, u.reason));
        assert_eq!(
            why("examples/notes/app/notes.js:99").as_deref(),
            Some("examples/notes/app/notes.js:99 is past the file's end: it has 37 lines")
        );
        assert_eq!(why("examples/notes/app/notes.js:37"), None);
        assert_eq!(
            why("examples/notes/app/notes.js:0").as_deref(),
            Some("examples/notes/app/notes.js:0 names line 0: a file's lines count from 1")
        );
        assert_eq!(
            why("examples/notes/app").as_deref(),
            Some("examples/notes/app is a folder: an anchor names a file")
        );
        assert_eq!(why("examples/nowhere.js:3").as_deref(), Some("examples/nowhere.js does not exist"));
    }
}
