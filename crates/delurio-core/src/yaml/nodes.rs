//! The yaml library's nodes/ folder as delurio reads it: `Scalar`, `YAMLMap`, `YAMLSeq`, `Pair` and `Alias`, with
//! what `toJS` makes of them (nodes/toJS.ts, nodes/addPairToJSMap.ts, the classes' `toJSON`). Only what a parsed
//! document holds is kept: a node's value, range, anchor and tag, a map's or a list's `flow`; comments only as
//! whether a scalar has one (what `!!set` asks).

use crate::js::{self, Object, Value};

/// `Scalar`. `value` is the resolved value; a value the library holds as a JavaScript object of its own (a
/// `!!binary` Buffer, a `!!timestamp` Date, the `!!merge` symbol) is `opaque`: equal to no other, and given to
/// JavaScript as its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    pub value: Value,
    pub range: [usize; 3],
    pub anchor: Option<String>,
    pub tag: Option<String>,
    pub(crate) opaque: bool,
    pub(crate) commented: bool,
}

/// `YAMLMap`
#[derive(Debug, Clone, PartialEq)]
pub struct YamlMap {
    pub items: Vec<Pair>,
    pub flow: bool,
    pub range: [usize; 3],
    pub anchor: Option<String>,
    pub tag: Option<String>,
}

/// `YAMLSeq`
#[derive(Debug, Clone, PartialEq)]
pub struct YamlSeq {
    pub items: Vec<Node>,
    pub flow: bool,
    pub range: [usize; 3],
    pub anchor: Option<String>,
    pub tag: Option<String>,
}

/// `Pair`: a key without a value (`{ a, b }`, `? a`) has `value` None (`null`)
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub key: Option<Node>,
    pub value: Option<Node>,
}

/// `Alias`: `source` is the anchor's name
#[derive(Debug, Clone, PartialEq)]
pub struct Alias {
    pub source: String,
    pub range: [usize; 3],
}

/// a node of a document
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Scalar(Scalar),
    Map(YamlMap),
    Seq(YamlSeq),
    Alias(Alias),
}

impl Node {
    /// `node.range`: where it starts, where its value ends, where it ends with what follows it on its line
    pub fn range(&self) -> [usize; 3] {
        match self {
            Node::Scalar(s) => s.range,
            Node::Map(m) => m.range,
            Node::Seq(s) => s.range,
            Node::Alias(a) => a.range,
        }
    }
    pub(crate) fn range_mut(&mut self) -> &mut [usize; 3] {
        match self {
            Node::Scalar(s) => &mut s.range,
            Node::Map(m) => &mut m.range,
            Node::Seq(s) => &mut s.range,
            Node::Alias(a) => &mut a.range,
        }
    }
    /// `isMap(node)`
    pub fn as_map(&self) -> Option<&YamlMap> {
        if let Node::Map(m) = self { Some(m) } else { None }
    }
    /// `isSeq(node)`
    pub fn as_seq(&self) -> Option<&YamlSeq> {
        if let Node::Seq(s) = self { Some(s) } else { None }
    }
    /// `isScalar(node)`
    pub fn as_scalar(&self) -> Option<&Scalar> {
        if let Node::Scalar(s) = self { Some(s) } else { None }
    }
    pub fn anchor(&self) -> Option<&str> {
        match self {
            Node::Scalar(s) => s.anchor.as_deref(),
            Node::Map(m) => m.anchor.as_deref(),
            Node::Seq(s) => s.anchor.as_deref(),
            Node::Alias(_) => None,
        }
    }
    pub(crate) fn set_anchor(&mut self, anchor: String) {
        match self {
            Node::Scalar(s) => s.anchor = Some(anchor),
            Node::Map(m) => m.anchor = Some(anchor),
            Node::Seq(s) => s.anchor = Some(anchor),
            Node::Alias(_) => {}
        }
    }
    pub(crate) fn set_tag(&mut self, tag: String) {
        match self {
            Node::Scalar(s) => s.tag = Some(tag),
            Node::Map(m) => m.tag = Some(tag),
            Node::Seq(s) => s.tag = Some(tag),
            Node::Alias(_) => {}
        }
    }
    /// a comment given to the node (`node.comment = ..` or `node.commentBefore = ..`): kept only on a scalar
    pub(crate) fn set_commented(&mut self) {
        if let Node::Scalar(s) = self {
            s.commented = true;
        }
    }
}

impl Pair {
    /// `String(pair.key?.value ?? pair.key)`: how delurio's readers name a pair's key
    pub fn key_string(&self) -> String {
        match &self.key {
            None => "null".to_string(),
            Some(Node::Scalar(s)) => js::to_js_string(&s.value),
            Some(Node::Alias(a)) => format!("*{}", a.source),
            Some(node) => js::stringify(&to_json_without_context(node)),
        }
    }
}

/// `scalar.value === other.value` of `mapIncludes`
pub(crate) fn same_scalar_value(a: &Scalar, b: &Scalar) -> bool {
    !a.opaque && !b.opaque && a.value.strict_eq(&b.value)
}

/// `node.toJSON()` with no context, as `JSON.stringify(node)` calls it: what `String(node)` is for a collection
fn to_json_without_context(node: &Node) -> Value {
    match node {
        Node::Scalar(s) => s.value.clone(),
        Node::Alias(a) => {
            let mut o = Object::new();
            o.insert("source", Value::String(a.source.clone()));
            Value::Object(o)
        }
        Node::Seq(s) => Value::Array(s.items.iter().map(to_json_without_context).collect()),
        Node::Map(m) => {
            let mut o = Object::new();
            for pair in &m.items {
                let key = pair.key.as_ref().map_or(Value::Null, to_json_without_context);
                let key = match key {
                    Value::Null => String::new(),
                    Value::Array(_) | Value::Object(_) => js::stringify(&key),
                    k => js::to_js_string(&k),
                };
                o.insert(key, pair.value.as_ref().map_or(Value::Null, to_json_without_context));
            }
            Value::Object(o)
        }
    }
}

/// the context of `toJS`: the anchors converted so far, for the aliases after them
pub(crate) struct ToJs {
    anchors: Vec<(String, Value)>,
    /// aliases resolved so far: past the library's own limit an alias resolves to null instead of the library's
    /// "Excessive alias count" error
    aliases: usize,
}

const MAX_ALIASES: usize = 10_000;

impl ToJs {
    pub(crate) fn new() -> Self {
        ToJs { anchors: Vec::new(), aliases: 0 }
    }

    /// `toJS(node, arg, ctx)` with `keep: true`
    pub(crate) fn js_of(&mut self, node: &Node) -> Value {
        let value = match node {
            Node::Scalar(s) => s.value.clone(),
            Node::Map(m) => {
                let mut map = Object::new();
                for pair in &m.items {
                    self.add_pair_to_js_map(&mut map, pair);
                }
                Value::Object(map)
            }
            Node::Seq(s) => Value::Array(s.items.iter().map(|item| self.js_of(item)).collect()),
            Node::Alias(a) => {
                // the last anchor of that name before the alias; the library throws where there is none
                self.aliases += 1;
                if self.aliases > MAX_ALIASES {
                    return Value::Null;
                }
                self.anchors.iter().rev().find(|(name, _)| *name == a.source).map_or(Value::Null, |(_, v)| v.clone())
            }
        };
        if let Some(anchor) = node.anchor() {
            self.anchors.push((anchor.to_string(), value.clone()));
        }
        value
    }

    /// `addPairToJSMap(ctx, map, pair)` for a plain object: the key as a string, a later key of the same string
    /// replacing the value in its place. Merge keys (`<<`) are not read: the core schema has no merge tag.
    fn add_pair_to_js_map(&mut self, map: &mut Object, pair: &Pair) {
        let js_key = pair.key.as_ref().map_or(Value::Null, |k| self.js_of(k));
        let string_key = match &js_key {
            Value::Null => String::new(),
            Value::Array(_) | Value::Object(_) => match &pair.key {
                Some(key) => stringify_key(key),
                None => js::stringify(&js_key),
            },
            k => js::to_js_string(k),
        };
        let js_value = pair.value.as_ref().map_or(Value::Null, |v| self.js_of(v));
        map.insert(string_key, js_value);
    }
}

/// `stringifyKey` of a key that is a list or a map: the library writes it as YAML in flow style. Approximated here
/// (`[ a, b ]`, `{ a: 1 }`, strings plain or double-quoted): delurio refuses such keys before reading a value.
fn stringify_key(node: &Node) -> String {
    match node {
        Node::Scalar(s) => match &s.value {
            Value::String(t) => {
                let plain = !t.is_empty()
                    && t.chars().all(|c| c.is_alphanumeric() || " _-./".contains(c))
                    && !t.starts_with(' ')
                    && !t.ends_with(' ');
                if plain { t.clone() } else { js::quote_string(t) }
            }
            v => js::to_js_string(v),
        },
        Node::Alias(a) => format!("*{}", a.source),
        Node::Seq(s) => {
            if s.items.is_empty() {
                return "[]".to_string();
            }
            format!("[ {} ]", s.items.iter().map(stringify_key).collect::<Vec<_>>().join(", "))
        }
        Node::Map(m) => {
            if m.items.is_empty() {
                return "{}".to_string();
            }
            let pairs = m.items.iter().map(|p| {
                let k = p.key.as_ref().map_or(String::new(), stringify_key);
                match &p.value {
                    Some(v) => format!("{k}: {}", stringify_key(v)),
                    None => k,
                }
            });
            format!("{{ {} }}", pairs.collect::<Vec<_>>().join(", "))
        }
    }
}
