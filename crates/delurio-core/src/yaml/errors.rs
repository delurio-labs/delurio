//! The port of the yaml library's errors.ts: a `YAMLParseError` or a `YAMLWarning` of a document, and
//! `prettifyError`, which adds the line and column and the line itself to the message.

use super::parse::line_counter::LineCounter;
use crate::error::At;

/// a `YAMLParseError` or a `YAMLWarning`: its code, message, `pos` (UTF-16 offsets), and `linePos[0]` once
/// prettified (None in a document read with `prettyErrors: false`)
#[derive(Debug, Clone, PartialEq)]
pub struct YamlIssue {
    pub code: String,
    pub message: String,
    pub pos: (usize, usize),
    pub line_pos: Option<At>,
}

impl YamlIssue {
    pub(crate) fn new(pos: (usize, usize), code: &str, message: impl Into<String>) -> Self {
        YamlIssue { code: code.to_string(), message: message.into(), pos, line_pos: None }
    }
}

const SP: u16 = b' ' as u16;

/// `src.substring(a, b)` in UTF-16 code units: both ends clamped to the text, taken in either order
fn substring(src: &[u16], a: usize, b: Option<usize>) -> &[u16] {
    let a = a.min(src.len());
    let b = b.unwrap_or(src.len()).min(src.len());
    if a <= b { &src[a..b] } else { &src[b..a] }
}

/// `prettifyError(src, lc)(error)`: ` at line L, column C`, and when the line holds more than spaces, the line (the
/// one before too when the place is at its start) and a caret under the place
pub(crate) fn prettify(src: &[u16], lc: &LineCounter, error: &mut YamlIssue) {
    let lp0 = lc.line_pos(error.pos.0);
    let lp1 = lc.line_pos(error.pos.1);
    error.line_pos = Some(lp0);
    let (line, col) = (lp0.line, lp0.column);
    error.message += &format!(" at line {line}, column {col}");
    let starts = &lc.line_starts;
    let start_of = |l: usize| -> Option<usize> { l.checked_sub(1).and_then(|i| starts.get(i).copied()) };
    let mut ci = col as isize - 1;
    // lc.lineStarts[line - 1] is undefined for line 0: substring(undefined, x) reads from 0
    let mut line_str: Vec<u16> = substring(src, start_of(line).unwrap_or(0), starts.get(line).copied()).to_vec();
    while matches!(line_str.last(), Some(&0x0a | &0x0d)) {
        line_str.pop();
    }
    // Trim to max 80 chars, keeping col position near the middle
    if ci >= 60 && line_str.len() > 80 {
        let trim_start = (ci - 39).min(line_str.len() as isize - 79);
        let mut trimmed = vec![0x2026]; // …
        trimmed.extend_from_slice(substring(&line_str, trim_start.max(0) as usize, None));
        line_str = trimmed;
        ci -= trim_start - 1;
    }
    if line_str.len() > 80 {
        line_str.truncate(79);
        line_str.push(0x2026);
    }
    // Include previous line in context if pointing at line start
    let before_ci = substring(&line_str, 0, Some(ci.max(0) as usize));
    if line > 1 && before_ci.iter().all(|&c| c == SP) {
        // Regexp won't match if start is trimmed
        let mut prev: Vec<u16> =
            substring(src, start_of(line - 1).unwrap_or(0), starts.get(line - 1).copied()).to_vec();
        if prev.len() > 80 {
            prev.truncate(79);
            prev.extend("…\n".encode_utf16());
        }
        prev.extend_from_slice(&line_str);
        line_str = prev;
    }
    if line_str.iter().any(|&c| c != SP) {
        let mut count: isize = 1;
        if lp1.line == line && lp1.column > col {
            count = ((lp1.column - col) as isize).min(80 - ci).max(1);
        }
        let pointer = " ".repeat(ci.max(0) as usize) + &"^".repeat(count as usize);
        error.message += &format!(":\n\n{}\n{pointer}\n", String::from_utf16_lossy(&line_str));
    }
}
