//! The navigation graph (kind: nav): pages, the entries that lead from one to another, and the people who use them.
//! It reuses the guard language of the flow graph; a person is a fixed set of facts, so no search over contexts is needed.
//! The port of nav.ts (SPEC.md §6).
//!
//! What the graph's files say as they were written (frames, facts, guards, the entries as written, goals, the source,
//! the catalog) is kept as it was read (`Value`) and read through the views below (`FrameSpec`, `FactSpec`,
//! `EntrySpec`, `NavSource`, `ExtractCfg`, `Goal`), which give a key of the wrong type as absent; what `load_nav`
//! computes (pages, entries, personas) and what is computed from it (reaches, views, menu rows, findings) are typed,
//! and serialize with the property names of the TypeScript objects.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::LazyLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde::{Serialize, Serializer};

use crate::error::{Error, Result};
use crate::expr::{Expr, Op, evaluate, identifiers, parse_expr};
use crate::graph::{given, read_file};
use crate::js::{self, Object, Value};
use crate::nodepath;
use crate::yaml::read_yaml;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PageKind {
    Screen,
    Placeholder,
    Missing,
    Redirect,
    External,
}

impl PageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PageKind::Screen => "screen",
            PageKind::Placeholder => "placeholder",
            PageKind::Missing => "missing",
            PageKind::Redirect => "redirect",
            PageKind::External => "external",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        PAGE_KINDS.iter().position(|k| *k == s).map(|i| PAGE_KIND_VALUES[i])
    }
}

pub type FrameId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    Auto,
    Menu,
    Picker,
    User,
    Link,
    Button,
}

impl How {
    pub fn as_str(self) -> &'static str {
        match self {
            How::Auto => "auto",
            How::Menu => "menu",
            How::Picker => "picker",
            How::User => "user",
            How::Link => "link",
            How::Button => "button",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        HOWS.iter().position(|k| *k == s).map(|i| HOW_VALUES[i])
    }
}

const PAGE_KINDS: [&str; 5] = ["screen", "placeholder", "missing", "redirect", "external"];
const PAGE_KIND_VALUES: [PageKind; 5] =
    [PageKind::Screen, PageKind::Placeholder, PageKind::Missing, PageKind::Redirect, PageKind::External];
const HOWS: [&str; 6] = ["auto", "menu", "picker", "user", "link", "button"];
const HOW_VALUES: [How; 6] = [How::Auto, How::Menu, How::Picker, How::User, How::Link, How::Button];
// an entry of the frame (menu, scope picker, person row) is drawn even around a page that refuses the person
const FRAME_LEVEL: [How; 3] = [How::Menu, How::Picker, How::User];

/// a value as a template literal writes it: `undefined` when the key is not there
fn text(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), js::to_js_string)
}

/// `x ?? {}` read as an object: a value that is no object is an empty one
fn object_or_empty(v: Option<&Value>) -> Object {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

/// `x ?? []` read as a list: a value that is no list is an empty one
fn list_or_empty(v: Option<&Value>) -> Vec<Value> {
    v.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `x ?? null` as text: absent and null are None, a string is itself, another value its template-literal text
fn text_or_null(v: &Value, key: &str) -> Option<String> {
    given(v, key).map(js::to_js_string)
}

/// a `Map` (or an object built key by key) serialized as an object, its keys in the order they were set
fn map_fields<V: Serialize, S: Serializer>(m: &IndexMap<String, V>, s: S) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut out = s.serialize_map(Some(m.len()))?;
    for (k, v) in m {
        out.serialize_entry(k, v)?;
    }
    out.end()
}

/// a JavaScript object serialized as the object it is, its keys in its order
fn object_fields<S: Serializer>(o: &Object, s: S) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut m = s.serialize_map(Some(o.len()))?;
    for (k, v) in o.iter() {
        m.serialize_entry(k, v)?;
    }
    m.end()
}

// A frame is the chrome drawn around a page. `menu` names the menu it carries (a frame without one has none);
// `shows: { fact }` makes its pages draw the menu of the frame the person's fact names (a page outside any scope shows the menu of the scope the person lands in).
#[derive(Debug, Clone, Copy)]
pub struct FrameSpec<'a>(pub &'a Value);

impl<'a> FrameSpec<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn title(&self) -> Option<&'a str> {
        self.0.get_str("title")
    }
    /// `f.title` as a template literal writes it
    pub fn title_text(&self) -> String {
        text(self.0.get("title"))
    }
    /// `f.menu !== undefined`: the frame carries a menu
    pub fn has_menu(&self) -> bool {
        self.0.get("menu").is_some()
    }
    /// `f.menu ?? ..`: the menu's name, when it is given
    pub fn menu(&self) -> Option<String> {
        given(self.0, "menu").map(js::to_js_string)
    }
    /// `f.shows` (any value: `f.shows !== undefined` asks whether the key is there)
    pub fn shows(&self) -> Option<&'a Value> {
        self.0.get("shows")
    }
    /// `f.shows.fact` as a template literal writes it, when the frame shows another frame's menu
    pub fn shows_fact(&self) -> Option<String> {
        self.shows().map(|s| text(s.get("fact")))
    }
    pub fn source(&self) -> Option<&'a str> {
        self.0.get_str("source")
    }
}

/// a fact a persona gives a value for: one of `values`, or a list of `of`
#[derive(Debug, Clone, Copy)]
pub struct FactSpec<'a>(pub &'a Value);

impl<'a> FactSpec<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    /// `values`, unless null
    pub fn values(&self) -> Option<&'a Value> {
        given(self.0, "values")
    }
    /// `of`, unless null
    pub fn of(&self) -> Option<&'a Value> {
        given(self.0, "of")
    }
    pub fn say(&self) -> Option<&'a str> {
        self.0.get_str("say")
    }
}

/// an entry as it is written in nav.yaml, before its `from` is spread over the pages it names
#[derive(Debug, Clone, Copy)]
pub struct EntrySpec<'a>(pub &'a Value);

impl<'a> EntrySpec<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn id(&self) -> Option<&'a str> {
        self.0.get_str("id")
    }
    /// `from`: a page, `frame:<frame>`, or a list of them
    pub fn from(&self) -> Option<&'a Value> {
        self.0.get("from")
    }
    /// `from` as the list of its tokens, each as a template literal writes it
    pub fn from_tokens(&self) -> Vec<String> {
        match self.from() {
            Some(Value::Array(items)) => items.iter().map(js::to_js_string).collect(),
            other => vec![text(other)],
        }
    }
    pub fn to(&self) -> Option<&'a str> {
        self.0.get_str("to")
    }
    /// `to` as a template literal writes it
    pub fn to_text(&self) -> String {
        text(self.0.get("to"))
    }
    pub fn how(&self) -> Option<&'a str> {
        self.0.get_str("how")
    }
    pub fn label(&self) -> Option<&'a str> {
        self.0.get_str("label")
    }
    /// `when ?? null`
    pub fn when(&self) -> Option<&'a str> {
        given(self.0, "when").and_then(Value::as_str)
    }
    pub fn clicks(&self) -> Option<f64> {
        given(self.0, "clicks").and_then(Value::as_f64)
    }
    pub fn data(&self) -> bool {
        self.0.get("data") == Some(&Value::Bool(true))
    }
    pub fn back(&self) -> bool {
        self.0.get("back") == Some(&Value::Bool(true))
    }
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.0.get(key)
    }
}

/// what the extractor is told (SPEC.md §6)
#[derive(Debug, Clone, Copy)]
pub struct ExtractCfg<'a>(pub &'a Value);

impl<'a> ExtractCfg<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn adapter(&self) -> Option<&'a str> {
        self.0.get_str("adapter")
    }
    pub fn names(&self) -> Option<&'a Object> {
        self.0.get_object("names")
    }
    pub fn titles(&self) -> Option<&'a Object> {
        self.0.get_object("titles")
    }
    pub fn locales(&self) -> Vec<&'a str> {
        crate::graph::strings(self.0.get("locales"))
    }
    // fields laid over a generated entry, keyed by its ends as it is cited (`<from>-><to>`, then its how or its label
    // where two share their ends), or skip with the reason it is left out (SPEC.md §6)
    pub fn entries(&self) -> Option<&'a Object> {
        self.0.get_object("entries")
    }
}

// where the graph was read from (SPEC.md §6): `locales` is the app's message file or every file it reads, `{locale}` in each
#[derive(Debug, Clone, Copy)]
pub struct NavSource<'a>(pub &'a Value);

impl<'a> NavSource<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn repo(&self) -> Option<&'a str> {
        self.0.get_str("repo")
    }
    pub fn commit(&self) -> Option<&'a str> {
        self.0.get_str("commit")
    }
    pub fn app(&self) -> Option<&'a str> {
        self.0.get_str("app")
    }
    pub fn menu(&self) -> Option<&'a str> {
        self.0.get_str("menu")
    }
    /// `locales`: one file or a list of them
    pub fn locales(&self) -> Vec<&'a str> {
        match self.0.get("locales") {
            Some(Value::String(s)) => vec![s.as_str()],
            other => crate::graph::strings(other),
        }
    }
    /// any other file the graph names
    pub fn get(&self, file: &str) -> Option<&'a Value> {
        self.0.get(file)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub route: String,
    pub title: String, // plain words (catalog applied)
    pub title_key: Option<String>,
    pub kind: PageKind,
    pub frame: FrameId,
    pub source: Option<String>,
    pub opens_for: Option<String>,
    pub page_codes: Option<String>,
    pub evidence: Option<String>,
    pub refusal: Option<String>, // how the page says no, in words
    pub by_address: bool,        // people open it by its address, typed or followed from outside the app (SPEC.md §6)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub spec: usize, // index of the line it was written on
    pub from: String,
    pub of_frame: Option<FrameId>, // the frame selector it was written with (frame:ders), null when it starts at one page
    pub to: String,
    pub how: How,
    pub label: String,
    pub label_key: Option<String>,
    pub when: Option<String>,
    pub clicks: f64,
    pub data: bool,
    pub back: bool,
    pub order: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Persona {
    pub id: String,
    pub title: String,
    pub say: String,
    #[serde(serialize_with = "object_fields")]
    pub facts: Object,
}

/// a thing a person comes to do: reach `page` within `within` clicks, for each persona of `for`
#[derive(Debug, Clone, Copy)]
pub struct Goal<'a>(pub &'a Value);

impl<'a> Goal<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn id(&self) -> Option<&'a str> {
        self.0.get_str("id")
    }
    /// `goal.id` as a template literal writes it
    pub fn id_text(&self) -> String {
        text(self.0.get("id"))
    }
    pub fn say(&self) -> Option<&'a str> {
        self.0.get_str("say")
    }
    /// `goal.say` as a template literal writes it
    pub fn say_text(&self) -> String {
        text(self.0.get("say"))
    }
    /// `goal.page` as a template literal writes it, and the key `pages[goal.page]` looks up
    pub fn page(&self) -> String {
        text(self.0.get("page"))
    }
    /// `for..of goal.for`: the items of a list, the characters of a text, else none
    pub fn for_who(&self) -> Vec<Value> {
        match self.0.get("for") {
            Some(Value::Array(items)) => items.clone(),
            Some(Value::String(s)) => s.chars().map(|c| Value::String(c.to_string())).collect(),
            _ => Vec::new(),
        }
    }
    /// `goal.within` (any value)
    pub fn within(&self) -> Option<&'a Value> {
        self.0.get("within")
    }
    /// `goal.within` as a template literal writes it
    pub fn within_text(&self) -> String {
        text(self.0.get("within"))
    }
}

impl Serialize for Goal<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavGraph {
    pub dir: String,
    pub adapter: Option<String>,
    #[serde(serialize_with = "object_fields")]
    pub frames: Object,
    pub menu_frames: Vec<String>, // frames that carry a menu, in the order they are declared
    pub id: String,
    pub title: String,
    pub status: String,
    pub summary: String,
    pub locale: String,
    pub source: Value,
    #[serde(serialize_with = "object_fields")]
    pub facts: Object,
    #[serde(serialize_with = "object_fields")]
    pub guards: Object,
    #[serde(serialize_with = "map_fields")]
    pub pages: IndexMap<String, Page>,
    pub entries: Vec<Entry>,
    pub specs: Vec<Value>,
    pub personas: Vec<Persona>,
    pub goals: Vec<Value>,
    pub not_modeled: Vec<String>,
    pub catalog: Value,
}

impl NavGraph {
    /// `g.frames[name]` (a frame that is not there reads as one with no keys)
    pub fn frame(&self, name: &str) -> FrameSpec<'_> {
        FrameSpec(self.frames.get(name).unwrap_or(&Value::Null))
    }
    /// `g.specs[i]`, the entry as it is written
    pub fn spec(&self, i: usize) -> EntrySpec<'_> {
        EntrySpec(self.specs.get(i).unwrap_or(&Value::Null))
    }
    /// `g.goals`
    pub fn goals(&self) -> impl Iterator<Item = Goal<'_>> {
        self.goals.iter().map(Goal)
    }
    /// `g.facts[name]`
    pub fn fact(&self, name: &str) -> Option<FactSpec<'_>> {
        self.facts.get(name).map(FactSpec)
    }
    /// `g.guards[name]`: the guard's expression
    pub fn guard(&self, name: &str) -> Option<&str> {
        self.guards.get(name).and_then(Value::as_str)
    }
    /// `g.source`
    pub fn source(&self) -> NavSource<'_> {
        NavSource(&self.source)
    }
    /// `g.catalog[key]`, when the catalog has it
    pub fn message(&self, key: &str) -> Option<String> {
        self.catalog.as_object().and_then(|c| c.get(key)).map(js::to_js_string)
    }
    /// `g.pages[id]`
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.get(id)
    }
    /// `g.personas.find((p) => p.id === id)`
    pub fn persona(&self, id: &str) -> Option<&Persona> {
        self.personas.iter().find(|p| p.id == id)
    }
}

/// `loadNav(..)`: the graph and what was found odd while reading it
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LoadedNav {
    pub g: NavGraph,
    pub warnings: Vec<String>,
}

pub fn frame_level(e: &Entry) -> bool {
    FRAME_LEVEL.contains(&e.how)
}

fn graph_error(message: String) -> Error {
    Error::graph(message, None)
}

/// `{ ...v }` into `target`: an object's own keys, a list's indices, a text's characters
fn spread(target: &mut Object, v: Option<&Value>) {
    match v {
        Some(Value::Object(o)) => {
            for (k, x) in o.iter() {
                target.insert(k.clone(), x.clone());
            }
        }
        Some(Value::Array(items)) => {
            for (i, x) in items.iter().enumerate() {
                target.insert(i.to_string(), x.clone());
            }
        }
        Some(Value::String(s)) => {
            for (i, u) in s.encode_utf16().enumerate() {
                let c = char::decode_utf16([u]).next().and_then(|r| r.ok()).unwrap_or('\u{FFFD}');
                target.insert(i.to_string(), Value::String(c.to_string()));
            }
        }
        _ => {}
    }
}

// nav.yaml may extend a generated file (nav.extracted.yaml): its pages, guards and entries come first, and the hand-written
// file adds to them or overrides single fields of a page. A guard may not be defined twice with different text.
fn merge_docs(base: &Value, hand: &Value) -> Result<Object> {
    let hand_pages = hand.get_object("pages");
    let mut pages = Object::new();
    for (id, p) in base.get_object("pages").into_iter().flat_map(Object::iter) {
        let mut page = Object::new();
        spread(&mut page, Some(p));
        spread(&mut page, hand_pages.and_then(|h| h.get(id)));
        pages.insert(id.clone(), Value::Object(page));
    }
    for (id, p) in hand_pages.into_iter().flat_map(Object::iter) {
        if pages.contains_key(id) {
            continue;
        }
        if p.get("route").is_none() {
            return Err(graph_error(format!(
                "page {id} is not in the extracted file and gives no route (a misspelt override?)"
            )));
        }
        pages.insert(id.clone(), p.clone());
    }
    let mut guards = Object::new();
    spread(&mut guards, base.get("guards"));
    for (name, src) in hand.get_object("guards").into_iter().flat_map(Object::iter) {
        if guards.get(name).is_some_and(|old| !old.strict_eq(src)) {
            return Err(graph_error(format!(
                "guard {name} is written in both the extracted file and nav.yaml, with different text"
            )));
        }
        guards.insert(name.clone(), src.clone());
    }
    let mut doc = hand.as_object().cloned().unwrap_or_default();
    doc.insert("pages", Value::Object(pages));
    doc.insert("guards", Value::Object(guards));
    let mut entries = list_or_empty(base.get("entries"));
    entries.extend(list_or_empty(hand.get("entries")));
    doc.insert("entries", Value::Array(entries));
    let mut source = Object::new();
    spread(&mut source, base.get("source"));
    spread(&mut source, hand.get("source"));
    doc.insert("source", Value::Object(source));
    Ok(doc)
}

/// `s.replace(pattern, replacement)` with a string pattern: the first occurrence, `$$`, `$&`, `` $` `` and `$'` read
/// as JavaScript reads them in the replacement
fn replace_first(s: &str, pattern: &str, replacement: &str) -> String {
    let Some(at) = s.find(pattern) else { return s.to_string() };
    let (before, after) = (&s[..at], &s[at + pattern.len()..]);
    let mut rep = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            rep.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => rep.push('$'),
            Some('&') => rep.push_str(pattern),
            Some('`') => rep.push_str(before),
            Some('\'') => rep.push_str(after),
            _ => {
                rep.push('$');
                continue;
            }
        }
        chars.next();
    }
    format!("{before}{rep}{after}")
}

static LOOKS_LIKE_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)+$").expect("the pattern compiles"));

/// `loadNav(dir, locale)`: the navigation graph in a folder, read from the disk
pub fn load_nav(dir: &str, locale: Option<&str>) -> Result<LoadedNav> {
    load_nav_with(dir, locale, &|file| read_file(dir, file))
}

// `read` takes a path relative to `dir`; the revision diff passes one that reads a git revision instead of the disk
pub fn load_nav_with(dir: &str, locale: Option<&str>, read: &dyn Fn(&str) -> Result<String>) -> Result<LoadedNav> {
    let raw = read_yaml(&read("nav.yaml")?)?;
    // an `extends` that is not text, and a file that is not a map, are read as if they were not there
    let doc = match raw.get("extends") {
        Some(Value::String(extends)) => merge_docs(&read_yaml(&read(extends)?)?, &raw)?,
        _ => raw.as_object().cloned().unwrap_or_default(),
    };
    nav_of(dir, locale, doc, read)
}

/// the rest of `loadNav`: the graph of nav.yaml's keys, merged with the file it extends
fn nav_of(dir: &str, locale: Option<&str>, doc: Object, read: &dyn Fn(&str) -> Result<String>) -> Result<LoadedNav> {
    let mut warnings: Vec<String> = Vec::new();
    if doc.get("kind").and_then(Value::as_str) != Some("nav") {
        return Err(graph_error(format!(
            "{} is not a navigation graph (kind: nav is missing)",
            nodepath::join2(dir, "nav.yaml")
        )));
    }
    // a graph that names no catalog file is read with an empty one (the schema rule reports it)
    let catalog = match doc.get("catalog") {
        Some(Value::String(named)) => {
            let file = match locale {
                None => named.clone(),
                Some(l) => replace_first(named, &format!("{}.json", text(doc.get("locale"))), &format!("{l}.json")),
            };
            js::json_parse(&read(&file)?).map_err(|e| graph_error(e.message))?
        }
        _ => Value::Object(Object::new()),
    };
    let catalog_keys = catalog.as_object().cloned().unwrap_or_default();
    let guards = object_or_empty(doc.get("guards"));
    let facts = object_or_empty(doc.get("facts"));
    let sets = object_or_empty(doc.get("sets"));

    let known = |name: &str| facts.contains_key(name) || guards.contains_key(name);
    let check_guard = |src: &Value, place: &str| -> Result<()> {
        // a guard that is not text reads as no tokens at all
        let Value::String(src) = src else {
            return Err(graph_error(format!("{place}: unexpected end in: {}", js::to_js_string(src))));
        };
        let e = parse_expr(src).map_err(|m| graph_error(format!("{place}: {m}")))?;
        for name in identifiers(&e) {
            if !known(&name) {
                return Err(graph_error(format!("{place}: \"{name}\" is neither a fact nor a named guard")));
            }
        }
        Ok(())
    };
    for (name, src) in guards.iter() {
        check_guard(src, &format!("guard {name}"))?;
    }

    let mut resolve_label = |s: &Value, place: &str| -> (String, Option<String>) {
        let s = js::to_js_string(s);
        if let Some(text) = catalog_keys.get(&s) {
            return (js::to_js_string(text), Some(s));
        }
        if LOOKS_LIKE_KEY.is_match(&s) {
            warnings.push(format!("{place}: \"{s}\" looks like a catalog key but the catalog has no such key"));
        }
        (s, None)
    };

    let frames = object_or_empty(doc.get("frames"));
    if frames.is_empty() {
        return Err(graph_error("a navigation graph declares its frames (frames:)".to_string()));
    }
    for (name, f) in frames.iter() {
        if let Some(fact) = FrameSpec(f).shows_fact()
            && !facts.contains_key(&fact)
        {
            return Err(graph_error(format!("frame {name} shows the menu named by {fact}, which is not a fact")));
        }
    }
    let frame = |name: &str| FrameSpec(frames.get(name).unwrap_or(&Value::Null));
    let mut pages: IndexMap<String, Page> = IndexMap::new();
    for (id, p) in object_or_empty(doc.get("pages")).iter() {
        let kind = p.get("kind");
        let Some(page_kind) = kind.and_then(Value::as_str).and_then(PageKind::parse) else {
            return Err(graph_error(format!("page {id}: unknown kind {}", text(kind))));
        };
        let page_frame = text(p.get("frame"));
        if !frames.contains_key(&page_frame) {
            return Err(graph_error(format!("page {id}: unknown frame {page_frame}")));
        }
        if let Some(opens_for) = p.get("opensFor") {
            check_guard(opens_for, &format!("page {id} opensFor"))?;
        }
        let (title, title_key) = match p.get("title") {
            None => (id.clone(), None),
            Some(t) => resolve_label(t, &format!("page {id}")),
        };
        pages.insert(
            id.clone(),
            Page {
                id: id.clone(),
                route: text(p.get("route")),
                title,
                title_key,
                kind: page_kind,
                frame: page_frame,
                source: text_or_null(p, "source"),
                opens_for: text_or_null(p, "opensFor"),
                page_codes: text_or_null(p, "pageCodes"),
                evidence: text_or_null(p, "evidence"),
                refusal: text_or_null(p, "refusal"),
                by_address: p.get("byAddress") == Some(&Value::Bool(true)),
            },
        );
    }
    if !pages.contains_key("start") {
        return Err(graph_error("a navigation graph needs a page called start".to_string()));
    }

    let mut entries: Vec<Entry> = Vec::new();
    let specs = list_or_empty(doc.get("entries"));
    for (spec, sv) in specs.iter().enumerate() {
        let s = EntrySpec(sv);
        let to = s.to_text();
        let place = format!("entry {} ({to})", spec + 1);
        let how = s.get("how");
        let Some(how) = how.and_then(Value::as_str).and_then(How::parse) else {
            return Err(graph_error(format!("{place}: unknown how {}", text(how))));
        };
        if !pages.contains_key(&to) {
            return Err(graph_error(format!("{place}: goes to {to}, which is not a page")));
        }
        if let Some(when) = s.get("when") {
            check_guard(when, &format!("{place} when"))?;
        }
        let (label, label_key) = match s.get("label") {
            None => (String::new(), None),
            Some(l) => resolve_label(l, &place),
        };
        if label.is_empty() && how != How::Auto {
            return Err(graph_error(format!("{place}: needs a label")));
        }
        for token in s.from_tokens() {
            let mut sources: Vec<(String, Option<String>)> = Vec::new();
            let mut of_frame: Option<FrameId> = None;
            if let Some(f) = token.strip_prefix("frame:") {
                if !frames.contains_key(f) {
                    return Err(graph_error(format!("{place}: unknown frame in {token}")));
                }
                of_frame = Some(f.to_string());
                for p in pages.values() {
                    if p.frame == f {
                        sources.push((p.id.clone(), None));
                    }
                    // a page whose frame shows the menu of another frame is drawn with this frame's menu when the person's fact names it
                    if let Some(fact) = frame(&p.frame).shows_fact()
                        && p.frame != f
                        && frame(f).has_menu()
                    {
                        sources.push((p.id.clone(), Some(format!("{fact} == '{f}'"))));
                    }
                }
                if sources.is_empty() {
                    return Err(graph_error(format!("{place}: no page is in {token}")));
                }
            } else {
                if !pages.contains_key(&token) {
                    return Err(graph_error(format!("{place}: starts at {token}, which is not a page")));
                }
                sources.push((token.clone(), None));
            }
            for (page, implicit) in sources {
                if page == to && frame_level_how(how) {
                    continue; // the page you are on is not a way to itself
                }
                let parts: Vec<&str> = [implicit.as_deref(), s.when()].into_iter().flatten().collect();
                let when = match parts.len() {
                    0 => None,
                    1 => Some(parts[0].to_string()),
                    _ => Some(parts.iter().map(|x| format!("({x})")).collect::<Vec<_>>().join(" and ")),
                };
                entries.push(Entry {
                    id: given(sv, "id").map_or_else(|| format!("{page}>{to}#{}", spec + 1), js::to_js_string),
                    spec,
                    from: page,
                    of_frame: of_frame.clone(),
                    to: to.clone(),
                    how,
                    label: label.clone(),
                    label_key: label_key.clone(),
                    when,
                    clicks: s.clicks().unwrap_or(if how == How::Auto { 0.0 } else { 1.0 }),
                    data: s.data(),
                    back: s.back(),
                    order: entries.len(),
                });
            }
        }
    }

    let mut personas: Vec<Persona> = Vec::new();
    for (id, p) in object_or_empty(doc.get("personas")).iter() {
        let given_facts = object_or_empty(p.get("facts"));
        let mut f = Object::new();
        for (name, spec) in facts.iter() {
            let Some(given_value) = given_facts.get(name) else {
                return Err(graph_error(format!("persona {id} gives no value for {name}")));
            };
            let mut v = given_value.clone();
            let spec = FactSpec(spec);
            if let Some(of) = spec.of() {
                if let Value::String(set) = &v {
                    let Some(members) = sets.get(set) else {
                        return Err(graph_error(format!("persona {id}: {set} is not a named set")));
                    };
                    v = members.clone();
                }
                let Value::Array(items) = &v else {
                    return Err(graph_error(format!("persona {id}: {name} must be a list")));
                };
                for item in items {
                    let item = js::to_js_string(item);
                    if !includes(of, &Value::String(item.clone())) {
                        return Err(graph_error(format!("persona {id}: {item} is not one of the values of {name}")));
                    }
                }
            } else if let Some(values) = spec.values()
                && !includes(values, &v)
            {
                return Err(graph_error(format!("persona {id}: {} is not a value of {name}", js::to_js_string(&v))));
            }
            f.insert(name.clone(), v);
        }
        for name in given_facts.keys() {
            if !facts.contains_key(name) {
                return Err(graph_error(format!("persona {id} gives {name}, which is not a declared fact")));
            }
        }
        personas.push(Persona { id: id.clone(), title: text(p.get("title")), say: text(p.get("say")), facts: f });
    }

    let not_modeled = list_or_empty(doc.get("notModeled"));
    let mut not_modeled_text = Vec::new();
    for (i, x) in not_modeled.iter().enumerate() {
        let Value::String(x) = x else {
            return Err(graph_error(format!(
                "notModeled item {} is not text (a colon followed by a space turns a line into a map: quote it)",
                i + 1
            )));
        };
        not_modeled_text.push(x.clone());
    }
    let goals = list_or_empty(doc.get("goals"));
    for goal in goals.iter().map(Goal) {
        if !pages.contains_key(&goal.page()) {
            return Err(graph_error(format!("goal {}: {} is not a page", goal.id_text(), goal.page())));
        }
        for who in goal.for_who() {
            if !personas.iter().any(|p| who.as_str() == Some(p.id.as_str())) {
                return Err(graph_error(format!(
                    "goal {}: {} is not a persona",
                    goal.id_text(),
                    js::to_js_string(&who)
                )));
            }
        }
    }

    let menu_frames = frames.iter().filter(|(_, f)| FrameSpec(f).has_menu()).map(|(name, _)| name.clone()).collect();
    Ok(LoadedNav {
        g: NavGraph {
            dir: dir.to_string(),
            adapter: doc.get("extract").and_then(|x| given(x, "adapter")).map(js::to_js_string),
            frames,
            menu_frames,
            id: text(doc.get("id")),
            title: text(doc.get("title")),
            status: text(doc.get("status")),
            summary: js::trim(&doc.get("summary").filter(|v| !v.is_null()).map(js::to_js_string).unwrap_or_default())
                .to_string(),
            locale: locale.map_or_else(|| text(doc.get("locale")), str::to_string),
            source: doc.get("source").filter(|v| !v.is_null()).cloned().unwrap_or(Value::Object(Object::new())),
            facts,
            guards,
            pages,
            entries,
            specs,
            personas,
            goals,
            not_modeled: not_modeled_text,
            catalog,
        },
        warnings,
    })
}

/// SameValueZero, which `includes` compares with: `===`, but NaN is NaN
fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a.strict_eq(b),
    }
}

/// `container.includes(v)`: an item of a list, or a part of a text; anything else has nothing
fn includes(container: &Value, v: &Value) -> bool {
    match container {
        Value::Array(items) => items.iter().any(|x| same_value_zero(x, v)),
        Value::String(s) => s.contains(&js::to_js_string(v)),
        _ => false,
    }
}

fn frame_level_how(how: How) -> bool {
    FRAME_LEVEL.contains(&how)
}

/// `lookupFor(g, p, depth)(name)`: a fact of the person, or a named guard evaluated over them; the first error, which
/// the TypeScript checker threw, is kept in `err`, and every name read after it is null
fn lookup_for(g: &NavGraph, p: &Persona, depth: usize, name: &str, err: &RefCell<Option<Error>>) -> Value {
    if err.borrow().is_some() {
        return Value::Null;
    }
    if let Some(v) = p.facts.get(name) {
        return v.clone();
    }
    let fail = |message: String| {
        *err.borrow_mut() = Some(graph_error(message));
        Value::Null
    };
    let Some(guard) = g.guards.get(name) else { return fail(format!("unknown name \"{name}\"")) };
    if depth > 8 {
        return fail(format!("guard {name} nests too deep"));
    }
    let src = js::to_js_string(guard);
    match parse_expr(&src) {
        Err(m) => fail(m),
        Ok(e) => evaluate(&e, &|n| lookup_for(g, p, depth + 1, n, err)),
    }
}

pub fn holds(g: &NavGraph, p: &Persona, expr: Option<&str>) -> Result<bool> {
    let Some(expr) = expr else { return Ok(true) };
    let e = parse_expr(expr).map_err(graph_error)?;
    let err = RefCell::new(None);
    let v = evaluate(&e, &|n| lookup_for(g, p, 0, n, &err));
    match err.into_inner() {
        Some(e) => Err(e),
        None => Ok(v.truthy()),
    }
}

pub fn refused_for(g: &NavGraph, p: &Persona, page: &str) -> Result<bool> {
    match g.pages.get(page).and_then(|pg| pg.opens_for.as_deref()) {
        None => Ok(false),
        Some(guard) => Ok(!holds(g, p, Some(guard))?),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reach<'g> {
    pub page: String,
    pub clicks: f64,
    pub via: Option<&'g Entry>,
    pub prev: Option<String>,
    pub uses_data: bool,
    pub by_address: Option<String>, // the page opened by its address that the way starts at; null when it starts at start
}

fn settle<'g>(
    g: &'g NavGraph,
    p: &Persona,
    best: &mut IndexMap<String, Reach<'g>>,
    done: &mut HashSet<String>,
    fixed: &HashSet<String>,
) -> Result<()> {
    let index = |id: &str| g.pages.get_index_of(id).map_or(-1, |i| i as isize);
    loop {
        let mut pick: Option<&Reach<'g>> = None;
        for r in best.values() {
            if done.contains(&r.page) {
                continue;
            }
            match pick {
                Some(q) if !(r.clicks < q.clicks || (r.clicks == q.clicks && index(&r.page) < index(&q.page))) => {}
                _ => pick = Some(r),
            }
        }
        let Some(pick) = pick.cloned() else { break };
        done.insert(pick.page.clone());
        let refused = refused_for(g, p, &pick.page)?;
        for e in &g.entries {
            if e.from != pick.page || fixed.contains(&e.to) {
                continue;
            }
            if refused && !frame_level(e) {
                continue;
            }
            if !holds(g, p, e.when.as_deref())? {
                continue;
            }
            let clicks = pick.clicks + e.clicks;
            if best.get(&e.to).is_none_or(|held| clicks < held.clicks) {
                best.insert(
                    e.to.clone(),
                    Reach {
                        page: e.to.clone(),
                        clicks,
                        via: Some(e),
                        prev: Some(pick.page.clone()),
                        uses_data: pick.uses_data || e.data,
                        by_address: pick.by_address.clone(),
                    },
                );
            }
        }
    }
    Ok(())
}

// Fewest clicks from "start"; ties go to the entry written first. Around a page that refuses the person only the frame's own entries work.
// A page opened by its address (byAddress) that no way from start reaches is reached so, with no clicks, and what it
// leads to that no way from start reaches is counted from it; every way from start stays as it is (SPEC.md §6).
pub fn walk<'g>(g: &'g NavGraph, p: &Persona) -> Result<IndexMap<String, Reach<'g>>> {
    let mut best: IndexMap<String, Reach<'g>> = IndexMap::new();
    let mut done: HashSet<String> = HashSet::new();
    let at = |page: &str, by_address: Option<String>| Reach {
        page: page.to_string(),
        clicks: 0.0,
        via: None,
        prev: None,
        uses_data: false,
        by_address,
    };
    best.insert("start".to_string(), at("start", None));
    settle(g, p, &mut best, &mut done, &HashSet::new())?;
    let from_start: HashSet<String> = best.keys().cloned().collect();
    for pg in g.pages.values() {
        if pg.by_address && !best.contains_key(&pg.id) {
            best.insert(pg.id.clone(), at(&pg.id, Some(pg.id.clone())));
        }
    }
    if best.len() > from_start.len() {
        settle(g, p, &mut best, &mut done, &from_start)?;
    }
    Ok(best)
}

/// the entries of the way to a page, from where it starts; a way that runs in a circle (which only entries of no
/// clicks or fewer can make) is cut where it would come round again
pub fn path_to<'g>(reach: &IndexMap<String, Reach<'g>>, page: &str) -> Vec<&'g Entry> {
    let mut out: Vec<&'g Entry> = Vec::new();
    let mut cur = reach.get(page);
    while let Some(c) = cur
        && let Some(via) = c.via
        && out.len() <= reach.len()
    {
        out.push(via);
        cur = c.prev.as_deref().and_then(|prev| reach.get(prev));
    }
    out.reverse();
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Standing {
    Ok,
    Placeholder,
    Missing,
    Refused,
    Unreachable,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::Ok => "ok",
            Standing::Placeholder => "placeholder",
            Standing::Missing => "missing",
            Standing::Refused => "refused",
            Standing::Unreachable => "unreachable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PersonaView<'g> {
    pub persona: &'g Persona,
    #[serde(serialize_with = "map_fields")]
    pub reach: IndexMap<String, Reach<'g>>,
    #[serde(serialize_with = "map_fields")]
    pub standing: IndexMap<String, Standing>,
    pub landing: Option<String>,
}

impl PersonaView<'_> {
    /// `v.standing[page]`
    pub fn standing_of(&self, page: &str) -> Option<Standing> {
        self.standing.get(page).copied()
    }
}

/// what a page is for a person who reaches it: built, not built, not there
fn built(kind: PageKind) -> Standing {
    match kind {
        PageKind::Placeholder => Standing::Placeholder,
        PageKind::Missing => Standing::Missing,
        _ => Standing::Ok,
    }
}

pub fn view_of<'g>(g: &'g NavGraph, p: &'g Persona) -> Result<PersonaView<'g>> {
    let reach = walk(g, p)?;
    let mut standing: IndexMap<String, Standing> = IndexMap::new();
    for (id, pg) in &g.pages {
        let s = if !reach.contains_key(id) {
            Standing::Unreachable
        } else if refused_for(g, p, id)? {
            Standing::Refused
        } else {
            built(pg.kind)
        };
        standing.insert(id.clone(), s);
    }
    let mut auto: Option<&Entry> = None;
    for e in &g.entries {
        if e.from == "start" && e.how == How::Auto && holds(g, p, e.when.as_deref())? {
            auto = Some(e);
            break;
        }
    }
    // a start page that is a screen (not a redirect) is where the person lands when no rule sends them on
    let landing = match auto {
        Some(e) => Some(e.to.clone()),
        None if g.pages.get("start").is_some_and(|s| s.kind == PageKind::Redirect) => None,
        None => Some("start".to_string()),
    };
    Ok(PersonaView { persona: p, reach, standing, landing })
}

pub fn frame_reached(g: &NavGraph, v: &PersonaView<'_>, frame: &str) -> bool {
    g.pages.values().any(|pg| pg.frame == frame && v.standing_of(&pg.id) != Some(Standing::Unreachable))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MenuCell {
    pub shown: bool,                // the persona's menu has the entry
    pub standing: Option<Standing>, // what its target is for this persona, when shown
}

// One row per menu entry written in the graph; a column per persona.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuRow<'g> {
    pub spec: usize,
    pub frame: FrameId,
    pub entry: &'g Entry,
    pub frame_known: Vec<bool>, // by persona: has this menu at all
    pub cells: Vec<MenuCell>,   // by persona; shown is false when the persona never has that menu
}

pub fn menu_rows<'g>(g: &'g NavGraph, views: &[PersonaView<'g>]) -> Result<Vec<MenuRow<'g>>> {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut rows: Vec<MenuRow<'g>> = Vec::new();
    for e in &g.entries {
        if e.how != How::Menu || seen.contains(&e.spec) {
            continue;
        }
        seen.insert(e.spec);
        let frame = match &e.of_frame {
            Some(f) => f.clone(),
            None => g.pages.get(&e.from).map(|p| p.frame.clone()).unwrap_or_default(),
        };
        let frame_known: Vec<bool> = views.iter().map(|v| frame_reached(g, v, &frame)).collect();
        let mut cells = Vec::with_capacity(views.len());
        for (i, v) in views.iter().enumerate() {
            if !frame_known[i] {
                cells.push(MenuCell { shown: false, standing: None });
                continue;
            }
            let shown = holds(g, v.persona, g.spec(e.spec).when())?;
            let standing = if shown { Some(standing_after_menu(g, v, &e.to)?) } else { None };
            cells.push(MenuCell { shown, standing });
        }
        rows.push(MenuRow { spec: e.spec, frame, entry: e, frame_known, cells });
    }
    Ok(rows)
}

fn standing_after_menu(g: &NavGraph, v: &PersonaView<'_>, page: &str) -> Result<Standing> {
    if refused_for(g, v.persona, page)? {
        return Ok(Standing::Refused);
    }
    Ok(g.pages.get(page).map_or(Standing::Ok, |p| built(p.kind)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Problem,
    Note,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub level: Level,
    pub code: String,
    pub say: String,
    pub detail: Vec<String>,
}

pub(crate) fn lower(s: &str) -> String {
    js::to_lower_tr(s)
}

fn word(g: &NavGraph, key: &str) -> String {
    g.message(key).unwrap_or_else(|| key.to_string())
}

pub(crate) fn quote(s: &str) -> String {
    format!("“{s}”")
}

pub(crate) fn join_words(xs: &[String]) -> String {
    match xs {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

// the words on the way in and on arrival name the same thing: one contains the other, or they share a stem
// (Turkish suffixes: hesap, hesabım)
fn similar(a: &str, b: &str) -> bool {
    if a.contains(b) || b.contains(a) {
        return true;
    }
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    let i = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    i >= 4 && i as f64 >= 0.6 * a.len().min(b.len()) as f64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Outcome {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "too many clicks")]
    TooManyClicks,
    #[serde(rename = "not built")]
    NotBuilt,
    #[serde(rename = "does not exist")]
    DoesNotExist,
    #[serde(rename = "refused")]
    Refused,
    #[serde(rename = "cannot reach")]
    CannotReach,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::TooManyClicks => "too many clicks",
            Outcome::NotBuilt => "not built",
            Outcome::DoesNotExist => "does not exist",
            Outcome::Refused => "refused",
            Outcome::CannotReach => "cannot reach",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Goalcheck<'g> {
    pub goal: Goal<'g>,
    pub persona: &'g Persona,
    pub outcome: Outcome,
    pub clicks: Option<f64>,
}

/// `Number(s)` of a text: blank is 0, a decimal, `Infinity` or a 0x / 0o / 0b integer, else NaN
fn number_of_text(s: &str) -> f64 {
    let t = js::trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |digits: &str, radix: u32| {
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return f64::NAN;
        }
        digits.chars().fold(0.0, |n, c| n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)))
    };
    match t.get(..2) {
        Some("0x" | "0X") => return radix(&t[2..], 16),
        Some("0o" | "0O") => return radix(&t[2..], 8),
        Some("0b" | "0B") => return radix(&t[2..], 2),
        _ => {}
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if !t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `x > within` as JavaScript compares a number with any value: the value taken as a number
fn exceeds(x: f64, within: Option<&Value>) -> bool {
    let limit = match within {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => *n,
        Some(Value::String(s)) => number_of_text(s),
        Some(v @ Value::Array(_)) => number_of_text(&js::to_js_string(v)),
    };
    x > limit
}

pub fn check_goals<'g>(g: &'g NavGraph, views: &[PersonaView<'g>]) -> Vec<Goalcheck<'g>> {
    let mut out = Vec::new();
    for goal in g.goals() {
        let page = goal.page();
        for who in goal.for_who() {
            let Some(v) = views.iter().find(|x| who.as_str() == Some(x.persona.id.as_str())) else { continue };
            let standing = v.standing_of(&page);
            let clicks = v.reach.get(&page).map(|r| r.clicks);
            let outcome = match standing {
                Some(Standing::Unreachable) => Outcome::CannotReach,
                Some(Standing::Refused) => Outcome::Refused,
                Some(Standing::Placeholder) => Outcome::NotBuilt,
                Some(Standing::Missing) => Outcome::DoesNotExist,
                _ if exceeds(clicks.unwrap_or(0.0), goal.within()) => Outcome::TooManyClicks,
                _ => Outcome::Ok,
            };
            out.push(Goalcheck { goal, persona: v.persona, outcome, clicks });
        }
    }
    out
}

/// labels and the people whose menu offers them, by the page they open
type MenuHits = IndexMap<String, (IndexSet<String>, IndexSet<String>)>;

pub fn findings(g: &NavGraph, views: &[PersonaView<'_>], goals: &[Goalcheck<'_>]) -> Result<Vec<Finding>> {
    let mut out: Vec<Finding> = Vec::new();
    let rows = menu_rows(g, views)?;
    let persona_title = |i: usize| views[i].persona.title.clone();
    let page = |id: &str| g.pages.get(id);
    let title_of = |id: &str| page(id).map(|p| p.title.clone()).unwrap_or_else(|| "undefined".to_string());
    let route_of = |id: &str| page(id).map(|p| p.route.clone()).unwrap_or_else(|| "undefined".to_string());

    let who = |xs: &[String]| join_words(&xs.iter().map(|x| quote(x)).collect::<Vec<_>>());
    let menu_name = |frame: &str| g.frame(frame).menu().unwrap_or_else(|| frame.to_string()).to_lowercase();
    let problem = |code: &str, say: String, detail: Vec<String>| Finding {
        level: Level::Problem,
        code: code.to_string(),
        say,
        detail,
    };
    let note = |code: &str, say: String, detail: Vec<String>| Finding {
        level: Level::Note,
        code: code.to_string(),
        say,
        detail,
    };

    // 1. the first page a person sees is not built
    let landing: Vec<&PersonaView<'_>> = views
        .iter()
        .filter(|v| v.landing.as_deref().and_then(page).is_some_and(|p| p.kind == PageKind::Placeholder))
        .collect();
    if !landing.is_empty() {
        let targets: IndexSet<&str> = landing.iter().filter_map(|v| v.landing.as_deref()).collect();
        let titles: Vec<String> = landing.iter().map(|v| v.persona.title.clone()).collect();
        out.push(problem(
            "landing-not-built",
            format!(
                "{} {} on a page that says it is not ready.",
                who(&titles),
                if landing.len() == 1 { "lands" } else { "land" }
            ),
            targets
                .iter()
                .map(|t| {
                    let people: Vec<String> = landing
                        .iter()
                        .filter(|v| v.landing.as_deref() == Some(*t))
                        .map(|v| v.persona.title.clone())
                        .collect();
                    format!(
                        "“/” sends {} to {} ({}), which answers “{}”",
                        who(&people),
                        quote(&title_of(t)),
                        route_of(t),
                        word(g, "Shell.placeholder")
                    )
                })
                .collect(),
        ));
    }

    // 2. the menu offers an entry and the page behind it refuses
    let gated: Vec<String> = rows
        .iter()
        .filter(|r| names_held(g.spec(r.spec).when().unwrap_or("")))
        .map(|r| r.entry.label.clone())
        .collect::<IndexSet<String>>()
        .into_iter()
        .collect();
    let mut refused_lines: Vec<String> = Vec::new();
    for i in 0..views.len() {
        for frame in &g.menu_frames {
            let in_frame: Vec<&MenuRow<'_>> = rows.iter().filter(|r| &r.frame == frame && r.cells[i].shown).collect();
            let of = |s: Standing| -> Vec<&MenuRow<'_>> {
                in_frame.iter().copied().filter(|r| r.cells[i].standing == Some(s)).collect()
            };
            if of(Standing::Refused).is_empty() {
                continue;
            }
            let mut parts = vec![
                format!("{} open", of(Standing::Ok).len()),
                format!(
                    "{} refuse ({})",
                    of(Standing::Refused).len(),
                    of(Standing::Refused).iter().map(|r| quote(&r.entry.label)).collect::<Vec<_>>().join(", ")
                ),
            ];
            if !of(Standing::Placeholder).is_empty() {
                parts.push(format!("{} are not built", of(Standing::Placeholder).len()));
            }
            if !of(Standing::Missing).is_empty() {
                parts.push(format!("{} lead to a page that does not exist", of(Standing::Missing).len()));
            }
            refused_lines.push(format!(
                "{}: the {} has {} {}; {}.",
                persona_title(i),
                menu_name(frame),
                in_frame.len(),
                if in_frame.len() == 1 { "entry" } else { "entries" },
                parts.join(", ")
            ));
        }
    }
    if !refused_lines.is_empty() {
        let gated_words = if gated.is_empty() {
            "No entry is drawn by permission".to_string()
        } else {
            format!("Only {} {} drawn by permission", who(&gated), if gated.len() == 1 { "is" } else { "are" })
        };
        out.push(problem(
            "menu-refused",
            format!(
                "The menu offers entries whose page then refuses the person. {gated_words}; every other entry is drawn for everyone and the page decides."
            ),
            refused_lines,
        ));
    }

    // 2b. a link or button drawn on a page the person can open leads to a page that refuses them
    let mut dead_links: IndexMap<String, String> = IndexMap::new();
    for v in views {
        for e in &g.entries {
            if e.how != How::Link && e.how != How::Button {
                continue;
            }
            if !matches!(v.standing_of(&e.from), Some(Standing::Ok | Standing::Placeholder)) {
                continue;
            }
            if !holds(g, v.persona, e.when.as_deref())? || !refused_for(g, v.persona, &e.to)? {
                continue;
            }
            let to = page(&e.to);
            let refusal = to.and_then(|p| p.refusal.clone()).unwrap_or_else(|| "refuses them".to_string());
            dead_links.insert(
                format!("{}|{}|{}|{}", v.persona.id, e.from, e.to, e.label),
                format!(
                    "{}: {} on {} leads to {}, which {refusal}.",
                    v.persona.title,
                    quote(&e.label),
                    quote(&title_of(&e.from)),
                    quote(&title_of(&e.to))
                ),
            );
        }
    }
    if !dead_links.is_empty() {
        out.push(problem(
            "link-refused",
            "A link or button is drawn for a person whose destination then refuses them.".to_string(),
            dead_links.into_values().collect(),
        ));
    }

    // 3. a menu entry opens a page that is not built
    let hits = |kind: PageKind| -> MenuHits {
        let mut hits: MenuHits = IndexMap::new();
        for r in &rows {
            if page(&r.entry.to).map(|p| p.kind) != Some(kind) {
                continue;
            }
            let hit = hits.entry(r.entry.to.clone()).or_default();
            for (i, c) in r.cells.iter().enumerate() {
                if c.shown {
                    hit.1.insert(persona_title(i));
                }
            }
            hit.0.insert(r.entry.label.clone());
        }
        hits
    };
    let in_menus = |hits: &MenuHits| -> Vec<String> {
        hits.iter()
            .map(|(pg, (labels, people))| {
                format!(
                    "{} ({}) is in the menu of {}.",
                    labels.iter().map(|l| quote(l)).collect::<Vec<_>>().join(" / "),
                    route_of(pg),
                    who(&people.iter().cloned().collect::<Vec<_>>())
                )
            })
            .collect()
    };
    let unbuilt = hits(PageKind::Placeholder);
    if !unbuilt.is_empty() {
        out.push(problem(
            "menu-not-built",
            format!("{} pages that the menus offer say “{}”", unbuilt.len(), word(g, "Shell.placeholder")),
            in_menus(&unbuilt),
        ));
    }

    // 3b. a menu entry leads to a page that does not exist
    let gone = hits(PageKind::Missing);
    if !gone.is_empty() {
        out.push(problem(
            "menu-missing",
            format!(
                "{} {} in the menus lead to a page that does not exist, so the server answers not found.",
                gone.len(),
                if gone.len() == 1 { "entry" } else { "entries" }
            ),
            in_menus(&gone),
        ));
    }

    // 4. goals that cannot be done
    let failed: Vec<&Goalcheck<'_>> = goals.iter().filter(|c| c.outcome != Outcome::Ok).collect();
    if !failed.is_empty() {
        out.push(problem(
            "goals",
            format!("{} of {} things people come to do fail.", failed.len(), goals.len()),
            failed
                .iter()
                .map(|c| {
                    let why = match c.outcome {
                        Outcome::TooManyClicks => format!(
                            "{} clicks, the limit is {}",
                            c.clicks.map_or_else(|| "null".to_string(), js::number_to_string),
                            c.goal.within_text()
                        ),
                        Outcome::NotBuilt => "the page is not built".to_string(),
                        Outcome::DoesNotExist => "the page does not exist".to_string(),
                        Outcome::Refused => page(&c.goal.page())
                            .and_then(|p| p.refusal.clone())
                            .unwrap_or_else(|| "the page refuses them".to_string()),
                        _ => "there is no way there".to_string(),
                    };
                    format!("{}, {}: {why}.", c.persona.title, lower(&c.goal.say_text()))
                })
                .collect(),
        ));
    }

    // 5. pages nobody can reach
    let lost: Vec<&Page> = g
        .pages
        .values()
        .filter(|p| {
            p.kind != PageKind::External
                && p.kind != PageKind::Redirect
                && views.iter().all(|v| v.standing_of(&p.id) == Some(Standing::Unreachable))
        })
        .collect();
    if !lost.is_empty() {
        out.push(problem(
            "unreachable",
            "Some pages cannot be reached by any kind of user.".to_string(),
            lost.iter().map(|p| format!("{} ({})", p.title, p.route)).collect(),
        ));
    }

    // 6. two entries of one menu open the same page
    let mut dup: Vec<String> = Vec::new();
    for i in 0..views.len() {
        for frame in &g.menu_frames {
            let mut by_target: IndexMap<&str, Vec<String>> = IndexMap::new();
            for r in rows.iter().filter(|r| &r.frame == frame && r.cells[i].shown) {
                by_target.entry(r.entry.to.as_str()).or_default().push(r.entry.label.clone());
            }
            for (pg, labels) in by_target {
                if labels.len() > 1 {
                    dup.push(format!(
                        "{}: {} both open {}.",
                        persona_title(i),
                        join_words(&labels.iter().map(|l| quote(l)).collect::<Vec<_>>()),
                        route_of(pg)
                    ));
                }
            }
        }
    }
    if !dup.is_empty() {
        out.push(note("menu-twins", "Two entries in one menu open the same page.".to_string(), dup));
    }

    // 7. the words on the way in are not the words on arrival
    let mut mismatch: IndexMap<String, String> = IndexMap::new();
    for e in &g.entries {
        let Some(target) = page(&e.to) else { continue };
        if e.label_key.is_none() || e.back || e.how == How::Auto || e.how == How::Picker || target.title_key.is_none() {
            continue;
        }
        let a = lower(&e.label);
        let b = lower(&target.title);
        if similar(&a, &b) {
            continue;
        }
        let from_frame = || page(&e.from).map(|p| p.frame.clone()).unwrap_or_default();
        let place = if e.how == How::Menu {
            format!("in the {}", menu_name(&e.of_frame.clone().unwrap_or_else(from_frame)))
        } else {
            format!("{} on {}", e.how.as_str(), title_of(&e.from))
        };
        mismatch.insert(
            format!("{}|{}|{}", e.label, e.to, e.how == How::Menu),
            format!("{} ({place}) opens a page titled {}.", quote(&e.label), quote(&target.title)),
        );
    }
    if !mismatch.is_empty() {
        let mut detail: Vec<String> = mismatch.into_values().collect();
        js::sort_utf16(&mut detail);
        out.push(note(
            "words-differ",
            "A link or button is named differently from the page it opens.".to_string(),
            detail,
        ));
    }

    // 8. different pages that carry the same title
    let mut titles: IndexMap<String, Vec<&Page>> = IndexMap::new();
    for p in g.pages.values() {
        if p.kind == PageKind::Redirect || p.kind == PageKind::External || p.frame == "none" {
            continue;
        }
        titles.entry(lower(&p.title)).or_default().push(p);
    }
    let same: Vec<&Vec<&Page>> = titles.values().filter(|ps| ps.len() > 1).collect();
    if !same.is_empty() {
        out.push(note(
            "same-title",
            "Different pages share a title, so two tabs or two history entries cannot be told apart.".to_string(),
            same.iter()
                .map(|ps| {
                    format!(
                        "{}: {}",
                        quote(&ps[0].title),
                        ps.iter().map(|p| p.route.as_str()).collect::<Vec<_>>().join(" and ")
                    )
                })
                .collect(),
        ));
    }
    Ok(out)
}

static HELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u:\b)held(?-u:\b)").expect("the pattern compiles"));

/// `/\bheld\b/.test(when)`: the guard asks about the permissions the person holds
fn names_held(when: &str) -> bool {
    HELD.is_match(when)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoCheck {
    pub ok: bool,
    pub what: String,
    pub detail: String,
}

// the permission codes a guard asks `'code' in held` about, named guards followed
pub fn guard_codes(g: &NavGraph, src: &str, depth: usize) -> Result<IndexSet<String>> {
    let mut out = IndexSet::new();
    fn visit(g: &NavGraph, e: &Expr, depth: usize, out: &mut IndexSet<String>) -> Result<()> {
        match e {
            Expr::Cmp { op, l, r } => {
                if *op == Op::In
                    && let Expr::Lit { v: Value::String(code) } = &**l
                    && let Expr::Id { name } = &**r
                    && name == "held"
                {
                    out.insert(code.clone());
                }
                visit(g, l, depth, out)?;
                visit(g, r, depth, out)?;
            }
            Expr::And { l, r } | Expr::Or { l, r } => {
                visit(g, l, depth, out)?;
                visit(g, r, depth, out)?;
            }
            Expr::Not { e } => visit(g, e, depth, out)?,
            Expr::Id { name } => {
                if let Some(guard) = g.guards.get(name)
                    && depth < 8
                {
                    out.extend(guard_codes(g, &js::to_js_string(guard), depth + 1)?);
                }
            }
            Expr::List { items } => {
                for x in items {
                    visit(g, x, depth, out)?;
                }
            }
            Expr::Lit { .. } => {}
        }
        Ok(())
    }
    let e = parse_expr(src).map_err(graph_error)?;
    visit(g, &e, depth, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // the expected values below are what the TypeScript nav.js gives for the same graph (written as nav.yaml, the
    // catalog as c.json, and run through loadNav, walk, pathTo, viewOf, menuRows, checkGoals, findings, guardCodes;
    // the load errors with the graph changed as each test changes it)
    const TINY: &str = r#"{"kind":"nav","id":"t","title":"T","status":"proposed","locale":"en","catalog":"c.json","summary":" Tiny. ",
     "frames":{"f":{"title":"F","menu":"Side Menu"},"g":{"title":"G","shows":{"fact":"scope"}}},
     "facts":{"role":{"values":["a","b"]},"scope":{"values":["f","none"]}},
     "guards":{"isA":"role == 'a'","viaA":"isA and 'x' in ['x']"},
     "personas":{"a":{"title":"Aİ","say":"a","facts":{"role":"a","scope":"f"}},"b":{"title":"B","say":"b","facts":{"role":"b","scope":"none"}}},
     "pages":{"start":{"route":"/","kind":"screen","frame":"g"},"x":{"route":"/x","title":"X","kind":"screen","frame":"f"},
       "y":{"route":"/y","title":"Y","kind":"placeholder","frame":"f","opensFor":"viaA"},"z":{"route":"/z","title":"Hesabım","kind":"screen","frame":"f"},
       "typed":{"route":"/t","title":"Typed","kind":"screen","frame":"f","byAddress":true},"w":{"route":"/w","title":"W","kind":"screen","frame":"f"}},
     "entries":[{"from":"frame:f","to":"x","how":"menu","label":"X"},{"from":"frame:f","to":"y","how":"menu","label":"Y"},
       {"from":"start","to":"z","how":"link","label":"Z","clicks":2},{"from":"x","to":"z","how":"link","label":"Z.key"},{"from":"typed","to":"w","how":"link","label":"W"}],
     "goals":[{"id":"g","say":"Reach Y","page":"y","for":["a","b"],"within":1}]}"#;

    fn doc(text: &str) -> Object {
        js::json_parse(text).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default()
    }

    fn load(text: &str) -> Result<LoadedNav> {
        nav_of("t", None, doc(text), &|file| {
            if file == "c.json" {
                Ok(r#"{"Shell.placeholder":"Soon","Z.key":"Hesap"}"#.to_string())
            } else {
                Err(Error::graph(format!("no {file}"), None))
            }
        })
    }

    fn tiny() -> NavGraph {
        load(TINY).expect("the tiny graph loads").g
    }

    #[test]
    fn entries_spread_over_frames_with_the_implicit_guard() {
        let g = tiny();
        let got: Vec<(String, Option<String>, f64)> =
            g.entries.iter().map(|e| (e.id.clone(), e.when.clone(), e.clicks)).collect();
        let f = Some("scope == 'f'".to_string());
        let want = [
            ("start>x#1", f.clone(), 1.0),
            ("y>x#1", None, 1.0),
            ("z>x#1", None, 1.0),
            ("typed>x#1", None, 1.0),
            ("w>x#1", None, 1.0),
            ("start>y#2", f, 1.0),
            ("x>y#2", None, 1.0),
            ("z>y#2", None, 1.0),
            ("typed>y#2", None, 1.0),
            ("w>y#2", None, 1.0),
            ("start>z#3", None, 2.0),
            ("x>z#4", None, 1.0),
            ("typed>w#5", None, 1.0),
        ];
        assert_eq!(got, want.map(|(id, w, c)| (id.to_string(), w, c)));
        assert_eq!(g.summary, "Tiny.");
        assert_eq!(g.menu_frames, ["f"]);
    }

    #[test]
    fn walk_breaks_ties_by_page_order_and_counts_from_an_address() {
        let g = tiny();
        let rows = |p: &Persona| -> Vec<(String, f64, Option<String>, Option<String>)> {
            let reach = walk(&g, p).expect("walks");
            assert_eq!(path_to(&reach, "w").iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["typed>w#5"]);
            reach
                .values()
                .map(|r| (r.page.clone(), r.clicks, r.via.map(|e| e.id.clone()), r.by_address.clone()))
                .collect()
        };
        let s = |x: &str| Some(x.to_string());
        assert_eq!(
            rows(&g.personas[0]),
            [
                ("start".into(), 0.0, None, None),
                ("x".into(), 1.0, s("start>x#1"), None),
                ("y".into(), 1.0, s("start>y#2"), None),
                ("z".into(), 2.0, s("start>z#3"), None),
                ("typed".into(), 0.0, None, s("typed")),
                ("w".into(), 1.0, s("typed>w#5"), s("typed")),
            ]
        );
        assert_eq!(
            rows(&g.personas[1]),
            [
                ("start".into(), 0.0, None, None),
                ("z".into(), 2.0, s("start>z#3"), None),
                ("x".into(), 3.0, s("z>x#1"), None),
                ("y".into(), 3.0, s("z>y#2"), None),
                ("typed".into(), 0.0, None, s("typed")),
                ("w".into(), 1.0, s("typed>w#5"), s("typed")),
            ]
        );
    }

    #[test]
    fn views_rows_goals_and_findings() {
        let g = tiny();
        let views: Vec<PersonaView<'_>> = g.personas.iter().map(|p| view_of(&g, p).expect("a view")).collect();
        assert_eq!(views[0].landing.as_deref(), Some("start"));
        assert_eq!(views[0].standing_of("y"), Some(Standing::Placeholder));
        assert_eq!(views[1].standing_of("y"), Some(Standing::Refused));
        let rows = menu_rows(&g, &views).expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].cells[1], MenuCell { shown: true, standing: Some(Standing::Refused) });
        let goals = check_goals(&g, &views);
        let got: Vec<(&str, Outcome, Option<f64>)> =
            goals.iter().map(|c| (c.persona.id.as_str(), c.outcome, c.clicks)).collect();
        assert_eq!(got, [("a", Outcome::NotBuilt, Some(1.0)), ("b", Outcome::Refused, Some(3.0))]);
        let found = findings(&g, &views, &goals).expect("findings");
        let codes: Vec<&str> = found.iter().map(|f| f.code.as_str()).collect();
        assert_eq!(codes, ["menu-refused", "menu-not-built", "goals"]);
        assert_eq!(
            found[0].say,
            "The menu offers entries whose page then refuses the person. No entry is drawn by permission; every other entry is drawn for everyone and the page decides."
        );
        assert_eq!(found[0].detail, ["B: the side menu has 2 entries; 1 open, 1 refuse (“Y”)."]);
        assert_eq!(found[1].say, "1 pages that the menus offer say “Soon”");
        assert_eq!(found[1].detail, ["“Y” (/y) is in the menu of “Aİ” and “B”."]);
        assert_eq!(found[2].detail, ["Aİ, reach y: the page is not built.", "B, reach y: the page refuses them."]);
        assert_eq!(guard_codes(&g, "viaA or 'q' in held", 0).expect("codes").into_iter().collect::<Vec<_>>(), ["q"]);
    }

    #[test]
    fn load_errors_as_the_ts_loader_words_them() {
        let err = |from: &str, to: &str| load(&TINY.replacen(from, to, 1)).map(|_| ()).unwrap_err().message;
        assert_eq!(err(r#""kind":"placeholder""#, r#""kind":"soon""#), "page y: unknown kind soon");
        assert_eq!(err(r#","label":"W"}"#, "}"), "entry 5 (w): needs a label");
        assert_eq!(err(r#","scope":"none"}"#, "}"), "persona b gives no value for scope");
        assert_eq!(err(r#""for":["a","b"]"#, r#""for":"ax""#), "goal g: x is not a persona");
        assert_eq!(
            err(r#""isA":"role == 'a'""#, r#""isA":"rank == 'a'""#),
            "guard isA: \"rank\" is neither a fact nor a named guard"
        );
        // a guard that never ends is an error of the walk, not of the load
        let g = load(&TINY.replacen(r#""isA":"role == 'a'""#, r#""isA":"viaA""#, 1)).expect("loads").g;
        assert_eq!(walk(&g, &g.personas[0]).map(|_| ()).unwrap_err().message, "guard isA nests too deep");
    }

    #[test]
    fn an_extended_file_merges_pages_guards_and_entries() {
        let base = js::json_parse(r#"{"pages":{"a":{"route":"/a","kind":"screen"}},"guards":{"g":"x"},"entries":[1],"source":{"commit":"c"}}"#)
            .expect("json");
        let hand = js::json_parse(r#"{"kind":"nav","pages":{"a":{"kind":"placeholder"},"b":{"route":"/b"}},"guards":{"g":"x"},"entries":[2],"source":{"repo":"r"}}"#)
            .expect("json");
        let doc = Value::Object(merge_docs(&base, &hand).expect("merges"));
        assert_eq!(
            js::stringify(&doc),
            r#"{"kind":"nav","pages":{"a":{"route":"/a","kind":"placeholder"},"b":{"route":"/b"}},"guards":{"g":"x"},"entries":[1,2],"source":{"commit":"c","repo":"r"}}"#
        );
        let clash = js::json_parse(r#"{"guards":{"g":"y"}}"#).expect("json");
        assert_eq!(
            merge_docs(&base, &clash).unwrap_err().message,
            "guard g is written in both the extracted file and nav.yaml, with different text"
        );
        let stray = js::json_parse(r#"{"pages":{"c":{"title":"C"}}}"#).expect("json");
        assert_eq!(
            merge_docs(&base, &stray).unwrap_err().message,
            "page c is not in the extracted file and gives no route (a misspelt override?)"
        );
    }

    #[test]
    fn words_and_numbers_as_javascript_reads_them() {
        // node -e "console.log('catalog/en.json'.replace('en.json', '\$&x.json'), 'a.json'.replace('a.json', '\$\$-\$\`-\$1'))"
        assert_eq!(replace_first("catalog/en.json", "en.json", "$&x.json"), "catalog/en.jsonx.json");
        assert_eq!(replace_first("a.json", "a.json", "$$-$`-$1"), "$--$1");
        assert!(similar("hesap", "hesabım"));
        assert!(similar("raporlar", "rapor"));
        assert!(!similar("genel bakış", "pano"));
        assert_eq!(lower("IRAK İstanbul"), "ırak istanbul");
        assert_eq!(join_words(&["a".into(), "b".into(), "c".into()]), "a, b and c");
        assert!(exceeds(2.0, Some(&Value::from(" 0x1 "))));
        assert!(exceeds(1.0, Some(&Value::Null)));
        assert!(!exceeds(1.0, None));
        assert!(names_held("'a' in held and x"));
        assert!(!names_held("withheld"));
    }
}
