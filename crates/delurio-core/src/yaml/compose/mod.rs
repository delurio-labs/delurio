//! The yaml library's compose/ folder: the concrete syntax tree composed into a document's nodes. One module per
//! library module; the functions that take the library's `ctx` and `onError` are methods of `Ctx`, which holds both.
//!
//! The library composes by recursion, and a text nested deeper than the JavaScript stack allows is caught where a
//! collection is composed (compose-node.ts: "Almost certainly here due to a stack overflow") and reported as
//! `RESOURCE_EXHAUSTION`. Here that depth is fixed: a collection nested deeper than `MAX_DEPTH` is that error, with
//! the message JavaScript gives, so that neither the stack nor the time runs out.

pub(crate) mod compose_collection;
pub(crate) mod compose_node;
pub(crate) mod compose_scalar;
pub(crate) mod composer;
pub(crate) mod resolve_block_map;
pub(crate) mod resolve_block_scalar;
pub(crate) mod resolve_block_seq;
pub(crate) mod resolve_end;
pub(crate) mod resolve_flow_collection;
pub(crate) mod resolve_flow_scalar;
pub(crate) mod resolve_props;
pub(crate) mod util;

use super::doc::directives::Directives;
use super::errors::YamlIssue;
use super::parse::cst::{Cst, Id};

/// how deep collections may nest before composing one is `RESOURCE_EXHAUSTION` (Node's stack gives out between
/// about 700 and 1500 levels, depending on the kind of nesting and on how deep the caller already is)
pub(crate) const MAX_DEPTH: usize = 1000;

/// where an error is: the `source` of `onError` (a number, a range, or a token)
#[derive(Debug, Clone, Copy)]
pub(crate) enum ErrAt {
    Offset(usize),
    Range(usize, usize),
    Token(Id),
}

/// the errors and warnings the composer gathers (`this.errors`, `this.warnings` of composer.ts)
#[derive(Debug, Default)]
pub(crate) struct Sink {
    pub errors: Vec<YamlIssue>,
    pub warnings: Vec<YamlIssue>,
}

impl Sink {
    /// `getErrorPos(src)`
    pub(crate) fn pos(cst: &Cst, at: ErrAt) -> (usize, usize) {
        match at {
            ErrAt::Offset(n) => (n, n + 1),
            ErrAt::Range(a, b) => (a, b),
            ErrAt::Token(id) => {
                let t = cst.tok(id);
                let offset = t.offset();
                (offset, offset + t.source().map_or(1, |s| s.len()))
            }
        }
    }
}

/// `ctx` of the composer (`atKey`, `atRoot`, the document's directives; options are the defaults: strict, unique
/// keys, no `stringKeys`, no compat schema), with its `onError`
pub(crate) struct Ctx<'a> {
    pub cst: &'a Cst,
    pub at_key: bool,
    pub at_root: bool,
    pub directives: &'a Directives,
    pub sink: &'a mut Sink,
    pub depth: usize,
}

impl Ctx<'_> {
    /// `onError(source, code, message)`
    pub(crate) fn error(&mut self, at: ErrAt, code: &str, message: impl Into<String>) {
        let pos = Sink::pos(self.cst, at);
        self.sink.errors.push(YamlIssue::new(pos, code, message));
    }
    /// `onError(source, code, message, true)`
    pub(crate) fn warn(&mut self, at: ErrAt, code: &str, message: impl Into<String>) {
        let pos = Sink::pos(self.cst, at);
        self.sink.warnings.push(YamlIssue::new(pos, code, message));
    }
}

/// a string of UTF-16 code units as text
pub(crate) fn text(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}
