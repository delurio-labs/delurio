//! A JavaScript regular expression with the `u` flag (`new RegExp(p, 'u')`, as Ajv builds every `pattern` and
//! `patternProperties` key) written for the regex crate. The two agree on most of the syntax; where they differ in
//! meaning this translates: JavaScript's `\s` is its WhiteSpace and LineTerminator (U+FEFF in, U+0085 out), `\w`,
//! `\d` and `\b` stay ASCII under `u`, and `.` stops at the four line terminators. Anything else that could read
//! differently (lookaround, back references, property escapes, named groups, class set operators) is refused, so a
//! schema that needs it fails when the schemas are compiled instead of matching other strings.

use regex::Regex;

// JavaScript's \s: WhiteSpace (TAB, VT, FF, ZWNBSP, the Zs category) and LineTerminator (LF, CR, LS, PS)
const SPACE: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";
const WORD: &str = "A-Za-z0-9_";
const DIGIT: &str = "0-9";

/// The regex crate's spelling of a JavaScript `u`-flag pattern, or why it cannot be written so.
pub fn translate(pattern: &str) -> Result<String, String> {
    let refuse = |what: &str| Err(format!("the pattern {pattern:?} uses {what}, which the checker does not translate"));
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let Some(&e) = chars.get(i + 1) else { return refuse("a trailing backslash") };
                i += 2;
                match e {
                    's' | 'w' | 'd' => {
                        let set = match e {
                            's' => SPACE,
                            'w' => WORD,
                            _ => DIGIT,
                        };
                        if in_class {
                            out.push_str(set);
                        } else {
                            out.push('[');
                            out.push_str(set);
                            out.push(']');
                        }
                    }
                    'S' | 'W' | 'D' => {
                        let set = match e {
                            'S' => SPACE,
                            'W' => WORD,
                            _ => DIGIT,
                        };
                        out.push_str("[^");
                        out.push_str(set);
                        out.push(']');
                    }
                    'b' if !in_class => out.push_str(r"(?-u:\b)"),
                    'B' if !in_class => out.push_str(r"(?-u:\B)"),
                    'n' => out.push_str(r"\n"),
                    'r' => out.push_str(r"\r"),
                    't' => out.push_str(r"\t"),
                    'f' => out.push_str(r"\x0C"),
                    'v' => out.push_str(r"\x0B"),
                    '0' if !chars.get(i).is_some_and(char::is_ascii_digit) => out.push_str(r"\x00"),
                    'u' => {
                        // \u{X..} or \uXXXX
                        let (code, used) = if chars.get(i) == Some(&'{') {
                            let end = chars[i..].iter().position(|&x| x == '}').map(|p| i + p);
                            let Some(end) = end else { return refuse("an unterminated \\u{") };
                            (chars[i + 1..end].iter().collect::<String>(), end + 1 - i)
                        } else {
                            (chars.get(i..i + 4).map(|x| x.iter().collect::<String>()).unwrap_or_default(), 4)
                        };
                        let Ok(n) = u32::from_str_radix(&code, 16) else { return refuse("a malformed \\u escape") };
                        if (0xD800..0xE000).contains(&n) {
                            return refuse("a surrogate escape");
                        }
                        out.push_str(&format!(r"\x{{{n:X}}}"));
                        i += used;
                    }
                    'x' => {
                        let code: String = chars.get(i..i + 2).map(|x| x.iter().collect()).unwrap_or_default();
                        let Ok(n) = u32::from_str_radix(&code, 16) else { return refuse("a malformed \\x escape") };
                        out.push_str(&format!(r"\x{{{n:X}}}"));
                        i += 2;
                    }
                    // a syntax character, escaped: the same in both (`/` needs no escape in the regex crate)
                    '/' => out.push('/'),
                    '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '-' => {
                        out.push('\\');
                        out.push(e);
                    }
                    _ => return refuse(&format!("the escape \\{e}")),
                }
                continue;
            }
            '[' if in_class => out.push_str(r"\["),
            '[' => {
                in_class = true;
                out.push('[');
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                // JavaScript's `[]` matches nothing and `[^]` anything; the regex crate reads a `]` there as a literal
                if chars.get(i + 1) == Some(&']') {
                    return refuse("an empty class");
                }
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '&' | '~' if in_class && chars.get(i + 1) == Some(&c) => return refuse("a doubled & or ~ in a class"),
            '-' if in_class && chars.get(i + 1) == Some(&'-') => return refuse("a doubled - in a class"),
            '&' | '~' if in_class => {
                out.push('\\');
                out.push(c);
            }
            '.' if !in_class => out.push_str(r"[^\n\r\x{2028}\x{2029}]"),
            '(' if !in_class && chars.get(i + 1) == Some(&'?') => {
                if chars.get(i + 2) != Some(&':') {
                    return refuse("a lookaround or a named group");
                }
                out.push_str("(?:");
                i += 3;
                continue;
            }
            _ => out.push(c),
        }
        i += 1;
    }
    if in_class {
        return refuse("an unterminated class");
    }
    Ok(out)
}

/// `new RegExp(pattern, 'u')`, for `test`.
pub fn compile(pattern: &str) -> Result<Regex, String> {
    let translated = translate(pattern)?;
    Regex::new(&translated).map_err(|e| format!("the pattern {pattern:?} does not compile: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations() {
        assert_eq!(translate(r"^[^\s:]+(:[0-9]+)?$").unwrap(), format!(r"^[^{SPACE}:]+(:[0-9]+)?$"));
        assert_eq!(translate(r"[\w-]+").unwrap(), r"[A-Za-z0-9_-]+");
        assert_eq!(translate(r"\S*$").unwrap(), format!(r"[^{SPACE}]*$"));
        assert!(translate(r"(?=a)").is_err());
        assert!(translate(r"\1").is_err());
        assert!(translate(r"\p{L}").is_err());
        assert!(compile(r"a\bb").is_ok());
    }
}
