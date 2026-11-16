//! The port of the yaml library's compose/composer.ts and compose/compose-doc.ts: the top-level tokens of the tree
//! composed into documents, the errors of the stream given to the document they belong to.

use super::resolve_props::PropsOpts;
use super::{Ctx, ErrAt, Sink};
use crate::yaml::doc::directives::Directives;
use crate::yaml::doc::document::Document;
use crate::yaml::errors::YamlIssue;
use crate::yaml::parse::cst::{Cst, Id, Token, Ty};

/// `Composer`, with default options. Comments are not kept, so neither is the prelude.
pub(crate) struct Composer<'a> {
    cst: &'a Cst,
    doc: Option<Document>,
    at_directives: bool,
    sink: Sink,
    directives: Directives,
}

impl<'a> Composer<'a> {
    pub(crate) fn new(cst: &'a Cst) -> Self {
        Composer { cst, doc: None, at_directives: false, sink: Sink::default(), directives: Directives::new() }
    }

    fn on_error(&mut self, at: ErrAt, code: &str, message: impl Into<String>) {
        let pos = Sink::pos(self.cst, at);
        self.sink.errors.push(YamlIssue::new(pos, code, message));
    }

    /// `decorate(doc, afterDoc)`: the errors and warnings gathered since the last document given to this one
    fn decorate(&mut self, doc: &mut Document, after_doc: bool) {
        let sink = std::mem::take(&mut self.sink);
        if after_doc {
            doc.errors.extend(sink.errors);
            doc.warnings.extend(sink.warnings);
        } else {
            doc.errors = sink.errors;
            doc.warnings = sink.warnings;
        }
    }

    /// Advance the composer by one CST token: the document it completes, if any
    pub(crate) fn next(&mut self, token: Id) -> Option<Document> {
        let cst = self.cst;
        match cst.tok(token) {
            Token::Directive { offset, source } => {
                let line = cst.text(*source);
                let (offset, len) = (*offset, source.len());
                let mut reports = Vec::new();
                self.directives.add(&line, |rel, message, warning| reports.push((rel, message, warning)));
                for (rel, message, warning) in reports {
                    let issue = YamlIssue::new((offset + rel, offset + len), "BAD_DIRECTIVE", message);
                    if warning {
                        self.sink.warnings.push(issue);
                    } else {
                        self.sink.errors.push(issue);
                    }
                }
                self.at_directives = true;
                None
            }
            Token::Document { .. } => {
                let mut doc = self.compose_doc(token);
                if self.at_directives && !doc.directives.doc_start {
                    self.on_error(
                        ErrAt::Token(token),
                        "MISSING_CHAR",
                        "Missing directives-end/doc-start indicator line",
                    );
                }
                self.decorate(&mut doc, false);
                let done = self.doc.replace(doc);
                self.at_directives = false;
                done
            }
            Token::Source { ty: Ty::ByteOrderMark | Ty::Space | Ty::Comment | Ty::Newline, .. } => None,
            Token::Error { source, message, .. } => {
                let msg = if source.is_empty() {
                    message.clone()
                } else {
                    format!("{message}: {}", crate::js::quote_string(&cst.text(*source)))
                };
                let error = YamlIssue::new(Sink::pos(cst, ErrAt::Token(token)), "UNEXPECTED_TOKEN", msg);
                match &mut self.doc {
                    Some(doc) if !self.at_directives => doc.errors.push(error),
                    _ => self.sink.errors.push(error),
                }
                None
            }
            Token::DocEnd { offset, source, end } => {
                let Some(mut doc) = self.doc.take() else {
                    let msg = "Unexpected doc-end without preceding document";
                    self.on_error(ErrAt::Token(token), "UNEXPECTED_TOKEN", msg);
                    return None;
                };
                doc.directives.doc_end = true;
                let directives = doc.directives.clone();
                let end_offset = self.ctx(&directives).resolve_end(end.as_deref(), offset + source.len(), true).1;
                self.decorate(&mut doc, true);
                doc.range[2] = end_offset;
                self.doc = Some(doc);
                None
            }
            t => {
                let msg = format!("Unsupported token {}", t.ty().name());
                self.on_error(ErrAt::Token(token), "UNEXPECTED_TOKEN", msg);
                None
            }
        }
    }

    /// `end(forceDoc, endOffset)`: the last document, or with `force_doc` an empty one for a stream that has none
    pub(crate) fn end(&mut self, force_doc: bool, end_offset: usize) -> Option<Document> {
        if let Some(mut doc) = self.doc.take() {
            self.decorate(&mut doc, true);
            return Some(doc);
        }
        if force_doc {
            let mut doc = Document::new(self.directives.at_document());
            if self.at_directives {
                self.on_error(ErrAt::Offset(end_offset), "MISSING_CHAR", "Missing directives-end indicator line");
            }
            doc.range = [0, end_offset, end_offset];
            self.decorate(&mut doc, false);
            return Some(doc);
        }
        None
    }

    fn ctx<'b>(&'b mut self, directives: &'b Directives) -> Ctx<'b> {
        Ctx { cst: self.cst, at_key: false, at_root: true, directives, sink: &mut self.sink, depth: 0 }
    }

    /// `composeDoc(options, directives, token, onError)`
    fn compose_doc(&mut self, token: Id) -> Document {
        let cst = self.cst;
        let Token::Document { offset, start, value, end } = cst.tok(token) else {
            return Document::new(self.directives.at_document());
        };
        let mut doc = Document::new(self.directives.at_document());
        let directives = doc.directives.clone();
        let mut ctx = self.ctx(&directives);
        let props = ctx.resolve_props(
            start,
            PropsOpts {
                flow: None,
                indicator: Ty::DocStart,
                next: value.or_else(|| end.as_ref().and_then(|e| e.first().copied())),
                offset: *offset,
                parent_indent: 0,
                start_on_newline: true,
            },
        );
        let doc_start = props.found.is_some();
        if doc_start && value.is_some_and(|v| matches!(cst.ty(v), Ty::BlockMap | Ty::BlockSeq)) && !props.has_newline {
            ctx.error(
                ErrAt::Offset(props.end),
                "MISSING_CHAR",
                "Block collection cannot start on same line with directives-end marker",
            );
        }
        let contents = match value {
            Some(v) => ctx.compose_node(*v, &props),
            None => ctx.compose_empty_node(props.end, Some(start), None, &props),
        };
        let content_end = contents.range()[2];
        let (_, re_offset) = ctx.resolve_end(end.as_deref(), content_end, false);
        doc.directives.doc_start = doc_start;
        doc.contents = Some(contents);
        doc.range = [*offset, content_end, re_offset];
        doc
    }
}
