//! The port of the yaml library's public-api.ts: `parseDocument` and `parse`, for one document with the default
//! options. A text of more than one document is read as the library reads it: `parseDocument` keeps the first and
//! adds a `MULTIPLE_DOCS` error at the second, and stops there (its line counter then holds only the lines the
//! parser had reached); `parse` throws that error.

use super::compose::composer::Composer;
use super::doc::document::Document;
use super::errors::{YamlIssue, prettify};
use super::parse::cst::Cst;
use super::parse::line_counter::LineCounter;
use crate::error::{Error, ErrorKind};
use crate::js::Value;

/// `parseDocument(text, { lineCounter, prettyErrors })` of a parsed text
pub(crate) fn parse_document(cst: &Cst, pretty_errors: bool) -> (Document, LineCounter) {
    let mut composer = Composer::new(cst);
    let mut doc: Option<Document> = None;
    let mut lines = cst.line_starts.len();
    // the first document, and whether a second one stopped the reading
    let take = |next: Document, doc: &mut Option<Document>| -> bool {
        match doc {
            None => {
                *doc = Some(next);
                false
            }
            Some(first) => {
                let msg = "Source contains multiple documents; please use YAML.parseAllDocuments()";
                first.errors.push(YamlIssue::new((next.range[0], next.range[1]), "MULTIPLE_DOCS", msg));
                true
            }
        }
    };
    let mut stopped = false;
    for &(token, lines_then) in &cst.top {
        if let Some(next) = composer.next(token)
            && take(next, &mut doc)
        {
            lines = lines_then;
            stopped = true;
            break;
        }
    }
    if !stopped && let Some(next) = composer.end(true, cst.len) {
        take(next, &mut doc);
    }
    let line_counter = LineCounter { line_starts: cst.line_starts[..lines.min(cst.line_starts.len())].to_vec() };
    // `doc` is always set by compose.end(true) at the very latest
    let mut doc = doc.unwrap_or_else(|| Document::new(super::doc::directives::Directives::new()));
    if pretty_errors {
        let src = &cst.buf[..cst.len];
        for e in doc.errors.iter_mut().chain(doc.warnings.iter_mut()) {
            prettify(src, &line_counter, e);
        }
    }
    (doc, line_counter)
}

/// a document's error as the error `parse` throws
pub(crate) fn thrown(e: &YamlIssue) -> Error {
    Error {
        kind: ErrorKind::Yaml,
        message: e.message.clone(),
        place: None,
        line_pos: e.line_pos,
        code: Some(e.code.clone()),
    }
}

/// `parse(text)`: the document's value, or the first of its errors; its warnings, which the library hands to
/// `process.emitWarning`, are returned beside it
pub(crate) fn parse(cst: &Cst) -> (Result<Value, Error>, Vec<YamlIssue>) {
    let (doc, _) = parse_document(cst, true);
    let warnings = doc.warnings.clone();
    if let Some(first) = doc.errors.first() {
        return (Err(thrown(first)), warnings);
    }
    (Ok(doc.to_js()), warnings)
}
