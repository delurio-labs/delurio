//! The port of the yaml library's compose/util-contains-newline.ts, util-empty-scalar-position.ts and
//! util-map-includes.ts (util-flow-indent-check.ts only runs with a compat schema, which delurio does not use).

use crate::yaml::nodes::{Node, Pair, same_scalar_value};
use crate::yaml::parse::cst::{Cst, Id, Token, Ty};

/// `containsNewline(key)`: whether a key spans lines; None for no key (`null`). Walked with a stack of its own, the
/// library's recursion overflowing on a deeply nested key.
pub(crate) fn contains_newline(cst: &Cst, key: Option<Id>) -> Option<bool> {
    let key = key?;
    let newline_in = |list: &[Id]| list.iter().any(|&t| cst.ty(t) == Ty::Newline);
    let mut stack = vec![key];
    while let Some(k) = stack.pop() {
        match cst.tok(k) {
            Token::FlowScalar { source, end, .. } => {
                if cst.units(*source).contains(&(b'\n' as u16)) {
                    return Some(true);
                }
                if end.as_ref().is_some_and(|e| newline_in(e)) {
                    return Some(true);
                }
            }
            Token::FlowCollection { items, .. } => {
                for it in items {
                    if newline_in(&it.start) || it.sep.as_ref().is_some_and(|s| newline_in(s)) {
                        return Some(true);
                    }
                    stack.extend(it.key);
                    stack.extend(it.value);
                }
            }
            _ => return Some(true),
        }
    }
    Some(false)
}

/// `emptyScalarPosition(offset, before, pos)`: where an empty scalar is, after the last token before it that is not
/// a space, comment or newline, and after any spaces that follow that token
pub(crate) fn empty_scalar_position(cst: &Cst, offset: usize, before: Option<&[Id]>, pos: Option<usize>) -> usize {
    let mut offset = offset as isize;
    if let Some(before) = before {
        let pos = pos.unwrap_or(before.len());
        let mut i = pos as isize - 1;
        while i >= 0 {
            let st = before[i as usize];
            match cst.ty(st) {
                Ty::Space | Ty::Comment | Ty::Newline => {
                    offset -= cst.source_of(st).len() as isize;
                    i -= 1;
                    continue;
                }
                _ => {}
            }
            // Technically, an empty scalar is immediately after the last non-empty node, but it's more useful to
            // place it after any whitespace.
            i += 1;
            while let Some(&st) = before.get(i as usize) {
                if cst.ty(st) != Ty::Space {
                    break;
                }
                offset += cst.source_of(st).len() as isize;
                i += 1;
            }
            break;
        }
    }
    offset.max(0) as usize
}

/// `mapIncludes(ctx, items, search)`: whether a map already has the key (with `uniqueKeys`: an equal scalar value)
pub(crate) fn map_includes(items: &[Pair], search: &Node) -> bool {
    let Node::Scalar(b) = search else { return false };
    items.iter().any(|pair| matches!(&pair.key, Some(Node::Scalar(a)) if same_scalar_value(a, b)))
}
