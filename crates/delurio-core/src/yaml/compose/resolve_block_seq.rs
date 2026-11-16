//! The port of the yaml library's compose/resolve-block-seq.ts: a block sequence's items.

use super::resolve_props::PropsOpts;
use super::{Ctx, ErrAt};
use crate::yaml::nodes::{Node, YamlSeq};
use crate::yaml::parse::cst::{Id, Token, Ty};

impl Ctx<'_> {
    pub(crate) fn resolve_block_seq(&mut self, bs: Id) -> Node {
        let cst = self.cst;
        let mut seq = YamlSeq { items: Vec::new(), flow: false, range: [0; 3], anchor: None, tag: None };
        let Token::BlockSeq { offset: bs_offset, indent: bs_indent, items } = cst.tok(bs) else {
            return Node::Seq(seq);
        };
        self.at_root = false;
        self.at_key = false;
        let mut offset = *bs_offset;
        let mut comment_end: Option<usize> = None;
        for item in items {
            let (start, value) = (&item.start[..], item.value);
            let props = self.resolve_props(
                start,
                PropsOpts {
                    flow: None,
                    indicator: Ty::SeqItemInd,
                    next: value,
                    offset,
                    parent_indent: *bs_indent,
                    start_on_newline: true,
                },
            );
            if props.found.is_none() {
                if props.anchor.is_some() || props.tag.is_some() || value.is_some() {
                    if value.is_some_and(|v| cst.ty(v) == Ty::BlockSeq) {
                        self.error(
                            ErrAt::Offset(props.end),
                            "BAD_INDENT",
                            "All sequence items must start at the same column",
                        );
                    } else {
                        self.error(ErrAt::Offset(offset), "MISSING_CHAR", "Sequence item without - indicator");
                    }
                } else {
                    comment_end = Some(props.end);
                    continue;
                }
            }
            let node = match value {
                Some(v) => self.compose_node(v, &props),
                None => self.compose_empty_node(props.end, Some(start), None, &props),
            };
            offset = node.range()[2];
            seq.items.push(node);
        }
        seq.range = [*bs_offset, offset, comment_end.unwrap_or(offset)];
        Node::Seq(seq)
    }
}
