//! YAML as delurio reads it (SPEC.md §1): the port of src/yaml.ts, on a port of the yaml library (eemeli/yaml 2.9.1)
//! that the TypeScript checker reads every graph file with.
//!
//! YAML 1.2 without anchors, aliases, tags, or keys that are lists or maps. No graph needs them, and two of them keep
//! the yaml library busy far beyond a file's size: on a 40 KB file, maps used as keys, or aliases inside keys, held
//! `parse` for more than 30 seconds, which stops a server that reads the file. So the text is first read into the
//! library's concrete syntax tree, which resolves nothing and takes time in proportion to the text, and walked once;
//! a file that holds one of them is refused at its place, as a file that is not YAML is. Every reader of a graph file
//! goes through `read_yaml`.
//!
//! The library's modules are ported in the folders named as its own (parse/, compose/, doc/, nodes, schema, errors,
//! public-api), each naming the file it ports. What delurio does not use is left out: stringifying, the CST visitor,
//! comments, options other than the defaults, more than one document, and the YAML 1.1 schema (a `%YAML 1.1` document
//! is read with the 1.2 core schema; the library would switch to its 1.1 schema). Every offset is in UTF-16 code
//! units, as in JavaScript.

mod compose;
mod doc;
mod errors;
mod nodes;
mod parse;
mod public_api;
mod schema;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub use doc::document::Document;
pub use errors::YamlIssue;
pub use nodes::{Alias, Node, Pair, Scalar, YamlMap, YamlSeq};
pub use parse::line_counter::LineCounter;

use crate::error::{At, Error, ErrorKind, Result};
use crate::js::Value;
use parse::cst::{Cst, Id, Token, Ty};

/// the first place in a text that delurio does not read: `Unsupported` of yaml.ts
#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
    pub code: String,
    pub message: String, // `unsupported YAML: …`, with what to write instead
}

const WHY: &str = "delurio reads YAML without anchors, aliases, tags or keys that are lists or maps (SPEC.md §1)";

/// the 1-based line and column of a UTF-16 offset into the text
pub fn line_and_column(text: &str, offset: usize) -> At {
    let mut line = 1;
    let mut from = 0;
    let mut at = 0;
    for c in text.chars() {
        if at >= offset {
            break;
        }
        at += c.len_utf16();
        if c == '\n' && at <= offset {
            line += 1;
            from = at;
        }
    }
    At { line, column: offset - from + 1 }
}

/// the first place in the text, by offset, that delurio does not read; None when there is none. A text that is not
/// YAML at all is left to the parser, which says why.
pub fn unsupported(text: &str) -> Option<Unsupported> {
    unsupported_in(&parse::parser::parse(text), text)
}

/// what is found at a place delurio does not read
#[derive(Clone, Copy)]
enum Found {
    Prop(Id),
    Key { list: bool },
}

fn unsupported_in(cst: &Cst, text: &str) -> Option<Unsupported> {
    let mut first: Option<(usize, Found)> = None;
    let mut found = |offset: usize, what: Found| {
        if first.is_none_or(|(o, _)| offset < o) {
            first = Some((offset, what));
        }
    };
    // walked with a stack of its own: a deeply nested text must not overflow the call stack here
    let mut stack: Vec<Id> = cst.top.iter().rev().map(|&(id, _)| id).collect();
    while let Some(id) = stack.pop() {
        let t = cst.tok(id);
        if matches!(t.ty(), Ty::Anchor | Ty::Alias | Ty::Tag) {
            found(t.offset(), Found::Prop(id));
        }
        let mut children: Vec<Id> = Vec::new();
        match t {
            Token::Document { start, value, end, .. } => {
                children.extend(start);
                children.extend(value);
                children.extend(end.iter().flatten());
            }
            Token::FlowScalar { end, .. } | Token::DocEnd { end, .. } => children.extend(end.iter().flatten()),
            Token::BlockScalar { props, .. } => children.extend(props),
            Token::BlockMap { items, .. } | Token::BlockSeq { items, .. } | Token::FlowCollection { items, .. } => {
                if let Token::FlowCollection { start, .. } = t {
                    children.push(*start);
                }
                for it in items {
                    if let Some(key) = it.key {
                        let list = match cst.tok(key) {
                            Token::BlockSeq { .. } => Some(true),
                            Token::BlockMap { .. } => Some(false),
                            Token::FlowCollection { start, .. } => Some(cst.source_of(*start) == [b'[' as u16]),
                            _ => None,
                        };
                        if let Some(list) = list {
                            found(cst.tok(key).offset(), Found::Key { list });
                        }
                    }
                    children.extend(&it.start);
                    children.extend(it.key);
                    children.extend(it.sep.iter().flatten());
                    children.extend(it.value);
                }
                if let Token::FlowCollection { end, .. } = t {
                    children.extend(end);
                }
            }
            Token::Source { .. } | Token::Directive { .. } | Token::Error { .. } => {}
        }
        stack.extend(children.into_iter().rev());
    }
    let (offset, what) = first?;
    let (code, what) = match what {
        Found::Prop(id) => {
            let source = cst.text(cst.tok(id).source().unwrap_or_default());
            match cst.ty(id) {
                Ty::Anchor => ("BAD_ALIAS", format!("an anchor ({source}): write the value where it is used")),
                Ty::Alias => ("BAD_ALIAS", format!("an alias ({source}): write the value where it is used")),
                _ => ("TAG_RESOLVE_FAILED", format!("a tag ({source}): write the value without it")),
            }
        }
        Found::Key { list } => {
            ("NON_STRING_KEY", format!("a key that is a {}: name it with a string", if list { "list" } else { "map" }))
        }
    };
    let At { line, column } = line_and_column(text, offset);
    Some(Unsupported {
        offset,
        line,
        column,
        code: code.to_string(),
        message: format!("unsupported YAML: {what}; {WHY}"),
    })
}

/// the refusal as the error the yaml library throws for a text that is not YAML, so that every reader that already
/// says "cannot be loaded" for one says it for this too, at the same place
fn refusal(u: &Unsupported) -> Error {
    Error {
        kind: ErrorKind::Yaml,
        message: format!("{} at line {}, column {}", u.message, u.line, u.column),
        place: None,
        line_pos: Some(At { line: u.line, column: u.column }),
        code: Some(u.code.clone()),
    }
}

/// a text read with `parseDocument(text, { lineCounter, prettyErrors: false })`: the document and its line counter
#[derive(Debug, Clone)]
pub struct ParsedDocument {
    pub doc: Document,
    pub lines: LineCounter,
}

impl ParsedDocument {
    /// `lineCounter.linePos(offset)`: the 1-based line and column of an offset (line 0 before the first line)
    pub fn line_pos(&self, offset: usize) -> At {
        self.lines.line_pos(offset)
    }
}

/// what a run has read already, by text, so that a file many graphs share is parsed once in it: `check` parsed the
/// domain again for every flow that names it and every flow again for every flow that starts another, more than half
/// of its time on delurio's own graphs. Kept only for the run `parsing_once` is given, so a process that reads graph
/// after graph (a server) keeps nothing between runs.
#[derive(Default)]
struct Read {
    values: HashMap<String, Kept>,
    documents: HashMap<String, Rc<ParsedDocument>>,
}

enum Kept {
    Value(Value),
    Refused(Unsupported),
}

thread_local! {
    static READ: RefCell<Option<Read>> = const { RefCell::new(None) };
    static WARNINGS: RefCell<Vec<YamlIssue>> = const { RefCell::new(Vec::new()) };
}

/// `run`, with every text it reads through `read_yaml`, `key_at` or `parse_document` parsed once; the same as `run`
/// alone otherwise
pub fn parsing_once<T>(run: impl FnOnce() -> T) -> T {
    if READ.with_borrow(Option::is_some) {
        return run();
    }
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            READ.with_borrow_mut(|r| *r = None);
        }
    }
    READ.with_borrow_mut(|r| *r = Some(Read::default()));
    let _reset = Reset;
    run()
}

/// the value of a graph file's text: the yaml library's `parse`, once nothing unsupported is found. Inside
/// `parsing_once` each call gets its own copy, so a reader that changes the value changes no other reader's. A text
/// that is not YAML, or holds what delurio does not read, is an error of kind Yaml at the place the parser gave.
pub fn read_yaml(text: &str) -> Result<Value> {
    let kept = READ.with_borrow(|r| {
        r.as_ref().and_then(|r| match r.values.get(text)? {
            Kept::Value(v) => Some(Ok(v.clone())),
            Kept::Refused(u) => Some(Err(refusal(u))),
        })
    });
    if let Some(kept) = kept {
        return kept;
    }
    let cst = parse::parser::parse(text);
    if let Some(u) = unsupported_in(&cst, text) {
        let error = refusal(&u);
        READ.with_borrow_mut(|r| {
            if let Some(r) = r {
                r.values.insert(text.to_string(), Kept::Refused(u));
            }
        });
        return Err(error);
    }
    let value = parse_cst(&cst)?;
    READ.with_borrow_mut(|r| {
        if let Some(r) = r {
            r.values.insert(text.to_string(), Kept::Value(value.clone()));
        }
    });
    Ok(value)
}

/// the yaml library's `parse(text)`: the value of the text's one document, or the first error in it (the message
/// as the library writes it, with the line and column and the line itself). The library hands the document's
/// warnings to `process.emitWarning`; they are kept for `take_warnings`.
pub fn parse(text: &str) -> Result<Value> {
    parse_cst(&parse::parser::parse(text))
}

fn parse_cst(cst: &Cst) -> Result<Value> {
    let (value, warnings) = public_api::parse(cst);
    if !warnings.is_empty() {
        WARNINGS.with_borrow_mut(|w| w.extend(warnings));
    }
    value
}

/// the warnings `parse` (and so `read_yaml`) met since the last call, oldest first. Node prints each, once the
/// running code is done (`process.emitWarning`), as `node_warning` writes it: `delurio check` exits before that and
/// prints none; `page` and `diff` print them.
pub fn take_warnings() -> Vec<YamlIssue> {
    WARNINGS.with_borrow_mut(std::mem::take)
}

/// a warning as Node prints it to stderr for `process.emitWarning`: `(node:PID) [CODE] YAMLWarning: message`, and
/// after the first warning of the process the hint `(Use \`node --trace-warnings ...\` ...)`
pub fn node_warning(warning: &YamlIssue, pid: u32, first: bool) -> String {
    let mut out = format!("(node:{pid}) [{}] YAMLWarning: {}\n", warning.code, warning.message);
    if first {
        out.push_str("(Use `node --trace-warnings ...` to show where the warning was created)\n");
    }
    out
}

/// `parseDocument(text, { lineCounter, prettyErrors: false })`: the document (errors and warnings as the library
/// gives them, without line and column), and its line counter; inside `parsing_once`, read once per text
pub fn parse_document(text: &str) -> Rc<ParsedDocument> {
    let kept = READ.with_borrow(|r| r.as_ref().and_then(|r| r.documents.get(text).cloned()));
    if let Some(kept) = kept {
        return kept;
    }
    let cst = parse::parser::parse(text);
    let (doc, lines) = public_api::parse_document(&cst, false);
    let parsed = Rc::new(ParsedDocument { doc, lines });
    READ.with_borrow_mut(|r| {
        if let Some(r) = r {
            r.documents.insert(text.to_string(), Rc::clone(&parsed));
        }
    });
    parsed
}

/// the 1-based line and column of the key at `path` in a YAML text (of the item, for a list index), else the file's
/// start; for a text `read_yaml` has read
pub fn key_at<S: AsRef<str>>(text: &str, path: &[S]) -> At {
    let parsed = parse_document(text);
    let mut node: Option<&Node> = parsed.doc.contents.as_ref();
    let mut at: Option<usize> = None;
    for step in path {
        let step = step.as_ref();
        match node {
            Some(Node::Map(map)) => {
                let Some(pair) = map.items.iter().find(|p| p.key_string() == step) else { break };
                at = pair.key.as_ref().map(|k| k.range()[0]).or(at);
                node = pair.value.as_ref();
            }
            Some(Node::Seq(seq)) => {
                node = array_index(step).and_then(|i| seq.items.get(i));
                at = node.map(|n| n.range()[0]).or(at);
            }
            _ => break,
        }
    }
    parsed.line_pos(at.unwrap_or(0))
}

/// the index `list[Number(step)]` reads: `Number(step)` when it is an integer from 0 (`-0` included) up
pub fn array_index(step: &str) -> Option<usize> {
    let n = js_number(step)?;
    (n.fract() == 0.0 && (0.0..4294967295.0).contains(&n)).then_some(n as usize)
}

/// `Number(s)` of a string: None where it is NaN
fn js_number(s: &str) -> Option<f64> {
    let t = crate::js::trim(s);
    if t.is_empty() {
        return Some(0.0);
    }
    let radix = |prefix: &str, radix: u32| -> Option<Option<f64>> {
        let digits = t.strip_prefix(prefix).or_else(|| t.strip_prefix(&prefix.to_uppercase()))?;
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return Some(None);
        }
        Some(Some(digits.chars().fold(0.0, |n, c| n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)))))
    };
    for (prefix, r) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(n) = radix(prefix, r) {
            return n;
        }
    }
    let unsigned = t.strip_prefix(['+', '-']).unwrap_or(t);
    if unsigned == "Infinity" {
        return Some(if t.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY });
    }
    // a StrDecimalLiteral: digits, a point, an exponent; Rust's parser also takes inf and nan, which JavaScript does not
    let ok = unsigned.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        && unsigned.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.');
    if !ok {
        return None;
    }
    t.parse::<f64>().ok()
}

#[cfg(test)]
mod tests;
