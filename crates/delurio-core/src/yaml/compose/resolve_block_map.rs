//! The port of the yaml library's compose/resolve-block-map.ts: a block mapping's pairs, with the errors of its keys
//! (indentation, implicit keys, duplicates).

use super::resolve_props::PropsOpts;
use super::util::{contains_newline, map_includes};
use super::{Ctx, ErrAt};
use crate::yaml::nodes::{Node, Pair, YamlMap};
use crate::yaml::parse::cst::{Id, Token, Ty};

const START_COL_MSG: &str = "All mapping items must start at the same column";

impl Ctx<'_> {
    pub(crate) fn resolve_block_map(&mut self, bm: Id) -> Node {
        let cst = self.cst;
        let mut map = YamlMap { items: Vec::new(), flow: false, range: [0; 3], anchor: None, tag: None };
        let Token::BlockMap { offset: bm_offset, indent: bm_indent, items } = cst.tok(bm) else {
            return Node::Map(map);
        };
        let (bm_offset, bm_indent) = (*bm_offset, *bm_indent);
        self.at_root = false;
        let mut offset = bm_offset;
        let mut comment_end: Option<usize> = None;
        for item in items {
            let (start, key, sep, value) = (&item.start[..], item.key, item.sep.as_deref(), item.value);
            // key properties
            let key_props = self.resolve_props(
                start,
                PropsOpts {
                    flow: None,
                    indicator: Ty::ExplicitKeyInd,
                    next: key.or_else(|| sep.and_then(|s| s.first().copied())),
                    offset,
                    parent_indent: bm_indent,
                    start_on_newline: true,
                },
            );
            let implicit_key = key_props.found.is_none();
            if implicit_key {
                if let Some(k) = key {
                    if cst.ty(k) == Ty::BlockSeq {
                        self.error(
                            ErrAt::Offset(offset),
                            "BLOCK_AS_IMPLICIT_KEY",
                            "A block sequence may not be used as an implicit map key",
                        );
                    } else if cst.tok(k).indent().is_some_and(|i| i != bm_indent) {
                        self.error(ErrAt::Offset(offset), "BAD_INDENT", START_COL_MSG);
                    }
                }
                if key_props.anchor.is_none() && key_props.tag.is_none() && sep.is_none() {
                    comment_end = Some(key_props.end);
                    continue;
                }
                if key_props.newline_after_prop.is_some() || contains_newline(cst, key) == Some(true) {
                    let at = key.or_else(|| start.last().copied());
                    if let Some(at) = at {
                        self.error(
                            ErrAt::Token(at),
                            "MULTILINE_IMPLICIT_KEY",
                            "Implicit keys need to be on a single line",
                        );
                    }
                }
            } else if key_props.found.and_then(|f| cst.tok(f).indent()) != Some(bm_indent) {
                self.error(ErrAt::Offset(offset), "BAD_INDENT", START_COL_MSG);
            }

            // key value
            self.at_key = true;
            let key_start = key_props.end;
            let key_node = match key {
                Some(k) => self.compose_node(k, &key_props),
                None => self.compose_empty_node(key_start, Some(start), None, &key_props),
            };
            self.at_key = false;
            if map_includes(&map.items, &key_node) {
                self.error(ErrAt::Offset(key_start), "DUPLICATE_KEY", "Map keys must be unique");
            }

            // value properties
            let value_props = self.resolve_props(
                sep.unwrap_or(&[]),
                PropsOpts {
                    flow: None,
                    indicator: Ty::MapValueInd,
                    next: value,
                    offset: key_node.range()[2],
                    parent_indent: bm_indent,
                    start_on_newline: key.is_none_or(|k| cst.ty(k) == Ty::BlockScalar),
                },
            );
            offset = value_props.end;
            if let Some(found) = value_props.found {
                if implicit_key {
                    if value.is_some_and(|v| cst.ty(v) == Ty::BlockMap) && !value_props.has_newline {
                        self.error(
                            ErrAt::Offset(offset),
                            "BLOCK_AS_IMPLICIT_KEY",
                            "Nested mappings are not allowed in compact mappings",
                        );
                    }
                    if (key_props.start as isize) < cst.tok(found).offset() as isize - 1024 {
                        let r = key_node.range();
                        self.error(
                            ErrAt::Range(r[0], r[1]),
                            "KEY_OVER_1024_CHARS",
                            "The : indicator must be at most 1024 chars after the start of an implicit block mapping key",
                        );
                    }
                }
                // value value
                let value_node = match value {
                    Some(v) => self.compose_node(v, &value_props),
                    None => self.compose_empty_node(offset, sep, None, &value_props),
                };
                offset = value_node.range()[2];
                map.items.push(Pair { key: Some(key_node), value: Some(value_node) });
            } else {
                // key with no value
                let mut key_node = key_node;
                if implicit_key {
                    let r = key_node.range();
                    self.error(
                        ErrAt::Range(r[0], r[1]),
                        "MISSING_CHAR",
                        "Implicit map keys need to be followed by map values",
                    );
                }
                if !value_props.comment.is_empty() {
                    key_node.set_commented();
                }
                map.items.push(Pair { key: Some(key_node), value: None });
            }
        }
        if let Some(ce) = comment_end
            && ce != 0
            && ce < offset
        {
            self.error(ErrAt::Offset(ce), "IMPOSSIBLE", "Map comment with trailing content");
        }
        map.range = [bm_offset, offset, comment_end.unwrap_or(offset)];
        Node::Map(map)
    }
}
