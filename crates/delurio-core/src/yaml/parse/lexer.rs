//! The port of the yaml library's parse/lexer.ts: a text split into lexical tokens, each a slice of the text or one
//! of the control characters DOCUMENT, FLOW_END and SCALAR. The library's lexer is a generator that may stop at the
//! end of incomplete input; delurio always gives it the whole text (`atEnd`), so it runs to the end at once, and the
//! branches that wait for more input are left out.
//!
//! The text is read as JavaScript reads a string: by UTF-16 code units, `buffer[i]` being undefined (None) past
//! either end.

const SP: u16 = b' ' as u16;
const TAB: u16 = b'\t' as u16;
const NL: u16 = b'\n' as u16;
const CR: u16 = b'\r' as u16;
const HASH: u16 = b'#' as u16;

/// a lexical token: a slice of the text, or a control character
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lexeme {
    Document,
    FlowEnd,
    Scalar,
    Text(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// the lexer's states; 'line-start', 'quoted-scalar', 'block-scalar' and 'plain-scalar' are only ever the next state of
/// incomplete input
enum State {
    Stream,
    BlockStart,
    Doc,
    Flow,
}

fn is_empty(ch: Option<u16>) -> bool {
    matches!(ch, None | Some(SP | NL | CR | TAB))
}

fn is_hex(ch: Option<u16>) -> bool {
    matches!(ch, Some(c) if (c as u8 as u16 == c) && (c as u8).is_ascii_hexdigit())
}

// "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-#;/?:@&=+$_.!~*'()"
fn is_tag_char(ch: u16) -> bool {
    ch < 0x80 && {
        let c = ch as u8;
        c.is_ascii_alphanumeric() || b"-#;/?:@&=+$_.!~*'()".contains(&c)
    }
}

fn is_flow_indicator(ch: Option<u16>) -> bool {
    matches!(ch, Some(0x2c | 0x5b | 0x5d | 0x7b | 0x7d)) // , [ ] { }
}

// `isNotAnchorChar`: !ch || ' ,[]{}\n\r\t'.includes(ch)
fn is_not_anchor_char(ch: Option<u16>) -> bool {
    matches!(ch, None | Some(SP | 0x2c | 0x5b | 0x5d | 0x7b | 0x7d | NL | CR | TAB))
}

/// the line `getLine` gives: `buffer.substring(pos, end)`, as its own bounds
#[derive(Debug, Clone, Copy)]
struct Line {
    start: usize,
    end: usize,
}

impl Line {
    fn len(self) -> isize {
        (self.end - self.start) as isize
    }
}

struct Lexer<'a> {
    buf: &'a [u16],
    block_scalar_indent: i64,
    block_scalar_keep: bool,
    flow_key: bool,
    flow_level: i64,
    indent_next: i64,
    indent_value: i64,
    line_end_pos: Option<isize>,
    pos: usize,
    out: Vec<Lexeme>,
}

/// `new Lexer().lex(text)`: every lexical token of the text
pub(crate) fn lex(buf: &[u16]) -> Vec<Lexeme> {
    let mut lx = Lexer {
        buf,
        block_scalar_indent: -1,
        block_scalar_keep: false,
        flow_key: false,
        flow_level: 0,
        indent_next: 0,
        indent_value: 0,
        line_end_pos: None,
        pos: 0,
        out: Vec::with_capacity(buf.len() / 2 + 4),
    };
    let mut next = State::Stream;
    while lx.has_chars(1) {
        next = lx.parse_next(next);
    }
    lx.out
}

impl Lexer<'_> {
    /// `buffer[i]`
    fn at(&self, i: isize) -> Option<u16> {
        if i < 0 { None } else { self.buf.get(i as usize).copied() }
    }
    fn char_at(&self, n: isize) -> Option<u16> {
        self.at(self.pos as isize + n)
    }
    fn line_at(&self, line: Line, k: isize) -> Option<u16> {
        if k < 0 || k >= line.len() { None } else { self.buf.get(line.start + k as usize).copied() }
    }
    fn len(&self) -> usize {
        self.buf.len()
    }
    /// `buffer.indexOf(ch, from)`, -1 when it is not there
    fn index_of(&self, ch: u16, from: isize) -> isize {
        let from = from.max(0) as usize;
        if from >= self.len() {
            return -1;
        }
        self.buf[from..].iter().position(|&c| c == ch).map_or(-1, |i| (from + i) as isize)
    }
    /// `buffer.substr(from, 3) === s`
    fn has_at(&self, from: isize, s: &[u8]) -> bool {
        s.iter().enumerate().all(|(k, &c)| self.at(from + k as isize) == Some(c as u16))
    }

    fn at_line_end(&self) -> bool {
        let mut i = self.pos as isize;
        let mut ch = self.at(i);
        while ch == Some(SP) || ch == Some(TAB) {
            i += 1;
            ch = self.at(i);
        }
        match ch {
            None | Some(HASH) | Some(NL) => true,
            Some(CR) => self.at(i + 1) == Some(NL),
            _ => false,
        }
    }

    fn continue_scalar(&self, offset: isize) -> isize {
        let mut ch = self.at(offset);
        if self.indent_next > 0 {
            let mut indent: isize = 0;
            while ch == Some(SP) {
                indent += 1;
                ch = self.at(indent + offset);
            }
            if ch == Some(CR) {
                let next = self.at(indent + offset + 1);
                if next == Some(NL) {
                    return offset + indent + 1;
                }
            }
            return if ch == Some(NL) || indent as i64 >= self.indent_next { offset + indent } else { -1 };
        }
        if (ch == Some(b'-' as u16) || ch == Some(b'.' as u16))
            && (self.has_at(offset, b"---") || self.has_at(offset, b"..."))
            && is_empty(self.at(offset + 3))
        {
            return -1;
        }
        offset
    }

    fn get_line(&mut self) -> Line {
        let mut end = match self.line_end_pos {
            Some(end) if end == -1 || end >= self.pos as isize => end,
            _ => {
                let end = self.index_of(NL, self.pos as isize);
                self.line_end_pos = Some(end);
                end
            }
        };
        if end == -1 {
            return Line { start: self.pos.min(self.len()), end: self.len() };
        }
        if self.at(end - 1) == Some(CR) {
            end -= 1;
        }
        // `substring` takes its two ends in either order
        let (a, b) = (self.pos.min(self.len()), end as usize);
        Line { start: a.min(b), end: a.max(b) }
    }

    fn has_chars(&self, n: usize) -> bool {
        self.pos + n <= self.len()
    }

    fn peek_is(&self, s: &[u8]) -> bool {
        self.has_at(self.pos as isize, s)
    }

    fn parse_next(&mut self, next: State) -> State {
        match next {
            State::Stream => self.parse_stream(),
            State::BlockStart => self.parse_block_start(),
            State::Doc => self.parse_document(),
            State::Flow => self.parse_flow_collection(),
        }
    }

    fn parse_stream(&mut self) -> State {
        let mut line = self.get_line();
        if self.line_at(line, 0) == Some(super::cst::BOM) {
            self.push_count(1);
            line.start += 1;
        }
        if self.line_at(line, 0) == Some(b'%' as u16) {
            let mut dir_end = line.len();
            let index_in_line = |lx: &Self, from: isize| -> isize {
                let mut k = from.max(0);
                while k < line.len() {
                    if lx.line_at(line, k) == Some(HASH) {
                        return k;
                    }
                    k += 1;
                }
                -1
            };
            let mut cs = index_in_line(self, 0);
            while cs != -1 {
                let ch = self.line_at(line, cs - 1);
                if ch == Some(SP) || ch == Some(TAB) {
                    dir_end = cs - 1;
                    break;
                } else {
                    cs = index_in_line(self, cs + 1);
                }
            }
            loop {
                let ch = self.line_at(line, dir_end - 1);
                if ch == Some(SP) || ch == Some(TAB) {
                    dir_end -= 1;
                } else {
                    break;
                }
            }
            let n = self.push_count(dir_end) + self.push_spaces(true);
            self.push_count(line.len() - n); // possible comment
            // the library calls `this.pushNewline()` here without `yield*`, which pushes nothing
            return State::Stream;
        }
        if self.at_line_end() {
            let sp = self.push_spaces(true);
            self.push_count(line.len() - sp);
            self.push_newline();
            return State::Stream;
        }
        self.out.push(Lexeme::Document);
        self.parse_line_start()
    }

    fn parse_line_start(&mut self) -> State {
        let ch = self.char_at(0);
        if (ch == Some(b'-' as u16) || ch == Some(b'.' as u16))
            && (self.peek_is(b"---") || self.peek_is(b"..."))
            && is_empty(self.char_at(3))
        {
            let doc = self.peek_is(b"---");
            self.push_count(3);
            self.indent_value = 0;
            self.indent_next = 0;
            return if doc { State::Doc } else { State::Stream };
        }
        self.indent_value = self.push_spaces(false) as i64;
        if self.indent_next > self.indent_value && !is_empty(self.char_at(1)) {
            self.indent_next = self.indent_value;
        }
        self.parse_block_start()
    }

    fn parse_block_start(&mut self) -> State {
        let ch0 = self.char_at(0);
        let ch1 = self.char_at(1);
        if matches!(ch0, Some(0x2d | 0x3f | 0x3a)) && is_empty(ch1) {
            let n = self.push_count(1) + self.push_spaces(true);
            self.indent_next = self.indent_value + 1;
            self.indent_value += n as i64;
            return State::BlockStart;
        }
        State::Doc
    }

    fn parse_document(&mut self) -> State {
        self.push_spaces(true);
        let line = self.get_line();
        let mut n = self.push_indicators();
        match self.line_at(line, n) {
            Some(HASH) => {
                self.push_count(line.len() - n);
                self.push_newline();
                self.parse_line_start()
            }
            None => {
                self.push_newline();
                self.parse_line_start()
            }
            Some(0x7b | 0x5b) => {
                self.push_count(1);
                self.flow_key = false;
                self.flow_level = 1;
                State::Flow
            }
            Some(0x7d | 0x5d) => {
                // this is an error
                self.push_count(1);
                State::Doc
            }
            Some(0x2a) => {
                self.push_until(is_not_anchor_char);
                State::Doc
            }
            Some(0x22 | 0x27) => self.parse_quoted_scalar(),
            Some(0x7c | 0x3e) => {
                n += self.parse_block_scalar_header();
                n += self.push_spaces(true);
                self.push_count(line.len() - n);
                self.push_newline();
                self.parse_block_scalar()
            }
            _ => self.parse_plain_scalar(),
        }
    }

    fn parse_flow_collection(&mut self) -> State {
        let mut indent: i64 = -1;
        loop {
            let nl = self.push_newline();
            let mut sp;
            if nl > 0 {
                sp = self.push_spaces(false);
                indent = sp as i64;
                self.indent_value = indent;
            } else {
                sp = 0;
            }
            sp += self.push_spaces(true);
            if nl + sp <= 0 {
                break;
            }
        }
        let line = self.get_line();
        let starts_marker = |lx: &Self| {
            let at = |k| lx.line_at(line, k);
            let dashes = (0..3).all(|k| at(k) == Some(b'-' as u16));
            let dots = (0..3).all(|k| at(k) == Some(b'.' as u16));
            (dashes || dots) && is_empty(at(3))
        };
        if (indent != -1 && indent < self.indent_next && self.line_at(line, 0) != Some(HASH))
            || (indent == 0 && starts_marker(self))
        {
            // Allowing for the terminal ] or } at the same (rather than greater) indent level as the initial [ or {
            // is technically invalid, but failing here would be surprising to users.
            let at_flow_end_marker = indent == self.indent_next - 1
                && self.flow_level == 1
                && matches!(self.line_at(line, 0), Some(0x5d | 0x7d));
            if !at_flow_end_marker {
                // this is an error
                self.flow_level = 0;
                self.out.push(Lexeme::FlowEnd);
                return self.parse_line_start();
            }
        }
        let mut n: isize = 0;
        while self.line_at(line, n) == Some(b',' as u16) {
            n += self.push_count(1);
            n += self.push_spaces(true);
            self.flow_key = false;
        }
        n += self.push_indicators();
        match self.line_at(line, n) {
            None => State::Flow,
            Some(HASH) => {
                self.push_count(line.len() - n);
                State::Flow
            }
            Some(0x7b | 0x5b) => {
                self.push_count(1);
                self.flow_key = false;
                self.flow_level += 1;
                State::Flow
            }
            Some(0x7d | 0x5d) => {
                self.push_count(1);
                self.flow_key = true;
                self.flow_level -= 1;
                if self.flow_level != 0 { State::Flow } else { State::Doc }
            }
            Some(0x2a) => {
                self.push_until(is_not_anchor_char);
                State::Flow
            }
            Some(0x22 | 0x27) => {
                self.flow_key = true;
                self.parse_quoted_scalar()
            }
            Some(0x3a) => {
                let next = self.char_at(1);
                if self.flow_key || is_empty(next) || next == Some(b',' as u16) {
                    self.flow_key = false;
                    self.push_count(1);
                    self.push_spaces(true);
                    return State::Flow;
                }
                self.flow_key = false;
                self.parse_plain_scalar()
            }
            _ => {
                self.flow_key = false;
                self.parse_plain_scalar()
            }
        }
    }

    fn parse_quoted_scalar(&mut self) -> State {
        let Some(quote) = self.char_at(0) else { return self.after_scalar() };
        let mut end = self.index_of(quote, self.pos as isize + 1);
        if quote == b'\'' as u16 {
            while end != -1 && self.at(end + 1) == Some(b'\'' as u16) {
                end = self.index_of(b'\'' as u16, end + 2);
            }
        } else {
            // double-quote
            while end != -1 {
                let mut n = 0;
                while self.at(end - 1 - n) == Some(b'\\' as u16) {
                    n += 1;
                }
                if n % 2 == 0 {
                    break;
                }
                end = self.index_of(b'"' as u16, end + 1);
            }
        }
        // Only looking for newlines within the quotes: `qb = buffer.substring(0, end)`, empty when end is -1
        let qb_len = end.max(0);
        let qb_index_of_nl = |lx: &Self, from: isize| -> isize {
            let i = lx.index_of(NL, from);
            if i != -1 && i < qb_len { i } else { -1 }
        };
        let mut nl = qb_index_of_nl(self, self.pos as isize);
        if nl != -1 {
            while nl != -1 {
                let cs = self.continue_scalar(nl + 1);
                if cs == -1 {
                    break;
                }
                nl = qb_index_of_nl(self, cs);
            }
            if nl != -1 {
                // this is an error caused by an unexpected unindent
                end = nl - if self.at(nl - 1) == Some(CR) && nl - 1 < qb_len { 2 } else { 1 };
            }
        }
        if end == -1 {
            end = self.len() as isize;
        }
        self.push_to_index(end + 1, false);
        self.after_scalar()
    }

    fn after_scalar(&self) -> State {
        if self.flow_level != 0 { State::Flow } else { State::Doc }
    }

    fn parse_block_scalar_header(&mut self) -> isize {
        self.block_scalar_indent = -1;
        self.block_scalar_keep = false;
        let mut i = self.pos as isize;
        loop {
            i += 1;
            let ch = self.at(i);
            if ch == Some(b'+' as u16) {
                self.block_scalar_keep = true;
            } else if let Some(c @ 0x31..=0x39) = ch {
                self.block_scalar_indent = (c - 0x30) as i64 - 1;
            } else if ch != Some(b'-' as u16) {
                break;
            }
        }
        self.push_until(|ch| is_empty(ch) || ch == Some(HASH))
    }

    fn parse_block_scalar(&mut self) -> State {
        let mut nl = self.pos as isize - 1; // may be -1 if this.pos === 0
        let mut indent: i64 = 0;
        let mut ch;
        let mut i = self.pos as isize;
        loop {
            ch = self.at(i);
            match ch {
                None => break,
                Some(SP) => indent += 1,
                Some(NL) => {
                    nl = i;
                    indent = 0;
                }
                Some(CR) if self.at(i + 1) == Some(NL) => {}
                Some(_) => break,
            }
            i += 1;
        }
        if indent >= self.indent_next {
            if self.block_scalar_indent == -1 {
                self.indent_next = indent;
            } else {
                self.indent_next = self.block_scalar_indent + if self.indent_next == 0 { 1 } else { self.indent_next };
            }
            loop {
                let cs = self.continue_scalar(nl + 1);
                if cs == -1 {
                    break;
                }
                nl = self.index_of(NL, cs);
                if nl == -1 {
                    break;
                }
            }
            if nl == -1 {
                nl = self.len() as isize;
            }
        }
        // Trailing insufficiently indented tabs are invalid.
        // To catch that during parsing, we include them in the block scalar value.
        let mut i = nl + 1;
        ch = self.at(i);
        while ch == Some(SP) {
            i += 1;
            ch = self.at(i);
        }
        if ch == Some(TAB) {
            while matches!(ch, Some(TAB | SP | CR | NL)) {
                i += 1;
                ch = self.at(i);
            }
            nl = i - 1;
        } else if !self.block_scalar_keep {
            loop {
                let mut i = nl - 1;
                let mut ch = self.at(i);
                if ch == Some(CR) {
                    i -= 1;
                    ch = self.at(i);
                }
                let last_char = i; // Drop the line if last char not more indented
                while ch == Some(SP) {
                    i -= 1;
                    ch = self.at(i);
                }
                if ch == Some(NL) && i >= self.pos as isize && i + 1 + indent as isize > last_char {
                    nl = i;
                } else {
                    break;
                }
            }
        }
        self.out.push(Lexeme::Scalar);
        self.push_to_index(nl + 1, true);
        self.parse_line_start()
    }

    fn parse_plain_scalar(&mut self) -> State {
        let in_flow = self.flow_level > 0;
        let mut end = self.pos as isize - 1;
        let mut i = self.pos as isize - 1;
        loop {
            i += 1;
            let Some(mut ch) = self.at(i) else { break };
            if ch == b':' as u16 {
                let next = self.at(i + 1);
                if is_empty(next) || (in_flow && is_flow_indicator(next)) {
                    break;
                }
                end = i;
            } else if is_empty(Some(ch)) {
                let mut next = self.at(i + 1);
                if ch == CR {
                    if next == Some(NL) {
                        i += 1;
                        ch = NL;
                        next = self.at(i + 1);
                    } else {
                        end = i;
                    }
                }
                if next == Some(HASH) || (in_flow && is_flow_indicator(next)) {
                    break;
                }
                if ch == NL {
                    let cs = self.continue_scalar(i + 1);
                    if cs == -1 {
                        break;
                    }
                    i = i.max(cs - 2); // to advance, but still account for ' #'
                }
            } else {
                if in_flow && is_flow_indicator(Some(ch)) {
                    break;
                }
                end = i;
            }
        }
        self.out.push(Lexeme::Scalar);
        self.push_to_index(end + 1, true);
        if in_flow { State::Flow } else { State::Doc }
    }

    /// `pushCount(n)`: `buffer.substr(pos, n)`, and the position moved by n even past the end
    fn push_count(&mut self, n: isize) -> isize {
        if n > 0 {
            let a = self.pos.min(self.len());
            let b = (self.pos + n as usize).min(self.len());
            self.out.push(Lexeme::Text(a, b));
            self.pos += n as usize;
            return n;
        }
        0
    }

    /// `pushToIndex(i, allowEmpty)`: `buffer.slice(pos, i)`
    fn push_to_index(&mut self, i: isize, allow_empty: bool) -> isize {
        let a = self.pos.min(self.len());
        let b = (i.max(0) as usize).min(self.len());
        if b > a {
            self.out.push(Lexeme::Text(a, b));
            self.pos += b - a;
            return (b - a) as isize;
        } else if allow_empty {
            self.out.push(Lexeme::Text(a, a));
        }
        0
    }

    fn push_indicators(&mut self) -> isize {
        let mut n = 0;
        loop {
            match self.char_at(0) {
                Some(0x21) => {
                    // !
                    n += self.push_tag();
                    n += self.push_spaces(true);
                }
                Some(0x26) => {
                    // &
                    n += self.push_until(is_not_anchor_char);
                    n += self.push_spaces(true);
                }
                // - is an error, ? is an error outside flow collections
                Some(0x2d | 0x3f | 0x3a) => {
                    let in_flow = self.flow_level > 0;
                    let ch1 = self.char_at(1);
                    if is_empty(ch1) || (in_flow && is_flow_indicator(ch1)) {
                        if !in_flow {
                            self.indent_next = self.indent_value + 1;
                        } else if self.flow_key {
                            self.flow_key = false;
                        }
                        n += self.push_count(1);
                        n += self.push_spaces(true);
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        n
    }

    fn push_tag(&mut self) -> isize {
        if self.char_at(1) == Some(b'<' as u16) {
            let mut i = self.pos as isize + 2;
            let mut ch = self.at(i);
            while !is_empty(ch) && ch != Some(b'>' as u16) {
                i += 1;
                ch = self.at(i);
            }
            self.push_to_index(if ch == Some(b'>' as u16) { i + 1 } else { i }, false)
        } else {
            let mut i = self.pos as isize + 1;
            let mut ch = self.at(i);
            while let Some(c) = ch {
                if is_tag_char(c) {
                    i += 1;
                    ch = self.at(i);
                } else if c == b'%' as u16 && is_hex(self.at(i + 1)) && is_hex(self.at(i + 2)) {
                    i += 3;
                    ch = self.at(i);
                } else {
                    break;
                }
            }
            self.push_to_index(i, false)
        }
    }

    fn push_newline(&mut self) -> isize {
        match self.char_at(0) {
            Some(NL) => self.push_count(1),
            Some(CR) if self.char_at(1) == Some(NL) => self.push_count(2),
            _ => 0,
        }
    }

    fn push_spaces(&mut self, allow_tabs: bool) -> isize {
        let mut i = self.pos;
        while let Some(&ch) = self.buf.get(i) {
            if ch == SP || (allow_tabs && ch == TAB) {
                i += 1;
            } else {
                break;
            }
        }
        if i > self.pos {
            let n = i - self.pos;
            self.out.push(Lexeme::Text(self.pos, i));
            self.pos = i;
            return n as isize;
        }
        0
    }

    fn push_until(&mut self, test: impl Fn(Option<u16>) -> bool) -> isize {
        let mut i = self.pos as isize;
        let mut ch = self.at(i);
        while !test(ch) {
            i += 1;
            ch = self.at(i);
        }
        self.push_to_index(i, false)
    }
}
