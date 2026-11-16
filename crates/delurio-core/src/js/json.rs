//! `JSON.parse` with V8's error messages (Node 26), and `JSON.stringify`. The messages reach the output when a
//! catalog or a kit's tokens are not JSON, so they are V8's word for word, positions in UTF-16 code units.

use std::fmt;

use super::number::number_to_string;
use super::object::Object;
use super::value::Value;

/// A text that is not JSON: `message` is V8's `SyntaxError` message.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonError {
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsonError {}

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Eos,
    Number,
    String,
    Other,
}

struct Parser<'a> {
    src: &'a [u16],
    pos: usize,
}

const MAX_CONTEXT: usize = 10;

impl Parser<'_> {
    fn peek(&self) -> Option<u16> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' as u16 || c == b'\t' as u16 || c == b'\n' as u16 || c == b'\r' as u16 {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn token_at(&self, at: usize) -> Token {
        match self.src.get(at).copied() {
            None => Token::Eos,
            Some(c) if c == b'"' as u16 => Token::String,
            Some(c) if c == b'-' as u16 || (b'0' as u16..=b'9' as u16).contains(&c) => Token::Number,
            Some(_) => Token::Other,
        }
    }

    fn location(&self, at: usize) -> String {
        format!("in JSON {}", self.place(at))
    }

    fn place(&self, at: usize) -> String {
        let (cr, lf) = (b'\r' as u16, b'\n' as u16);
        let mut line = 1;
        let mut last_break = 0;
        let mut i = 0;
        while i < at {
            if self.src[i] == cr && i + 1 < at && self.src[i + 1] == lf {
                i += 1;
            }
            if self.src[i] == cr || self.src[i] == lf {
                line += 1;
                last_break = i + 1;
            }
            i += 1;
        }
        format!("at position {at} (line {line} column {})", 1 + at - last_break)
    }

    fn fail<T>(&self, what: &str, at: usize) -> Result<T, JsonError> {
        Err(JsonError { message: format!("{what} {}", self.location(at)) })
    }

    fn text(&self, from: usize, to: usize) -> String {
        String::from_utf16_lossy(&self.src[from..to.min(self.src.len())])
    }

    /// the token at `at` is not what may stand there, and no particular message says why
    fn unexpected<T>(&self, at: usize) -> Result<T, JsonError> {
        let message = match self.token_at(at) {
            Token::Eos => "Unexpected end of JSON input".to_string(),
            Token::Number => format!("Unexpected number {}", self.location(at)),
            Token::String => format!("Unexpected string {}", self.location(at)),
            Token::Other => {
                let whole = self.text(0, self.src.len());
                if matches!(whole.as_str(), "undefined" | "NaN" | "Infinity" | "[object Object]") {
                    format!("\"{whole}\" is not valid JSON")
                } else {
                    let ch = self.text(at, at + 1);
                    let n = self.src.len();
                    if n < MAX_CONTEXT * 2 + 1 {
                        format!("Unexpected token '{ch}', \"{whole}\" is not valid JSON")
                    } else if at < MAX_CONTEXT {
                        format!("Unexpected token '{ch}', \"{}\"... is not valid JSON", self.text(0, at + MAX_CONTEXT))
                    } else if at < n - MAX_CONTEXT {
                        format!(
                            "Unexpected token '{ch}', ...\"{}\"... is not valid JSON",
                            self.text(at - MAX_CONTEXT, at + MAX_CONTEXT)
                        )
                    } else {
                        format!("Unexpected token '{ch}', ...\"{}\" is not valid JSON", self.text(at - MAX_CONTEXT, n))
                    }
                }
            }
        };
        Err(JsonError { message })
    }

    fn is(&self, c: u8) -> bool {
        self.peek() == Some(c as u16)
    }

    fn value(&mut self) -> Result<Value, JsonError> {
        self.skip_ws();
        let Some(c) = self.peek() else { return self.unexpected(self.pos) };
        match c {
            0x22 => self.string().map(Value::String),
            0x7B => self.object(),
            0x5B => self.array(),
            0x74 => self.literal("true", Value::Bool(true)),
            0x66 => self.literal("false", Value::Bool(false)),
            0x6E => self.literal("null", Value::Null),
            c if c == b'-' as u16 || (b'0' as u16..=b'9' as u16).contains(&c) => self.number(),
            _ => self.unexpected(self.pos),
        }
    }

    fn literal(&mut self, word: &str, v: Value) -> Result<Value, JsonError> {
        for b in word.bytes() {
            if self.peek() != Some(b as u16) {
                return self.unexpected(self.pos);
            }
            self.pos += 1;
        }
        Ok(v)
    }

    fn digit(&self) -> bool {
        self.peek().is_some_and(|c| (b'0' as u16..=b'9' as u16).contains(&c))
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.pos;
        if self.is(b'-') {
            self.pos += 1;
            if !self.digit() {
                return self.fail("No number after minus sign", self.pos);
            }
        }
        if self.is(b'0') {
            self.pos += 1;
            if self.digit() {
                return self.unexpected(self.pos);
            }
        } else {
            while self.digit() {
                self.pos += 1;
            }
        }
        if self.is(b'.') {
            self.pos += 1;
            if !self.digit() {
                return self.fail("Unterminated fractional number", self.pos);
            }
            while self.digit() {
                self.pos += 1;
            }
        }
        if self.is(b'e') || self.is(b'E') {
            self.pos += 1;
            if self.is(b'+') || self.is(b'-') {
                self.pos += 1;
            }
            if !self.digit() {
                return self.fail("Exponent part is missing a number", self.pos);
            }
            while self.digit() {
                self.pos += 1;
            }
        }
        let text = self.text(start, self.pos);
        Ok(Value::Number(text.parse::<f64>().unwrap_or(f64::NAN)))
    }

    fn hex(&self, at: usize) -> Option<u16> {
        let c = self.src.get(at).copied()?;
        char::from_u32(u32::from(c))?.to_digit(16).map(|d| d as u16)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1; // the opening quote
        let mut units: Vec<u16> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("Unterminated string", self.pos) };
            if c == b'"' as u16 {
                self.pos += 1;
                return Ok(String::from_utf16_lossy(&units));
            }
            if c < 0x20 {
                return self.fail("Bad control character in string literal", self.pos);
            }
            if c != b'\\' as u16 {
                units.push(c);
                self.pos += 1;
                continue;
            }
            self.pos += 1;
            let Some(e) = self.peek() else { return self.unexpected(self.pos) };
            let plain = match e {
                0x22 => Some(0x22),
                0x5C => Some(0x5C),
                0x2F => Some(0x2F),
                0x62 => Some(0x08),
                0x66 => Some(0x0C),
                0x6E => Some(0x0A),
                0x72 => Some(0x0D),
                0x74 => Some(0x09),
                _ => None,
            };
            if let Some(p) = plain {
                units.push(p);
                self.pos += 1;
                continue;
            }
            if e != b'u' as u16 {
                return self.fail("Bad escaped character", self.pos);
            }
            let mut code: u16 = 0;
            for k in 1..=4 {
                match self.hex(self.pos + k) {
                    Some(d) => code = code * 16 + d,
                    None => {
                        if self.pos + k >= self.src.len() {
                            return self.unexpected(self.src.len());
                        }
                        return self.fail("Bad Unicode escape", self.pos + k);
                    }
                }
            }
            units.push(code);
            self.pos += 5;
        }
    }

    fn object(&mut self) -> Result<Value, JsonError> {
        self.pos += 1;
        let mut out = Object::new();
        self.skip_ws();
        if self.is(b'}') {
            self.pos += 1;
            return Ok(Value::Object(out));
        }
        let mut first = true;
        loop {
            self.skip_ws();
            if !self.is(b'"') {
                let what = if first { "Expected property name or '}'" } else { "Expected double-quoted property name" };
                return self.fail(what, self.pos);
            }
            first = false;
            let key = self.string()?;
            self.skip_ws();
            if !self.is(b':') {
                return self.fail("Expected ':' after property name", self.pos);
            }
            self.pos += 1;
            let v = self.value()?;
            out.insert(key, v);
            self.skip_ws();
            if self.is(b',') {
                self.pos += 1;
                continue;
            }
            if self.is(b'}') {
                self.pos += 1;
                return Ok(Value::Object(out));
            }
            return self.fail("Expected ',' or '}' after property value", self.pos);
        }
    }

    fn array(&mut self) -> Result<Value, JsonError> {
        self.pos += 1;
        let mut out = Vec::new();
        self.skip_ws();
        if self.is(b']') {
            self.pos += 1;
            return Ok(Value::Array(out));
        }
        loop {
            out.push(self.value()?);
            self.skip_ws();
            if self.is(b',') {
                self.pos += 1;
                continue;
            }
            if self.is(b']') {
                self.pos += 1;
                return Ok(Value::Array(out));
            }
            return self.fail("Expected ',' or ']' after array element", self.pos);
        }
    }
}

/// `JSON.parse(text)`
pub fn json_parse(text: &str) -> Result<Value, JsonError> {
    let src: Vec<u16> = text.encode_utf16().collect();
    let mut p = Parser { src: &src, pos: 0 };
    let v = p.value()?;
    p.skip_ws();
    if p.pos < src.len() {
        return Err(JsonError {
            message: format!("Unexpected non-whitespace character after JSON {}", p.place(p.pos)),
        });
    }
    Ok(v)
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(v: &Value, indent: &str, depth: usize, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&if n.is_finite() { number_to_string(*n) } else { "null".to_string() }),
        Value::String(s) => quote(s, out),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, x) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !indent.is_empty() {
                    out.push('\n');
                    out.push_str(&indent.repeat(depth + 1));
                }
                write(x, indent, depth + 1, out);
            }
            if !indent.is_empty() {
                out.push('\n');
                out.push_str(&indent.repeat(depth));
            }
            out.push(']');
        }
        Value::Object(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !indent.is_empty() {
                    out.push('\n');
                    out.push_str(&indent.repeat(depth + 1));
                }
                quote(k, out);
                out.push(':');
                if !indent.is_empty() {
                    out.push(' ');
                }
                write(x, indent, depth + 1, out);
            }
            if !indent.is_empty() {
                out.push('\n');
                out.push_str(&indent.repeat(depth));
            }
            out.push('}');
        }
    }
}

/// `JSON.stringify(v)`
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write(v, "", 0, &mut out);
    out
}

/// `JSON.stringify(v, null, indent)`
pub fn stringify_pretty(v: &Value, indent: usize) -> String {
    let mut out = String::new();
    write(v, &" ".repeat(indent), 0, &mut out);
    out
}

/// `JSON.stringify(s)` of a string
pub fn quote_string(s: &str) -> String {
    let mut out = String::new();
    quote(s, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // each from `node -e 'try { JSON.parse(x) } catch (e) { console.log(e.message) }'` on Node 26
    const CASES: &[(&str, &str)] = &[
        ("", "Unexpected end of JSON input"),
        ("{", "Expected property name or '}' in JSON at position 1 (line 1 column 2)"),
        ("{\"a\":1", "Expected ',' or '}' after property value in JSON at position 6 (line 1 column 7)"),
        ("{\"a\":1,}", "Expected double-quoted property name in JSON at position 7 (line 1 column 8)"),
        ("{\"a\" 1}", "Expected ':' after property name in JSON at position 5 (line 1 column 6)"),
        ("[1,]", "Unexpected token ']', \"[1,]\" is not valid JSON"),
        ("[1 2]", "Expected ',' or ']' after array element in JSON at position 3 (line 1 column 4)"),
        ("{\"a\":1}x", "Unexpected non-whitespace character after JSON at position 7 (line 1 column 8)"),
        ("{\"a\":\"b\nc\"}", "Bad control character in string literal in JSON at position 7 (line 1 column 8)"),
        ("{\"a\":tru}", "Unexpected token '}', \"{\"a\":tru}\" is not valid JSON"),
        ("{\"a\":01}", "Unexpected number in JSON at position 6 (line 1 column 7)"),
        ("{\"a\":-}", "No number after minus sign in JSON at position 6 (line 1 column 7)"),
        ("{\"a\":1.}", "Unterminated fractional number in JSON at position 7 (line 1 column 8)"),
        ("{\"a\":\"\\x\"}", "Bad escaped character in JSON at position 7 (line 1 column 8)"),
        ("{\"a\":\"\\u12\"}", "Bad Unicode escape in JSON at position 10 (line 1 column 11)"),
        ("\"abc", "Unterminated string in JSON at position 4 (line 1 column 5)"),
        ("{\"a\":1}\n\n  }", "Unexpected non-whitespace character after JSON at position 11 (line 3 column 3)"),
        (
            "{\n  \"a\": 1\n  \"b\": 2\n}",
            "Expected ',' or '}' after property value in JSON at position 13 (line 3 column 3)",
        ),
        ("[", "Unexpected end of JSON input"),
        ("nul", "Unexpected end of JSON input"),
        ("{\"a\":1e}", "Exponent part is missing a number in JSON at position 7 (line 1 column 8)"),
        (
            "{\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\": x}",
            "Unexpected token 'x', ...\"aaaaaaa\": x}\" is not valid JSON",
        ),
        ("{1:2}", "Expected property name or '}' in JSON at position 1 (line 1 column 2)"),
        ("{\"a\"", "Expected ':' after property name in JSON at position 4 (line 1 column 5)"),
        ("{\"a\":", "Unexpected end of JSON input"),
        ("{\"a\":1,", "Expected double-quoted property name in JSON at position 7 (line 1 column 8)"),
        ("\r\n{\r\n\"a\" 1}", "Expected ':' after property name in JSON at position 9 (line 3 column 5)"),
        ("undefined", "\"undefined\" is not valid JSON"),
        ("\u{feff}{}", "Unexpected token '\u{feff}', \"\u{feff}{}\" is not valid JSON"),
        ("[-01]", "Unexpected number in JSON at position 3 (line 1 column 4)"),
        ("[\"\\", "Unexpected end of JSON input"),
        ("[tr", "Unexpected end of JSON input"),
        ("{\"a\":nulll}", "Expected ',' or '}' after property value in JSON at position 9 (line 1 column 10)"),
        ("{\"key\": value-that-is-long-enough}", "Unexpected token 'v', \"{\"key\": value-that\"... is not valid JSON"),
        ("{\"a\":1} \n x", "Unexpected non-whitespace character after JSON at position 10 (line 2 column 2)"),
    ];

    #[test]
    fn v8_messages() {
        for (text, want) in CASES {
            assert_eq!(json_parse(text).unwrap_err().message, *want, "{text:?}");
        }
    }

    #[test]
    fn values_and_order() {
        let v = json_parse("{\"b\":1,\"2\":[true,null,\"x\\n\"],\"a\":-0.5e1,\"b\":3}").unwrap();
        assert_eq!(stringify(&v), "{\"2\":[true,null,\"x\\n\"],\"b\":3,\"a\":-5}");
        assert_eq!(
            stringify_pretty(&v, 2),
            "{\n  \"2\": [\n    true,\n    null,\n    \"x\\n\"\n  ],\n  \"b\": 3,\n  \"a\": -5\n}"
        );
        assert_eq!(stringify(&json_parse("{\"a\":{},\"b\":[]}").unwrap()), "{\"a\":{},\"b\":[]}");
        assert_eq!(quote_string("a\u{1}\"\\"), "\"a\\u0001\\\"\\\\\"");
    }
}
