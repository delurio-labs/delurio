//! The port of the yaml library's compose/resolve-flow-scalar.ts: the value of a plain, single-quoted or
//! double-quoted scalar, its lines folded and its escapes read. Values are built in UTF-16 code units, as the library
//! builds JavaScript strings (an escape may give half of a surrogate pair).

use super::{Ctx, ErrAt};
use crate::yaml::parse::cst::{Id, Token, Ty};

const SP: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const NL: u16 = b'\n' as u16;
const CR: u16 = b'\r' as u16;

/// what a flow scalar resolves to: its value, whether a comment follows it, its range
pub(crate) struct FlowScalar {
    pub value: Vec<u16>,
    pub comment: bool,
    pub range: [usize; 3],
}

impl Ctx<'_> {
    pub(crate) fn resolve_flow_scalar(&mut self, scalar: Id) -> FlowScalar {
        let cst = self.cst;
        let (ty, offset, end) = match cst.tok(scalar) {
            Token::FlowScalar { ty, offset, end, .. } => (*ty, *offset, end.as_deref()),
            t => (t.ty(), t.offset(), None),
        };
        let source = cst.source_of(scalar);
        let mut errors: Vec<(usize, &'static str, String)> = Vec::new();
        let mut on_error = |rel: usize, code: &'static str, msg: String| errors.push((offset + rel, code, msg));
        let value = match ty {
            Ty::Scalar => plain_value(source, &mut on_error),
            Ty::SingleQuotedScalar => single_quoted_value(source, &mut on_error),
            Ty::DoubleQuotedScalar => double_quoted_value(source, &mut on_error),
            _ => {
                self.error(
                    ErrAt::Token(scalar),
                    "UNEXPECTED_TOKEN",
                    format!("Expected a flow scalar value, but found: {}", ty.name()),
                );
                let e = offset + source.len();
                return FlowScalar { value: Vec::new(), comment: false, range: [offset, e, e] };
            }
        };
        for (at, code, msg) in errors {
            self.error(ErrAt::Offset(at), code, msg);
        }
        let value_end = offset + source.len();
        let (comment, re_offset) = self.resolve_end(end, value_end, true);
        FlowScalar { value, comment: !comment.is_empty(), range: [offset, value_end, re_offset] }
    }
}

fn plain_value(source: &[u16], on_error: &mut impl FnMut(usize, &'static str, String)) -> Vec<u16> {
    let bad_char = match source.first().copied() {
        Some(TAB) => Some("a tab character".to_string()),
        Some(0x2c) => Some("flow indicator character ,".to_string()),
        Some(0x25) => Some("directive indicator character %".to_string()),
        Some(c @ (0x7c | 0x3e)) => Some(format!("block scalar indicator {}", c as u8 as char)),
        Some(c @ (0x40 | 0x60)) => Some(format!("reserved character {}", c as u8 as char)),
        _ => None,
    };
    if let Some(bad) = bad_char {
        on_error(0, "BAD_SCALAR_START", format!("Plain value cannot start with {bad}"));
    }
    unfold_lines(source)
}

fn single_quoted_value(source: &[u16], on_error: &mut impl FnMut(usize, &'static str, String)) -> Vec<u16> {
    if source.last() != Some(&(b'\'' as u16)) || source.len() == 1 {
        on_error(source.len(), "MISSING_CHAR", "Missing closing 'quote".to_string());
    }
    // source.slice(1, -1)
    let inner = if source.len() >= 2 { &source[1..source.len() - 1] } else { &[][..] };
    let unfolded = unfold_lines(inner);
    // .replace(/''/g, "'")
    let mut out = Vec::with_capacity(unfolded.len());
    let mut i = 0;
    while i < unfolded.len() {
        out.push(unfolded[i]);
        if unfolded[i] == b'\'' as u16 && unfolded.get(i + 1) == Some(&(b'\'' as u16)) {
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

fn is_ws(c: u16) -> bool {
    c == SP || c == TAB
}

fn trim_end_ws(s: &[u16]) -> &[u16] {
    let mut e = s.len();
    while e > 0 && is_ws(s[e - 1]) {
        e -= 1;
    }
    &s[..e]
}

fn trim_ws(s: &[u16]) -> &[u16] {
    let s = trim_end_ws(s);
    let mut a = 0;
    while a < s.len() && is_ws(s[a]) {
        a += 1;
    }
    &s[a..]
}

/// `unfoldLines(source)`: each line break folded into a space, or into newlines where lines are empty, the white
/// space around it taken off
fn unfold_lines(source: &[u16]) -> Vec<u16> {
    // /(.*?)\r?\n/sy: the text up to the next newline, without one \r before it
    let next_line = |pos: usize| -> Option<(&[u16], usize)> {
        let nl = source.get(pos..)?.iter().position(|&c| c == NL)? + pos;
        let mut content = &source[pos..nl];
        if content.last() == Some(&CR) {
            content = &content[..content.len() - 1];
        }
        Some((content, nl + 1))
    };
    let Some((first, mut pos)) = next_line(0) else { return source.to_vec() };
    let mut res: Vec<u16> = trim_end_ws(first).to_vec();
    let mut sep: &[u16] = &[SP];
    while let Some((line, after)) = next_line(pos) {
        let lm = trim_ws(line);
        if lm.is_empty() {
            if sep == [NL] {
                res.push(NL);
            } else {
                sep = &[NL];
            }
        } else {
            res.extend_from_slice(sep);
            res.extend_from_slice(lm);
            sep = &[SP];
        }
        pos = after;
    }
    // /[ \t]*(.*)/sy from pos
    let mut rest = &source[pos.min(source.len())..];
    while let Some((&c, tail)) = rest.split_first() {
        if !is_ws(c) {
            break;
        }
        rest = tail;
    }
    res.extend_from_slice(sep);
    res.extend_from_slice(rest);
    res
}

fn double_quoted_value(source: &[u16], on_error: &mut impl FnMut(usize, &'static str, String)) -> Vec<u16> {
    let mut res: Vec<u16> = Vec::new();
    let at = |i: usize| source.get(i).copied();
    let mut i = 1;
    while i + 1 < source.len() {
        let ch = source[i];
        if ch == CR && at(i + 1) == Some(NL) {
            i += 1;
            continue;
        }
        if ch == NL {
            let (fold, offset) = fold_newline(source, i);
            res.extend_from_slice(&fold);
            i = offset;
        } else if ch == b'\\' as u16 {
            i += 1;
            let next = at(i);
            if let Some(cc) = next.and_then(escape_code) {
                res.push(cc);
            } else if next == Some(NL) {
                // skip escaped newlines, but still trim the following line
                let mut next = at(i + 1);
                while next == Some(SP) || next == Some(TAB) {
                    i += 1;
                    next = at(i + 1);
                }
            } else if next == Some(CR) && at(i + 1) == Some(NL) {
                // skip escaped CRLF newlines, but still trim the following line
                i += 1;
                let mut next = at(i + 1);
                while next == Some(SP) || next == Some(TAB) {
                    i += 1;
                    next = at(i + 1);
                }
            } else if matches!(next, Some(0x78 | 0x75 | 0x55)) {
                // x u U
                let length = match next {
                    Some(0x78) => 2,
                    Some(0x75) => 4,
                    _ => 8,
                };
                res.extend(parse_char_code(source, i + 1, length, on_error));
                i += length;
            } else {
                let raw = &source[i - 1..(i + 1).min(source.len())];
                on_error(i - 1, "BAD_DQ_ESCAPE", format!("Invalid escape sequence {}", String::from_utf16_lossy(raw)));
                res.extend_from_slice(raw);
            }
        } else if ch == SP || ch == TAB {
            // trim trailing whitespace
            let ws_start = i;
            let mut next = at(i + 1);
            while next == Some(SP) || next == Some(TAB) {
                i += 1;
                next = at(i + 1);
            }
            if next != Some(NL) && !(next == Some(CR) && at(i + 2) == Some(NL)) {
                if i > ws_start {
                    res.extend_from_slice(&source[ws_start..i + 1]);
                } else {
                    res.push(ch);
                }
            }
        } else {
            res.push(ch);
        }
        i += 1;
    }
    if source.last() != Some(&(b'"' as u16)) || source.len() == 1 {
        on_error(source.len(), "MISSING_CHAR", "Missing closing \"quote".to_string());
    }
    res
}

/// Fold a single newline into a space, multiple newlines to N - 1 newlines. Presumes `source[offset] === '\n'`
fn fold_newline(source: &[u16], mut offset: usize) -> (Vec<u16>, usize) {
    let mut fold: Vec<u16> = Vec::new();
    let mut ch = source.get(offset + 1).copied();
    while matches!(ch, Some(SP | TAB | NL | CR)) {
        if ch == Some(CR) && source.get(offset + 2) != Some(&NL) {
            break;
        }
        if ch == Some(NL) {
            fold.push(NL);
        }
        offset += 1;
        ch = source.get(offset + 1).copied();
    }
    if fold.is_empty() {
        fold.push(SP);
    }
    (fold, offset)
}

fn escape_code(c: u16) -> Option<u16> {
    Some(match c {
        0x30 => 0x00,   // 0: null character
        0x61 => 0x07,   // a: bell character
        0x62 => 0x08,   // b: backspace
        0x65 => 0x1b,   // e: escape character
        0x66 => 0x0c,   // f: form feed
        0x6e => 0x0a,   // n: line feed
        0x72 => 0x0d,   // r: carriage return
        0x74 => 0x09,   // t: horizontal tab
        0x76 => 0x0b,   // v: vertical tab
        0x4e => 0x85,   // N: Unicode next line
        0x5f => 0xa0,   // _: Unicode non-breaking space
        0x4c => 0x2028, // L: Unicode line separator
        0x50 => 0x2029, // P: Unicode paragraph separator
        0x20 => 0x20,   // ' '
        0x22 => 0x22,   // "
        0x2f => 0x2f,   // /
        0x5c => 0x5c,   // \
        0x09 => 0x09,   // \t
        _ => return None,
    })
}

/// `parseCharCode(source, offset, length, onError)`: `String.fromCodePoint` of the hex digits, or the escape as it
/// is written where it is not a code point
fn parse_char_code(
    source: &[u16],
    offset: usize,
    length: usize,
    on_error: &mut impl FnMut(usize, &'static str, String),
) -> Vec<u16> {
    let cc = source.get(offset..(offset + length).min(source.len())).unwrap_or(&[]);
    let ok = cc.len() == length && cc.iter().all(|&c| c < 0x80 && (c as u8).is_ascii_hexdigit());
    let code = if ok { u32::from_str_radix(&String::from_utf16_lossy(cc), 16).ok() } else { None };
    match code {
        Some(c) if c <= 0x10FFFF => match char::from_u32(c) {
            Some(ch) => {
                let mut buf = [0u16; 2];
                ch.encode_utf16(&mut buf).to_vec()
            }
            None => vec![c as u16], // a lone surrogate, as fromCodePoint gives it
        },
        _ => {
            let from = offset.saturating_sub(2);
            let raw = source.get(from..(offset + length).min(source.len())).unwrap_or(&[]);
            on_error(from, "BAD_DQ_ESCAPE", format!("Invalid escape sequence {}", String::from_utf16_lossy(raw)));
            raw.to_vec()
        }
    }
}
