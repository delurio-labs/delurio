//! The port of the yaml library's compose/resolve-end.ts: the tokens after a node on its line (spaces, a comment, the
//! newline) read for its comment and where it ends.

use super::{Ctx, ErrAt};
use crate::yaml::parse::cst::{Id, Ty};

impl Ctx<'_> {
    /// `resolveEnd(end, offset, reqSpace, onError)`: the comment and the offset after the tokens
    pub(crate) fn resolve_end(&mut self, end: Option<&[Id]>, mut offset: usize, req_space: bool) -> (Vec<u16>, usize) {
        let cst = self.cst;
        let mut comment: Vec<u16> = Vec::new();
        if let Some(end) = end {
            let mut has_space = false;
            let mut sep: Vec<u16> = Vec::new();
            for &token in end {
                let source = cst.source_of(token);
                match cst.ty(token) {
                    Ty::Space => has_space = true,
                    Ty::Comment => {
                        if req_space && !has_space {
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
                            comment.extend_from_slice(&sep);
                            comment.extend_from_slice(cb);
                        }
                        sep.clear();
                    }
                    Ty::Newline => {
                        if !comment.is_empty() {
                            sep.extend_from_slice(source);
                        }
                        has_space = true;
                    }
                    ty => self.error(
                        ErrAt::Token(token),
                        "UNEXPECTED_TOKEN",
                        format!("Unexpected {} at node end", ty.name()),
                    ),
                }
                offset += source.len();
            }
        }
        (comment, offset)
    }
}
