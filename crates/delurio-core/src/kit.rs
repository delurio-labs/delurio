//! A kit says how the components of ui.yaml look in one product: design tokens (W3C design tokens JSON, written out as
//! CSS custom properties), a template per component and optional CSS. Templates are data: a few slots and flags are
//! filled in, nothing in a kit is ever run, and anything that could reach the network or run a script is refused when
//! the kit loads. The port of kit.ts.
//!
//! `KitError` of kit.ts is an [`Error`] of kind [`ErrorKind::Kit`](crate::error::ErrorKind::Kit), made by
//! [`Error::kit`]; a YAML error in kit.yaml is returned as `read_yaml` gives it, as the TypeScript checker threw it.

use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::graph::{TONES, read_error};
use crate::js::{self, Object, Value};
use crate::nodepath;
use crate::yaml::read_yaml;

/// `KitError` of kit.ts: an error of kind Kit
pub type KitError = Error;

/// `s` with `&`, `<`, `>` and `"` written as entities
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// a value a template is filled with
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    Text,
    Name,
    Id,
    Intent,
    Children,
    Tone,
}

const SLOTS: [(&str, Slot); 6] = [
    ("text", Slot::Text),
    ("name", Slot::Name),
    ("id", Slot::Id),
    ("intent", Slot::Intent),
    ("children", Slot::Children),
    ("tone", Slot::Tone),
];

/// a flag a section of a template is drawn on
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    Disabled,
    Named,
    Conditional,
    Checked,
}

const FLAGS: [(&str, Flag); 4] = [
    ("disabled", Flag::Disabled),
    ("named", Flag::Named),
    ("conditional", Flag::Conditional),
    ("checked", Flag::Checked),
];

impl Flag {
    fn name(self) -> &'static str {
        FLAGS.iter().find(|(_, f)| *f == self).map_or("", |(n, _)| n)
    }
}

/// `ctx` is where the slot stands: escaping differs only in a single-quoted attribute value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ctx {
    Text,
    Dq,
    Sq,
}

/// a piece of a template: literal text, a slot, or a section drawn when a flag is set (or, negated, when it is not)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Part {
    Lit { lit: String },
    Slot { slot: Slot, ctx: Ctx },
    Section { flag: Flag, negate: bool, parts: Vec<Part> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub source: String,
    pub parts: Vec<Part>,
}

impl Template {
    /// whether the template fills the slot anywhere, inside a section too
    pub fn uses(&self, slot: Slot) -> bool {
        fn any(parts: &[Part], slot: Slot) -> bool {
            parts.iter().any(|p| match p {
                Part::Slot { slot: s, .. } => *s == slot,
                Part::Section { parts, .. } => any(parts, slot),
                Part::Lit { .. } => false,
            })
        }
        any(&self.parts, slot)
    }
}

/// a map written as an object, its keys in their order
fn map<V: Serialize, S: serde::Serializer>(m: &IndexMap<String, V>, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.collect_map(m.iter())
}

fn optional_map<V: Serialize, S: serde::Serializer>(
    m: &Option<IndexMap<String, V>>,
    s: S,
) -> std::result::Result<S::Ok, S::Error> {
    match m {
        Some(m) => s.collect_map(m.iter()),
        None => s.serialize_none(),
    }
}

/// an object read as a map, its keys in their order
struct MapVisitor<V>(std::marker::PhantomData<V>);

impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for MapVisitor<V> {
    type Value = IndexMap<String, V>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("an object")
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut a: A) -> std::result::Result<Self::Value, A::Error> {
        let mut m = IndexMap::new();
        while let Some((k, v)) = a.next_entry()? {
            m.insert(k, v);
        }
        Ok(m)
    }
}

fn read_map<'de, V: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<IndexMap<String, V>, D::Error> {
    d.deserialize_map(MapVisitor(std::marker::PhantomData))
}

/// an object read as a map; undefined or null is none
fn read_optional_map<'de, V: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<IndexMap<String, V>>, D::Error> {
    struct OptionVisitor<V>(std::marker::PhantomData<V>);
    impl<'de, V: Deserialize<'de>> serde::de::Visitor<'de> for OptionVisitor<V> {
        type Value = Option<IndexMap<String, V>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an object, or nothing")
        }
        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> std::result::Result<Self::Value, D2::Error> {
            read_map(d).map(Some)
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, a: A) -> std::result::Result<Self::Value, A::Error> {
            serde::de::Visitor::visit_map(MapVisitor(std::marker::PhantomData), a).map(Some)
        }
    }
    d.deserialize_option(OptionVisitor(std::marker::PhantomData))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KitComponent {
    pub template: Template,
    /// a node whose role has its own template is drawn with it
    #[serde(serialize_with = "map", deserialize_with = "read_map")]
    pub roles: IndexMap<String, Template>,
    /// node intent -> the word {intent} gives; `default` for a node without one
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_map",
        deserialize_with = "read_optional_map"
    )]
    pub intents: Option<IndexMap<String, String>>,
    /// node tone -> the word {tone} gives, read before the kit's own `tones`; `default` for a node without one
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_map",
        deserialize_with = "read_optional_map"
    )]
    pub tones: Option<IndexMap<String, String>>,
}

/// A font the kit carries (`fonts:` in kit.yaml): a file of the kit's folder, embedded in the page and the video stage
/// as a data: URL, so that nothing is fetched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KitFont {
    pub family: String,
    /// as kit.yaml gives it, relative to the kit's folder
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, rename = "unicodeRange", skip_serializing_if = "Option::is_none")]
    pub unicode_range: Option<String>,
    /// the file as a data: URL; none when it cannot be embedded, which the kit's findings say why
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
}

/// the rule of a font file a kit lists that cannot be embedded (SPEC.md §8)
pub const KIT_FONT: &str = "kit-font";
/// the rule of a family the kit's CSS draws in first that the kit does not carry (SPEC.md §8)
pub const KIT_FONT_UNEMBEDDED: &str = "kit-font-unembedded";

/// the most bytes one font file of a kit may have
pub const FONT_FILE_MAX: u64 = 2 * 1024 * 1024;
/// the most bytes a kit's font files may have together: four variable fonts of about 1.5 MB fit
pub const FONTS_MAX: u64 = 8 * 1024 * 1024;

/// What loading a kit found that does not keep it from loading: a font file that cannot be embedded (`kit-font`), and
/// a family its CSS draws in that it does not carry (`kit-font-unembedded`). `check` gives each once per kit.
#[derive(Debug, Clone, PartialEq)]
pub struct KitFinding {
    pub rule: &'static str,
    /// the file it is in
    pub file: String,
    /// where in kit.yaml, for an entry of `fonts`
    pub path: Vec<String>,
    /// the words to find in `file` otherwise
    pub needle: Option<String>,
    /// what it is about, for the fingerprint
    pub element: String,
    pub message: String,
}

/// A kit as `loadKit` gives it; the Node bindings hand it to JavaScript and read it back, as the same object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kit {
    pub id: String,
    pub title: String,
    pub file: String,
    #[serde(serialize_with = "map", deserialize_with = "read_map")]
    pub components: IndexMap<String, KitComponent>,
    /// custom properties from `tokens`
    pub light: Vec<(String, String)>,
    /// the same names, re-pointed for a dark page
    pub dark: Vec<(String, String)>,
    /// the kit's own CSS and each component's, checked, not yet scoped
    pub css: String,
    /// at most how much larger the screens are drawn on the video stage
    pub zoom: f64,
    /// node tone -> the word {tone} gives, for a component that gives none of its own; `default` for a node without one
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "optional_map",
        deserialize_with = "read_optional_map"
    )]
    pub tones: Option<IndexMap<String, String>>,
    /// the fonts it carries, in the order of kit.yaml
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<KitFont>,
    /// what loading it found that does not keep it from loading
    #[serde(skip)]
    pub findings: Vec<KitFinding>,
}

/// What a template is filled with, for one node in one state. Read from JavaScript (the Node bindings' `drawWith`), a
/// key that is not there is absent (an intent, role or children that is null too): a flag not given is not set,
/// where the TypeScript checker, comparing `undefined` with true and false, drew both its `{?flag}` and `{!flag}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fill {
    pub id: String,
    pub text: String,
    /// the accessible name the graph expects; the visible text when it gives none
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// List only: the drawn list items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<String>,
    pub disabled: bool,
    /// the graph gives an accessible name that differs from the visible text
    pub named: bool,
    /// the node appears only when a flag is set (`when`)
    pub conditional: bool,
    /// the node is drawn checked: its `checked` is true
    pub checked: bool,
    /// the tone the node is drawn in (SPEC.md §4): its fixed `tone`, or the first of its map that holds where the
    /// context is known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
}

impl Fill {
    fn flag(&self, flag: Flag) -> bool {
        match flag {
            Flag::Disabled => self.disabled,
            Flag::Named => self.named,
            Flag::Conditional => self.conditional,
            Flag::Checked => self.checked,
        }
    }
}

// Lists are drawn without a wrapper around each item, so their templates must be one element the renderer can mark.
const SINGLE: [&str; 2] = ["List", "ListItem"];

const VOID: [&str; 9] = ["area", "br", "col", "hr", "img", "input", "source", "track", "wbr"];
const DENIED_TAGS: [&str; 17] = [
    "script", "style", "iframe", "frame", "frameset", "object", "embed", "applet", "link", "meta", "base", "form",
    "noscript", "template", "portal", "audio", "video",
];
const URL_ATTRS: [&str; 16] = [
    "href",
    "src",
    "srcset",
    "action",
    "formaction",
    "xlink:href",
    "poster",
    "data",
    "background",
    "ping",
    "codebase",
    "cite",
    "longdesc",
    "manifest",
    "srcdoc",
    "lowsrc",
];

/// whether `hay` holds `needle` (ASCII, lowercase), ASCII letters in any case: a JavaScript regex with the `i` flag
/// and no `u` flag folds no other letter onto an ASCII one
fn contains_ci(hay: &str, needle: &str) -> bool {
    find_ci(hay, needle, 0).is_some()
}

/// the first byte offset from `from` at which `hay` holds `needle` (ASCII, lowercase), ASCII letters in any case
fn find_ci(hay: &str, needle: &str, from: usize) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(from);
    }
    (from..=h.len().saturating_sub(n.len()))
        .filter(|&i| i + n.len() <= h.len())
        .find(|&i| h[i..i + n.len()].iter().zip(n).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

/// whether `s` starts with `prefix` (ASCII, lowercase), ASCII letters in any case
fn starts_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// `s` without its leading JavaScript white space (`\s*`)
fn skip_space(s: &str) -> &str {
    s.trim_start_matches(js::is_js_space)
}

/// `/["'](https?:)?\/\//i.test(s)`: a quoted address in CSS, which image-set() fetches without url()
fn fetched_string(s: &str) -> bool {
    s.char_indices().any(|(i, c)| {
        if c != '"' && c != '\'' {
            return false;
        }
        let rest = &s[i + 1..];
        rest.starts_with("//") || (starts_ci(rest, "http://")) || starts_ci(rest, "https://")
    })
}

/// `s.replace(/url\([^)]*\)/gi, '')`
fn strip_urls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut from = 0;
    while let Some(at) = find_ci(s, "url(", from) {
        let Some(close) = s[at + 4..].find(')') else { break };
        out.push_str(&s[from..at]);
        from = at + 4 + close + 1;
    }
    out.push_str(&s[from..]);
    out
}

/// `/url\(\s*(?!['"]?(data:|#))/i.test(value)`: a url( that does not go on with a data: URL or a `#`. The regex may
/// give back the white space it took, and a white space is neither, so any white space after `url(` matches.
fn fetching_url(value: &str) -> bool {
    let mut from = 0;
    while let Some(at) = find_ci(value, "url(", from) {
        let rest = &value[at + 4..];
        let after = skip_space(rest);
        if after.len() < rest.len() {
            return true;
        }
        let inner = after.strip_prefix(['"', '\'']);
        let safe = |s: &str| starts_ci(s, "data:") || s.starts_with('#');
        if !(inner.is_some_and(safe) || safe(after)) {
            return true;
        }
        from = at + 1;
    }
    false
}

/// `/^[a-z_:][-a-z0-9_:.]*$/.test(attr)`
fn attribute_name(attr: &str) -> bool {
    let mut chars = attr.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase() || c == '_' || c == ':')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | ':' | '.'))
}

fn check_attr(loc: &str, tag: &str, attr: &str, value: &str) -> Result<()> {
    let no = |why: &str| Err(Error::kit(format!("{loc}: <{tag} {attr}>: {why}")));
    if !attribute_name(attr) {
        return no("not an attribute name");
    }
    if attr.starts_with("on") {
        return no("event handlers are not allowed: a kit is data, never code");
    }
    if contains_ci(value, "javascript:") {
        return no("javascript: is not allowed");
    }
    if URL_ATTRS.contains(&attr)
        && !value.is_empty()
        && !value.starts_with('#')
        && !(attr == "src" && value.starts_with("data:image/"))
    {
        return no("may only point inside the page (\"#…\") or, for src, hold a data:image; nothing is fetched");
    }
    if attr == "style"
        && (fetching_url(value)
            || fetched_string(&strip_urls(value))
            || contains_ci(value, "expression(")
            || contains_ci(value, "@import"))
    {
        return no("a style may only use data: URLs");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Text,
    Tag,
    Dq,
    Sq,
    Comment,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Text => "text",
            Mode::Tag => "tag",
            Mode::Dq => "dq",
            Mode::Sq => "sq",
            Mode::Comment => "comment",
        }
    }
}

/// a section that is open: the parts drawn on its flag, and the place it opened in
struct Frame {
    flag: Flag,
    negate: bool,
    key: String,
    parts: Vec<Part>,
}

/// `/^\{([?!/]?)([a-z]+)\}/.exec(rest)`: the marker's length, sigil and word
fn marker(rest: &str) -> Option<(usize, &str, &str)> {
    let body = rest.strip_prefix('{')?;
    let sigil = if body.starts_with(['?', '!', '/']) { &body[..1] } else { "" };
    let after = &body[sigil.len()..];
    let word_len = after.bytes().take_while(u8::is_ascii_lowercase).count();
    if word_len == 0 || after.as_bytes().get(word_len) != Some(&b'}') {
        return None;
    }
    Some((1 + sigil.len() + word_len + 1, sigil, &after[..word_len]))
}

/// the length of the run of characters at the start of `s` that are no JavaScript white space and not in `stop`
fn run_without(s: &str, stop: &[char]) -> usize {
    s.char_indices().find(|(_, c)| js::is_js_space(*c) || stop.contains(c)).map_or(s.len(), |(i, _)| i)
}

/// the length of `[a-zA-Z][a-zA-Z0-9-]*` at the start of `s`
fn tag_name(s: &str) -> usize {
    let b = s.as_bytes();
    if !b.first().is_some_and(u8::is_ascii_alphabetic) {
        return 0;
    }
    1 + b[1..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'-').count()
}

/// `/^<\/([a-zA-Z][a-zA-Z0-9-]*)\s*>/.exec(rest)`: the closing tag's length and its name
fn close_tag(rest: &str) -> Option<(usize, &str)> {
    let body = rest.strip_prefix("</")?;
    let n = tag_name(body);
    if n == 0 {
        return None;
    }
    let after = skip_space(&body[n..]);
    if !after.starts_with('>') {
        return None;
    }
    Some((rest.len() - after.len() + 1, &body[..n]))
}

/// `/^\s*=\s*/.exec(rest)`: its length
fn equals(rest: &str) -> Option<usize> {
    let a = skip_space(rest);
    let b = a.strip_prefix('=')?;
    Some(rest.len() - skip_space(b).len())
}

/// The template parser's state: the parts so far, the sections open, and where in the HTML it is.
struct TemplateParser<'a> {
    loc: &'a str,
    root: Vec<Part>,
    frames: Vec<Frame>,
    lit: String,
    mode: Mode,
    stack: Vec<String>,
    tag: String,
    attr: String,
    opened: usize,
}

impl TemplateParser<'_> {
    fn fail<T>(&self, why: &str) -> Result<T> {
        Err(Error::kit(format!("{}: {why}", self.loc)))
    }
    fn parts(&mut self) -> &mut Vec<Part> {
        match self.frames.last_mut() {
            Some(f) => &mut f.parts,
            None => &mut self.root,
        }
    }
    fn flush(&mut self) {
        if !self.lit.is_empty() {
            let lit = std::mem::take(&mut self.lit);
            self.parts().push(Part::Lit { lit });
        }
    }
    fn key(&self) -> String {
        let opened = if self.mode == Mode::Text { String::new() } else { self.opened.to_string() };
        let attr = if matches!(self.mode, Mode::Dq | Mode::Sq) { self.attr.as_str() } else { "" };
        format!("{}|{}|{opened}|{attr}", self.mode.name(), self.stack.join(">"))
    }
}

/// Reads a template, checks that it is well-formed HTML with slots only where a value can stand, and splits it into
/// parts.
fn parse_template(source: &str, loc: &str, component: &str) -> Result<Template> {
    let src = js::trim(source);
    let mut p = TemplateParser {
        loc,
        root: Vec::new(),
        frames: Vec::new(),
        lit: String::new(),
        mode: Mode::Text,
        stack: Vec::new(),
        tag: String::new(),
        attr: String::new(),
        opened: 0, // tags opened so far: a section that opens inside a tag must close inside the same one
    };
    let mut svg = 0usize;
    let mut value = String::new();
    let mut top_elements = 0;
    let mut top_text = false;
    let mut root_end: Option<usize> = None;
    let mut i = 0;
    while i < src.len() {
        let rest = &src[i..];
        let Some(c) = rest.chars().next() else { break };
        let found = if c == '{' && p.mode != Mode::Comment { marker(rest) } else { None };
        if let Some((whole_len, sigil, word)) = found {
            let whole = &rest[..whole_len];
            if sigil.is_empty() {
                let Some(slot) = SLOTS.iter().find(|(n, _)| *n == word).map(|(_, s)| *s) else {
                    let all = SLOTS.iter().map(|(s, _)| format!("{{{s}}}")).collect::<Vec<_>>().join(" ");
                    return p.fail(&format!("unknown slot {{{word}}}; the slots are {all}"));
                };
                if word == "children" && component != "List" {
                    return p.fail("{children} is only for List");
                }
                if !matches!(p.mode, Mode::Text | Mode::Dq | Mode::Sq) {
                    return p.fail(&format!("{{{word}}} must stand in text or inside a quoted attribute value"));
                }
                if word == "children" && p.mode != Mode::Text {
                    return p.fail("{children} must stand in text");
                }
                if p.mode == Mode::Text && p.stack.is_empty() {
                    top_text = true;
                }
                if p.mode != Mode::Text {
                    value.push_str(whole);
                }
                p.flush();
                let ctx = match p.mode {
                    Mode::Dq => Ctx::Dq,
                    Mode::Sq => Ctx::Sq,
                    _ => Ctx::Text,
                };
                p.parts().push(Part::Slot { slot, ctx });
            } else if sigil == "/" {
                let open = p.frames.last().map(|f| f.flag.name() == word).unwrap_or(false);
                if !open {
                    return p.fail(&format!("{{/{word}}} does not close the section that is open"));
                }
                let same_place = p.frames.last().is_some_and(|f| f.key == p.key());
                if !same_place {
                    return p.fail(&format!(
                        "{{?{word}}}…{{/{word}}} must close in the same place it opens (same element, tag or attribute)"
                    ));
                }
                p.flush();
                if let Some(f) = p.frames.pop() {
                    p.parts().push(Part::Section { flag: f.flag, negate: f.negate, parts: f.parts });
                }
            } else {
                let Some(flag) = FLAGS.iter().find(|(n, _)| *n == word).map(|(_, f)| *f) else {
                    let all = FLAGS.iter().map(|(f, _)| *f).collect::<Vec<_>>().join(", ");
                    return p.fail(&format!("unknown flag {word}; the flags are {all}"));
                };
                if p.frames.iter().any(|f| f.flag == flag) {
                    return p.fail(&format!("{{{sigil}{word}}} inside a section on the same flag"));
                }
                p.flush();
                let key = p.key();
                p.frames.push(Frame { flag, negate: sigil == "!", key, parts: Vec::new() });
            }
            i += whole_len;
            continue;
        }
        if p.mode == Mode::Comment {
            if rest.starts_with("-->") {
                p.mode = Mode::Text;
                p.lit.push_str("-->");
                i += 3;
            } else {
                p.lit.push(c);
                i += c.len_utf8();
            }
            continue;
        }
        if matches!(p.mode, Mode::Dq | Mode::Sq) {
            if c == (if p.mode == Mode::Dq { '"' } else { '\'' }) {
                check_attr(loc, &p.tag, &p.attr, &value)?;
                p.mode = Mode::Tag;
            } else {
                value.push(c);
            }
            p.lit.push(c);
            i += c.len_utf8();
            continue;
        }
        if p.mode == Mode::Tag {
            if c == '>' || rest.starts_with("/>") {
                let self_closing = c == '/';
                let void = VOID.contains(&p.tag.as_str());
                if self_closing && svg == 0 && !void {
                    return p.fail(&format!("<{} />: only void elements and SVG may close themselves", p.tag));
                }
                if !self_closing && !void {
                    p.stack.push(p.tag.clone());
                    if p.tag == "svg" {
                        svg += 1;
                    }
                }
                p.lit.push_str(if self_closing { "/>" } else { ">" });
                i += if self_closing { 2 } else { 1 };
                p.mode = Mode::Text;
                p.attr.clear();
                if p.stack.is_empty() {
                    root_end = Some(i);
                }
                continue;
            }
            if js::is_js_space(c) {
                p.lit.push(c);
                i += c.len_utf8();
                continue;
            }
            let name_len = run_without(rest, &['"', '\'', '>', '/', '=', '{']);
            if name_len == 0 {
                return p.fail(&format!("unexpected \"{c}\" in <{}>", p.tag));
            }
            let name = &rest[..name_len];
            p.attr = name.to_lowercase();
            p.lit.push_str(name);
            i += name_len;
            let Some(eq) = equals(&src[i..]) else {
                check_attr(loc, &p.tag, &p.attr, "")?;
                continue;
            };
            p.lit.push_str(&src[i..i + eq]);
            i += eq;
            let q = src[i..].chars().next();
            if q == Some('"') || q == Some('\'') {
                p.mode = if q == Some('"') { Mode::Dq } else { Mode::Sq };
                value.clear();
                p.lit.push(if q == Some('"') { '"' } else { '\'' });
                i += 1;
            } else {
                let bare_len = run_without(&src[i..], &['"', '\'', '>', '`', '=', '<', '{']);
                if bare_len == 0 {
                    return p.fail(&format!("<{} {}=…>: write the value in quotes", p.tag, p.attr));
                }
                let bare = &src[i..i + bare_len];
                check_attr(loc, &p.tag, &p.attr, bare)?;
                p.lit.push_str(bare);
                i += bare_len;
            }
            continue;
        }
        // text
        if c == '<' {
            if rest.starts_with("<!--") {
                p.mode = Mode::Comment;
                p.lit.push_str("<!--");
                i += 4;
                continue;
            }
            if let Some((len, name)) = close_tag(rest) {
                let name = name.to_lowercase();
                let top = p.stack.pop();
                if top.as_deref() != Some(name.as_str()) {
                    let closes = top.map_or_else(|| "nothing".to_string(), |t| format!("<{t}>"));
                    return p.fail(&format!("</{name}> closes {closes}"));
                }
                if name == "svg" {
                    svg = svg.saturating_sub(1);
                }
                p.lit.push_str(&rest[..len]);
                i += len;
                if p.stack.is_empty() {
                    root_end = Some(i);
                }
                continue;
            }
            let n = tag_name(&rest[1..]);
            if n == 0 {
                return p.fail("a bare \"<\" in text: write &lt;");
            }
            p.tag = rest[1..1 + n].to_lowercase();
            if DENIED_TAGS.contains(&p.tag.as_str()) {
                return p.fail(&format!("<{}> is not allowed in a kit template", p.tag));
            }
            if p.stack.is_empty() {
                top_elements += 1;
            }
            p.opened += 1;
            p.lit.push_str(&rest[..1 + n]);
            i += 1 + n;
            p.mode = Mode::Tag;
            continue;
        }
        if p.stack.is_empty() && !js::is_js_space(c) {
            top_text = true;
        }
        p.lit.push(c);
        i += c.len_utf8();
    }
    p.flush();
    if p.mode != Mode::Text {
        return p.fail("the template ends inside a tag, an attribute value or a comment");
    }
    if let Some(top) = p.stack.last() {
        return p.fail(&format!("<{top}> is not closed"));
    }
    if let Some(f) = p.frames.last() {
        return p.fail(&format!("{{?{}}} is not closed", f.flag.name()));
    }
    if component == "List" && !src.contains("{children}") {
        return p.fail("a List template needs {children}, where its items go");
    }
    if SINGLE.contains(&component)
        && (top_elements != 1 || top_text || root_end != Some(src.len()) || !src.starts_with('<'))
    {
        return p.fail(&format!(
            "a {component} template must be one element (the renderer marks it with the node it draws)"
        ));
    }
    Ok(Template { source: src.to_string(), parts: p.root })
}

fn fill_parts(parts: &[Part], f: &Fill, intent: &str, tone: &str) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            Part::Lit { lit } => out.push_str(lit),
            Part::Slot { slot: Slot::Children, .. } => out.push_str(f.children.as_deref().unwrap_or("")),
            Part::Slot { slot, ctx } => {
                let v = match slot {
                    Slot::Intent => intent,
                    Slot::Tone => tone,
                    Slot::Text => &f.text,
                    Slot::Name => &f.name,
                    Slot::Id | Slot::Children => &f.id,
                };
                if *ctx == Ctx::Sq {
                    out.push_str(&esc(v).replace('\'', "&#39;"));
                } else {
                    out.push_str(&esc(v));
                }
            }
            Part::Section { flag, negate, parts } => {
                if f.flag(*flag) != *negate {
                    out.push_str(&fill_parts(parts, f, intent, tone));
                }
            }
        }
    }
    out
}

/// the template a node is drawn with: its role's, else the component's
fn template_for<'k>(c: &'k KitComponent, f: &Fill) -> &'k Template {
    f.role.as_ref().and_then(|r| c.roles.get(r.as_str())).unwrap_or(&c.template)
}

/// the maps a component's {tone} reads, its own first and then the kit's
fn tone_maps<'k>(kit: &'k Kit, c: &'k KitComponent) -> Vec<&'k IndexMap<String, String>> {
    c.tones.iter().chain(kit.tones.iter()).collect()
}

/// the word the maps give, the first that has an entry for it
fn tone_entry<'k>(maps: &[&'k IndexMap<String, String>], word: &str) -> Option<&'k String> {
    maps.iter().find_map(|m| m.get(word))
}

/// the warning for a tone a template that fills {tone} draws and neither `tones` maps, or None when it is covered
fn tone_uncovered(kit: &Kit, component: &str, c: &KitComponent, f: &Fill, word: &str) -> Option<String> {
    let maps = tone_maps(kit, c);
    if maps.is_empty() || !template_for(c, f).uses(Slot::Tone) || tone_entry(&maps, word).is_some() {
        return None;
    }
    let default = match tone_entry(&maps, "default") {
        Some(d) if !d.is_empty() => d.as_str(),
        _ => "no tone",
    };
    Some(format!("kit {}: {component} has no entry for tone {word} (node {}); drawn as {default}", kit.id, f.id))
}

/// What {tone} gives a node: with no `tones` (the component's or the kit's), its tone itself, or nothing; with them,
/// the word they give its tone, or `default` for a node without one. A tone they do not give, in a template that
/// fills {tone}, is warned of and drawn as `default`.
fn tone_value(kit: &Kit, component: &str, c: &KitComponent, f: &Fill, warn: &mut dyn FnMut(&str)) -> String {
    let maps = tone_maps(kit, c);
    if maps.is_empty() {
        return f.tone.clone().unwrap_or_default();
    }
    if let Some(word) = &f.tone {
        if let Some(v) = tone_entry(&maps, word) {
            return v.clone();
        }
        if let Some(w) = tone_uncovered(kit, component, c, f, word) {
            warn(&w);
        }
    }
    tone_entry(&maps, "default").cloned().unwrap_or_default()
}

/// Warns, as drawing it would, when the node would take `word` in a template that fills {tone} and the kit gives no
/// entry for it: a tone of a node's map that the screen being drawn does not show (SPEC.md §8).
pub fn check_tone(kit: &Kit, component: &str, f: &Fill, word: &str, warn: &mut dyn FnMut(&str)) {
    if let Some(c) = kit.components.get(component)
        && let Some(w) = tone_uncovered(kit, component, c, f, word)
    {
        warn(&w);
    }
}

/// The node drawn with the kit, or None when the kit has no template for the component.
pub fn draw_with(kit: &Kit, component: &str, f: &Fill, warn: &mut dyn FnMut(&str)) -> Option<String> {
    let c = kit.components.get(component)?;
    let mut intent = f.intent.clone().unwrap_or_default();
    if let Some(intents) = &c.intents {
        let default = intents.get("default");
        let mapped = match &f.intent {
            None => default,
            Some(i) => intents.get(i.as_str()),
        };
        if let Some(i) = &f.intent
            && mapped.is_none()
        {
            warn(&format!(
                "kit {}: {component} has no entry for intent {i} (node {}); drawn as {}",
                kit.id,
                f.id,
                default.map_or("no intent", String::as_str)
            ));
        }
        intent = mapped.or(default).cloned().unwrap_or_default();
    }
    let tone = tone_value(kit, component, c, f, warn);
    let t = template_for(c, f);
    Some(fill_parts(&t.parts, f, &intent, &tone))
}

/// `s.slice(start, end)` on UTF-16 code units, negative offsets counted from the end
fn slice_js(units: &[u16], start: isize, end: Option<isize>) -> String {
    let len = units.len() as isize;
    let at = |x: isize| if x < 0 { (len + x).max(0) } else { x.min(len) } as usize;
    let a = at(start);
    let b = end.map_or(units.len(), at);
    if b <= a { String::new() } else { String::from_utf16_lossy(&units[a..b]) }
}

/// A List or ListItem is drawn without a wrapper: the node id goes on the template's one element, and on the page the
/// annotation chips go inside it, last.
pub fn mark(html: &str, id: &str, chips: &str) -> String {
    let units: Vec<u16> = html.encode_utf16().collect();
    let (dq, sq, gt, slash) = (u16::from(b'"'), u16::from(b'\''), u16::from(b'>'), u16::from(b'/'));
    let mut q: Option<u16> = None;
    let mut end: isize = -1;
    let mut i = 0;
    while i < units.len() && end < 0 {
        let c = units[i];
        if let Some(open) = q {
            if c == open {
                q = None;
            }
        } else if c == dq || c == sq {
            q = Some(c);
        } else if c == gt {
            end = if i > 0 && units[i - 1] == slash { i as isize - 1 } else { i as isize };
        }
        i += 1;
    }
    let close = units.windows(2).rposition(|w| w[0] == u16::from(b'<') && w[1] == slash).map_or(-1, |p| p as isize);
    format!(
        "{} data-node=\"{}\"{}{chips}{}",
        slice_js(&units, 0, Some(end)),
        esc(id),
        slice_js(&units, end, Some(close)),
        slice_js(&units, close, None)
    )
}

// ---- CSS

/// `/^(:root|html|body)\b/i.test(sel)`
fn outside_selector(sel: &str) -> bool {
    [":root", "html", "body"].iter().any(|w| {
        starts_ci(sel, w) && !sel.as_bytes().get(w.len()).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    })
}

/// `/^@([a-z-]+)/i.exec(p)?.[1]`
fn at_rule(p: &str) -> Option<&str> {
    let body = p.strip_prefix('@')?;
    let n = body.bytes().take_while(|b| b.is_ascii_alphabetic() || *b == b'-').count();
    if n == 0 { None } else { Some(&body[..n]) }
}

/// `/expression\(|javascript:|-moz-binding|(?:^|[^-\w])behavior\s*:/i.test(css)`: `behavior:` is old Internet
/// Explorer's script binding; `scroll-behavior` and `overscroll-behavior` are not
fn script_like(css: &str) -> bool {
    if contains_ci(css, "expression(") || contains_ci(css, "javascript:") || contains_ci(css, "-moz-binding") {
        return true;
    }
    let mut from = 0;
    while let Some(at) = find_ci(css, "behavior", from) {
        let after_word = at > 0 && css.as_bytes()[at - 1].is_ascii_alphanumeric()
            || at > 0 && matches!(css.as_bytes()[at - 1], b'-' | b'_');
        if !after_word && skip_space(&css[at + 8..]).starts_with(':') {
            return true;
        }
        from = at + 1;
    }
    false
}

/// Checks kit CSS before it is scoped to the wireframe: nothing fetched, no way out of the wireframe's rule, nothing
/// that cannot sit inside it. Fails on the first problem.
fn check_css(css: &str, loc: &str) -> Result<()> {
    let fail = |why: &str| Err(Error::kit(format!("{loc}: {why}")));
    if css.contains('<') {
        return fail("\"<\" is not allowed in kit CSS (percent-encode it in a data: URL)");
    }
    if script_like(css) {
        return fail("script-like CSS is not allowed");
    }
    let mut depth: i64 = 0;
    let mut prelude = String::new();
    let mut i = 0;
    while i < css.len() {
        let rest = &css[i..];
        let Some(c) = rest.chars().next() else { break };
        if rest.starts_with("/*") {
            let Some(end) = css[i + 2..].find("*/") else { return fail("a comment is not closed") };
            i = i + 2 + end + 2;
            continue;
        }
        if c == '"' || c == '\'' {
            // a backslash takes the character after it, whatever it is
            let at = |j: usize| css.get(j..).and_then(|r| r.chars().next());
            let mut j = i + 1;
            while let Some(d) = at(j) {
                if d == c || d == '\n' {
                    break;
                }
                j += d.len_utf8();
                if d == '\\' {
                    j += at(j).map_or(1, char::len_utf8);
                }
            }
            if at(j) != Some(c) {
                return fail("a string is not closed");
            }
            prelude.push_str(&css[i..j + 1]);
            i = j + 1;
            continue;
        }
        if c == '{' {
            let p = js::trim(&prelude);
            let at = at_rule(p);
            if let Some(name) = at
                && !["media", "supports", "container"].contains(&name.to_ascii_lowercase().as_str())
            {
                return fail(&format!(
                    "@{name} is not allowed in kit CSS (only @media, @supports and @container, which can sit inside the wireframe's rule)"
                ));
            }
            if at.is_none() {
                for sel in p.split(',') {
                    if outside_selector(js::trim(sel)) {
                        return fail(&format!(
                            "\"{}\" never matches inside the wireframe: style the screen itself with &",
                            js::trim(sel)
                        ));
                    }
                }
            }
            depth += 1;
            prelude.clear();
        } else if c == '}' {
            depth -= 1;
            if depth < 0 {
                return fail("a \"}\" closes nothing");
            }
            prelude.clear();
        } else if c == ';' {
            if skip_space(&prelude).starts_with('@') {
                return fail(&format!("{} is not allowed in kit CSS", js::trim(&prelude)));
            }
            prelude.clear();
        } else {
            prelude.push(c);
        }
        i += c.len_utf8();
    }
    if depth != 0 {
        return fail("a \"{\" is not closed");
    }
    if skip_space(&prelude).starts_with('@') {
        return fail(&format!("{} is not allowed in kit CSS", js::trim(&prelude)));
    }
    // css.matchAll(/url\(\s*(['"]?)([^'")]*)/gi)
    let mut from = 0;
    while let Some(at) = find_ci(css, "url(", from) {
        let after = skip_space(&css[at + 4..]);
        let inner = after.strip_prefix(['"', '\'']).unwrap_or(after);
        let n = inner.find(['\'', '"', ')']).unwrap_or(inner.len());
        let url = &inner[..n];
        if !url.starts_with("data:") && !url.starts_with('#') {
            return fail(&format!("url({url}) would be fetched; only data: URLs are allowed"));
        }
        from = css.len() - inner.len() + n;
    }
    if fetched_string(&strip_urls(css)) {
        return fail("a quoted address would be fetched; only data: URLs are allowed");
    }
    Ok(())
}

// ---- design tokens

const GENERIC_FAMILIES: [&str; 13] = [
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];
const UNITS: [&str; 6] = ["px", "rem", "em", "%", "ms", "s"];

fn css_name<S: AsRef<str>>(path: &[S], loc: &str) -> Result<String> {
    for seg in path {
        let seg = seg.as_ref();
        if seg.is_empty() || seg.contains(['{', '}', '.']) {
            return Err(Error::kit(format!("{loc}: token name \"{seg}\" may not be empty or contain {{, }} or .")));
        }
    }
    let segs: Vec<String> = path
        .iter()
        .map(|s| {
            // s.replace(/[^A-Za-z0-9_-]+/g, '-')
            let mut out = String::new();
            let mut in_run = false;
            for c in s.as_ref().chars() {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    out.push(c);
                    in_run = false;
                } else if !in_run {
                    out.push('-');
                    in_run = true;
                }
            }
            out
        })
        .collect();
    Ok(format!("--{}", segs.join("-")))
}

/// an alias: the token `from` is written as the token `to`
#[derive(Debug, Clone)]
struct Ref {
    from: String,
    to: String,
}

struct TokenFile {
    decls: Vec<(String, String)>,
    refs: Vec<Ref>,
}

/// `JSON.stringify(v)` of a value that may be undefined
fn json_of(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), js::stringify)
}

/// `v[key]` of a value that may be undefined
fn field<'a>(v: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    v.and_then(|v| v.get(key))
}

/// the reading of one tokens file: the declarations and aliases so far, and every name the set has defined
struct Tokens<'a> {
    file: &'a str,
    all: &'a mut IndexSet<String>,
    decls: Vec<(String, String)>,
    refs: Vec<Ref>,
}

impl Tokens<'_> {
    fn walk(&mut self, node: &Object, path: &[String], kind: Option<&str>) -> Result<()> {
        if node.contains_key("$extends") || node.contains_key("$ref") {
            let at = if path.is_empty() { "the root".to_string() } else { path.join(".") };
            return Err(Error::kit(format!(
                "{}: {at}: $extends and $ref are not supported; write an alias \"{{group.token}}\"",
                self.file
            )));
        }
        for (k, v) in node.iter() {
            if k.starts_with('$') {
                continue;
            }
            let mut here = path.to_vec();
            here.push(k.clone());
            let Some(group) = v.as_object() else {
                return Err(Error::kit(format!(
                    "{}: {} is neither a group nor a token (a token has $value)",
                    self.file,
                    here.join(".")
                )));
            };
            let t = group.get("$type").and_then(Value::as_str).or(kind);
            if group.contains_key("$value") {
                self.token(&here, t, group.get("$value"))?;
            } else {
                self.walk(group, &here, t)?;
            }
        }
        Ok(())
    }

    fn token(&mut self, path: &[String], kind: Option<&str>, value: Option<&Value>) -> Result<()> {
        let loc = format!("{}: {}", self.file, path.join("."));
        let name = css_name(path, self.file)?;
        let t = TokenValue { loc: &loc, name: &name };
        let a = t.alias(value, &mut self.refs)?;
        if let Some(a) = a {
            return self.out(&loc, name.clone(), a);
        }
        let refs = &mut self.refs;
        let v = match kind {
            Some("color") => t.color(value, refs)?,
            Some("dimension" | "duration") => t.dimension(value, refs)?,
            Some("fontFamily") => t.family(value, refs)?,
            Some("fontWeight" | "number") => t.plain(value, refs)?,
            Some("cubicBezier") => {
                let Some(items) = value.and_then(Value::as_array).filter(|a| a.len() == 4) else {
                    return Err(Error::kit(format!("{loc}: a cubicBezier is four numbers")));
                };
                let mut out = Vec::new();
                for x in items {
                    out.push(t.plain(Some(x), refs)?);
                }
                format!("cubic-bezier({})", out.join(", "))
            }
            Some("shadow") => t.shadow(value, refs)?,
            Some("border") => match value.filter(|v| v.is_object()) {
                None => t.plain(value, refs)?,
                Some(o) => {
                    let width = t.dimension(o.get("width"), refs)?;
                    let style = t.plain(o.get("style"), refs)?;
                    let color = t.color(o.get("color"), refs)?;
                    format!("{width} {style} {color}")
                }
            },
            Some("typography") => {
                let Some(o) = value.filter(|v| v.is_object()) else {
                    return Err(Error::kit(format!("{loc}: a typography token is an object")));
                };
                // one custom property per field: --name-font-family, --name-font-size, ...
                for (f, css) in [
                    ("fontFamily", "font-family"),
                    ("fontSize", "font-size"),
                    ("fontWeight", "font-weight"),
                    ("letterSpacing", "letter-spacing"),
                    ("lineHeight", "line-height"),
                ] {
                    let Some(x) = o.get(f) else { continue };
                    let n = format!("{name}-{css}");
                    let refs = &mut self.refs;
                    let v = match f {
                        "fontFamily" => t.family(Some(x), refs)?,
                        "fontSize" | "letterSpacing" => t.dimension(Some(x), refs)?,
                        _ => t.plain(Some(x), refs)?,
                    };
                    self.out(&loc, n, v)?;
                }
                return Ok(());
            }
            None => {
                let o = value.and_then(Value::as_object);
                if o.is_some_and(|o| o.contains_key("unit")) {
                    t.dimension(value, refs)?
                } else if o.is_some_and(|o| o.contains_key("colorSpace") || o.contains_key("hex")) {
                    t.color(value, refs)?
                } else {
                    t.plain(value, refs)?
                }
            }
            Some(other) => return Err(Error::kit(format!("{loc}: $type {other} is not supported yet"))),
        };
        self.out(&loc, name.clone(), v)
    }

    fn out(&mut self, loc: &str, n: String, v: String) -> Result<()> {
        if self.all.contains(&n) {
            return Err(Error::kit(format!("{loc}: {n} is defined twice")));
        }
        self.all.insert(n.clone());
        self.decls.push((n, v));
        Ok(())
    }
}

/// a token's value written as CSS: the token's place in its file and its custom property
struct TokenValue<'a> {
    loc: &'a str,
    name: &'a str,
}

impl TokenValue<'_> {
    fn fail<T>(&self, why: &str) -> Result<T> {
        Err(Error::kit(format!("{}: {why}", self.loc)))
    }

    fn alias(&self, v: Option<&Value>, refs: &mut Vec<Ref>) -> Result<Option<String>> {
        let Some(Value::String(s)) = v else { return Ok(None) };
        // /^\{([^{}]+)\}$/
        let Some(inner) = s.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else { return Ok(None) };
        if inner.is_empty() || inner.contains(['{', '}']) {
            return Ok(None);
        }
        let segs: Vec<&str> = inner.split('.').collect();
        let to = css_name(&segs, self.loc)?;
        refs.push(Ref { from: self.name.to_string(), to: to.clone() });
        Ok(Some(format!("var({to})")))
    }

    fn plain(&self, v: Option<&Value>, refs: &mut Vec<Ref>) -> Result<String> {
        if let Some(a) = self.alias(v, refs)? {
            return Ok(a);
        }
        match v {
            Some(Value::Number(n)) => Ok(js::number_to_string(*n)),
            Some(Value::String(s)) => {
                if s.contains([';', '{', '}', '<', '@', '\\']) || contains_ci(s, "url(") {
                    return self.fail(&format!("{} is not a plain CSS value", js::quote_string(s)));
                }
                Ok(s.clone())
            }
            _ => self.fail(&format!("expected a string or a number, got {}", json_of(v))),
        }
    }

    fn dimension(&self, v: Option<&Value>, refs: &mut Vec<Ref>) -> Result<String> {
        if let Some(o) = v.filter(|v| v.is_object()) {
            let value = o.get("value").and_then(Value::as_f64);
            let unit = o.get("unit").and_then(Value::as_str);
            let (Some(value), Some(unit)) = (value, unit.filter(|u| UNITS.contains(u))) else {
                return self
                    .fail(&format!("a dimension needs a number value and one of the units {}", UNITS.join(" ")));
            };
            return Ok(format!("{}{unit}", js::number_to_string(value)));
        }
        self.plain(v, refs)
    }

    fn color(&self, v: Option<&Value>, refs: &mut Vec<Ref>) -> Result<String> {
        let Some(o) = v.filter(|v| v.is_object()) else { return self.plain(v, refs) };
        let a = o.get("alpha").and_then(Value::as_f64).unwrap_or(1.0);
        if let Some(hex) = o.get_str("hex")
            && hex.len() == 7
            && hex.starts_with('#')
            && hex[1..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            if a == 1.0 {
                return Ok(hex.to_string());
            }
            let n = u32::from_str_radix(&hex[1..], 16).unwrap_or(0);
            return Ok(format!("rgb({} {} {} / {})", n >> 16, (n >> 8) & 255, n & 255, js::number_to_string(a)));
        }
        let comps = match o.get("components") {
            Some(Value::Array(items)) => items
                .iter()
                .map(|x| x.as_f64().map_or_else(|| "none".to_string(), js::number_to_string))
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        let space = o.get("colorSpace").map_or_else(|| "undefined".to_string(), js::to_js_string);
        if comps.is_empty() {
            return self.fail("a colour needs components or a hex");
        }
        let tail = if a == 1.0 { String::new() } else { format!(" / {}", js::number_to_string(a)) };
        if ["oklch", "oklab", "lab", "lch"].contains(&space.as_str()) {
            return Ok(format!("{space}({comps}{tail})"));
        }
        if ["srgb", "srgb-linear", "display-p3", "a98-rgb", "prophoto-rgb", "rec2020", "xyz-d50", "xyz-d65"]
            .contains(&space.as_str())
        {
            return Ok(format!("color({space} {comps}{tail})"));
        }
        self.fail(&format!("colour space {space} is not supported (give a hex as well)"))
    }

    fn family(&self, v: Option<&Value>, refs: &mut Vec<Ref>) -> Result<String> {
        let Some(items) = v.and_then(Value::as_array) else { return self.plain(v, refs) };
        let mut out = Vec::new();
        for f in items {
            let s = self.plain(Some(f), refs)?;
            out.push(if GENERIC_FAMILIES.contains(&s.as_str()) || s.starts_with("var(") || s.starts_with('"') {
                s
            } else {
                format!("\"{}\"", s.replace('"', ""))
            });
        }
        Ok(out.join(", "))
    }

    fn shadow(&self, v: Option<&Value>, refs: &mut Vec<Ref>) -> Result<String> {
        if let Some(items) = v.and_then(Value::as_array) {
            let mut out = Vec::new();
            for x in items {
                out.push(self.shadow(Some(x), refs)?);
            }
            return Ok(out.join(", "));
        }
        if !v.is_some_and(Value::is_object) {
            return self.plain(v, refs);
        }
        let inset = if field(v, "inset") == Some(&Value::Bool(true)) { "inset".to_string() } else { String::new() };
        let x = self.dimension(field(v, "offsetX"), refs)?;
        let y = self.dimension(field(v, "offsetY"), refs)?;
        let blur = self.dimension(field(v, "blur"), refs)?;
        let spread = match field(v, "spread").filter(|s| !s.is_null()) {
            None => "0px".to_string(), // { value: 0, unit: 'px' }
            Some(s) => self.dimension(Some(s), refs)?,
        };
        let color = self.color(field(v, "color"), refs)?;
        Ok([inset, x, y, blur, spread, color].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "))
    }
}

/// a file of the kit, read as `readFileSync(file, 'utf8')` reads it, or Node's message for one that cannot be read
fn read_text(path: &str) -> std::result::Result<String, String> {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| read_error(&e, path))
}

/// Reads one W3C design tokens file (the Design Tokens Community Group format): groups nest, a token has `$value`,
/// `$type` is inherited from its group, and "{group.token}" is an alias, written as var(--group-token).
fn read_tokens(file: &str, all: &mut IndexSet<String>) -> Result<TokenFile> {
    let doc = read_text(file)
        .and_then(|text| js::json_parse(&text).map_err(|e| e.message))
        .map_err(|message| Error::kit(format!("{file}: {message}")))?;
    let Some(root) = doc.as_object() else {
        return Err(Error::kit(format!("{file}: a tokens file is a JSON object")));
    };
    let mut t = Tokens { file, all, decls: Vec::new(), refs: Vec::new() };
    t.walk(root, &[], root.get("$type").and_then(Value::as_str))?;
    Ok(TokenFile { decls: t.decls, refs: t.refs })
}

fn read_token_set(
    dir: &str,
    files: &[String],
    loc: &str,
    known: Option<&IndexSet<String>>,
) -> Result<Vec<(String, String)>> {
    let mut names = IndexSet::new();
    let mut decls = Vec::new();
    let mut refs: Vec<Ref> = Vec::new();
    for f in files {
        let r = read_tokens(&nodepath::join2(dir, f), &mut names)?;
        decls.extend(r.decls);
        refs.extend(r.refs);
    }
    let visible = |n: &str| names.contains(n) || known.is_some_and(|k| k.contains(n));
    for r in &refs {
        if !visible(&r.to) {
            return Err(Error::kit(format!("{loc}: {} refers to {}, which no tokens file defines", r.from, r.to)));
        }
    }
    if let Some(known) = known {
        for (n, _) in &decls {
            if !known.contains(n) {
                return Err(Error::kit(format!("{loc}: dark token {n} has no light value")));
            }
        }
    }
    // an alias cycle would leave every token on it without a value
    let mut edges: IndexMap<String, Vec<String>> = IndexMap::new();
    for r in &refs {
        edges.entry(r.from.clone()).or_default().push(r.to.clone());
    }
    let mut state: IndexMap<String, u8> = IndexMap::new();
    fn visit(
        n: &str,
        trail: &[String],
        edges: &IndexMap<String, Vec<String>>,
        state: &mut IndexMap<String, u8>,
        loc: &str,
    ) -> Result<()> {
        match state.get(n) {
            Some(2) => return Ok(()),
            Some(1) => {
                let mut cycle = trail.to_vec();
                cycle.push(n.to_string());
                return Err(Error::kit(format!("{loc}: alias cycle {}", cycle.join(" -> "))));
            }
            _ => {}
        }
        state.insert(n.to_string(), 1);
        let mut next = trail.to_vec();
        next.push(n.to_string());
        for t in edges.get(n).map(Vec::as_slice).unwrap_or_default() {
            visit(t, &next, edges, state, loc)?;
        }
        state.insert(n.to_string(), 2);
        Ok(())
    }
    for n in edges.keys() {
        visit(n, &[], &edges, &mut state, loc)?;
    }
    Ok(decls)
}

// ---- loading

/// `Object.keys(v)`: an object's keys, an array's or a string's indices
fn js_keys(v: &Value) -> Vec<String> {
    match v {
        Value::Object(o) => o.keys().cloned().collect(),
        Value::Array(a) => (0..a.len()).map(|i| i.to_string()).collect(),
        Value::String(s) => (0..js::len16(s)).map(|i| i.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// `Object.entries(v)`: an object's entries, an array's items or a string's characters by index (a character beyond
/// the BMP is one entry here, two halves in JavaScript)
fn js_entries(v: &Value) -> Vec<(String, Value)> {
    match v {
        Value::Object(o) => o.iter().map(|(k, x)| (k.clone(), x.clone())).collect(),
        Value::Array(a) => a.iter().enumerate().map(|(i, x)| (i.to_string(), x.clone())).collect(),
        Value::String(s) => {
            let mut at = 0;
            s.chars()
                .map(|c| {
                    let e = (at.to_string(), Value::String(c.to_string()));
                    at += c.len_utf16();
                    e
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// a file name or a list of them
fn list(v: Option<&Value>, loc: &str) -> Result<Vec<String>> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let xs: Vec<&Value> = match v {
        Value::Array(items) => items.iter().collect(),
        x => vec![x],
    };
    xs.into_iter()
        .map(|x| {
            x.as_str()
                .map(str::to_string)
                .ok_or_else(|| Error::kit(format!("{loc}: expected a file name or a list of them")))
        })
        .collect()
}

fn only(obj: &Value, keys: &[&str], loc: &str) -> Result<()> {
    for k in js_keys(obj) {
        if !keys.contains(&k.as_str()) {
            return Err(Error::kit(format!("{loc}: unknown key {k} (known: {})", keys.join(", "))));
        }
    }
    Ok(())
}

/// `/^[a-z0-9][a-z0-9-]*$/.test(id)`
fn kit_id(id: &str) -> bool {
    let mut b = id.bytes();
    b.next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

/// the kit in a folder (its kit.yaml), or in a kit.yaml given itself
pub fn load_kit(path: &str) -> Result<Kit> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(Error::kit(format!("kit {path} does not exist")));
    }
    let file = if p.is_dir() { nodepath::join2(path, "kit.yaml") } else { path.to_string() };
    if !std::path::Path::new(&file).exists() {
        return Err(Error::kit(format!("kit {path} has no kit.yaml")));
    }
    let dir = nodepath::dirname(&file);
    // a kit.yaml that cannot be read (a folder, say) is a kit error here; the TypeScript checker threw Node's error
    let raw = read_yaml(&read_text(&file).map_err(Error::kit)?)?;
    if !raw.is_collection() {
        return Err(Error::kit(format!("{file}: empty")));
    }
    if raw.get_str("kind") != Some("kit") {
        return Err(Error::kit(format!("{file} is not a kit (kind: kit is missing)")));
    }
    only(
        &raw,
        &["schema", "kind", "id", "title", "source", "tokens", "dark", "css", "stage", "components", "tones", "fonts"],
        &file,
    )?;
    let Some(id) = raw.get_str("id").filter(|id| kit_id(id)) else {
        return Err(Error::kit(format!("{file}: id must be lowercase letters, digits and dashes")));
    };
    // components: null is read as missing (Object.keys(null) threw a TypeError)
    let comps = raw.get("components").filter(|c| c.is_collection() && !js_keys(c).is_empty());
    let Some(comps) = comps else { return Err(Error::kit(format!("{file}: components is missing"))) };

    let mut components: IndexMap<String, KitComponent> = IndexMap::new();
    let mut css: Vec<String> = Vec::new();
    // each file the kit's CSS is read from, for where a family it names is written
    let mut css_files: Vec<String> = Vec::new();
    for f in list(raw.get("css"), &format!("{file}: css"))? {
        let at = nodepath::join2(&dir, &f);
        // a CSS file that cannot be read is a kit error here; the TypeScript checker threw Node's error
        let text = read_text(&at).map_err(Error::kit)?;
        check_css(&text, &at)?;
        css.push(js::trim(&text).to_string());
        css_files.push(at);
    }
    for (name, spec) in js_entries(comps) {
        let loc = format!("{file}: {name}");
        if name == "Page" {
            return Err(Error::kit(format!(
                "{loc}: Page is the screen itself and has no template; style the screen with & in the kit CSS"
            )));
        }
        if !spec.is_collection() {
            return Err(Error::kit(format!("{loc}: needs a template")));
        }
        only(&spec, &["template", "roles", "intents", "css", "tones"], &loc)?;
        let Some(template) = spec.get_str("template") else {
            return Err(Error::kit(format!("{loc}: needs a template")));
        };
        let mut roles: IndexMap<String, Template> = IndexMap::new();
        for (role, t) in spec.get("roles").filter(|r| !r.is_null()).map(js_entries).unwrap_or_default() {
            let Some(t) = t.as_str() else { return Err(Error::kit(format!("{loc}: roles.{role} must be a template"))) };
            let parsed = parse_template(t, &format!("{loc}: roles.{role}"), &name)?;
            roles.insert(role, parsed);
        }
        // intents: null is read as absent (Object.entries(null) threw a TypeError)
        let mut intents: Option<IndexMap<String, String>> = None;
        if let Some(given) = spec.get("intents").filter(|v| !v.is_null()) {
            let mut words = IndexMap::new();
            for (k, v) in js_entries(given) {
                let Some(v) = v.as_str() else { return Err(Error::kit(format!("{loc}: intents.{k} must be a word"))) };
                words.insert(k, v.to_string());
            }
            intents = Some(words);
        }
        if let Some(c) = spec.get("css") {
            let Some(c) = c.as_str() else { return Err(Error::kit(format!("{loc}: css must be CSS text"))) };
            check_css(c, &format!("{loc}: css"))?;
            css.push(js::trim(c).to_string());
        }
        let tones = read_tones(spec.get("tones"), &loc)?;
        let template = parse_template(template, &format!("{loc}: template"), &name)?;
        components.insert(name, KitComponent { template, roles, intents, tones });
    }

    let light =
        read_token_set(&dir, &list(raw.get("tokens"), &format!("{file}: tokens"))?, &format!("{file}: tokens"), None)?;
    let known: IndexSet<String> = light.iter().map(|(n, _)| n.clone()).collect();
    let dark = read_token_set(
        &dir,
        &list(raw.get("dark"), &format!("{file}: dark"))?,
        &format!("{file}: dark"),
        Some(&known),
    )?;
    let stage = raw.get("stage").filter(|s| !s.is_null()).cloned().unwrap_or(Value::Object(Object::new()));
    only(&stage, &["zoom"], &format!("{file}: stage"))?;
    let zoom = match stage.get("zoom").filter(|z| !z.is_null()) {
        None => Some(1.5),
        Some(z) => z.as_f64().filter(|z| !(*z < 0.5 || *z > 4.0)),
    };
    let Some(zoom) = zoom else { return Err(Error::kit(format!("{file}: stage.zoom must be a number from 0.5 to 4"))) };
    let title = raw.get_str("title").unwrap_or(id).to_string();
    let tones = read_tones(raw.get("tones"), &file)?;
    let (fonts, mut findings) = read_fonts(raw.get("fonts"), &dir, &file, id)?;
    let css = css.join("\n\n");
    // where a family's name is written: the CSS first, then the tokens, then kit.yaml (a component's css)
    let mut files = css_files;
    for key in ["tokens", "dark"] {
        files.extend(list(raw.get(key), &file)?.iter().map(|f| nodepath::join2(&dir, f)));
    }
    files.push(file.clone());
    findings.extend(unembedded(id, &css, &light, &dark, &fonts, &files));
    Ok(Kit {
        id: id.to_string(),
        title,
        file: file.clone(),
        components,
        light,
        dark,
        css,
        zoom,
        tones,
        fonts,
        findings,
    })
}

/// `tones`: a map from a tone, or `default`, to the word {tone} gives (SPEC.md §8); null is absent
fn read_tones(given: Option<&Value>, loc: &str) -> Result<Option<IndexMap<String, String>>> {
    let Some(given) = given.filter(|v| !v.is_null()) else { return Ok(None) };
    let Some(o) = given.as_object() else {
        return Err(Error::kit(format!("{loc}: tones maps a tone to the word {{tone}} gives")));
    };
    let mut words = IndexMap::new();
    for (k, v) in o.iter() {
        if k != "default" && !TONES.contains(&k.as_str()) {
            return Err(Error::kit(format!(
                "{loc}: tones.{k} is not a tone; the tones are {}, and default is the word for a node with none",
                TONES.join(", ")
            )));
        }
        let Some(v) = v.as_str() else { return Err(Error::kit(format!("{loc}: tones.{k} must be a word"))) };
        words.insert(k.clone(), v.to_string());
    }
    Ok(Some(words))
}

// ---- fonts

/// a format a kit's font may be in: its extension, its media type (RFC 8081), its name in `format()`, and whether a
/// file's first four bytes are that format's
type Format = (&'static str, &'static str, &'static str, fn(&[u8]) -> bool);

const FORMATS: [Format; 4] = [
    ("woff2", "font/woff2", "woff2", |b| b == b"wOF2"),
    ("woff", "font/woff", "woff", |b| b == b"wOFF"),
    ("ttf", "font/ttf", "truetype", sfnt),
    ("otf", "font/otf", "opentype", sfnt),
];

/// an OpenType or TrueType file: TrueType outlines, CFF outlines, or Apple's TrueType
fn sfnt(b: &[u8]) -> bool {
    b == [0, 1, 0, 0] || b == b"OTTO" || b == b"true"
}

/// `bytes` in base64 (RFC 4648, with padding)
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let at = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        let digit = |shift: u32| char::from(A[((n >> shift) & 63) as usize]);
        out.push(digit(18));
        out.push(digit(12));
        out.push(if chunk.len() > 1 { digit(6) } else { '=' });
        out.push(if chunk.len() > 2 { digit(0) } else { '=' });
    }
    out
}

/// a family's name as a CSS string can hold it, written in quotes: no quote, backslash, `<` or control character
fn family_name(s: &str) -> bool {
    !js::trim(s).is_empty()
        && !s.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\' | '<' | '>' | ';' | '{' | '}'))
}

/// `font-weight` of an @font-face: a number from 1 to 1000, a range of two, `normal` or `bold`
fn weight_of(v: &Value) -> Option<String> {
    let number = |s: &str| s.parse::<f64>().ok().filter(|n| (1.0..=1000.0).contains(n) && !s.starts_with('+'));
    match v {
        Value::Number(n) if (1.0..=1000.0).contains(n) => Some(js::number_to_string(*n)),
        Value::String(s) => {
            let words: Vec<&str> = s.split_whitespace().collect();
            let ok = match words.as_slice() {
                [w] => matches!(*w, "normal" | "bold") || number(w).is_some(),
                [a, b] => number(a).is_some() && number(b).is_some(),
                _ => false,
            };
            ok.then(|| words.join(" "))
        }
        _ => None,
    }
}

/// `unicode-range`: code points and ranges, `U+0-7F`, `U+0400-04FF`, `U+4??`, separated by commas
fn unicode_range(s: &str) -> bool {
    s.split(',').all(|item| {
        let item = js::trim(item);
        let Some(body) = item.strip_prefix("U+").or_else(|| item.strip_prefix("u+")) else { return false };
        let hex = |h: &str, wild: bool| {
            !h.is_empty() && h.len() <= 6 && h.chars().all(|c| c.is_ascii_hexdigit() || (wild && c == '?'))
        };
        match body.split_once('-') {
            Some((a, b)) => hex(a, false) && hex(b, false),
            None => hex(body, true) && !body.trim_end_matches('?').contains('?'),
        }
    })
}

/// The font file a kit lists, as a data: URL: a woff2, woff, ttf or otf file inside the kit's folder (given relative
/// to it, with no `..` and no link out of it), of that format by its bytes, and at most FONT_FILE_MAX bytes. Err says
/// why it cannot be embedded.
fn font_file(dir: &str, given: &str) -> std::result::Result<(usize, String), String> {
    let absolute = given.starts_with(['/', '\\'])
        || std::path::Path::new(given).is_absolute()
        || given.as_bytes().get(1) == Some(&b':');
    if given.is_empty() || absolute {
        return Err("is not a path relative to the kit's folder: a kit's font is a file in the kit's folder".into());
    }
    if given.split(['/', '\\']).any(|seg| seg == "..") {
        return Err("climbs out of the kit's folder (..): a kit's font is a file in the kit's folder".into());
    }
    let ext = given.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let Some((ext, mime, _, is_format)) = FORMATS.iter().find(|(e, ..)| *e == ext) else {
        return Err("is not a woff2, woff, ttf or otf file by its name".into());
    };
    let at = std::path::Path::new(dir).join(given);
    let missing = || "does not exist in the kit's folder".to_string();
    std::fs::symlink_metadata(&at).map_err(|_| missing())?;
    let real = std::fs::canonicalize(&at).map_err(|_| missing())?;
    let home = std::fs::canonicalize(dir).map_err(|_| missing())?;
    if !real.starts_with(&home) {
        return Err("is a link to a file outside the kit's folder: a kit's font is a file in the kit's folder".into());
    }
    let meta = std::fs::metadata(&real).map_err(|_| missing())?;
    if !meta.is_file() {
        return Err("is not a file".into());
    }
    let too_large =
        |n: u64| format!("is {n} bytes, more than the {FONT_FILE_MAX} bytes (2 MiB) a kit's font file may be");
    if meta.len() > FONT_FILE_MAX {
        return Err(too_large(meta.len()));
    }
    let bytes = std::fs::read(&real).map_err(|e| read_error(&e, given))?;
    if bytes.len() as u64 > FONT_FILE_MAX {
        return Err(too_large(bytes.len() as u64));
    }
    if !bytes.get(..4).is_some_and(is_format) {
        return Err(format!("is no {ext} file by its bytes"));
    }
    Ok((bytes.len(), format!("data:{mime};base64,{}", base64(&bytes))))
}

/// `fonts`: the fonts a kit carries, `{ family, file, weight?, style?, unicodeRange? }` each (SPEC.md §8). An entry
/// that is not one keeps the kit from loading; a file that cannot be embedded is a finding (`kit-font`), and the font
/// is listed without its `src`.
fn read_fonts(given: Option<&Value>, dir: &str, file: &str, id: &str) -> Result<(Vec<KitFont>, Vec<KitFinding>)> {
    let Some(given) = given.filter(|v| !v.is_null()) else { return Ok((Vec::new(), Vec::new())) };
    let Some(entries) = given.as_array() else {
        return Err(Error::kit(format!("{file}: fonts is a list of {{ family, file }}")));
    };
    let mut fonts = Vec::new();
    let mut findings = Vec::new();
    let mut total: u64 = 0;
    for (i, e) in entries.iter().enumerate() {
        let loc = format!("{file}: fonts[{i}]");
        if !e.is_object() {
            return Err(Error::kit(format!("{loc}: a font is {{ family, file }}")));
        }
        only(e, &["family", "file", "weight", "style", "unicodeRange"], &loc)?;
        let Some(family) = e.get_str("family").filter(|f| family_name(f)) else {
            return Err(Error::kit(format!(
                "{loc}: family is the font's name, with no quote, backslash, <, >, ;, {{, }} or control character"
            )));
        };
        let Some(path) = e.get_str("file") else {
            return Err(Error::kit(format!("{loc}: file is the font's file, relative to the kit's folder")));
        };
        let weight = match e.get("weight").filter(|w| !w.is_null()) {
            None => None,
            Some(w) => Some(weight_of(w).ok_or_else(|| {
                Error::kit(format!("{loc}: weight is a number from 1 to 1000, a range of two, normal or bold"))
            })?),
        };
        let style = match e.get("style").filter(|s| !s.is_null()) {
            None => None,
            Some(s) => match s.as_str() {
                Some(s @ ("normal" | "italic" | "oblique")) => Some(s.to_string()),
                _ => return Err(Error::kit(format!("{loc}: style is normal, italic or oblique"))),
            },
        };
        let range = match e.get("unicodeRange").filter(|s| !s.is_null()) {
            None => None,
            Some(r) => match r.as_str().filter(|r| unicode_range(r)) {
                Some(r) => Some(js::trim(r).to_string()),
                None => {
                    return Err(Error::kit(format!(
                        "{loc}: unicodeRange is code points and ranges, U+0-7F, U+0400-04FF, separated by commas"
                    )));
                }
            },
        };
        let found = font_file(dir, path).and_then(|(size, src)| {
            if total + size as u64 > FONTS_MAX {
                return Err(format!(
                    "would make the kit's fonts {} bytes together, more than the {FONTS_MAX} bytes (8 MiB) they may be",
                    total + size as u64
                ));
            }
            total += size as u64;
            Ok(src)
        });
        let src = match found {
            Ok(src) => Some(src),
            Err(why) => {
                findings.push(KitFinding {
                    rule: KIT_FONT,
                    file: file.to_string(),
                    path: vec!["fonts".into(), i.to_string(), "file".into()],
                    needle: None,
                    element: format!("fonts/{path}"),
                    message: format!(
                        "kit {id}: the font file {path} {why}; {} is not embedded, and the screens are drawn in it only where the machine that draws them has it (SPEC.md §8)",
                        js::trim(family)
                    ),
                });
                None
            }
        };
        fonts.push(KitFont {
            family: js::trim(family).to_string(),
            file: path.to_string(),
            weight,
            style,
            unicode_range: range,
            src,
        });
    }
    Ok((fonts, findings))
}

/// The families of a kit's own fonts (`fonts:`), each once, in the order of kit.yaml: what a drawing of its screens
/// should be drawn in, for a tool that asks the browser what it drew.
pub fn kit_families(kit: &Kit) -> Vec<String> {
    let mut out: IndexSet<String> = IndexSet::new();
    for f in &kit.fonts {
        out.insert(f.family.clone());
    }
    out.into_iter().collect()
}

/// a family's name as CSS compares it: ASCII letters in any case, white space collapsed
fn family_key(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

/// the CSS-wide keywords and the system fonts of `font`, which name no family
const NO_FAMILY: [&str; 11] = [
    "inherit",
    "initial",
    "unset",
    "revert",
    "revert-layer",
    "caption",
    "icon",
    "menu",
    "message-box",
    "small-caption",
    "status-bar",
];

/// `s` split at `sep` where it stands outside quotes and parentheses (`sep` ' ' splits at any white space)
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut from) = (0i32, None::<char>, 0);
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if depth == 0 && (c == sep || (sep == ' ' && c.is_whitespace())) => {
                out.push(&s[from..i]);
                from = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[from..]);
    out.into_iter().filter(|p| !(sep == ' ' && p.is_empty())).collect()
}

/// `var(--name)` or `var(--name, fallback)`: the name and the fallback
fn var_of(item: &str) -> Option<(&str, Option<&str>)> {
    let inner = item.get(..4).filter(|p| p.eq_ignore_ascii_case("var("))?;
    let body = item[inner.len()..].strip_suffix(')')?;
    Some(match body.split_once(',') {
        Some((name, fallback)) => (js::trim(name), Some(js::trim(fallback))),
        None => (js::trim(body), None),
    })
}

/// the custom properties a kit's CSS can read: its tokens, light and dark, and those its CSS declares
type Vars<'a> = IndexMap<&'a str, Vec<&'a str>>;

/// The family a `font-family` value draws in first, its stack's first: a custom property's (`var(--font-ui)`) read
/// from the tokens or the CSS, each of its values. A generic family or a keyword names none.
fn first_family(value: &str, vars: &Vars<'_>, depth: usize, out: &mut Vec<String>) {
    let Some(first) = split_top(value, ',').first().copied().map(js::trim) else { return };
    if depth > 10 {
        return;
    }
    if let Some((name, fallback)) = var_of(first) {
        match vars.get(name) {
            Some(values) => values.iter().for_each(|v| first_family(v, vars, depth + 1, out)),
            None => fallback.into_iter().for_each(|f| first_family(f, vars, depth + 1, out)),
        }
        return;
    }
    let name = match first.chars().next() {
        Some(q @ ('"' | '\'')) => first[1..].strip_suffix(q).unwrap_or(&first[1..]).replace('\\', ""),
        _ => first.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    let key = family_key(&name);
    if name.is_empty() || GENERIC_FAMILIES.contains(&key.as_str()) || NO_FAMILY.contains(&key.as_str()) {
        return;
    }
    if !out.iter().any(|n| family_key(n) == key) {
        out.push(name);
    }
}

/// The family a `font` shorthand draws in first: the families follow its size (and its line height, after `/`).
fn shorthand_family(value: &str, vars: &Vars<'_>, depth: usize, out: &mut Vec<String>) {
    let v = js::trim(value);
    if depth > 10 || NO_FAMILY.contains(&v.to_ascii_lowercase().as_str()) {
        return;
    }
    if let Some((name, fallback)) = var_of(v).filter(|_| split_top(v, ' ').len() == 1) {
        match vars.get(name) {
            Some(values) => values.iter().for_each(|x| shorthand_family(x, vars, depth + 1, out)),
            None => fallback.into_iter().for_each(|f| shorthand_family(f, vars, depth + 1, out)),
        }
        return;
    }
    let words = split_top(v, ' ');
    const SIZES: [&str; 10] =
        ["xx-small", "x-small", "small", "medium", "large", "x-large", "xx-large", "xxx-large", "larger", "smaller"];
    // a size has a unit or is a percentage; a number alone is a weight
    let size = |w: &str| {
        let lower = w.to_ascii_lowercase();
        let lead = lower.split('/').next().unwrap_or("");
        SIZES.contains(&lead)
            || (lead.starts_with(|c: char| c.is_ascii_digit() || c == '.')
                && lead.contains(|c: char| c.is_ascii_alphabetic() || c == '%'))
            || ["calc(", "clamp(", "min(", "max("].iter().any(|f| lead.starts_with(f))
    };
    let Some(at) = words.iter().position(|w| size(w)) else { return };
    let mut rest = at + 1;
    if words.get(rest).is_some_and(|w| w.starts_with('/')) {
        rest += if words[rest] == "/" { 2 } else { 1 };
    }
    let families = words.get(rest..).unwrap_or_default().join(" ");
    first_family(&families, vars, depth + 1, out);
}

/// a kit's CSS as declarations, `property: value`, each property in lowercase but a custom property's
fn declarations(css: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    fn take(chunk: &str, out: &mut Vec<(String, String)>) {
        let Some((prop, value)) = chunk.split_once(':') else { return };
        let prop = js::trim(prop);
        if prop.is_empty() || !prop.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return;
        }
        let mut value = js::trim(value);
        if value.len() >= 10 && value[value.len() - 10..].eq_ignore_ascii_case("!important") {
            value = js::trim(&value[..value.len() - 10]);
        }
        let prop = if prop.starts_with("--") { prop.to_string() } else { prop.to_ascii_lowercase() };
        out.push((prop, value.to_string()));
    }
    let mut i = 0;
    while i < css.len() {
        let rest = &css[i..];
        let Some(c) = rest.chars().next() else { break };
        if rest.starts_with("/*") {
            match css[i + 2..].find("*/") {
                Some(end) => i = i + 2 + end + 2,
                None => break,
            }
            continue;
        }
        if c == '"' || c == '\'' {
            let mut j = i + 1;
            while let Some(d) = css[j..].chars().next() {
                j += d.len_utf8();
                if d == c || d == '\n' {
                    break;
                }
                if d == '\\' {
                    j += css[j..].chars().next().map_or(0, char::len_utf8);
                }
            }
            chunk.push_str(&css[i..j]);
            i = j;
            continue;
        }
        match c {
            '{' => chunk.clear(),
            ';' | '}' => {
                take(&chunk, &mut out);
                chunk.clear();
            }
            _ => chunk.push(c),
        }
        i += c.len_utf8();
    }
    take(&chunk, &mut out);
    out
}

/// Each family the kit's CSS draws in first (a `font-family` or `font` declaration's stack, through the custom
/// properties it reads) that is neither a font of the kit nor a generic family: a finding at warning level
/// (`kit-font-unembedded`), once per family, in the first of `files` that writes its name.
fn unembedded(
    id: &str,
    css: &str,
    light: &[(String, String)],
    dark: &[(String, String)],
    fonts: &[KitFont],
    files: &[String],
) -> Vec<KitFinding> {
    let decls = declarations(css);
    let mut vars: Vars<'_> = IndexMap::new();
    for (n, v) in light.iter().chain(dark).chain(decls.iter().filter(|(p, _)| p.starts_with("--"))) {
        vars.entry(n.as_str()).or_default().push(v.as_str());
    }
    let mut named = Vec::new();
    for (prop, value) in &decls {
        match prop.as_str() {
            "font-family" => first_family(value, &vars, 0, &mut named),
            "font" => shorthand_family(value, &vars, 0, &mut named),
            _ => {}
        }
    }
    let carried: IndexSet<String> = fonts.iter().map(|f| family_key(&f.family)).collect();
    named
        .into_iter()
        .filter(|n| !carried.contains(&family_key(n)))
        .map(|n| {
            let file = files.iter().find(|f| read_text(f).is_ok_and(|t| t.contains(n.as_str()))).or(files.first());
            KitFinding {
                rule: KIT_FONT_UNEMBEDDED,
                file: file.cloned().unwrap_or_default(),
                path: Vec::new(),
                needle: Some(n.clone()),
                element: format!("font-family/{n}"),
                message: format!(
                    "kit {id}: its CSS draws in {n}, which the kit does not carry in fonts: the screens are drawn in it only where the machine that draws them has it, and otherwise in the next family of the stack (SPEC.md §8)"
                ),
            }
        })
        .collect()
}

/// `--kit <path>` wins over `kit:` in ui.yaml (a path relative to the graph); `--kit none` draws the fixed components.
pub fn resolve_kit(graph_dir: &str, ui_kit: Option<&str>, flag_value: Option<&str>) -> Result<Option<Kit>> {
    if flag_value == Some("none") {
        return Ok(None);
    }
    if let Some(flag) = flag_value {
        return load_kit(&nodepath::resolve1(flag)).map(Some);
    }
    if let Some(k) = ui_kit {
        return load_kit(&nodepath::resolve(&[graph_dir, k])).map(Some);
    }
    Ok(None)
}

/// what the kit's CSS is for: the page of a flow, or the video stage (`'page'`, `'stage'`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CssTarget {
    Page,
    Stage,
}

/// The CSS a kit adds: its tokens and its CSS, scoped to the wireframe (`.wf.kit`, with native CSS nesting), and the
/// few rules that keep the page's own marks readable or fit the screens to the video stage.
pub fn kit_css(kit: &Kit, target: CssTarget) -> String {
    let decls = |ds: &[(String, String)]| ds.iter().map(|(n, v)| format!("{n}: {v};")).collect::<Vec<_>>().join(" ");
    let mut out: Vec<String> = vec![format!("/* kit {} */", kit.id)];
    // the kit's own fonts, once for the page or the stage, at the top: an @font-face cannot sit inside a rule
    out.extend(kit.fonts.iter().filter_map(font_face));
    if !kit.light.is_empty() {
        out.push(format!(".wf.kit {{ {} }}", decls(&kit.light)));
    }
    if target == CssTarget::Page && !kit.dark.is_empty() {
        out.push(format!("@media (prefers-color-scheme: dark) {{ .wf.kit {{ {} }} }}", decls(&kit.dark)));
    }
    out.push(format!(".wf.kit {{\n{}\n}}", kit.css));
    // a frame is a pure function of time, and a screen is a still picture: nothing in it moves
    out.push(
        ".wf.kit *, .wf.kit *::before, .wf.kit *::after { animation: none !important; transition: none !important; }"
            .to_string(),
    );
    if target == CssTarget::Page {
        // the chips and outlines are the page's marks, so they keep the page's colours whatever the kit names its tokens
        out.push(
            ".screen { --page-muted: var(--muted); --page-line: var(--line); --page-focus: var(--focus); }".into(),
        );
        out.push(
            ".wf.kit .chip { color: var(--page-muted); border-color: var(--page-line); background: none; }".into(),
        );
        out.push(
            ".wf.kit .wf-cond { outline-color: var(--page-muted); } .wf.kit .wf-focus { outline-color: var(--page-focus); }"
                .into(),
        );
        out.push(":where(.wf.kit) button:disabled:not(.wf-btn, .wf-link) { opacity: 1; }".into());
    } else {
        out.push(format!(
            ":root {{ --kit-zoom: {}; }} .page {{ padding: 0; }} .view .appcard {{ padding: 38px 46px 30px; }}",
            js::number_to_string(kit.zoom)
        ));
        out.push(".view .wf.kit { zoom: var(--kit-zoom); padding: calc(26px / var(--kit-zoom)) calc(46px / var(--kit-zoom)) calc(32px / var(--kit-zoom)); }".into());
        out.push(".view .wf.kit .pressed { transform: scale(.95); }".into());
    }
    format!("\n{}\n", out.join("\n"))
}

/// The @font-face of a font the kit carries, its file as a data: URL; None for one that is not embedded, or whose
/// `src` is no font's data: URL (a kit object a caller built)
fn font_face(f: &KitFont) -> Option<String> {
    let src = f.src.as_deref()?;
    let (_, _, format, _) = FORMATS.iter().find(|(_, mime, ..)| src.starts_with(&format!("data:{mime};base64,")))?;
    let data = &src[src.find(',')? + 1..];
    if !family_name(&f.family) || !data.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=')) {
        return None;
    }
    let mut rule = format!("@font-face {{ font-family: \"{}\"; src: url(\"{src}\") format(\"{format}\");", f.family);
    let plain = |s: &Option<String>, ok: &dyn Fn(&str) -> bool| s.as_deref().filter(|v| ok(v)).map(str::to_string);
    if let Some(w) = plain(&f.weight, &|w| weight_of(&Value::from(w)).is_some()) {
        rule.push_str(&format!(" font-weight: {w};"));
    }
    if let Some(s) = plain(&f.style, &|s| matches!(s, "normal" | "italic" | "oblique")) {
        rule.push_str(&format!(" font-style: {s};"));
    }
    if let Some(r) = plain(&f.unicode_range, &unicode_range) {
        rule.push_str(&format!(" unicode-range: {r};"));
    }
    rule.push_str(" font-display: block; }");
    Some(rule)
}

/// Runs once in the video stage, before the first frame: a kit draws at its own sizes, so its screens are drawn at the
/// largest zoom (at most the kit's) at which every one of them fits between the window's top and the caption. One zoom
/// for all, so a screen does not change size from step to step.
pub fn kit_stage_script(kit: &Kit) -> String {
    format!(
        "(() => {{
  const room = 480; // the window's page starts at y 146; captions start near y 627
  const win = document.getElementById('win');
  const views = [...document.querySelectorAll('.view')];
  const fits = (z) => {{
    document.documentElement.style.setProperty('--kit-zoom', String(z));
    return views.every((v) => {{
      const wf = v.querySelector('.wf.kit');
      if (wf === null) return true;
      v.style.display = 'block';
      const h = wf.getBoundingClientRect().height;
      v.style.display = '';
      return h <= room;
    }});
  }};
  win.style.display = 'block';
  let z = {};
  while (z > 0.5 && !fits(z)) z = Math.round((z - 0.05) * 100) / 100;
  win.style.display = '';
}})();",
        js::number_to_string(kit.zoom)
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn notes() -> String {
        format!("{}/../../examples/notes", env!("CARGO_MANIFEST_DIR"))
    }

    /// a copy of examples/notes in a folder of its own, removed when dropped
    pub(crate) struct NotesCopy(pub(crate) String);

    impl NotesCopy {
        pub(crate) fn new(name: &str) -> NotesCopy {
            fn copy(from: &std::path::Path, to: &std::path::Path) {
                std::fs::create_dir_all(to).expect("a folder");
                for e in std::fs::read_dir(from).expect("a folder").flatten() {
                    let at = to.join(e.file_name());
                    if e.path().is_dir() {
                        copy(&e.path(), &at);
                    } else {
                        std::fs::copy(e.path(), at).expect("a copy");
                    }
                }
            }
            let dir = std::env::temp_dir().join(format!("delurio-kit-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            copy(std::path::Path::new(&notes()), &dir);
            NotesCopy(dir.to_string_lossy().into_owned())
        }
        pub(crate) fn edit(&self, file: &str, from: &str, to: &str) {
            let path = format!("{}/{file}", self.0);
            let text = std::fs::read_to_string(&path).expect("the file");
            assert!(text.contains(from), "{file} holds {from}");
            std::fs::write(&path, text.replacen(from, to, 1)).expect("written");
        }
        fn error(&self) -> String {
            load_kit(&format!("{}/kit", self.0)).expect_err("refused").message.replace(&self.0, "<T>")
        }
    }

    impl Drop for NotesCopy {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const TEXT: &str = "    template: '<p class=\"k-text\">{text}</p>'";

    // the CI step "The example kit draws the notes screens, and a kit that is not data is refused" (its drawing is
    // held in wireframe.rs), and the refusals of mutated kits, word for word as the TypeScript checker gave them
    // (work-kit/unit/expect.mjs)
    #[test]
    fn the_example_kit_has_every_component() {
        let kit = load_kit(&format!("{}/kit", notes())).expect("the example kit loads");
        for c in [
            "Heading",
            "Text",
            "Button",
            "LinkButton",
            "TextField",
            "Checkbox",
            "Radio",
            "Select",
            "List",
            "ListItem",
            "Status",
            "Alert",
        ] {
            assert!(kit.components.contains_key(c), "no template for {c}");
        }
        assert!(kit.dark.iter().all(|(n, _)| kit.light.iter().any(|(m, _)| m == n)));
        assert!(kit_css(&kit, CssTarget::Page).contains("--color-accent: #2a54d5;"));
        assert!(!kit_css(&kit, CssTarget::Stage).contains("prefers-color-scheme"));
        assert!(kit_stage_script(&kit).contains("  let z = 1.5;\n"));
        let edit = format!("{}/edit", notes());
        assert_eq!(
            resolve_kit(&edit, Some("../kit"), None).expect("its kit loads").map(|k| k.id),
            Some("example".into())
        );
        assert!(resolve_kit(&edit, Some("../kit"), Some("none")).expect("no kit").is_none());
    }

    #[test]
    fn a_kit_that_is_not_data_is_refused() {
        let t = NotesCopy::new("onclick");
        t.edit("kit/kit.yaml", TEXT, "    template: '<p class=\"k-text\" onclick=\"go()\">{text}</p>'");
        assert_eq!(
            t.error(),
            "<T>/kit/kit.yaml: Text: template: <p onclick>: event handlers are not allowed: a kit is data, never code"
        );
        let t = NotesCopy::new("script");
        t.edit("kit/kit.yaml", TEXT, "    template: '<p class=\"k-text\"><script>go()</script>{text}</p>'");
        assert_eq!(t.error(), "<T>/kit/kit.yaml: Text: template: <script> is not allowed in a kit template");
        let t = NotesCopy::new("tokens");
        t.edit("kit/tokens.json", "\"bg\": {", "\"bg\" {");
        assert_eq!(
            t.error(),
            "<T>/kit/tokens.json: Expected ':' after property name in JSON at position 46 (line 4 column 10)"
        );
        let t = NotesCopy::new("components");
        let yaml = std::fs::read_to_string(format!("{}/kit/kit.yaml", t.0)).expect("kit.yaml");
        let cut = yaml.find("components:").expect("components");
        std::fs::write(format!("{}/kit/kit.yaml", t.0), &yaml[..cut]).expect("written");
        assert_eq!(t.error(), "<T>/kit/kit.yaml: components is missing");
    }

    #[test]
    fn templates_filled() {
        let kit = load_kit(&format!("{}/kit", notes())).expect("the example kit loads");
        let f = Fill {
            id: "save".into(),
            text: "Save".into(),
            name: "Save it".into(),
            intent: Some("confirm".into()),
            role: Some("button".into()),
            disabled: true,
            named: true,
            ..Fill::default()
        };
        let mut warn = |_: &str| {};
        assert_eq!(
            draw_with(&kit, "Button", &f, &mut warn).as_deref(),
            Some("<button type=\"button\" class=\"k-button k-primary\" aria-label=\"Save it\" disabled>Save</button>")
        );
        let link = Fill { role: Some("link".into()), id: "a\"b'c".into(), ..f };
        assert_eq!(
            draw_with(&kit, "Button", &link, &mut warn).as_deref(),
            Some("<a class=\"k-link\" href=\"#a&quot;b'c\" aria-label=\"Save it\">Save</a>")
        );
        assert_eq!(draw_with(&kit, "Nope", &link, &mut warn), None);
    }

    // each from loadKit of the example kit with the Text template replaced (work-kit/gen-mutants.mjs)
    #[test]
    fn template_refusals() {
        let cases = [
            ("<p>{nope}</p>", "unknown slot {nope}; the slots are {text} {name} {id} {intent} {children} {tone}"),
            ("<p {text}>x</p>", "{text} must stand in text or inside a quoted attribute value"),
            ("<p>{?disabled}x{/named}</p>", "{/named} does not close the section that is open"),
            (
                "<p>{?disabled}<b>x{/disabled}</b></p>",
                "{?disabled}…{/disabled} must close in the same place it opens (same element, tag or attribute)",
            ),
            ("<p>{?disabled}{!disabled}x{/disabled}{/disabled}</p>", "{!disabled} inside a section on the same flag"),
            ("<p>x</b>", "</b> closes <p>"),
            ("</p>", "</p> closes nothing"),
            ("<p>a < b</p>", "a bare \"<\" in text: write &lt;"),
            ("<div/>", "<div />: only void elements and SVG may close themselves"),
            ("<p class=>x</p>", "<p class=…>: write the value in quotes"),
            ("<p \"x\">y</p>", "unexpected \"\"\" in <p>"),
            ("<p><!-- c", "the template ends inside a tag, an attribute value or a comment"),
            ("{?disabled}<p>x</p>", "{?disabled} is not closed"),
            (
                "<a href=\"http://x\">x</a>",
                "<a href>: may only point inside the page (\"#…\") or, for src, hold a data:image; nothing is fetched",
            ),
            ("<p style=\"background:url( data:x)\">x</p>", "<p style>: a style may only use data: URLs"),
            ("<p data-x=\"JavaScript:alert(1)\">x</p>", "<p data-x>: javascript: is not allowed"),
            ("<p ONCLICK=\"x\">x</p>", "<p onclick>: event handlers are not allowed: a kit is data, never code"),
            ("<p\u{85}class=\"a\">{text}</p>", "<p \u{85}class>: not an attribute name"),
            ("<p ÉA=\"x\">x</p>", "<p éa>: not an attribute name"),
            ("<svg/>", "<svg />: only void elements and SVG may close themselves"),
        ];
        for (template, why) in cases {
            let err = parse_template(template, "T", "Text").expect_err(template);
            assert_eq!(err.message, format!("T: {why}"), "{template}");
            assert_eq!(err.kind, crate::error::ErrorKind::Kit);
        }
        for ok in [
            "<p style=\"background:url(data:x)\">{text}</p>",
            "<p style=\"background:URL(DATA:x)\">{text}</p>",
            "<p\u{a0}class=\"a\">{text}</p>",
            "<p\u{feff}class=\"a\">{text}</p>",
            "<svg viewBox=\"0 0 1 1\"><path d=\"M0 0\"/></svg><p>{text}</p>",
            "<p title='{text}'>{text}</p>",
        ] {
            assert!(parse_template(ok, "T", "Text").is_ok(), "{ok}");
        }
        let one = |t: &str, c: &str| parse_template(t, "T", c).map(|_| ()).map_err(|e| e.message);
        assert_eq!(one("<ul>{children}</ul>", "List"), Ok(()));
        assert_eq!(
            one("<ul>{children}</ul><!-- c -->", "List"),
            Err("T: a List template must be one element (the renderer marks it with the node it draws)".into())
        );
        assert_eq!(one("<ul></ul>", "List"), Err("T: a List template needs {children}, where its items go".into()));
        assert_eq!(one("<br>", "ListItem"), Ok(()));
    }

    // 10904b5: `scroll-behavior` and `overscroll-behavior` are not script-like, a bare `behavior:` still is
    #[test]
    fn smooth_scrolling_is_no_script() {
        assert!(!script_like(".k-scroll { scroll-behavior: smooth; overscroll-behavior: contain; }"));
        assert!(!script_like("a { SCROLL-BEHAVIOR : auto }"));
        assert!(!script_like("a { x_behavior: 1; x9behavior: 2 }"));
        assert!(script_like(".k-old { behavior: url(x.htc); }"));
        assert!(script_like("behavior:url(x)"));
        assert!(script_like("a{ scroll-behavior: auto; BEHAVIOR : url(x) }"));
        assert!(script_like("é{behavior:url(x)}"));
    }

    // each from loadKit of the example kit with kit.css replaced (work-kit/gen-mutants.mjs)
    #[test]
    fn css_refusals() {
        let cases = [
            ("a < b", "\"<\" is not allowed in kit CSS (percent-encode it in a data: URL)"),
            ("a { behavior : url(x) }", "script-like CSS is not allowed"),
            ("behavior: url(x)", "script-like CSS is not allowed"),
            ("a{behavior:url(x)}", "script-like CSS is not allowed"),
            ("/* unclosed", "a comment is not closed"),
            ("a { b: 'x\ny' }", "a string is not closed"),
            ("a { b: \"\\", "a string is not closed"),
            ("@import url(x);", "@import url(x) is not allowed in kit CSS"),
            (
                "@font-face { }",
                "@font-face is not allowed in kit CSS (only @media, @supports and @container, which can sit inside the wireframe's rule)",
            ),
            ("a, body.x { }", "\"body.x\" never matches inside the wireframe: style the screen itself with &"),
            ("html-x { }", "\"html-x\" never matches inside the wireframe: style the screen itself with &"),
            ("a { } }", "a \"}\" closes nothing"),
            ("a {", "a \"{\" is not closed"),
            ("@charset \"x\"", "@charset \"x\" is not allowed in kit CSS"),
            ("a { background: url(  'https://x') }", "url(https://x) would be fetched; only data: URLs are allowed"),
            ("a{b:url(url(data:x)}", "url(url(data:x) would be fetched; only data: URLs are allowed"),
            ("a { b: \"HTTP://x\" }", "a quoted address would be fetched; only data: URLs are allowed"),
            ("a { b: url(DATA:x) }", "url(DATA:x) would be fetched; only data: URLs are allowed"),
        ];
        for (css, why) in cases {
            assert_eq!(check_css(css, "C").map_err(|e| e.message), Err(format!("C: {why}")), "{css}");
        }
        for ok in [
            "htmlx { }",
            "html_x { }",
            "@MEDIA x { a { } }",
            "a { background: url( \"data:x\") }",
            "a { b: \"x\\\"y\" }",
        ] {
            assert_eq!(check_css(ok, "C").map_err(|e| e.message), Ok(()), "{ok}");
        }
    }

    // the 15 cases of `mark` in work-kit/ts-dump.mjs, as the TypeScript checker drew them
    #[test]
    fn marked_elements() {
        let m = |h: &str| mark(h, "<a\"&>", "<span>c</span>");
        let id = "data-node=\"&lt;a&quot;&amp;&gt;\"";
        assert_eq!(m("<li>x</li>"), format!("<li {id}>x<span>c</span></li>"));
        assert_eq!(m("<br>"), format!("<br {id}<span>c</span>>"));
        assert_eq!(m("<input/>"), format!("<input {id}/<span>c</span>>"));
        assert_eq!(m(""), format!(" {id}<span>c</span>"));
        assert_eq!(m("abc"), format!("ab {id}<span>c</span>c"));
        assert_eq!(m("<li a=\">\">x</li>"), format!("<li a=\">\" {id}>x<span>c</span></li>"));
        assert_eq!(m("x</li>"), format!("x</li {id}<span>c</span></li>"));
        assert_eq!(m("😀<li>x</li>"), format!("😀<li {id}>x<span>c</span></li>"));
        assert_eq!(m("<li a=\"x>"), format!("<li a=\"x {id}<span>c</span>>"));
        assert_eq!(m("/>"), format!(" {id}/<span>c</span>>"));
    }

    #[test]
    fn style_urls() {
        assert!(fetching_url("background:url( data:x)"));
        assert!(fetching_url("background:url(http://x)"));
        assert!(!fetching_url("background:url(data:x)"));
        assert!(!fetching_url("background:URL(DATA:x)"));
        assert!(!fetching_url("background:url('#a')"));
        assert_eq!(strip_urls("a url(x) b URL(y) c url(z"), "a  b  c url(z");
        assert!(fetched_string("x 'HTTPS://y'"));
        assert!(!fetched_string("x 'ftp://y'"));
    }

    // a kit read back from JavaScript keeps its components, roles and intents in their order
    #[test]
    fn kit_maps_read_back() {
        use serde::de::value::{Error as DeError, MapDeserializer, UnitDeserializer};
        let pairs = || MapDeserializer::<_, DeError>::new(vec![("b", "1"), ("a", "2")].into_iter());
        let m: IndexMap<String, String> = read_map(pairs()).expect("a map");
        assert_eq!(m.keys().collect::<Vec<_>>(), ["b", "a"]);
        let some: Option<IndexMap<String, String>> = read_optional_map(pairs()).expect("a map");
        assert_eq!(some.map(|m| m.len()), Some(2));
        let none: Option<IndexMap<String, String>> =
            read_optional_map(UnitDeserializer::<DeError>::new()).expect("none");
        assert_eq!(none, None);
        let target = CssTarget::deserialize(serde::de::value::StrDeserializer::<DeError>::new("stage")).expect("stage");
        assert_eq!(target, CssTarget::Stage);
    }

    // each the value loadKit gave the token (work-kit/gen-mutants.mjs)
    #[test]
    fn token_values() {
        let dir = std::env::temp_dir().join(format!("delurio-kit-token-values-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder");
        let file = dir.join("t.json").to_string_lossy().into_owned();
        let value = |json: &str| {
            std::fs::write(&file, json).expect("written");
            read_tokens(&file, &mut IndexSet::new()).map(|t| t.decls).map_err(|e| e.message.replace(&file, "F"))
        };
        let one = |n: &str, v: &str| Ok(vec![(n.to_string(), v.to_string())]);
        assert_eq!(
            value(r##"{"a":{"$type":"color","$value":{"hex":"#ffffff","alpha":0.5}}}"##),
            one("--a", "rgb(255 255 255 / 0.5)")
        );
        assert_eq!(
            value(r#"{"a":{"$type":"color","$value":{"colorSpace":"oklch","components":[0.5,"x",1],"alpha":0.3}}}"#),
            one("--a", "oklch(0.5 none 1 / 0.3)")
        );
        assert_eq!(
            value(
                r#"{"a":{"$type":"shadow","$value":{"offsetX":"0","offsetY":"1px","blur":"","spread":null,"color":"red"}}}"#
            ),
            one("--a", "0 1px 0px red")
        );
        assert_eq!(value(r#"{"a":{"$type":"dimension","$value":{"value":1e21,"unit":"px"}}}"#), one("--a", "1e+21px"));
        assert_eq!(
            value(r#"{"a":{"$type":"shadow","$value":{"offsetY":"1px","blur":"2px","color":"red"}}}"#),
            Err("F: a: expected a string or a number, got undefined".into())
        );
        assert_eq!(value(r#"{"a b":{"$value":1}}"#), one("--a-b", "1"));
        assert_eq!(
            value(r#"{"a.b":{"$value":1}}"#),
            Err("F: token name \"a.b\" may not be empty or contain {, } or .".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // a Text that draws its tone, and the kit's `tones` (SPEC.md §8)
    const TONED: &str = "    template: '<p class=\"k-text k-{tone}\">{text}</p>'";

    fn text_fill(tone: Option<&str>) -> Fill {
        Fill { id: "t".into(), text: "Hi".into(), name: "Hi".into(), tone: tone.map(str::to_string), ..Fill::default() }
    }

    fn drawn(kit: &Kit, tone: Option<&str>) -> (String, Vec<String>) {
        let mut warnings = Vec::new();
        let html = draw_with(kit, "Text", &text_fill(tone), &mut |m| warnings.push(m.to_string())).expect("drawn");
        (html, warnings)
    }

    #[test]
    fn the_tone_slot_gives_the_word_the_tones_give() {
        // without tones, {tone} is the tone itself, and nothing for a node without one
        let t = NotesCopy::new("tone-plain");
        t.edit("kit/kit.yaml", TEXT, TONED);
        let kit = load_kit(&format!("{}/kit", t.0)).expect("the kit loads");
        assert_eq!(drawn(&kit, Some("danger")).0, "<p class=\"k-text k-danger\">Hi</p>");
        assert_eq!(drawn(&kit, None).0, "<p class=\"k-text k-\">Hi</p>");
        // the kit's tones, and default for a node without one; a tone they lack is warned of and drawn as default
        let t = NotesCopy::new("tone-kit");
        t.edit("kit/kit.yaml", TEXT, TONED);
        t.edit("kit/kit.yaml", "components:\n", "tones: { danger: alarm, default: calm }\ncomponents:\n");
        let kit = load_kit(&format!("{}/kit", t.0)).expect("the kit loads");
        assert_eq!(drawn(&kit, Some("danger")), ("<p class=\"k-text k-alarm\">Hi</p>".into(), vec![]));
        assert_eq!(drawn(&kit, None), ("<p class=\"k-text k-calm\">Hi</p>".into(), vec![]));
        assert_eq!(
            drawn(&kit, Some("success")),
            (
                "<p class=\"k-text k-calm\">Hi</p>".into(),
                vec!["kit example: Text has no entry for tone success (node t); drawn as calm".to_string()]
            )
        );
        let mut warned = Vec::new();
        check_tone(&kit, "Text", &text_fill(None), "info", &mut |m| warned.push(m.to_string()));
        assert_eq!(warned, ["kit example: Text has no entry for tone info (node t); drawn as calm"]);
        // a template that does not draw its tone is held to nothing
        let mut warned = Vec::new();
        let heading = draw_with(&kit, "Heading", &text_fill(Some("success")), &mut |m| warned.push(m.to_string()));
        assert_eq!((heading.as_deref(), warned.len()), (Some("<h2 class=\"k-heading\">Hi</h2>"), 0));
        // a component's own tones are read first
        let t = NotesCopy::new("tone-component");
        t.edit("kit/kit.yaml", TEXT, &format!("{TONED}\n    tones: {{ danger: red }}"));
        t.edit("kit/kit.yaml", "components:\n", "tones: { danger: alarm, success: fine }\ncomponents:\n");
        let kit = load_kit(&format!("{}/kit", t.0)).expect("the kit loads");
        assert_eq!(drawn(&kit, Some("danger")).0, "<p class=\"k-text k-red\">Hi</p>");
        assert_eq!(drawn(&kit, Some("success")).0, "<p class=\"k-text k-fine\">Hi</p>");
        assert_eq!(
            drawn(&kit, Some("warning")).1,
            ["kit example: Text has no entry for tone warning (node t); drawn as no tone"]
        );
        let kit_tones = kit.tones.as_ref().map(|t| t.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>());
        assert_eq!(kit_tones, Some(vec!["danger=alarm".to_string(), "success=fine".to_string()]));
    }

    #[test]
    fn tones_that_are_not_tones_are_refused() {
        for (tones, why) in [
            (
                "{ error: x }",
                "<T>/kit/kit.yaml: tones.error is not a tone; the tones are neutral, info, success, warning, danger, and default is the word for a node with none",
            ),
            ("{ danger: 1 }", "<T>/kit/kit.yaml: tones.danger must be a word"),
            ("[danger]", "<T>/kit/kit.yaml: tones maps a tone to the word {tone} gives"),
        ] {
            let t = NotesCopy::new("tone-refused");
            t.edit("kit/kit.yaml", "components:\n", &format!("tones: {tones}\ncomponents:\n"));
            assert_eq!(t.error(), why, "{tones}");
        }
        let t = NotesCopy::new("tone-refused-component");
        t.edit("kit/kit.yaml", TEXT, &format!("{TEXT}\n    tones: {{ critical: x }}"));
        assert!(t.error().starts_with("<T>/kit/kit.yaml: Text: tones.critical is not a tone"), "{}", t.error());
    }

    /// a file of the copy's kit folder, written
    fn put(t: &NotesCopy, file: &str, bytes: &[u8]) {
        let at = std::path::Path::new(&t.0).join("kit").join(file);
        std::fs::create_dir_all(at.parent().expect("a folder")).expect("its folder");
        std::fs::write(at, bytes).expect("written");
    }

    /// bytes that start as a woff2 file does
    fn woff2(len: usize) -> Vec<u8> {
        let mut b = b"wOF2".to_vec();
        b.resize(len, 7);
        b
    }

    fn with_fonts(t: &NotesCopy, entries: &str) -> Kit {
        t.edit("kit/kit.yaml", "components:\n", &format!("fonts:\n{entries}components:\n"));
        load_kit(&format!("{}/kit", t.0)).expect("the kit loads")
    }

    #[test]
    fn a_kit_carries_its_fonts_once_as_data() {
        let t = NotesCopy::new("fonts");
        put(&t, "fonts/test.woff2", &woff2(5));
        put(&t, "fonts/Test-Italic.TTF", &[0, 1, 0, 0, 9]);
        let kit = with_fonts(
            &t,
            "  - { family: Test Sans, file: fonts/test.woff2, weight: \"400 700\", unicodeRange: \"U+0000-00FF, U+0131\" }\n  - { family: Test Sans, file: fonts/Test-Italic.TTF, weight: 400, style: italic }\n",
        );
        assert_eq!(kit.findings, Vec::new());
        assert_eq!(kit.fonts[0].src.as_deref(), Some("data:font/woff2;base64,d09GMgc="));
        assert_eq!(kit_families(&kit), ["Test Sans"]);
        for target in [CssTarget::Page, CssTarget::Stage] {
            let css = kit_css(&kit, target);
            assert!(css.starts_with(
                "\n/* kit example */\n@font-face { font-family: \"Test Sans\"; src: url(\"data:font/woff2;base64,d09GMgc=\") format(\"woff2\"); font-weight: 400 700; unicode-range: U+0000-00FF, U+0131; font-display: block; }\n@font-face { font-family: \"Test Sans\"; src: url(\"data:font/ttf;base64,AAEAAAk=\") format(\"truetype\"); font-weight: 400; font-style: italic; font-display: block; }\n.wf.kit {"
            ));
            assert_eq!(css.matches("@font-face").count(), 2);
        }
        let f = &kit.fonts[0];
        assert_eq!(
            (f.file.as_str(), f.weight.as_deref(), f.style.as_deref(), f.unicode_range.as_deref()),
            ("fonts/test.woff2", Some("400 700"), None, Some("U+0000-00FF, U+0131"))
        );
        // a kit without fonts says nothing of them
        let example = load_kit(&format!("{}/kit", notes())).expect("the example kit loads");
        assert!(!kit_css(&example, CssTarget::Page).contains("@font-face"));
        assert!(example.fonts.is_empty() && example.tones.is_none());
        assert_eq!(example.findings, Vec::new());
    }

    #[test]
    fn a_font_file_that_cannot_be_embedded_is_a_finding() {
        let t = NotesCopy::new("fonts-refused");
        put(&t, "fonts/fake.woff2", b"<svg>");
        put(&t, "fonts/big.woff2", &woff2(FONT_FILE_MAX as usize + 1));
        put(&t, "fonts/notes.txt", b"wOF2");
        put(&t, "fonts/folder.woff2/x", b"");
        let outside = std::path::Path::new(&t.0).join("outside.woff2");
        std::fs::write(&outside, woff2(8)).expect("written");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, std::path::Path::new(&t.0).join("kit/fonts/link.woff2")).expect("a link");
        let kit = with_fonts(
            &t,
            "  - { family: A, file: fonts/missing.woff2 }\n  - { family: A, file: ../outside.woff2 }\n  - { family: A, file: /etc/a.woff2 }\n  - { family: A, file: fonts/notes.txt }\n  - { family: A, file: fonts/fake.woff2 }\n  - { family: A, file: fonts/big.woff2 }\n  - { family: A, file: fonts/folder.woff2 }\n  - { family: A, file: fonts/link.woff2 }\n",
        );
        let said: Vec<(String, String)> = kit
            .findings
            .iter()
            .map(|f| {
                assert_eq!((f.rule, f.path[0].as_str(), f.path[2].as_str()), (KIT_FONT, "fonts", "file"));
                let why = f.message.split_once(": the font file ").map_or("", |(_, w)| w);
                (f.path[1].clone(), why.split_once("; A is not embedded").map_or(why, |(w, _)| w).to_string())
            })
            .collect();
        let at = |i: &str, why: &str| (i.to_string(), why.to_string());
        let mut want = vec![
            at("0", "fonts/missing.woff2 does not exist in the kit's folder"),
            at("1", "../outside.woff2 climbs out of the kit's folder (..): a kit's font is a file in the kit's folder"),
            at(
                "2",
                "/etc/a.woff2 is not a path relative to the kit's folder: a kit's font is a file in the kit's folder",
            ),
            at("3", "fonts/notes.txt is not a woff2, woff, ttf or otf file by its name"),
            at("4", "fonts/fake.woff2 is no woff2 file by its bytes"),
            at("5", "fonts/big.woff2 is 2097153 bytes, more than the 2097152 bytes (2 MiB) a kit's font file may be"),
            at("6", "fonts/folder.woff2 is not a file"),
        ];
        if cfg!(unix) {
            want.push(at(
                "7",
                "fonts/link.woff2 is a link to a file outside the kit's folder: a kit's font is a file in the kit's folder",
            ));
        }
        assert_eq!(said, want);
        assert_eq!(
            kit.findings[0].message,
            "kit example: the font file fonts/missing.woff2 does not exist in the kit's folder; A is not embedded, and the screens are drawn in it only where the machine that draws them has it (SPEC.md §8)"
        );
        assert!(kit.fonts.iter().all(|f| f.src.is_none()));
        assert_eq!(kit_families(&kit), ["A"]);
        assert!(!kit_css(&kit, CssTarget::Page).contains("@font-face"));
        // together at most FONTS_MAX: the font past it is not embedded
        let t = NotesCopy::new("fonts-total");
        let mut entries = String::new();
        for i in 0..5 {
            put(&t, &format!("f{i}.woff2"), &woff2(1_900_000));
            entries.push_str(&format!("  - {{ family: F{i}, file: f{i}.woff2 }}\n"));
        }
        let kit = with_fonts(&t, &entries);
        assert_eq!(kit.fonts.iter().filter(|f| f.src.is_some()).count(), 4);
        assert_eq!(kit.findings.len(), 1);
        assert!(kit.findings[0].message.contains("the font file f4.woff2 would make the kit's fonts 9500000 bytes together, more than the 8388608 bytes (8 MiB) they may be; F4 is not embedded"));
    }

    #[test]
    fn a_font_entry_that_is_not_one_keeps_the_kit_from_loading() {
        for (entry, why) in [
            (
                "{ family: 'A\"B', file: a.woff2 }",
                "fonts[0]: family is the font's name, with no quote, backslash, <, >, ;, {, }} or control character",
            ),
            ("{ family: A }", "fonts[0]: file is the font's file, relative to the kit's folder"),
            (
                "{ family: A, file: a.woff2, size: 3 }",
                "fonts[0]: unknown key size (known: family, file, weight, style, unicodeRange)",
            ),
            (
                "{ family: A, file: a.woff2, weight: heavy }",
                "fonts[0]: weight is a number from 1 to 1000, a range of two, normal or bold",
            ),
            (
                "{ family: A, file: a.woff2, weight: 0 }",
                "fonts[0]: weight is a number from 1 to 1000, a range of two, normal or bold",
            ),
            ("{ family: A, file: a.woff2, style: slanted }", "fonts[0]: style is normal, italic or oblique"),
            (
                "{ family: A, file: a.woff2, unicodeRange: \"0-7F\" }",
                "fonts[0]: unicodeRange is code points and ranges, U+0-7F, U+0400-04FF, separated by commas",
            ),
            ("x.woff2", "fonts[0]: a font is { family, file }"),
        ] {
            let t = NotesCopy::new("fonts-shape");
            t.edit("kit/kit.yaml", "components:\n", &format!("fonts:\n  - {entry}\ncomponents:\n"));
            assert_eq!(t.error(), format!("<T>/kit/kit.yaml: {}", why.replace("}}", "}")), "{entry}");
        }
        let t = NotesCopy::new("fonts-list");
        t.edit("kit/kit.yaml", "components:\n", "fonts: { family: A }\ncomponents:\n");
        assert_eq!(t.error(), "<T>/kit/kit.yaml: fonts is a list of { family, file }");
    }

    #[test]
    fn the_families_a_kit_draws_in_first() {
        let first = |css: &str, light: &[(&str, &str)]| {
            let light: Vec<(String, String)> = light.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect();
            let fonts = [KitFont {
                family: "instrument  sans".into(),
                file: String::new(),
                weight: None,
                style: None,
                unicode_range: None,
                src: None,
            }];
            unembedded("k", css, &light, &[], &fonts, &[])
                .into_iter()
                .map(|f| f.needle.unwrap_or_default())
                .collect::<Vec<_>>()
        };
        let none: Vec<String> = Vec::new();
        // the first family of each stack, through custom properties; generic families and keywords name none
        assert_eq!(
            first(
                ".a { font-family: \"Instrument Sans\", Roboto, sans-serif } .b { font: italic 600 14px/1.4 var(--reading) } .c { font-family: var(--mono, 'Fallback Mono') !important; }",
                &[("--reading", "\"Literata\", Georgia, serif")]
            ),
            ["Literata", "Fallback Mono"]
        );
        assert_eq!(
            first(
                "& { --ui: Brand   Sans, system-ui; } .a { font-family: var(--ui) } .b { font: inherit } .c { font: 700 1rem / 1.5 'Bar Baz', serif } .d { font: menu }",
                &[]
            ),
            ["Brand Sans", "Bar Baz"]
        );
        assert_eq!(
            first(
                ".a { font-family: system-ui, Roboto } .b { font-family: UI-Monospace } .c { font: var(--x) } .d { font: 700 serif }",
                &[("--x", "12px cursive")]
            ),
            none
        );
        // a comment and a string hold no declaration; the same family is named once
        assert_eq!(
            first(
                "/* font-family: Nope */ .a::after { content: 'font-family: Nope'; font-family: Same } .b { font-family: \"same\" }",
                &[]
            ),
            ["Same"]
        );
    }

    #[test]
    fn the_example_kit_and_medaris_like_tokens() {
        // the example kit leads with system-ui: nothing to warn of
        let example = load_kit(&format!("{}/kit", notes())).expect("the example kit loads");
        assert_eq!(example.findings, Vec::new());
        // a stack written in the tokens, as a kit generated from an app's CSS has it, found in the file that writes it
        let t = NotesCopy::new("fonts-tokens");
        t.edit(
            "kit/tokens.json",
            "[\"system-ui\", \"Segoe UI\", \"Roboto\", \"sans-serif\"]",
            "[\"Brand Sans\", \"Segoe UI\", \"sans-serif\"]",
        );
        let kit = load_kit(&format!("{}/kit", t.0)).expect("the kit loads");
        assert_eq!(kit.findings.len(), 1);
        let f = &kit.findings[0];
        assert_eq!(
            (f.rule, f.needle.as_deref(), f.element.as_str()),
            (KIT_FONT_UNEMBEDDED, Some("Brand Sans"), "font-family/Brand Sans")
        );
        assert!(f.file.ends_with("/kit/tokens.json"), "{}", f.file);
        assert_eq!(
            f.message,
            "kit example: its CSS draws in Brand Sans, which the kit does not carry in fonts: the screens are drawn in it only where the machine that draws them has it, and otherwise in the next family of the stack (SPEC.md §8)"
        );
        // carried, it is no finding
        put(&t, "brand.woff", b"wOFFxyz");
        let kit = with_fonts(&t, "  - { family: Brand Sans, file: brand.woff }\n");
        assert_eq!(kit.findings, Vec::new());
        assert!(
            kit_css(&kit, CssTarget::Page)
                .contains("src: url(\"data:font/woff;base64,d09GRnh5eg==\") format(\"woff\");")
        );
    }

    #[test]
    fn base64_and_font_values() {
        for (plain, coded) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
        for ok in ["U+0-7F", "U+0400-04FF, u+0131", "U+4??", "U+10FFFF"] {
            assert!(unicode_range(ok), "{ok}");
        }
        for no in ["0-7F", "U+", "U+1234567", "U+4?4", "U+?-7F", "U+0-7F;x"] {
            assert!(!unicode_range(no), "{no}");
        }
        assert_eq!(weight_of(&Value::from("100  900")), Some("100 900".into()));
        assert_eq!(weight_of(&Value::Number(400.0)), Some("400".into()));
        assert_eq!(weight_of(&Value::from("bold")), Some("bold".into()));
        for no in ["+400", "100 200 300", "1001", "inf", "x"] {
            assert_eq!(weight_of(&Value::from(no)), None, "{no}");
        }
        assert!(!family_name("a\nb") && !family_name("  ") && family_name("Noto Naskh Arabic"));
        // a kit object a caller built embeds only a font's data: URL
        let f = |src: &str| KitFont {
            family: "A".into(),
            file: String::new(),
            weight: Some("bold; color: red".into()),
            style: None,
            unicode_range: None,
            src: Some(src.into()),
        };
        assert_eq!(
            font_face(&f("data:font/woff2;base64,AAAA")).as_deref(),
            Some(
                "@font-face { font-family: \"A\"; src: url(\"data:font/woff2;base64,AAAA\") format(\"woff2\"); font-display: block; }"
            )
        );
        assert_eq!(font_face(&f("https://x/a.woff2")), None);
        assert_eq!(font_face(&f("data:font/woff2;base64,AA\")")), None);
    }
}
