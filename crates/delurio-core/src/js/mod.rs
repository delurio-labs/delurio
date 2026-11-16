//! What the checker reads and writes, held as JavaScript holds it, so that every output is the one the TypeScript
//! checker gave: a value of a graph file (`Value`), an object whose keys keep JavaScript's order (`Object`), and the
//! few behaviours of the language that reach the output: `String(v)`, a number as text, `JSON.stringify`,
//! `JSON.parse` and its error messages, string order (`sort()` and `localeCompare`), `Math.round`, and lengths and
//! offsets counted in UTF-16 code units.

mod json;
mod number;
mod object;
mod order;
mod value;

pub use json::{JsonError, json_parse, quote_string, stringify, stringify_pretty};
pub use number::{number_to_string, round};
pub use object::Object;
pub use order::{cmp_utf16, locale_compare, sort_utf16};
pub use value::{Value, to_js_string, typeof_name};

/// A string's length as JavaScript counts it: in UTF-16 code units.
pub fn len16(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// The byte offset in `s` of the UTF-16 offset `at` (clamped to the string's end; an offset inside a surrogate pair
/// is taken to the character's start).
pub fn byte_at16(s: &str, at: usize) -> usize {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        if units >= at {
            return i;
        }
        units += c.len_utf16();
        if units > at {
            return i;
        }
    }
    s.len()
}

/// The UTF-16 offset of the byte offset `at` in `s`.
pub fn utf16_at(s: &str, at: usize) -> usize {
    len16(&s[..at.min(s.len())])
}

/// `s.slice(start, end)` with UTF-16 offsets, both already in range (`end` None: to the end).
pub fn slice16(s: &str, start: usize, end: Option<usize>) -> &str {
    let a = byte_at16(s, start);
    let b = end.map_or(s.len(), |e| byte_at16(s, e));
    if b <= a { "" } else { &s[a..b] }
}

/// `s.toLocaleLowerCase('tr')`: Turkish lowercase, I to dotless ı and İ to i.
pub fn to_lower_tr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            'I' if chars.peek() == Some(&'\u{0307}') => {
                chars.next();
                out.push('i');
            }
            'I' => out.push('ı'),
            'İ' => out.push('i'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// The characters JavaScript's `trim()` takes off: WhiteSpace and LineTerminator of ECMA-262.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// `s.trim()`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_lengths_and_slices() {
        assert_eq!(len16("ab"), 2);
        assert_eq!(len16("ğü"), 2);
        assert_eq!(len16("a😀b"), 4);
        assert_eq!(slice16("a😀b", 3, None), "b");
        assert_eq!(slice16("abcdef", 1, Some(3)), "bc");
        assert_eq!(utf16_at("a😀b", 5), 3);
    }

    #[test]
    fn turkish_lowercase() {
        assert_eq!(to_lower_tr("IİıiABC"), "ıiıiabc");
    }
}
