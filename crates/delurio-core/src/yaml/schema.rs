//! The yaml library's schema/ folder as a YAML 1.2 document uses it: the core schema's tags (schema/core/*.ts,
//! schema/common/*.ts) and the known tags a `!!` tag may name (`coreKnownTags` of schema/tags.ts: binary, merge, omap,
//! pairs, set, timestamp, from schema/yaml-1.1/). Each tag's test and resolve are ported; a known tag's value that
//! the library holds as an object of its own (Buffer, Date, Symbol, Map, Set) is approximated, delurio refusing tags
//! before it reads a value; what these tags report as errors is ported.

use std::sync::LazyLock;

use regex::Regex;

use crate::js::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    Map,
    Seq,
    Str,
    Null,
    Bool,
    IntOct,
    Int,
    IntHex,
    FloatNaN,
    FloatExp,
    Float,
    Binary,
    Merge,
    Omap,
    Pairs,
    Set,
    Timestamp,
}

/// a tag's `collection`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Collection {
    Map,
    Seq,
}

impl Collection {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Collection::Map => "map",
            Collection::Seq => "seq",
        }
    }
}

pub(crate) const MAP_TAG: &str = "tag:yaml.org,2002:map";
pub(crate) const SEQ_TAG: &str = "tag:yaml.org,2002:seq";
pub(crate) const STR_TAG: &str = "tag:yaml.org,2002:str";

/// the core schema's tags, in its order
pub(crate) const CORE: [Tag; 11] = [
    Tag::Map,
    Tag::Seq,
    Tag::Str,
    Tag::Null,
    Tag::Bool,
    Tag::IntOct,
    Tag::Int,
    Tag::IntHex,
    Tag::FloatNaN,
    Tag::FloatExp,
    Tag::Float,
];

/// `schema.knownTags[name]`
pub(crate) fn known_tag(name: &str) -> Option<Tag> {
    match name {
        "tag:yaml.org,2002:binary" => Some(Tag::Binary),
        "tag:yaml.org,2002:merge" => Some(Tag::Merge),
        "tag:yaml.org,2002:omap" => Some(Tag::Omap),
        "tag:yaml.org,2002:pairs" => Some(Tag::Pairs),
        "tag:yaml.org,2002:set" => Some(Tag::Set),
        "tag:yaml.org,2002:timestamp" => Some(Tag::Timestamp),
        _ => None,
    }
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|_| unreachable!("a constant pattern"))
}

static NULL: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:~|[Nn]ull|NULL)?$"));
static BOOL: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:[Tt]rue|TRUE|[Ff]alse|FALSE)$"));
static INT_OCT: LazyLock<Regex> = LazyLock::new(|| re(r"^0o[0-7]+$"));
static INT: LazyLock<Regex> = LazyLock::new(|| re(r"^[-+]?[0-9]+$"));
static INT_HEX: LazyLock<Regex> = LazyLock::new(|| re(r"^0x[0-9a-fA-F]+$"));
static FLOAT_NAN: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:[-+]?\.(?:inf|Inf|INF)|\.nan|\.NaN|\.NAN)$"));
static FLOAT_EXP: LazyLock<Regex> = LazyLock::new(|| re(r"^[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)[eE][-+]?[0-9]+$"));
static FLOAT: LazyLock<Regex> = LazyLock::new(|| re(r"^[-+]?(?:\.[0-9]+|[0-9]+\.[0-9]*)$"));
static MERGE: LazyLock<Regex> = LazyLock::new(|| re(r"^<<$"));
static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    re(concat!(
        r"^([0-9]{4})-([0-9]{1,2})-([0-9]{1,2})",
        r"(?:(?:t|T|[ \t]+)([0-9]{1,2}):([0-9]{1,2}):([0-9]{1,2}(\.[0-9]+)?)",
        r"(?:[ \t]*(Z|[-+][012]?[0-9](?::[0-9]{2})?))?)?$"
    ))
});

/// what a scalar's text resolves to: its value, and whether the library holds it as an object of its own
pub(crate) struct Resolved {
    pub value: Value,
    pub opaque: bool,
}

impl Tag {
    /// `tag.tag`
    pub(crate) fn name(self) -> &'static str {
        match self {
            Tag::Map => MAP_TAG,
            Tag::Seq => SEQ_TAG,
            Tag::Str => STR_TAG,
            Tag::Null => "tag:yaml.org,2002:null",
            Tag::Bool => "tag:yaml.org,2002:bool",
            Tag::IntOct | Tag::Int | Tag::IntHex => "tag:yaml.org,2002:int",
            Tag::FloatNaN | Tag::FloatExp | Tag::Float => "tag:yaml.org,2002:float",
            Tag::Binary => "tag:yaml.org,2002:binary",
            Tag::Merge => "tag:yaml.org,2002:merge",
            Tag::Omap => "tag:yaml.org,2002:omap",
            Tag::Pairs => "tag:yaml.org,2002:pairs",
            Tag::Set => "tag:yaml.org,2002:set",
            Tag::Timestamp => "tag:yaml.org,2002:timestamp",
        }
    }

    /// `tag.collection`
    pub(crate) fn collection(self) -> Option<Collection> {
        match self {
            Tag::Map | Tag::Set => Some(Collection::Map),
            Tag::Seq | Tag::Omap | Tag::Pairs => Some(Collection::Seq),
            _ => None,
        }
    }

    /// `tag.default === true` (merge's default is 'key', binary's and the collections' false)
    pub(crate) fn is_default(self) -> bool {
        !matches!(self, Tag::Binary | Tag::Merge | Tag::Omap | Tag::Pairs | Tag::Set)
    }

    /// `tag.test`
    fn test_regex(self) -> Option<&'static Regex> {
        Some(match self {
            Tag::Null => &NULL,
            Tag::Bool => &BOOL,
            Tag::IntOct => &INT_OCT,
            Tag::Int => &INT,
            Tag::IntHex => &INT_HEX,
            Tag::FloatNaN => &FLOAT_NAN,
            Tag::FloatExp => &FLOAT_EXP,
            Tag::Float => &FLOAT,
            Tag::Merge => &MERGE,
            Tag::Timestamp => &TIMESTAMP,
            _ => return None,
        })
    }

    pub(crate) fn has_test(self) -> bool {
        self.test_regex().is_some()
    }

    /// `tag.test.test(value)`
    pub(crate) fn test(self, value: &str) -> bool {
        self.test_regex().is_some_and(|r| r.is_match(value))
    }

    /// `tag.resolve(str)` of a scalar tag; Err is the message of what it throws
    pub(crate) fn resolve(self, s: &str) -> Result<Resolved, String> {
        let plain = |value: Value| Ok(Resolved { value, opaque: false });
        match self {
            Tag::Null => plain(Value::Null),
            Tag::Bool => plain(Value::Bool(s.starts_with(['t', 'T']))),
            Tag::IntOct => plain(Value::Number(parse_int_pow2(s.get(2..).unwrap_or(""), 3))),
            Tag::Int => plain(Value::Number(parse_int_10(s))),
            Tag::IntHex => plain(Value::Number(parse_int_pow2(s.get(2..).unwrap_or(""), 4))),
            Tag::FloatNaN => {
                let last3: String = s.chars().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
                plain(Value::Number(if last3.to_lowercase() == "nan" {
                    f64::NAN
                } else if s.starts_with('-') {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                }))
            }
            Tag::FloatExp | Tag::Float => plain(Value::Number(s.parse::<f64>().unwrap_or(f64::NAN))),
            Tag::Binary | Tag::Merge => Ok(Resolved { value: Value::String(s.to_string()), opaque: true }),
            Tag::Timestamp => {
                if TIMESTAMP.is_match(s) {
                    Ok(Resolved { value: Value::String(s.to_string()), opaque: true })
                } else {
                    Err("!!timestamp expects a date, starting with yyyy-mm-dd".to_string())
                }
            }
            // strings, and the collections' tags, which a scalar never resolves with
            _ => plain(Value::String(s.to_string())),
        }
    }
}

/// `parseInt(str, 10)` of `[-+]?[0-9]+`: the decimal value, correctly rounded
fn parse_int_10(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// `parseInt(digits, 8)` or `parseInt(digits, 16)` (`bits` 3 or 4): V8 rounds the value to the nearest double, ties
/// to even
fn parse_int_pow2(digits: &str, bits: u32) -> f64 {
    let mut mantissa: u64 = 0;
    let mut length: u32 = 0; // significant bits read so far
    let mut exponent: i32 = 0;
    let mut round_bit = false;
    let mut sticky = false;
    for c in digits.chars() {
        let Some(d) = c.to_digit(1 << bits) else { break };
        for k in (0..bits).rev() {
            let bit = (d >> k) & 1 == 1;
            if length == 0 && !bit {
                continue;
            }
            if length < 53 {
                mantissa = (mantissa << 1) | u64::from(bit);
                length += 1;
            } else {
                if length == 53 {
                    round_bit = bit;
                } else {
                    sticky |= bit;
                }
                length += 1;
                exponent += 1;
            }
        }
    }
    if round_bit && (sticky || mantissa & 1 == 1) {
        mantissa += 1;
        if mantissa == 1 << 53 {
            mantissa >>= 1;
            exponent += 1;
        }
    }
    let value = mantissa as f64;
    if exponent == 0 { value } else { value * 2f64.powi(exponent) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_octal_round_as_v8_does() {
        // values from node: parseInt('1fffffffffffff1', 16) etc.
        assert_eq!(parse_int_pow2("ff", 4), 255.0);
        assert_eq!(parse_int_pow2("777", 3), 511.0);
        assert_eq!(parse_int_pow2("20000000000001", 4), 9007199254740992.0);
        assert_eq!(parse_int_pow2("20000000000003", 4), 9007199254740996.0);
        assert_eq!(parse_int_pow2("1fffffffffffff8", 4), 144115188075855870.0);
        assert_eq!(parse_int_pow2("", 4), 0.0);
    }
}
