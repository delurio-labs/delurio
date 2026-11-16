//! The port of the yaml library's compose/resolve-props.ts: the tokens before a node read for its properties (anchor,
//! tag, the indicator before it, comments, newlines), with the errors they hold.

use super::{Ctx, ErrAt};
use crate::yaml::parse::cst::{Id, Ty};

/// the options of `resolveProps`
#[derive(Debug, Clone, Copy)]
pub(crate) struct PropsOpts<'f> {
    pub flow: Option<&'f str>,
    pub indicator: Ty,
    pub next: Option<Id>,
    pub offset: usize,
    pub parent_indent: i64,
    pub start_on_newline: bool,
}

/// what `resolveProps` returns
#[derive(Debug, Clone, Default)]
pub(crate) struct Props {
    pub comma: Option<Id>,
    pub found: Option<Id>,
    pub comment: Vec<u16>,
    pub has_newline: bool,
    pub anchor: Option<Id>,
    pub tag: Option<Id>,
    pub newline_after_prop: Option<Id>,
    pub end: usize,
    pub start: usize,
}

impl Ctx<'_> {
    pub(crate) fn resolve_props(&mut self, tokens: &[Id], o: PropsOpts) -> Props {
        let cst = self.cst;
        let mut at_newline = o.start_on_newline;
        let mut has_space = o.start_on_newline;
        let mut comment: Vec<u16> = Vec::new();
        let mut comment_sep: Vec<u16> = Vec::new();
        let mut has_newline = false;
        let mut req_space = false;
        let mut tab: Option<Id> = None;
        let mut anchor: Option<Id> = None;
        let mut tag: Option<Id> = None;
        let mut newline_after_prop: Option<Id> = None;
        let mut comma: Option<Id> = None;
        let mut found: Option<Id> = None;
        let mut start: Option<usize> = None;
        for &token in tokens {
            let ty = cst.ty(token);
            let offset = cst.tok(token).offset();
            let source = cst.source_of(token);
            if req_space {
                if ty != Ty::Space && ty != Ty::Newline && ty != Ty::Comma {
                    self.error(
                        ErrAt::Offset(offset),
                        "MISSING_CHAR",
                        "Tags and anchors must be separated from the next token by white space",
                    );
                }
                req_space = false;
            }
            if let Some(t) = tab {
                if at_newline && ty != Ty::Comment && ty != Ty::Newline {
                    self.error(ErrAt::Token(t), "TAB_AS_INDENT", "Tabs are not allowed as indentation");
                }
                tab = None;
            }
            match ty {
                Ty::Space => {
                    // At the doc level, tabs at line start may be parsed as leading white space rather than
                    // indentation. In a flow collection, only the parser handles indent.
                    if o.flow.is_none()
                        && (o.indicator != Ty::DocStart || o.next.map(|n| cst.ty(n)) != Some(Ty::FlowCollection))
                        && source.contains(&(b'\t' as u16))
                    {
                        tab = Some(token);
                    }
                    has_space = true;
                }
                Ty::Comment => {
                    if !has_space {
                        self.error(
                            ErrAt::Token(token),
                            "MISSING_CHAR",
                            "Comments must be separated from other tokens by white space characters",
                        );
                    }
                    let cb: &[u16] = match source.get(1..) {
                        Some(c) if !c.is_empty() => c,
                        _ => &[b' ' as u16],
                    };
                    if comment.is_empty() {
                        comment = cb.to_vec();
                    } else {
                        comment.extend_from_slice(&comment_sep);
                        comment.extend_from_slice(cb);
                    }
                    comment_sep.clear();
                    at_newline = false;
                }
                Ty::Newline => {
                    if at_newline {
                        if !comment.is_empty() {
                            comment.extend_from_slice(source);
                        }
                    } else {
                        comment_sep.extend_from_slice(source);
                    }
                    at_newline = true;
                    has_newline = true;
                    if anchor.is_some() || tag.is_some() {
                        newline_after_prop = Some(token);
                    }
                    has_space = true;
                }
                Ty::Anchor => {
                    if anchor.is_some() {
                        self.error(ErrAt::Token(token), "MULTIPLE_ANCHORS", "A node can have at most one anchor");
                    }
                    if source.last() == Some(&(b':' as u16)) {
                        self.warn(
                            ErrAt::Offset(offset + source.len() - 1),
                            "BAD_ALIAS",
                            "Anchor ending in : is ambiguous",
                        );
                    }
                    anchor = Some(token);
                    start.get_or_insert(offset);
                    at_newline = false;
                    has_space = false;
                    req_space = true;
                }
                Ty::Tag => {
                    if tag.is_some() {
                        self.error(ErrAt::Token(token), "MULTIPLE_TAGS", "A node can have at most one tag");
                    }
                    tag = Some(token);
                    start.get_or_insert(offset);
                    at_newline = false;
                    has_space = false;
                    req_space = true;
                }
                t if t == o.indicator => {
                    // Could here handle preceding comments differently
                    let s = super::text(source);
                    if anchor.is_some() || tag.is_some() {
                        self.error(
                            ErrAt::Token(token),
                            "BAD_PROP_ORDER",
                            format!("Anchors and tags must be after the {s} indicator"),
                        );
                    }
                    if found.is_some() {
                        self.error(
                            ErrAt::Token(token),
                            "UNEXPECTED_TOKEN",
                            format!("Unexpected {s} in {}", o.flow.unwrap_or("collection")),
                        );
                    }
                    found = Some(token);
                    at_newline = o.indicator == Ty::SeqItemInd || o.indicator == Ty::ExplicitKeyInd;
                    has_space = false;
                }
                Ty::Comma if o.flow.is_some() => {
                    if comma.is_some() {
                        let flow = o.flow.unwrap_or_default();
                        self.error(ErrAt::Token(token), "UNEXPECTED_TOKEN", format!("Unexpected , in {flow}"));
                    }
                    comma = Some(token);
                    at_newline = false;
                    has_space = false;
                }
                _ => {
                    self.error(ErrAt::Token(token), "UNEXPECTED_TOKEN", format!("Unexpected {} token", ty.name()));
                    at_newline = false;
                    has_space = false;
                }
            }
        }
        let end = match tokens.last() {
            Some(&last) => cst.tok(last).offset() + cst.source_of(last).len(),
            None => o.offset,
        };
        if req_space && let Some(next) = o.next {
            let nt = cst.ty(next);
            if nt != Ty::Space
                && nt != Ty::Newline
                && nt != Ty::Comma
                && (nt != Ty::Scalar || !cst.source_of(next).is_empty())
            {
                self.error(
                    ErrAt::Offset(cst.tok(next).offset()),
                    "MISSING_CHAR",
                    "Tags and anchors must be separated from the next token by white space",
                );
            }
        }
        if let Some(t) = tab {
            let next_is_block = matches!(o.next.map(|n| cst.ty(n)), Some(Ty::BlockMap | Ty::BlockSeq));
            if (at_newline && cst.tok(t).indent().unwrap_or(0) <= o.parent_indent) || next_is_block {
                self.error(ErrAt::Token(t), "TAB_AS_INDENT", "Tabs are not allowed as indentation");
            }
        }
        Props { comma, found, comment, has_newline, anchor, tag, newline_after_prop, end, start: start.unwrap_or(end) }
    }
}
