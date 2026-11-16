//! The port of the yaml library's compose/compose-scalar.ts: a scalar's value resolved by its tag, or for a plain
//! scalar without one by the first core tag whose test it passes (null, bool, int, float), else a string.

use super::{Ctx, ErrAt};
use crate::js::Value;
use crate::yaml::nodes::{Node, Scalar};
use crate::yaml::parse::cst::{Id, Ty};
use crate::yaml::schema::{CORE, STR_TAG, Tag, known_tag};

/// the token a scalar is composed from: one of the tree's, or the empty plain scalar `composeEmptyNode` makes
#[derive(Debug, Clone, Copy)]
pub(crate) enum ScalarSrc {
    Token(Id),
    Empty(usize),
}

impl Ctx<'_> {
    pub(crate) fn compose_scalar(&mut self, src: ScalarSrc, tag_token: Option<Id>) -> Node {
        let cst = self.cst;
        let (value, comment, range, plain, at) = match src {
            ScalarSrc::Token(id) if cst.ty(id) == Ty::BlockScalar => {
                let r = self.resolve_block_scalar(id);
                (r.value, r.comment, r.range, false, ErrAt::Token(id))
            }
            ScalarSrc::Token(id) => {
                let r = self.resolve_flow_scalar(id);
                (r.value, r.comment, r.range, cst.ty(id) == Ty::Scalar, ErrAt::Token(id))
            }
            // resolveFlowScalar of `{ type: 'scalar', offset, source: '' }`
            ScalarSrc::Empty(o) => (Vec::new(), false, [o, o, o], true, ErrAt::Range(o, o)),
        };
        let value = super::text(&value);
        let directives = self.directives;
        let tag_name = tag_token.and_then(|t| {
            let mut msgs = Vec::new();
            let name = directives.tag_name(cst.source_of(t), |m| msgs.push(m));
            for m in msgs {
                self.error(ErrAt::Token(t), "TAG_RESOLVE_FAILED", m);
            }
            name
        });
        let tag = if let Some(name) = &tag_name {
            self.find_scalar_tag_by_name(&value, name, tag_token)
        } else if plain {
            find_scalar_tag_by_test(&value)
        } else {
            Tag::Str
        };
        let at = tag_token.map_or(at, ErrAt::Token);
        let (resolved, opaque) = match tag.resolve(&value) {
            Ok(r) => (r.value, r.opaque),
            Err(message) => {
                self.error(at, "TAG_RESOLVE_FAILED", message);
                (Value::String(value), false)
            }
        };
        Node::Scalar(Scalar { value: resolved, range, anchor: None, tag: tag_name, opaque, commented: comment })
    }

    fn find_scalar_tag_by_name(&mut self, value: &str, tag_name: &str, tag_token: Option<Id>) -> Tag {
        if tag_name == "!" {
            return Tag::Str; // non-specific tag
        }
        let mut match_with_test = Vec::new();
        for tag in CORE {
            if tag.collection().is_none() && tag.name() == tag_name {
                if tag.is_default() && tag.has_test() {
                    match_with_test.push(tag);
                } else {
                    return tag;
                }
            }
        }
        if let Some(tag) = match_with_test.into_iter().find(|t| t.test(value)) {
            return tag;
        }
        if let Some(kt) = known_tag(tag_name)
            && kt.collection().is_none()
        {
            return kt;
        }
        let message = format!("Unresolved tag: {tag_name}");
        let at = ErrAt::Token(tag_token.unwrap_or(0));
        if tag_name != STR_TAG {
            self.warn(at, "TAG_RESOLVE_FAILED", message);
        } else {
            self.error(at, "TAG_RESOLVE_FAILED", message);
        }
        Tag::Str
    }
}

/// `findScalarTagByTest`: the first default core tag whose test the value passes, else a string
fn find_scalar_tag_by_test(value: &str) -> Tag {
    CORE.into_iter().find(|t| t.is_default() && t.test(value)).unwrap_or(Tag::Str)
}
