//! The port of the yaml library's compose/resolve-block-scalar.ts: the value of a literal (`|`) or folded (`>`)
//! block scalar, by its header's indentation and chomping.

use super::{Ctx, ErrAt};
use crate::yaml::parse::cst::{Id, Token, Ty};

const NL: u16 = b'\n' as u16;
const CR: u16 = b'\r' as u16;

/// what a block scalar resolves to: its value, whether its header has a comment, its range
pub(crate) struct BlockScalar {
    pub value: Vec<u16>,
    pub comment: bool,
    pub range: [usize; 3],
}

struct Header {
    folded: bool,
    indent: i64,
    chomp: Option<u16>,
    comment: bool,
    length: usize,
}

impl Ctx<'_> {
    pub(crate) fn resolve_block_scalar(&mut self, scalar: Id) -> BlockScalar {
        let cst = self.cst;
        let Token::BlockScalar { offset: start, indent: scalar_indent, props, source } = cst.tok(scalar) else {
            let o = cst.tok(scalar).offset();
            return BlockScalar { value: Vec::new(), comment: false, range: [o, o, o] };
        };
        let (start, scalar_indent) = (*start, *scalar_indent);
        let source = cst.units(*source);
        let Some(header) = self.parse_block_scalar_header(start, props) else {
            return BlockScalar { value: Vec::new(), comment: false, range: [start, start, start] };
        };
        let lines = if source.is_empty() { Vec::new() } else { split_lines(source) };
        let is_empty_line = |content: &[u16]| content.is_empty() || content == [CR];

        // determine the end of content & start of chomping
        let mut chomp_start = lines.len();
        for i in (0..lines.len()).rev() {
            if is_empty_line(lines[i].1) {
                chomp_start = i;
            } else {
                break;
            }
        }

        // shortcut for empty contents
        if chomp_start == 0 {
            let value = if header.chomp == Some(b'+' as u16) && !lines.is_empty() {
                vec![NL; 1.max(lines.len() - 1)]
            } else {
                Vec::new()
            };
            let mut end = start + header.length;
            if !source.is_empty() {
                end += source.len();
            }
            return BlockScalar { value, comment: header.comment, range: [start, end, end] };
        }

        // find the indentation level to trim from start
        let mut trim_indent = scalar_indent + header.indent;
        let mut offset = start + header.length;
        let mut content_start = 0;
        for (i, &(indent, content)) in lines.iter().enumerate().take(chomp_start) {
            if is_empty_line(content) {
                if header.indent == 0 && indent.len() as i64 > trim_indent {
                    trim_indent = indent.len() as i64;
                }
            } else {
                if (indent.len() as i64) < trim_indent {
                    let message = "Block scalars with more-indented leading empty lines must use an explicit indentation indicator";
                    self.error(ErrAt::Offset(offset + indent.len()), "MISSING_CHAR", message);
                }
                if header.indent == 0 {
                    trim_indent = indent.len() as i64;
                }
                content_start = i;
                if trim_indent == 0 && !self.at_root {
                    self.error(
                        ErrAt::Offset(offset),
                        "BAD_INDENT",
                        "Block scalar values in collections must be indented",
                    );
                }
                break;
            }
            offset += indent.len() + content.len() + 1;
        }

        // include trailing more-indented empty lines in content
        for i in (chomp_start..lines.len()).rev() {
            if lines[i].0.len() as i64 > trim_indent {
                chomp_start = i + 1;
            }
        }

        let slice_from = |s: &[u16]| -> Vec<u16> {
            let t = trim_indent.max(0) as usize;
            if t >= s.len() { Vec::new() } else { s[t..].to_vec() }
        };
        let mut value: Vec<u16> = Vec::new();
        let mut sep: &[u16] = &[];
        let mut prev_more_indented = false;
        // leading whitespace is kept intact
        for &(indent, _) in lines.iter().take(content_start) {
            value.extend(slice_from(indent));
            value.push(NL);
        }
        for &(indent, content) in lines.iter().take(chomp_start).skip(content_start) {
            let mut indent = indent;
            let mut content = content;
            offset += indent.len() + content.len() + 1;
            let crlf = content.last() == Some(&CR);
            if crlf {
                content = &content[..content.len() - 1];
            }
            if !content.is_empty() && (indent.len() as i64) < trim_indent {
                let src = if header.indent != 0 { "explicit indentation indicator" } else { "first line" };
                let message = format!("Block scalar lines must not be less indented than their {src}");
                let at = (offset as isize - content.len() as isize - if crlf { 2 } else { 1 }).max(0) as usize;
                self.error(ErrAt::Offset(at), "BAD_INDENT", message);
                indent = &[];
            }
            if !header.folded {
                value.extend_from_slice(sep);
                value.extend(slice_from(indent));
                value.extend_from_slice(content);
                sep = &[NL];
            } else if indent.len() as i64 > trim_indent || content.first() == Some(&(b'\t' as u16)) {
                // more-indented content within a folded block
                if sep == [b' ' as u16] {
                    sep = &[NL];
                } else if !prev_more_indented && sep == [NL] {
                    sep = &[NL, NL];
                }
                value.extend_from_slice(sep);
                value.extend(slice_from(indent));
                value.extend_from_slice(content);
                sep = &[NL];
                prev_more_indented = true;
            } else if content.is_empty() {
                // empty line
                if sep == [NL] {
                    value.push(NL);
                } else {
                    sep = &[NL];
                }
            } else {
                value.extend_from_slice(sep);
                value.extend_from_slice(content);
                sep = &[b' ' as u16];
                prev_more_indented = false;
            }
        }

        match header.chomp {
            Some(0x2d) => {} // -
            Some(0x2b) => {
                // +
                for &(indent, _) in lines.iter().skip(chomp_start) {
                    value.push(NL);
                    value.extend(slice_from(indent));
                }
                if value.last() != Some(&NL) {
                    value.push(NL);
                }
            }
            _ => value.push(NL),
        }
        let end = start + header.length + source.len();
        BlockScalar { value, comment: header.comment, range: [start, end, end] }
    }

    fn parse_block_scalar_header(&mut self, offset: usize, props: &[Id]) -> Option<Header> {
        let cst = self.cst;
        let &first = props.first()?;
        if cst.ty(first) != Ty::BlockScalarHeader {
            self.error(ErrAt::Token(first), "IMPOSSIBLE", "Block scalar header not found");
            return None;
        }
        let source = cst.source_of(first);
        let folded = source.first() == Some(&(b'>' as u16));
        let mut indent: i64 = 0;
        let mut chomp: Option<u16> = None;
        let mut error: Option<usize> = None;
        for (i, &ch) in source.iter().enumerate().skip(1) {
            if chomp.is_none() && (ch == b'-' as u16 || ch == b'+' as u16) {
                chomp = Some(ch);
            } else {
                // `Number(ch)`: a digit 1-9 sets the indentation once; anything else is extra
                let n = if (0x31..=0x39).contains(&ch) { i64::from(ch - 0x30) } else { 0 };
                if indent == 0 && n != 0 {
                    indent = n;
                } else if error.is_none() {
                    error = Some(offset + i);
                }
            }
        }
        if let Some(at) = error {
            let message = format!("Block scalar header includes extra characters: {}", super::text(source));
            self.error(ErrAt::Offset(at), "UNEXPECTED_TOKEN", message);
        }
        let mut has_space = false;
        let mut comment = false;
        let mut length = source.len();
        for &token in &props[1..] {
            let ts = cst.source_of(token);
            match cst.tok(token) {
                Token::Source { ty: Ty::Space, .. } => {
                    has_space = true;
                    length += ts.len();
                }
                Token::Source { ty: Ty::Newline, .. } => length += ts.len(),
                Token::Source { ty: Ty::Comment, .. } => {
                    if !has_space {
                        let message = "Comments must be separated from other tokens by white space characters";
                        self.error(ErrAt::Token(token), "MISSING_CHAR", message);
                    }
                    length += ts.len();
                    comment = ts.len() > 1;
                }
                Token::Error { message, .. } => {
                    self.error(ErrAt::Token(token), "UNEXPECTED_TOKEN", message.clone());
                    length += ts.len();
                }
                t => {
                    let message = format!("Unexpected token in block scalar header: {}", t.ty().name());
                    self.error(ErrAt::Token(token), "UNEXPECTED_TOKEN", message);
                    length += ts.len();
                }
            }
        }
        Some(Header { folded, indent, chomp, comment, length })
    }
}

/// `splitLines(source)`: each line as its indentation (leading spaces) and its content
fn split_lines(source: &[u16]) -> Vec<(&[u16], &[u16])> {
    let indent_of = |s: &[u16]| s.iter().take_while(|&&c| c == b' ' as u16).count();
    source
        .split(|&c| c == NL)
        .map(|line| {
            let n = indent_of(line);
            (&line[..n], &line[n..])
        })
        .collect()
}
