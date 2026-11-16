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
use crate::graph::read_error;
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
}

const SLOTS: [(&str, Slot); 5] = [
    ("text", Slot::Text),
    ("name", Slot::Name),
    ("id", Slot::Id),
    ("intent", Slot::Intent),
    ("children", Slot::Children),
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

fn fill_parts(parts: &[Part], f: &Fill, intent: &str) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            Part::Lit { lit } => out.push_str(lit),
            Part::Slot { slot: Slot::Children, .. } => out.push_str(f.children.as_deref().unwrap_or("")),
            Part::Slot { slot, ctx } => {
                let v = match slot {
                    Slot::Intent => intent,
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
                    out.push_str(&fill_parts(parts, f, intent));
                }
            }
        }
    }
    out
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
    let t = f.role.as_ref().and_then(|r| c.roles.get(r.as_str())).unwrap_or(&c.template);
    Some(fill_parts(&t.parts, f, &intent))
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

/// `/expression\(|javascript:|-moz-binding|behavior\s*:/i.test(css)`
fn script_like(css: &str) -> bool {
    if contains_ci(css, "expression(") || contains_ci(css, "javascript:") || contains_ci(css, "-moz-binding") {
        return true;
    }
    let mut from = 0;
    while let Some(at) = find_ci(css, "behavior", from) {
        if skip_space(&css[at + 8..]).starts_with(':') {
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
    only(&raw, &["schema", "kind", "id", "title", "source", "tokens", "dark", "css", "stage", "components"], &file)?;
    let Some(id) = raw.get_str("id").filter(|id| kit_id(id)) else {
        return Err(Error::kit(format!("{file}: id must be lowercase letters, digits and dashes")));
    };
    // components: null is read as missing (Object.keys(null) threw a TypeError)
    let comps = raw.get("components").filter(|c| c.is_collection() && !js_keys(c).is_empty());
    let Some(comps) = comps else { return Err(Error::kit(format!("{file}: components is missing"))) };

    let mut components: IndexMap<String, KitComponent> = IndexMap::new();
    let mut css: Vec<String> = Vec::new();
    for f in list(raw.get("css"), &format!("{file}: css"))? {
        let at = nodepath::join2(&dir, &f);
        // a CSS file that cannot be read is a kit error here; the TypeScript checker threw Node's error
        let text = read_text(&at).map_err(Error::kit)?;
        check_css(&text, &at)?;
        css.push(js::trim(&text).to_string());
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
        only(&spec, &["template", "roles", "intents", "css"], &loc)?;
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
        let template = parse_template(template, &format!("{loc}: template"), &name)?;
        components.insert(name, KitComponent { template, roles, intents });
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
    Ok(Kit { id: id.to_string(), title, file: file.clone(), components, light, dark, css: css.join("\n\n"), zoom })
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
            ("<p>{nope}</p>", "unknown slot {nope}; the slots are {text} {name} {id} {intent} {children}"),
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

    // each from loadKit of the example kit with kit.css replaced (work-kit/gen-mutants.mjs)
    #[test]
    fn css_refusals() {
        let cases = [
            ("a < b", "\"<\" is not allowed in kit CSS (percent-encode it in a data: URL)"),
            ("a { behavior : url(x) }", "script-like CSS is not allowed"),
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
        let dir = std::env::temp_dir().join(format!("delurio-kit-tokens-{}", std::process::id()));
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
}
