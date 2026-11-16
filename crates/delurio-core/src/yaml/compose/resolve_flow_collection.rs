//! The port of the yaml library's compose/resolve-flow-collection.ts: a flow mapping's pairs or a flow sequence's
//! items (a pair inside a flow sequence being a one-pair flow map), with the errors of commas, colons and the end.

use super::resolve_props::PropsOpts;
use super::util::{contains_newline, map_includes};
use super::{Ctx, ErrAt};
use crate::yaml::nodes::{Node, Pair, YamlMap, YamlSeq};
use crate::yaml::parse::cst::{Id, Token, Ty};

const BLOCK_MSG: &str = "Block collections are not allowed within flow collections";

impl Ctx<'_> {
    pub(crate) fn resolve_flow_collection(&mut self, fc: Id) -> Node {
        let cst = self.cst;
        let Token::FlowCollection { offset: fc_offset, indent: fc_indent, start: fc_start, items, end: fc_end } =
            cst.tok(fc)
        else {
            return Node::Seq(YamlSeq { items: Vec::new(), flow: true, range: [0; 3], anchor: None, tag: None });
        };
        let (fc_offset, fc_indent) = (*fc_offset, *fc_indent);
        let is_block = |t: Option<Id>| t.is_some_and(|t| matches!(cst.ty(t), Ty::BlockMap | Ty::BlockSeq));
        let start_source = cst.source_of(*fc_start);
        let is_map = start_source == [b'{' as u16];
        let fc_name = if is_map { "flow map" } else { "flow sequence" };
        let mut pairs: Vec<Pair> = Vec::new(); // the items of a flow map
        let mut nodes: Vec<Node> = Vec::new(); // the items of a flow sequence
        let at_root = self.at_root;
        self.at_root = false;
        self.at_key = false;
        let mut offset = fc_offset + start_source.len();
        for (i, item) in items.iter().enumerate() {
            let (start, key, sep, value) = (&item.start[..], item.key, item.sep.as_deref(), item.value);
            let mut props = self.resolve_props(
                start,
                PropsOpts {
                    flow: Some(fc_name),
                    indicator: Ty::ExplicitKeyInd,
                    next: key.or_else(|| sep.and_then(|s| s.first().copied())),
                    offset,
                    parent_indent: fc_indent,
                    start_on_newline: false,
                },
            );
            if props.found.is_none() {
                if props.anchor.is_none() && props.tag.is_none() && sep.is_none() && value.is_none() {
                    if i == 0 && props.comma.is_some() {
                        if let Some(c) = props.comma {
                            self.error(ErrAt::Token(c), "UNEXPECTED_TOKEN", format!("Unexpected , in {fc_name}"));
                        }
                    } else if i + 1 < items.len() {
                        self.error(
                            ErrAt::Offset(props.start),
                            "UNEXPECTED_TOKEN",
                            format!("Unexpected empty item in {fc_name}"),
                        );
                    }
                    offset = props.end;
                    continue;
                }
                if !is_map
                    && contains_newline(cst, key) == Some(true)
                    && let Some(k) = key
                {
                    self.error(
                        ErrAt::Token(k),
                        "MULTILINE_IMPLICIT_KEY",
                        "Implicit keys of flow sequence pairs need to be on a single line",
                    );
                }
            }
            if i == 0 {
                if let Some(c) = props.comma {
                    self.error(ErrAt::Token(c), "UNEXPECTED_TOKEN", format!("Unexpected , in {fc_name}"));
                }
            } else {
                if props.comma.is_none() {
                    self.error(
                        ErrAt::Offset(props.start),
                        "MISSING_CHAR",
                        format!("Missing , between {fc_name} items"),
                    );
                }
                if !props.comment.is_empty() {
                    let mut prev_item_comment: &[u16] = &[];
                    for &st in start {
                        match cst.ty(st) {
                            Ty::Comma | Ty::Space => {}
                            Ty::Comment => {
                                prev_item_comment = cst.source_of(st).get(1..).unwrap_or(&[]);
                                break;
                            }
                            _ => break,
                        }
                    }
                    if !prev_item_comment.is_empty() {
                        let prev = if is_map {
                            pairs.last_mut().and_then(|p| match p.value.as_mut() {
                                Some(v) => Some(v),
                                None => p.key.as_mut(),
                            })
                        } else {
                            nodes.last_mut()
                        };
                        if let Some(prev) = prev {
                            prev.set_commented();
                        }
                        let cut = (prev_item_comment.len() + 1).min(props.comment.len());
                        props.comment.drain(..cut);
                    }
                }
            }
            if !is_map && sep.is_none() && props.found.is_none() {
                // item is a value in a seq
                // → key & sep are empty, start does not include ? or :
                let value_node = match value {
                    Some(v) => self.compose_node(v, &props),
                    None => self.compose_empty_node(props.end, sep, None, &props),
                };
                let r = value_node.range();
                nodes.push(value_node);
                offset = r[2];
                if is_block(value) {
                    self.error(ErrAt::Range(r[0], r[1]), "BLOCK_IN_FLOW", BLOCK_MSG);
                }
            } else {
                // item is a key+value pair

                // key value
                self.at_key = true;
                let key_start = props.end;
                let mut key_node = match key {
                    Some(k) => self.compose_node(k, &props),
                    None => self.compose_empty_node(key_start, Some(start), None, &props),
                };
                if is_block(key) {
                    let r = key_node.range();
                    self.error(ErrAt::Range(r[0], r[1]), "BLOCK_IN_FLOW", BLOCK_MSG);
                }
                self.at_key = false;

                // value properties
                let value_props = self.resolve_props(
                    sep.unwrap_or(&[]),
                    PropsOpts {
                        flow: Some(fc_name),
                        indicator: Ty::MapValueInd,
                        next: value,
                        offset: key_node.range()[2],
                        parent_indent: fc_indent,
                        start_on_newline: false,
                    },
                );
                if let Some(found) = value_props.found {
                    if !is_map && props.found.is_none() {
                        if let Some(sep) = sep {
                            for &st in sep {
                                if st == found {
                                    break;
                                }
                                if cst.ty(st) == Ty::Newline {
                                    self.error(
                                        ErrAt::Token(st),
                                        "MULTILINE_IMPLICIT_KEY",
                                        "Implicit keys of flow sequence pairs need to be on a single line",
                                    );
                                    break;
                                }
                            }
                        }
                        if (props.start as isize) < cst.tok(found).offset() as isize - 1024 {
                            self.error(
                                ErrAt::Token(found),
                                "KEY_OVER_1024_CHARS",
                                "The : indicator must be at most 1024 chars after the start of an implicit flow sequence key",
                            );
                        }
                    }
                } else if let Some(v) = value {
                    if cst.tok(v).source().is_some() && cst.source_of(v).first() == Some(&(b':' as u16)) {
                        self.error(ErrAt::Token(v), "MISSING_CHAR", format!("Missing space after : in {fc_name}"));
                    } else {
                        self.error(
                            ErrAt::Offset(value_props.start),
                            "MISSING_CHAR",
                            format!("Missing , or : between {fc_name} items"),
                        );
                    }
                }

                // value value
                let value_node = match value {
                    Some(v) => Some(self.compose_node(v, &value_props)),
                    None if value_props.found.is_some() => {
                        Some(self.compose_empty_node(value_props.end, sep, None, &value_props))
                    }
                    None => None,
                };
                match &value_node {
                    Some(vn) => {
                        if is_block(value) {
                            let r = vn.range();
                            self.error(ErrAt::Range(r[0], r[1]), "BLOCK_IN_FLOW", BLOCK_MSG);
                        }
                    }
                    None => {
                        if !value_props.comment.is_empty() {
                            key_node.set_commented();
                        }
                    }
                }
                let next_offset = value_node.as_ref().map_or(value_props.end, |v| v.range()[2]);
                if is_map {
                    if map_includes(&pairs, &key_node) {
                        self.error(ErrAt::Offset(key_start), "DUPLICATE_KEY", "Map keys must be unique");
                    }
                    pairs.push(Pair { key: Some(key_node), value: value_node });
                } else {
                    let end_range = value_node.as_ref().map_or(key_node.range(), |v| v.range());
                    let range = [key_node.range()[0], end_range[1], end_range[2]];
                    nodes.push(Node::Map(YamlMap {
                        items: vec![Pair { key: Some(key_node), value: value_node }],
                        flow: true,
                        range,
                        anchor: None,
                        tag: None,
                    }));
                }
                offset = next_offset;
            }
        }

        let expected_end: u16 = if is_map { b'}' as u16 } else { b']' as u16 };
        let mut ee: Vec<Id> = fc_end.get(1..).unwrap_or(&[]).to_vec();
        let ce = fc_end.first().copied();
        let mut ce_pos = offset;
        match ce {
            Some(ce) if cst.source_of(ce) == [expected_end] => {
                ce_pos = cst.tok(ce).offset() + cst.source_of(ce).len();
            }
            _ => {
                let name = if is_map { "Flow map" } else { "Flow sequence" };
                let end_char = expected_end as u8 as char;
                let msg = if at_root {
                    format!("{name} must end with a {end_char}")
                } else {
                    format!("{name} in block collection must be sufficiently indented and end with a {end_char}")
                };
                self.error(ErrAt::Offset(offset), if at_root { "MISSING_CHAR" } else { "BAD_INDENT" }, msg);
                if let Some(ce) = ce
                    && cst.source_of(ce).len() != 1
                {
                    ee.insert(0, ce);
                }
            }
        }
        let range = if !ee.is_empty() {
            let (_, end_offset) = self.resolve_end(Some(&ee), ce_pos, true);
            [fc_offset, ce_pos, end_offset]
        } else {
            [fc_offset, ce_pos, ce_pos]
        };
        if is_map {
            Node::Map(YamlMap { items: pairs, flow: true, range, anchor: None, tag: None })
        } else {
            Node::Seq(YamlSeq { items: nodes, flow: true, range, anchor: None, tag: None })
        }
    }
}
