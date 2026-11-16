//! The port of the yaml library's compose/compose-node.ts: a token composed into a node, with its anchor; an empty
//! node where there is no token; an alias.

use super::resolve_props::Props;
use super::util::empty_scalar_position;
use super::{Ctx, ErrAt, MAX_DEPTH};
use crate::yaml::compose::compose_scalar::ScalarSrc;
use crate::yaml::nodes::{Alias, Node};
use crate::yaml::parse::cst::{Id, Token, Ty};

impl Ctx<'_> {
    pub(crate) fn compose_node(&mut self, token: Id, props: &Props) -> Node {
        let cst = self.cst;
        let anchor = props.anchor;
        let anchor_name = || anchor.map(|a| super::text(cst.source_of(a).get(1..).unwrap_or(&[])));
        let mut node: Option<Node> = None;
        match cst.ty(token) {
            Ty::Alias => {
                node = Some(self.compose_alias(token));
                if anchor.is_some() || props.tag.is_some() {
                    self.error(ErrAt::Token(token), "ALIAS_PROPS", "An alias node must not specify any properties");
                }
            }
            Ty::Scalar | Ty::SingleQuotedScalar | Ty::DoubleQuotedScalar | Ty::BlockScalar => {
                let mut n = self.compose_scalar(ScalarSrc::Token(token), props.tag);
                if let Some(a) = anchor_name() {
                    n.set_anchor(a);
                }
                node = Some(n);
            }
            Ty::BlockMap | Ty::BlockSeq | Ty::FlowCollection => {
                if self.depth >= MAX_DEPTH {
                    // Almost certainly here due to a stack overflow
                    self.error(ErrAt::Token(token), "RESOURCE_EXHAUSTION", "Maximum call stack size exceeded");
                } else {
                    self.depth += 1;
                    let mut n = self.compose_collection(token, props);
                    self.depth -= 1;
                    if let Some(a) = anchor_name() {
                        n.set_anchor(a);
                    }
                    node = Some(n);
                }
            }
            ty => {
                let message = match cst.tok(token) {
                    Token::Error { message, .. } => message.clone(),
                    _ => format!("Unsupported token (type: {})", ty.name()),
                };
                self.error(ErrAt::Token(token), "UNEXPECTED_TOKEN", message);
            }
        }
        let mut node = match node {
            Some(n) => n,
            None => self.compose_empty_node(cst.tok(token).offset(), None, None, props),
        };
        if let Some(a) = anchor
            && node.anchor() == Some("")
        {
            self.error(ErrAt::Token(a), "BAD_ALIAS", "Anchor cannot be an empty string");
        }
        if !props.comment.is_empty() {
            node.set_commented();
        }
        node
    }

    pub(crate) fn compose_empty_node(
        &mut self,
        offset: usize,
        before: Option<&[Id]>,
        pos: Option<usize>,
        props: &Props,
    ) -> Node {
        let cst = self.cst;
        let at = empty_scalar_position(cst, offset, before, pos);
        let mut node = self.compose_scalar(ScalarSrc::Empty(at), props.tag);
        if let Some(a) = props.anchor {
            let name = super::text(cst.source_of(a).get(1..).unwrap_or(&[]));
            let empty = name.is_empty();
            node.set_anchor(name);
            if empty {
                self.error(ErrAt::Token(a), "BAD_ALIAS", "Anchor cannot be an empty string");
            }
        }
        if !props.comment.is_empty() {
            node.set_commented();
            node.range_mut()[2] = props.end;
        }
        node
    }

    fn compose_alias(&mut self, token: Id) -> Node {
        let cst = self.cst;
        let t = cst.tok(token);
        let (offset, source) = (t.offset(), cst.source_of(token));
        let name = super::text(source.get(1..).unwrap_or(&[]));
        if name.is_empty() {
            self.error(ErrAt::Offset(offset), "BAD_ALIAS", "Alias cannot be an empty string");
        }
        if name.ends_with(':') {
            self.warn(ErrAt::Offset(offset + source.len() - 1), "BAD_ALIAS", "Alias ending in : is ambiguous");
        }
        let value_end = offset + source.len();
        let (_, end) = self.resolve_end(t.end().map(|e| &e[..]), value_end, true);
        Node::Alias(Alias { source: name, range: [offset, value_end, end] })
    }
}
