//! The port of the yaml library's compose/compose-collection.ts: a block map, block sequence or flow collection
//! composed, and its tag applied (the core schema's map and seq, or a known tag: omap, pairs, set).

use super::resolve_props::Props;
use super::{Ctx, ErrAt};
use crate::js::Value;
use crate::yaml::nodes::Node;
use crate::yaml::parse::cst::{Id, Token, Ty};
use crate::yaml::schema::{Collection, MAP_TAG, SEQ_TAG, Tag, known_tag};

impl Ctx<'_> {
    fn resolve_collection(&mut self, token: Id, tag_name: Option<&str>) -> Node {
        let mut coll = match self.cst.ty(token) {
            Ty::BlockMap => self.resolve_block_map(token),
            Ty::BlockSeq => self.resolve_block_seq(token),
            _ => self.resolve_flow_collection(token),
        };
        // If we got a tagName matching the class, or the tag name is '!', then use the tagName from the node class
        // used to create it.
        let class_tag = if matches!(coll, Node::Map(_)) { MAP_TAG } else { SEQ_TAG };
        match tag_name {
            Some("!") => coll.set_tag(class_tag.to_string()),
            Some(name) => coll.set_tag(name.to_string()),
            None => {}
        }
        coll
    }

    pub(crate) fn compose_collection(&mut self, token: Id, props: &Props) -> Node {
        let cst = self.cst;
        let tag_token = props.tag;
        let directives = self.directives;
        let tag_name = tag_token.and_then(|t| {
            let mut msgs = Vec::new();
            let name = directives.tag_name(cst.source_of(t), |m| msgs.push(m));
            for m in msgs {
                self.error(ErrAt::Token(t), "TAG_RESOLVE_FAILED", m);
            }
            name
        });
        if cst.ty(token) == Ty::BlockSeq {
            let (anchor, nl) = (props.anchor, props.newline_after_prop);
            let offset = |id: Id| cst.tok(id).offset();
            let last_prop = match (anchor, tag_token) {
                (Some(a), Some(t)) => Some(if offset(a) > offset(t) { a } else { t }),
                (a, t) => a.or(t),
            };
            if let Some(lp) = last_prop
                && nl.is_none_or(|nl| offset(nl) < offset(lp))
            {
                self.error(ErrAt::Token(lp), "MISSING_CHAR", "Missing newline after block sequence props");
            }
        }
        let exp_type = match cst.tok(token) {
            Token::BlockMap { .. } => Collection::Map,
            Token::BlockSeq { .. } => Collection::Seq,
            Token::FlowCollection { start, .. } if cst.source_of(*start) == [b'{' as u16] => Collection::Map,
            _ => Collection::Seq,
        };
        // shortcut: check if it's a generic YAMLMap or YAMLSeq before jumping into the custom tag logic.
        let name = match (&tag_token, &tag_name) {
            (Some(_), Some(name)) => name.as_str(),
            _ => return self.resolve_collection(token, tag_name.as_deref()),
        };
        if name == "!"
            || (name == MAP_TAG && exp_type == Collection::Map)
            || (name == SEQ_TAG && exp_type == Collection::Seq)
        {
            return self.resolve_collection(token, Some(name));
        }
        let tag_at = ErrAt::Token(tag_token.unwrap_or(token));
        let tag = match known_tag(name) {
            Some(kt) if kt.collection() == Some(exp_type) => kt,
            kt => {
                match kt {
                    Some(kt) => self.warn(
                        tag_at,
                        "BAD_COLLECTION_TYPE",
                        format!(
                            "{} used for {} collection, but expects {}",
                            kt.name(),
                            exp_type.name(),
                            kt.collection().map_or("scalar", Collection::name)
                        ),
                    ),
                    None => self.warn(tag_at, "TAG_RESOLVE_FAILED", format!("Unresolved tag: {name}")),
                }
                return self.resolve_collection(token, Some(name));
            }
        };
        let coll = self.resolve_collection(token, Some(name));
        for message in resolve_known_collection(tag, &coll) {
            self.error(tag_at, "TAG_RESOLVE_FAILED", message);
        }
        coll
    }
}

/// what `resolve` of omap, pairs or set reports of a collection (schema/yaml-1.1/omap.ts, pairs.ts, set.ts)
fn resolve_known_collection(tag: Tag, coll: &Node) -> Vec<String> {
    let mut out = Vec::new();
    match (tag, coll) {
        (Tag::Pairs | Tag::Omap, Node::Seq(seq)) => {
            // resolvePairs: a map item is its one pair, any other item the key of a pair
            let mut keys: Vec<Option<(Value, bool)>> = Vec::new();
            for item in &seq.items {
                let key = match item {
                    Node::Map(m) => {
                        if m.items.len() > 1 {
                            out.push("Each pair must have its own sequence indicator".to_string());
                        }
                        match m.items.first() {
                            Some(p) => p.key.as_ref().and_then(|k| k.as_scalar()).map(|s| (s.value.clone(), s.opaque)),
                            None => Some((Value::Null, false)), // `new Pair(new Scalar(null))`
                        }
                    }
                    Node::Scalar(s) => Some((s.value.clone(), s.opaque)),
                    _ => None,
                };
                keys.push(key);
            }
            if tag == Tag::Omap {
                let mut seen: Vec<Value> = Vec::new();
                for (value, opaque) in keys.into_iter().flatten() {
                    // seenKeys.includes(key.value): SameValueZero
                    let same = |v: &Value| match (v, &value) {
                        (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
                        _ => v.strict_eq(&value),
                    };
                    if opaque {
                        continue;
                    }
                    if seen.iter().any(same) {
                        let shown = crate::js::to_js_string(&value);
                        out.push(format!("Ordered maps must not include duplicate keys: {shown}"));
                    } else {
                        seen.push(value);
                    }
                }
            }
        }
        (Tag::Pairs | Tag::Omap, _) => out.push("Expected a sequence for this tag".to_string()),
        (Tag::Set, Node::Map(map)) => {
            let all_null = map.items.iter().all(|p| match &p.value {
                None => true,
                Some(Node::Scalar(s)) => s.value.is_null() && !s.commented && s.tag.is_none(),
                Some(_) => false,
            });
            if !all_null {
                out.push("Set items must all have null values".to_string());
            }
        }
        (Tag::Set, _) => out.push("Expected a mapping for this tag".to_string()),
        _ => {}
    }
    out
}
