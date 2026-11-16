//! The JSON Schema validator of `check`'s schema rule: Ajv 8.20.0 as schemas.ts builds it (`new Ajv2020({ allErrors:
//! true, verbose: true, strict: true, strictRequired: false, allowUnionTypes: true })`), for the keywords the schemas
//! of schemas/ use, giving the same errors in the same order: the simplified findings depend on each error's keyword,
//! instancePath, schemaPath, params, propertyName and parentSchema, and on where it stands in the list.
//!
//! What Ajv's generated code does, and this does alike:
//! - Keywords run in Ajv's rule order, not the schema's key order: first the keywords for any type (`$ref`, `const`,
//!   `enum`, `not`, `anyOf`, `oneOf`, `allOf`, `if`), then those for numbers, strings, arrays and objects, each group
//!   only when the value has that type (a finite number for the number group: `strictNumbers`).
//! - A `type` is checked before every keyword, except a single type whose group has keywords in the schema: then its
//!   error comes in that group's place, when the value is not of the type.
//! - A schema that is only a `$ref` (no other keyword with a definition) runs only it. A `$ref` whose target has no
//!   `$ref` inside is inlined, its errors' schemaPath starting at the `$ref` itself; any other is a function of its
//!   own: its errors' schemaPath starts at `#`, and they carry no propertyName.
//! - `anyOf`, `oneOf`, `not` and `if` drop the errors of their alternatives when they pass; `allErrors` keeps every
//!   other error; every branch of an `anyOf` runs (Ajv2020 tracks evaluated properties).
//! - `unevaluatedProperties` sees the properties Ajv knows at compile time to be evaluated (its `props`), which for
//!   these schemas is a fixed list; a schema where Ajv would track them at run time is refused at start-up.
//!
//! Every keyword the schemas use is implemented; any other keyword, and any form of one this does not follow, makes
//! `Validator::new` fail, as Ajv's strict mode fails to compile a schema it does not know.

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use super::regexp;
use crate::js::{self, Object, Value};

/// Where a schema is in the arena of a `Validator`.
pub type SchemaId = usize;

/// A JSON type, as `type` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonType {
    String,
    Number,
    Integer,
    Boolean,
    Null,
    Object,
    Array,
}

impl JsonType {
    fn parse(name: &str) -> Option<JsonType> {
        Some(match name {
            "string" => JsonType::String,
            "number" => JsonType::Number,
            "integer" => JsonType::Integer,
            "boolean" => JsonType::Boolean,
            "null" => JsonType::Null,
            "object" => JsonType::Object,
            "array" => JsonType::Array,
            _ => return None,
        })
    }
}

/// A value being validated: a value of the data, or a property name (what `propertyNames` validates).
#[derive(Debug, Clone, Copy)]
pub enum Data<'a> {
    Value(&'a Value),
    Key(&'a str),
}

impl<'a> Data<'a> {
    pub fn as_str(self) -> Option<&'a str> {
        match self {
            Data::Value(v) => v.as_str(),
            Data::Key(k) => Some(k),
        }
    }
    fn as_object(self) -> Option<&'a Object> {
        match self {
            Data::Value(v) => v.as_object(),
            Data::Key(_) => None,
        }
    }
    fn as_array(self) -> Option<&'a Vec<Value>> {
        match self {
            Data::Value(v) => v.as_array(),
            Data::Key(_) => None,
        }
    }
    fn as_f64(self) -> Option<f64> {
        match self {
            Data::Value(v) => v.as_f64(),
            Data::Key(_) => None,
        }
    }
    /// `JSON.stringify(data)`
    pub fn stringify(self) -> String {
        match self {
            Data::Value(v) => js::stringify(v),
            Data::Key(k) => js::quote_string(k),
        }
    }
    // Ajv's checkDataType with strictNumbers: a number is finite, an integer has no fraction, an object is no list
    fn is(self, t: JsonType) -> bool {
        match (self, t) {
            (Data::Key(_), JsonType::String) => true,
            (Data::Key(_), _) => false,
            (Data::Value(v), t) => match (v, t) {
                (Value::String(_), JsonType::String) => true,
                (Value::Number(n), JsonType::Number) => n.is_finite(),
                (Value::Number(n), JsonType::Integer) => n.is_finite() && n.fract() == 0.0,
                (Value::Bool(_), JsonType::Boolean) => true,
                (Value::Null, JsonType::Null) => true,
                (Value::Object(_), JsonType::Object) => true,
                (Value::Array(_), JsonType::Array) => true,
                _ => false,
            },
        }
    }
    fn is_any(self, ts: &[JsonType]) -> bool {
        ts.iter().any(|&t| self.is(t))
    }
}

/// An error's `params`, as Ajv gives them.
#[derive(Debug, Clone, PartialEq)]
pub enum Params<'a> {
    /// `{}`
    None,
    /// `{type}`: the schema's `type`, a name or a list of names
    Type(&'static Value),
    AdditionalProperty(&'a str),
    UnevaluatedProperty(&'a str),
    MissingProperty(&'static str),
    AllowedValues(&'static Value),
    AllowedValue(&'static Value),
    Pattern(&'static str),
    /// `{limit}`
    Limit(f64),
    /// `{comparison, limit}`
    Comparison(&'static str, f64),
    /// `{i, j}`: the two equal items of a `uniqueItems`
    Duplicate(usize, usize),
    /// `{passingSchemas}`: null when no form matched, else the two first that did
    PassingSchemas(Option<(usize, usize)>),
    /// `{failingKeyword}`: `then` or `else`
    FailingKeyword(&'static str),
    PropertyName(&'a str),
}

impl Params<'_> {
    /// the params as the object Ajv gives
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        let n = |x: usize| Value::Number(x as f64);
        match *self {
            Params::None => {}
            Params::Type(t) => o.insert("type", t.clone()),
            Params::AdditionalProperty(k) => o.insert("additionalProperty", Value::from(k)),
            Params::UnevaluatedProperty(k) => o.insert("unevaluatedProperty", Value::from(k)),
            Params::MissingProperty(k) => o.insert("missingProperty", Value::from(k)),
            Params::AllowedValues(v) => o.insert("allowedValues", v.clone()),
            Params::AllowedValue(v) => o.insert("allowedValue", v.clone()),
            Params::Pattern(p) => o.insert("pattern", Value::from(p)),
            Params::Limit(l) => o.insert("limit", Value::Number(l)),
            Params::Comparison(c, l) => {
                o.insert("comparison", Value::from(c));
                o.insert("limit", Value::Number(l));
            }
            Params::Duplicate(i, j) => {
                o.insert("i", n(i));
                o.insert("j", n(j));
            }
            Params::PassingSchemas(p) => {
                o.insert("passingSchemas", p.map_or(Value::Null, |(a, b)| Value::Array(vec![n(a), n(b)])))
            }
            Params::FailingKeyword(k) => o.insert("failingKeyword", Value::from(k)),
            Params::PropertyName(k) => o.insert("propertyName", Value::from(k)),
        }
        Value::Object(o)
    }
}

/// One error of Ajv's list. `schema` and `parentSchema` are found through the validator (`Validator::parent_schema`,
/// `Validator::keyword_schema`).
#[derive(Debug, Clone)]
pub struct AjvError<'a> {
    pub keyword: &'static str,
    pub instance_path: String,
    pub schema_path: String,
    pub params: Params<'a>,
    /// the schema the keyword is in (Ajv's `parentSchema`)
    pub node: SchemaId,
    pub data: Data<'a>,
    /// set inside `propertyNames`: the name being validated
    pub property_name: Option<&'a str>,
}

impl AjvError<'_> {
    /// Ajv's `message`
    pub fn message(&self) -> String {
        let n = js::number_to_string;
        match (self.keyword, &self.params) {
            ("type", Params::Type(t)) => format!("must be {}", js::to_js_string(t)),
            ("additionalProperties", _) => "must NOT have additional properties".to_string(),
            ("unevaluatedProperties", _) => "must NOT have unevaluated properties".to_string(),
            ("required", Params::MissingProperty(k)) => format!("must have required property '{k}'"),
            ("enum", _) => "must be equal to one of the allowed values".to_string(),
            ("const", _) => "must be equal to constant".to_string(),
            ("pattern", Params::Pattern(p)) => format!("must match pattern \"{p}\""),
            ("maxLength", Params::Limit(l)) => format!("must NOT have more than {} characters", n(*l)),
            ("minLength", Params::Limit(l)) => format!("must NOT have fewer than {} characters", n(*l)),
            ("maxItems", Params::Limit(l)) | ("items", Params::Limit(l)) => {
                format!("must NOT have more than {} items", n(*l))
            }
            ("minItems", Params::Limit(l)) => format!("must NOT have fewer than {} items", n(*l)),
            ("maxProperties", Params::Limit(l)) => format!("must NOT have more than {} properties", n(*l)),
            ("minProperties", Params::Limit(l)) => format!("must NOT have fewer than {} properties", n(*l)),
            (_, Params::Comparison(c, l)) => format!("must be {c} {}", n(*l)),
            ("uniqueItems", Params::Duplicate(i, j)) => {
                format!("must NOT have duplicate items (items ## {j} and {i} are identical)")
            }
            ("not", _) => "must NOT be valid".to_string(),
            ("anyOf", _) => "must match a schema in anyOf".to_string(),
            ("oneOf", _) => "must match exactly one schema in oneOf".to_string(),
            ("if", Params::FailingKeyword(k)) => format!("must match \"{k}\" schema"),
            ("propertyNames", _) => "property name must be valid".to_string(),
            ("false schema", _) => "boolean schema is false".to_string(),
            (k, _) => format!("must pass \"{k}\" keyword validation"),
        }
    }
}

// the rule groups of Ajv2020 in their order, each with its keywords in order (ajv.RULES, printed from Ajv 8.20.0 with
// ajv-formats): a group runs when the schema has any of its keywords
const ANY: &[&str] = &[
    "$dynamicAnchor",
    "$dynamicRef",
    "$recursiveAnchor",
    "$recursiveRef",
    "$comment",
    "id",
    "$ref",
    "type",
    "nullable",
    "const",
    "enum",
    "not",
    "anyOf",
    "oneOf",
    "allOf",
    "if",
    "then",
    "else",
];
const NUMBER: &[&str] = &["maximum", "minimum", "exclusiveMaximum", "exclusiveMinimum", "multipleOf", "format"];
const STRING: &[&str] = &[
    "maxLength",
    "minLength",
    "pattern",
    "format",
    "formatMaximum",
    "formatMinimum",
    "formatExclusiveMaximum",
    "formatExclusiveMinimum",
];
const ARRAY: &[&str] = &[
    "maxItems",
    "minItems",
    "prefixItems",
    "items",
    "contains",
    "uniqueItems",
    "maxContains",
    "minContains",
    "unevaluatedItems",
];
const OBJECT: &[&str] = &[
    "maxProperties",
    "minProperties",
    "required",
    "propertyNames",
    "additionalProperties",
    "dependencies",
    "properties",
    "patternProperties",
    "dependentRequired",
    "dependentSchemas",
    "unevaluatedProperties",
];
const GROUPS: [&[&str]; 5] = [ANY, NUMBER, STRING, ARRAY, OBJECT];
// the keywords this validator runs (the others of the groups make `Validator::new` fail)
const IMPLEMENTED: &[&str] = &[
    "$comment",
    "$ref",
    "type",
    "const",
    "enum",
    "not",
    "anyOf",
    "oneOf",
    "allOf",
    "if",
    "then",
    "else",
    "maximum",
    "minimum",
    "exclusiveMaximum",
    "exclusiveMinimum",
    "maxLength",
    "minLength",
    "pattern",
    "maxItems",
    "minItems",
    "prefixItems",
    "items",
    "uniqueItems",
    "maxProperties",
    "minProperties",
    "required",
    "propertyNames",
    "additionalProperties",
    "properties",
    "patternProperties",
    "unevaluatedProperties",
];
// keywords Ajv knows that have no rule: they say things to people and change no result
const ANNOTATIONS: &[&str] =
    &["$schema", "$id", "$defs", "title", "description", "default", "deprecated", "readOnly", "writeOnly", "examples"];
// the keywords whose presence makes Ajv compile a `$ref` target as a function of its own (resolve.ts REF_KEYWORDS)
const REF_KEYWORDS: &[&str] = &["$ref", "$recursiveRef", "$recursiveAnchor", "$dynamicRef", "$dynamicAnchor"];
// the names `data[name] !== undefined` finds on every object: Object.prototype's
const INHERITED: &[&str] = &[
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Any,
    Number,
    String,
    Array,
    Object,
}
const GROUP_ORDER: [Group; 5] = [Group::Any, Group::Number, Group::String, Group::Array, Group::Object];

fn is_rule(key: &str) -> bool {
    GROUPS.iter().any(|g| g.contains(&key))
}

// what Ajv knows at compile time of the properties a schema evaluated (its `it.props`)
#[derive(Debug, Clone, PartialEq)]
enum Props {
    Undefined,
    Static(IndexSet<String>),
    All,
    Dynamic,
}

impl Props {
    // mergeEvaluated.props without a Name: a static union
    fn merge(&mut self, from: Props) {
        if *self == Props::All || from == Props::Undefined {
            return;
        }
        *self = match (std::mem::replace(self, Props::Undefined), from) {
            (_, Props::All) => Props::All,
            (Props::Dynamic, _) | (_, Props::Dynamic) => Props::Dynamic,
            (Props::Undefined, x) => x,
            (Props::Static(mut a), Props::Static(b)) => {
                // `{ ...from, ...to }`: the order is no matter, only membership is asked
                a.extend(b);
                Props::Static(a)
            }
            (x, _) => x,
        };
    }
    // mergeValidEvaluated: merged only when the subschema passed, so Ajv keeps them at run time
    fn merge_when_valid(&mut self, from: Props) {
        if *self != Props::All && from != Props::Undefined {
            *self = Props::Dynamic;
        }
    }
}

#[derive(Debug)]
struct Ref {
    text: &'static str,
    target: SchemaId,
    // compiled as a function of its own (the target has a $ref inside), else inlined
    call: bool,
}

#[derive(Debug)]
struct Unique {
    // the types of `items`, when they let Ajv compare items by a key (loopN), else empty (loopN2, deep equality)
    item_types: Vec<JsonType>,
}

#[derive(Debug, Default)]
struct Obj {
    has_rules: bool,
    types: Vec<JsonType>,
    type_first: bool,
    type_group: Option<Group>,
    groups: [bool; 5],
    only_ref: bool,
    ref_text: Option<&'static str>,
    reference: Option<Ref>,
    const_: Option<&'static Value>,
    enum_: Option<&'static Vec<Value>>,
    not: Option<SchemaId>,
    any_of: Option<Vec<SchemaId>>,
    one_of: Option<Vec<SchemaId>>,
    all_of: Option<Vec<SchemaId>>,
    if_: Option<SchemaId>,
    then: Option<SchemaId>,
    else_: Option<SchemaId>,
    maximum: Option<f64>,
    minimum: Option<f64>,
    exclusive_maximum: Option<f64>,
    exclusive_minimum: Option<f64>,
    max_length: Option<f64>,
    min_length: Option<f64>,
    pattern: Option<(&'static str, Regex)>,
    max_items: Option<f64>,
    min_items: Option<f64>,
    prefix_items: Option<Vec<SchemaId>>,
    items: Option<SchemaId>,
    unique_items: Option<Unique>,
    max_properties: Option<f64>,
    min_properties: Option<f64>,
    required: Option<Vec<&'static str>>,
    property_names: Option<SchemaId>,
    additional_properties: Option<SchemaId>,
    properties: Option<Vec<(&'static str, SchemaId)>>,
    pattern_properties: Option<Vec<(&'static str, SchemaId)>>,
    unevaluated_properties: Option<SchemaId>,
    // the properties evaluated before `unevaluatedProperties` runs
    evaluated: Option<Props>,
}

#[derive(Debug)]
enum Kind {
    Bool(bool),
    Obj(Box<Obj>),
}

#[derive(Debug)]
struct Node {
    value: &'static Value,
    pointer: String,
    kind: Kind,
}

/// One schema of schemas/, compiled.
#[derive(Debug)]
pub struct Validator {
    nodes: Vec<Node>,
    root: SchemaId,
    by_pointer: IndexMap<String, SchemaId>,
    regexes: IndexMap<&'static str, Regex>,
}

// JSON pointer escaping of one step
fn escape(step: &str) -> String {
    step.replace('~', "~0").replace('/', "~1")
}

// encodeURIComponent
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// decodeURIComponent, for the steps of a `$ref`
fn decode_uri_component(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or_else(|| format!("a malformed escape in {s:?}"))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| format!("a malformed escape in {s:?}"))?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("a malformed escape in {s:?}"))
}

// Ajv's escapeFragment: a step of a schemaPath
fn fragment(step: &str) -> String {
    encode_uri_component(&escape(step))
}

// a `$ref` (or anything like it) anywhere inside (resolve.ts hasRef)
fn has_ref(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.iter().any(|(k, x)| REF_KEYWORDS.contains(&k.as_str()) || has_ref(x)),
        Value::Array(a) => a.iter().any(has_ref),
        _ => false,
    }
}

fn number(v: &Value, key: &str, at: &str) -> Result<f64, String> {
    v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| format!("{key} at {at:?} must be a number"))
}

struct Builder {
    nodes: Vec<Node>,
    by_pointer: IndexMap<String, SchemaId>,
    regexes: IndexMap<&'static str, Regex>,
}

impl Builder {
    fn regex(&mut self, pattern: &'static str) -> Result<Regex, String> {
        if let Some(r) = self.regexes.get(pattern) {
            return Ok(r.clone());
        }
        let r = regexp::compile(pattern)?;
        self.regexes.insert(pattern, r.clone());
        Ok(r)
    }

    fn list(&mut self, v: &'static Value, pointer: &str, key: &str) -> Result<Vec<SchemaId>, String> {
        let items = v.as_array().ok_or_else(|| format!("{key} at {pointer:?} must be a list"))?;
        let mut out = Vec::new();
        for (i, x) in items.iter().enumerate() {
            out.push(self.node(x, format!("{pointer}/{key}/{i}"), false)?);
        }
        Ok(out)
    }

    fn map(&mut self, v: &'static Value, pointer: &str, key: &str) -> Result<Vec<(&'static str, SchemaId)>, String> {
        let entries = v.as_object().ok_or_else(|| format!("{key} at {pointer:?} must be an object"))?;
        let mut out = Vec::new();
        for (name, x) in entries.iter() {
            let id = self.node(x, format!("{pointer}/{key}/{}", escape(name)), false)?;
            // allSchemaProperties leaves `__proto__` out
            if name != "__proto__" {
                out.push((name.as_str(), id));
            }
        }
        Ok(out)
    }

    fn node(&mut self, value: &'static Value, pointer: String, root: bool) -> Result<SchemaId, String> {
        let kind = match value {
            Value::Bool(b) => Kind::Bool(*b),
            Value::Object(o) => Kind::Obj(Box::new(self.obj(o, &pointer, root)?)),
            _ => return Err(format!("the schema at {pointer:?} is neither an object nor a boolean")),
        };
        let id = self.nodes.len();
        self.by_pointer.insert(pointer.clone(), id);
        self.nodes.push(Node { value, pointer, kind });
        Ok(id)
    }

    fn obj(&mut self, o: &'static Object, at: &str, root: bool) -> Result<Obj, String> {
        let mut s = Obj::default();
        for key in o.keys() {
            let known = IMPLEMENTED.contains(&key.as_str()) || ANNOTATIONS.contains(&key.as_str());
            if !known {
                return Err(format!("the keyword {key:?} at {at:?} is not one the checker's validator runs"));
            }
            if key == "$id" && !root {
                return Err(format!("$id at {at:?}: only a root schema may have one"));
            }
        }
        s.has_rules = o.keys().any(|k| is_rule(k));
        for (g, keywords) in GROUPS.iter().enumerate() {
            s.groups[g] = keywords.iter().any(|k| o.contains_key(k));
        }
        if let Some(t) = o.get("type") {
            let names: Vec<&Value> = match t {
                Value::Array(xs) => xs.iter().collect(),
                x => vec![x],
            };
            for name in names {
                let t = name.as_str().and_then(JsonType::parse);
                s.types.push(t.ok_or_else(|| format!("type at {at:?} must name JSON types"))?);
            }
        }
        // coerceAndCheckDataType: checked first unless one type whose group has keywords here
        let group_of = |t: JsonType| match t {
            JsonType::String => Some(Group::String),
            JsonType::Number => Some(Group::Number),
            JsonType::Array => Some(Group::Array),
            JsonType::Object => Some(Group::Object),
            _ => None,
        };
        let deferred = match s.types.as_slice() {
            [t] => group_of(*t).filter(|g| s.groups[*g as usize]),
            _ => None,
        };
        s.type_first = !s.types.is_empty() && deferred.is_none();
        s.type_group = deferred;
        if let Some(r) = o.get("$ref") {
            s.ref_text = Some(r.as_str().ok_or_else(|| format!("$ref at {at:?} must be a string"))?);
            // schemaHasRulesButRef
            s.only_ref = !o.keys().any(|k| k != "$ref" && is_rule(k));
        }
        if let Some(v) = o.get("const") {
            s.const_ = Some(v);
        }
        if let Some(v) = o.get("enum") {
            let xs = v.as_array().filter(|xs| !xs.is_empty());
            s.enum_ = Some(xs.ok_or_else(|| format!("enum at {at:?} must be a list of values"))?);
        }
        if let Some(v) = o.get("not") {
            s.not = Some(self.node(v, format!("{at}/not"), false)?);
        }
        if let Some(v) = o.get("anyOf") {
            s.any_of = Some(self.list(v, at, "anyOf")?);
        }
        if let Some(v) = o.get("oneOf") {
            s.one_of = Some(self.list(v, at, "oneOf")?);
        }
        if let Some(v) = o.get("allOf") {
            s.all_of = Some(self.list(v, at, "allOf")?);
        }
        if let Some(v) = o.get("if") {
            s.if_ = Some(self.node(v, format!("{at}/if"), false)?);
        }
        if let Some(v) = o.get("then") {
            s.then = Some(self.node(v, format!("{at}/then"), false)?);
        }
        if let Some(v) = o.get("else") {
            s.else_ = Some(self.node(v, format!("{at}/else"), false)?);
        }
        // strict mode: an `if` alone, or a `then` or `else` without it, is refused
        if s.if_.is_some() != (s.then.is_some() || s.else_.is_some()) {
            return Err(format!("if, then and else at {at:?} must go together"));
        }
        for (key, slot) in [
            ("maximum", &mut s.maximum),
            ("minimum", &mut s.minimum),
            ("exclusiveMaximum", &mut s.exclusive_maximum),
            ("exclusiveMinimum", &mut s.exclusive_minimum),
            ("maxLength", &mut s.max_length),
            ("minLength", &mut s.min_length),
            ("maxItems", &mut s.max_items),
            ("minItems", &mut s.min_items),
            ("maxProperties", &mut s.max_properties),
            ("minProperties", &mut s.min_properties),
        ] {
            if let Some(v) = o.get(key) {
                *slot = Some(number(v, key, at)?);
            }
        }
        if let Some(v) = o.get("pattern") {
            let p = v.as_str().ok_or_else(|| format!("pattern at {at:?} must be a string"))?;
            s.pattern = Some((p, self.regex(p)?));
        }
        if let Some(v) = o.get("prefixItems") {
            s.prefix_items = Some(self.list(v, at, "prefixItems")?);
        }
        if let Some(v) = o.get("items") {
            s.items = Some(self.node(v, format!("{at}/items"), false)?);
        }
        if let Some(v) = o.get("uniqueItems") {
            let on = v.as_bool().ok_or_else(|| format!("uniqueItems at {at:?} must be true or false"))?;
            if on {
                // getSchemaTypes(parentSchema.items)
                let mut item_types = Vec::new();
                if let Some(t) = o.get("items").and_then(|i| i.get("type")) {
                    let names: Vec<&Value> = match t {
                        Value::Array(xs) => xs.iter().collect(),
                        x => vec![x],
                    };
                    for name in names {
                        let t = name.as_str().and_then(JsonType::parse);
                        item_types.push(t.ok_or_else(|| format!("items.type at {at:?} must name JSON types"))?);
                    }
                }
                if item_types.iter().any(|t| matches!(t, JsonType::Object | JsonType::Array)) {
                    item_types.clear();
                }
                s.unique_items = Some(Unique { item_types });
            }
        }
        if let Some(v) = o.get("required") {
            let names = v.as_array().ok_or_else(|| format!("required at {at:?} must be a list"))?;
            let mut out = Vec::new();
            for n in names {
                let n = n.as_str().ok_or_else(|| format!("required at {at:?} must list names"))?;
                if INHERITED.contains(&n) {
                    return Err(format!("required at {at:?} names {n:?}, which every object inherits"));
                }
                out.push(n);
            }
            s.required = Some(out);
        }
        if let Some(v) = o.get("propertyNames") {
            s.property_names = Some(self.node(v, format!("{at}/propertyNames"), false)?);
        }
        if let Some(v) = o.get("additionalProperties") {
            s.additional_properties = Some(self.node(v, format!("{at}/additionalProperties"), false)?);
        }
        if let Some(v) = o.get("properties") {
            let props = self.map(v, at, "properties")?;
            if let Some((n, _)) = props.iter().find(|(n, _)| INHERITED.contains(n)) {
                return Err(format!("properties at {at:?} names {n:?}, which every object inherits"));
            }
            s.properties = Some(props);
        }
        if let Some(v) = o.get("patternProperties") {
            let pats = self.map(v, at, "patternProperties")?;
            for (p, _) in &pats {
                self.regex(p)?;
            }
            s.pattern_properties = Some(pats);
        }
        if let Some(v) = o.get("unevaluatedProperties") {
            s.unevaluated_properties = Some(self.node(v, format!("{at}/unevaluatedProperties"), false)?);
        }
        // the schemas a `$ref` may name
        if let Some(v) = o.get("$defs") {
            self.map(v, at, "$defs")?;
        }
        Ok(s)
    }
}

impl Validator {
    /// Compiles a root schema (one of schemas/), or says which keyword or form it uses that this does not run.
    pub fn new(root: &'static Value) -> Result<Validator, String> {
        let mut b = Builder { nodes: Vec::new(), by_pointer: IndexMap::new(), regexes: IndexMap::new() };
        let root = b.node(root, String::new(), true)?;
        let mut v = Validator { nodes: b.nodes, root, by_pointer: b.by_pointer, regexes: b.regexes };
        for id in 0..v.nodes.len() {
            let Some(text) = v.obj(id).and_then(|o| o.ref_text) else { continue };
            let target = v.resolve(text, 0)?;
            let call = has_ref(v.nodes[target].value);
            if let Kind::Obj(o) = &mut v.nodes[id].kind {
                o.reference = Some(Ref { text, target, call });
            }
        }
        for id in 0..v.nodes.len() {
            if v.obj(id).is_some_and(|o| o.unevaluated_properties.is_some()) {
                let props = v.props(id, true, 0);
                if props == Props::Dynamic {
                    return Err(format!(
                        "unevaluatedProperties at {:?}: Ajv tracks its evaluated properties at run time, which the \
                         checker's validator does not",
                        v.nodes[id].pointer
                    ));
                }
                if let Kind::Obj(o) = &mut v.nodes[id].kind {
                    o.evaluated = Some(props);
                }
            }
        }
        Ok(v)
    }

    fn obj(&self, id: SchemaId) -> Option<&Obj> {
        match &self.nodes[id].kind {
            Kind::Obj(o) => Some(o),
            Kind::Bool(_) => None,
        }
    }

    // the schema a `$ref` names, through the schemas that are only a `$ref` (getJsonPointer)
    fn resolve(&self, text: &str, depth: usize) -> Result<SchemaId, String> {
        let Some(path) = text.strip_prefix("#/") else {
            return Err(format!("the $ref {text:?} is not a pointer into its own file"));
        };
        let mut pointer = String::new();
        for step in path.split('/') {
            let step = decode_uri_component(step)?.replace("~1", "/").replace("~0", "~");
            pointer.push('/');
            pointer.push_str(&escape(&step));
        }
        let id = *self.by_pointer.get(&pointer).ok_or_else(|| format!("the $ref {text:?} names no schema"))?;
        match self.obj(id) {
            Some(o) if o.only_ref && depth < 32 => self.resolve(o.ref_text.unwrap_or_default(), depth + 1),
            Some(o) if o.only_ref => Err(format!("the $ref {text:?} goes round")),
            _ => Ok(id),
        }
    }

    // alwaysValidSchema
    fn always_valid(&self, id: SchemaId) -> bool {
        match &self.nodes[id].kind {
            Kind::Bool(b) => *b,
            Kind::Obj(o) => !o.has_rules,
        }
    }

    // the properties Ajv knows at compile time that a schema evaluated (`it.props`): at its end, or (`before_own`)
    // just before its own unevaluatedProperties runs
    fn props(&self, id: SchemaId, before_own: bool, depth: usize) -> Props {
        let Some(o) = self.obj(id) else { return Props::Undefined };
        if depth > 32 {
            return Props::Dynamic;
        }
        let mut p = Props::Undefined;
        if let Some(r) = &o.reference {
            p.merge(self.props(r.target, false, depth + 1));
        }
        if o.only_ref {
            return p;
        }
        if let Some(bs) = &o.any_of {
            for &b in bs {
                p.merge_when_valid(self.props(b, false, depth + 1));
            }
        }
        if let Some(bs) = &o.one_of {
            for &b in bs.iter().filter(|&&b| !self.always_valid(b)) {
                p.merge_when_valid(self.props(b, false, depth + 1));
            }
        }
        if let Some(bs) = &o.all_of {
            for &b in bs.iter().filter(|&&b| !self.always_valid(b)) {
                p.merge(self.props(b, false, depth + 1));
            }
        }
        if let Some(i) = o.if_ {
            let then = o.then.filter(|&t| !self.always_valid(t));
            let else_ = o.else_.filter(|&t| !self.always_valid(t));
            if then.is_some() || else_.is_some() {
                p.merge(self.props(i, false, depth + 1));
                for c in [then, else_].into_iter().flatten() {
                    p.merge_when_valid(self.props(c, false, depth + 1));
                }
            }
        }
        if o.additional_properties.is_some() {
            p = Props::All;
        }
        if let Some(props) = &o.properties
            && !props.is_empty()
        {
            p.merge(Props::Static(props.iter().map(|(n, _)| n.to_string()).collect()));
        }
        if let Some(pats) = &o.pattern_properties {
            let all_valid = pats.iter().all(|(_, s)| self.always_valid(*s));
            if !pats.is_empty() && !(all_valid && p == Props::All) && p != Props::All {
                p = Props::Dynamic;
            }
        }
        if o.unevaluated_properties.is_some() && !before_own {
            p = Props::All;
        }
        p
    }

    /// The root schema.
    pub fn root(&self) -> SchemaId {
        self.root
    }

    /// The schema as written: an object or a boolean.
    pub fn schema_value(&self, id: SchemaId) -> &'static Value {
        self.nodes[id].value
    }

    /// The JSON pointer of a schema in its file (`""` for the root).
    pub fn pointer(&self, id: SchemaId) -> &str {
        &self.nodes[id].pointer
    }

    /// The compiled pattern of a `pattern` or a `patternProperties` key of this schema.
    pub fn regex(&self, pattern: &str) -> Option<&Regex> {
        self.regexes.get(pattern)
    }

    /// Ajv's `schema` of an error: the value of its keyword (the false schema itself for `false schema`), and where
    /// it is in the file.
    pub fn keyword_schema(&self, e: &AjvError) -> (&'static Value, String) {
        let node = &self.nodes[e.node];
        if e.keyword == "false schema" {
            return (node.value, node.pointer.clone());
        }
        let v = node.value.get(e.keyword).unwrap_or(&Value::Null);
        (v, format!("{}/{}", node.pointer, escape(e.keyword)))
    }

    /// Every error Ajv gives for the value, in Ajv's order; empty when it is valid.
    pub fn validate<'a>(&self, data: &'a Value) -> Vec<AjvError<'a>> {
        let mut run = Run { v: self, errors: Vec::new() };
        run.schema(self.root, At { data: Data::Value(data), path: "", sp: "#", prop: None });
        run.errors
    }
}

#[derive(Clone, Copy)]
struct At<'a, 's> {
    data: Data<'a>,
    path: &'s str, // instancePath
    sp: &'s str,   // errSchemaPath
    prop: Option<&'a str>,
}

struct Run<'v, 'a> {
    v: &'v Validator,
    errors: Vec<AjvError<'a>>,
}

impl<'a> Run<'_, 'a> {
    fn error(&mut self, keyword: &'static str, at: At<'a, '_>, node: SchemaId, params: Params<'a>) {
        self.errors.push(AjvError {
            keyword,
            instance_path: at.path.to_string(),
            schema_path: format!("{}/{keyword}", at.sp),
            params,
            node,
            data: at.data,
            property_name: at.prop,
        });
    }

    // a subschema: whether it added no error
    fn schema(&mut self, id: SchemaId, at: At<'a, '_>) -> bool {
        let before = self.errors.len();
        match &self.v.nodes[id].kind {
            Kind::Bool(true) => {}
            Kind::Bool(false) => self.error("false schema", at, id, Params::None),
            Kind::Obj(o) => {
                if o.has_rules {
                    self.keywords(id, o, at);
                }
            }
        }
        self.errors.len() == before
    }

    // a subschema at a property or an item of the value
    fn child(&mut self, id: SchemaId, at: At<'a, '_>, data: &'a Value, step: &str, sp: &str) -> bool {
        let path = format!("{}/{}", at.path, escape(step));
        self.schema(id, At { data: Data::Value(data), path: &path, sp, prop: at.prop })
    }

    fn type_error(&mut self, id: SchemaId, at: At<'a, '_>) {
        let t = self.v.nodes[id].value.get("type").unwrap_or(&Value::Null);
        self.error("type", at, id, Params::Type(t));
    }

    // typeAndKeywords and schemaKeywords
    fn keywords(&mut self, id: SchemaId, o: &Obj, at: At<'a, '_>) {
        if o.type_first && !at.data.is_any(&o.types) {
            self.type_error(id, at);
        }
        if o.only_ref {
            self.reference(o, at);
            return;
        }
        for (g, group) in GROUP_ORDER.iter().enumerate() {
            if !o.groups[g] {
                continue;
            }
            let fits = match group {
                Group::Any => true,
                Group::Number => at.data.is(JsonType::Number),
                Group::String => at.data.is(JsonType::String),
                Group::Array => at.data.is(JsonType::Array),
                Group::Object => at.data.is(JsonType::Object),
            };
            if !fits {
                if o.type_group == Some(*group) {
                    self.type_error(id, at);
                }
                continue;
            }
            match group {
                Group::Any => self.any(id, o, at),
                Group::Number => self.number(id, o, at),
                Group::String => self.string(id, o, at),
                Group::Array => self.array(id, o, at),
                Group::Object => self.object(id, o, at),
            }
        }
    }

    fn reference(&mut self, o: &Obj, at: At<'a, '_>) {
        let Some(r) = &o.reference else { return };
        if r.call {
            self.schema(r.target, At { sp: "#", prop: None, ..at });
        } else {
            self.schema(r.target, At { sp: r.text, ..at });
        }
    }

    fn any(&mut self, id: SchemaId, o: &Obj, at: At<'a, '_>) {
        let v = self.v;
        self.reference(o, at);
        if let Some(c) = o.const_ {
            let equal = if c.is_collection() { deep_equal_data(at.data, c) } else { strict_equal_data(at.data, c) };
            if !equal {
                self.error("const", at, id, Params::AllowedValue(c));
            }
        }
        if let Some(xs) = o.enum_ {
            let loop_ = xs.len() >= 200;
            let found = xs.iter().any(|x| {
                if loop_ || x.is_collection() { deep_equal_data(at.data, x) } else { strict_equal_data(at.data, x) }
            });
            if !found {
                let all = v.nodes[id].value.get("enum").unwrap_or(&Value::Null);
                self.error("enum", at, id, Params::AllowedValues(all));
            }
        }
        if let Some(s) = o.not {
            if v.always_valid(s) {
                self.error("not", at, id, Params::None);
            } else {
                let start = self.errors.len();
                let sp = format!("{}/not", at.sp);
                let valid = self.schema(s, At { sp: &sp, ..at });
                self.errors.truncate(start);
                if valid {
                    self.error("not", at, id, Params::None);
                }
            }
        }
        if let Some(bs) = &o.any_of {
            let start = self.errors.len();
            let mut valid = false;
            for (i, &b) in bs.iter().enumerate() {
                let sp = format!("{}/anyOf/{i}", at.sp);
                valid |= self.schema(b, At { sp: &sp, ..at });
            }
            if valid {
                self.errors.truncate(start);
            } else {
                self.error("anyOf", at, id, Params::None);
            }
        }
        if let Some(bs) = &o.one_of {
            let start = self.errors.len();
            let mut valid = false;
            let mut passing: Option<usize> = None;
            let mut pair: Option<(usize, usize)> = None;
            for (i, &b) in bs.iter().enumerate() {
                let ok = if v.always_valid(b) {
                    true
                } else {
                    let sp = format!("{}/oneOf/{i}", at.sp);
                    self.schema(b, At { sp: &sp, ..at })
                };
                // Ajv's code nests each next form in the `else` of this test: a second form that matches ends it
                if i > 0 && ok && valid {
                    valid = false;
                    pair = passing.map(|p| (p, i));
                    break;
                } else if ok {
                    valid = true;
                    passing = Some(i);
                }
            }
            if valid {
                self.errors.truncate(start);
            } else {
                self.error("oneOf", at, id, Params::PassingSchemas(pair));
            }
        }
        if let Some(bs) = &o.all_of {
            for (i, &b) in bs.iter().enumerate() {
                if !v.always_valid(b) {
                    let sp = format!("{}/allOf/{i}", at.sp);
                    self.schema(b, At { sp: &sp, ..at });
                }
            }
        }
        if let Some(i) = o.if_ {
            let then = o.then.filter(|&t| !v.always_valid(t));
            let else_ = o.else_.filter(|&t| !v.always_valid(t));
            if then.is_none() && else_.is_none() {
                return;
            }
            let start = self.errors.len();
            let sp = format!("{}/if", at.sp);
            let holds = self.schema(i, At { sp: &sp, ..at });
            self.errors.truncate(start);
            let clause = match (holds, then, else_) {
                (true, Some(t), _) => Some(("then", t)),
                (false, _, Some(e)) => Some(("else", e)),
                _ => None,
            };
            if let Some((keyword, c)) = clause {
                let sp = format!("{}/{keyword}", at.sp);
                if !self.schema(c, At { sp: &sp, ..at }) {
                    self.error("if", at, id, Params::FailingKeyword(keyword));
                }
            }
        }
    }

    fn number(&mut self, id: SchemaId, o: &Obj, at: At<'a, '_>) {
        let Some(n) = at.data.as_f64() else { return };
        for (keyword, limit, comparison, fails) in [
            ("maximum", o.maximum, "<=", n > o.maximum.unwrap_or(0.0)),
            ("minimum", o.minimum, ">=", n < o.minimum.unwrap_or(0.0)),
            ("exclusiveMaximum", o.exclusive_maximum, "<", n >= o.exclusive_maximum.unwrap_or(0.0)),
            ("exclusiveMinimum", o.exclusive_minimum, ">", n <= o.exclusive_minimum.unwrap_or(0.0)),
        ] {
            if let Some(l) = limit
                && (fails || n.is_nan())
            {
                self.error(keyword, at, id, Params::Comparison(comparison, l));
            }
        }
    }

    fn string(&mut self, id: SchemaId, o: &Obj, at: At<'a, '_>) {
        let Some(s) = at.data.as_str() else { return };
        // ucs2length: code points
        let len = s.chars().count() as f64;
        if let Some(l) = o.max_length
            && len > l
        {
            self.error("maxLength", at, id, Params::Limit(l));
        }
        if let Some(l) = o.min_length
            && len < l
        {
            self.error("minLength", at, id, Params::Limit(l));
        }
        if let Some((p, re)) = &o.pattern
            && !re.is_match(s)
        {
            self.error("pattern", at, id, Params::Pattern(p));
        }
    }

    fn array(&mut self, id: SchemaId, o: &Obj, at: At<'a, '_>) {
        let v = self.v;
        let Some(items) = at.data.as_array() else { return };
        let len = items.len() as f64;
        if let Some(l) = o.max_items
            && len > l
        {
            self.error("maxItems", at, id, Params::Limit(l));
        }
        if let Some(l) = o.min_items
            && len < l
        {
            self.error("minItems", at, id, Params::Limit(l));
        }
        let prefix = o.prefix_items.as_deref().unwrap_or(&[]);
        for (i, &s) in prefix.iter().enumerate() {
            if !v.always_valid(s) && items.len() > i {
                let sp = format!("{}/prefixItems/{i}", at.sp);
                self.child(s, at, &items[i], &i.to_string(), &sp);
            }
        }
        if let Some(s) = o.items
            && !v.always_valid(s)
        {
            let sp = format!("{}/items", at.sp);
            if o.prefix_items.is_some() {
                // validateAdditionalItems
                if matches!(v.nodes[s].kind, Kind::Bool(false)) {
                    if items.len() > prefix.len() {
                        self.error("items", at, id, Params::Limit(prefix.len() as f64));
                    }
                } else {
                    for (i, x) in items.iter().enumerate().skip(prefix.len()) {
                        self.child(s, at, x, &i.to_string(), &sp);
                    }
                }
            } else {
                for (i, x) in items.iter().enumerate() {
                    self.child(s, at, x, &i.to_string(), &sp);
                }
            }
        }
        if let Some(u) = &o.unique_items
            && items.len() > 1
            && let Some((i, j)) = duplicate(items, &u.item_types)
        {
            self.error("uniqueItems", at, id, Params::Duplicate(i, j));
        }
    }

    fn object(&mut self, id: SchemaId, o: &Obj, at: At<'a, '_>) {
        let v = self.v;
        let Some(map) = at.data.as_object() else { return };
        let count = map.len() as f64;
        if let Some(l) = o.max_properties
            && count > l
        {
            self.error("maxProperties", at, id, Params::Limit(l));
        }
        if let Some(l) = o.min_properties
            && count < l
        {
            self.error("minProperties", at, id, Params::Limit(l));
        }
        for &name in o.required.as_deref().unwrap_or(&[]) {
            if !map.contains_key(name) {
                self.error("required", at, id, Params::MissingProperty(name));
            }
        }
        if let Some(s) = o.property_names
            && !v.always_valid(s)
        {
            let sp = format!("{}/propertyNames", at.sp);
            for key in map.keys() {
                if !self.schema(s, At { data: Data::Key(key), path: at.path, sp: &sp, prop: Some(key) }) {
                    self.error("propertyNames", at, id, Params::PropertyName(key));
                }
            }
        }
        let properties = o.properties.as_deref().unwrap_or(&[]);
        let patterns = o.pattern_properties.as_deref().unwrap_or(&[]);
        if let Some(s) = o.additional_properties
            && !v.always_valid(s)
        {
            let sp = format!("{}/additionalProperties", at.sp);
            for (key, x) in map.iter() {
                let known = properties.iter().any(|(n, _)| n == key)
                    || patterns.iter().any(|(p, _)| v.regex(p).is_some_and(|r| r.is_match(key)));
                if known {
                    continue;
                }
                if matches!(v.nodes[s].kind, Kind::Bool(false)) {
                    self.error("additionalProperties", at, id, Params::AdditionalProperty(key));
                } else {
                    self.child(s, at, x, key, &sp);
                }
            }
        }
        for &(name, s) in properties {
            if v.always_valid(s) {
                continue;
            }
            if let Some(x) = map.get(name) {
                let sp = format!("{}/properties/{}", at.sp, fragment(name));
                self.child(s, at, x, name, &sp);
            }
        }
        for &(pattern, s) in patterns {
            if v.always_valid(s) {
                continue;
            }
            let sp = format!("{}/patternProperties/{}", at.sp, fragment(pattern));
            for (key, x) in map.iter() {
                if v.regex(pattern).is_some_and(|r| r.is_match(key)) {
                    self.child(s, at, x, key, &sp);
                }
            }
        }
        if let Some(s) = o.unevaluated_properties {
            let evaluated = o.evaluated.as_ref().unwrap_or(&Props::Undefined);
            if *evaluated == Props::All || v.always_valid(s) {
                return;
            }
            let sp = format!("{}/unevaluatedProperties", at.sp);
            for (key, x) in map.iter() {
                if let Props::Static(names) = evaluated
                    && names.contains(key.as_str())
                {
                    continue;
                }
                if matches!(v.nodes[s].kind, Kind::Bool(false)) {
                    self.error("unevaluatedProperties", at, id, Params::UnevaluatedProperty(key));
                } else {
                    self.child(s, at, x, key, &sp);
                }
            }
        }
    }
}

// the first two equal items Ajv's uniqueItems finds, as (i, j)
fn duplicate(items: &[Value], item_types: &[JsonType]) -> Option<(usize, usize)> {
    if !item_types.is_empty() {
        // loopN: from the last item, each kept under its property key in an object (`indices`), skipping the items
        // of another type; a string gets "_" when there are several types, so that "1" and 1 differ
        let mut indices: IndexMap<String, usize> = IndexMap::new();
        for i in (0..items.len()).rev() {
            let item = &items[i];
            if !Data::Value(item).is_any(item_types) {
                continue;
            }
            let mut key = js::to_js_string(item);
            if item_types.len() > 1 && item.is_string() {
                key.push('_');
            }
            // `indices.__proto__ = i` sets no property: that key is never found
            if key == "__proto__" {
                continue;
            }
            if let Some(&j) = indices.get(&key) {
                return Some((i, j));
            }
            indices.insert(key, i);
        }
        return None;
    }
    // loopN2: every pair, from the last item back, by deep equality
    for i in (0..items.len()).rev() {
        for j in (0..i).rev() {
            if deep_equal(&items[i], &items[j]) {
                return Some((i, j));
            }
        }
    }
    None
}

fn strict_equal_data(a: Data, b: &Value) -> bool {
    match a {
        Data::Value(a) => a.strict_eq(b),
        Data::Key(k) => b.as_str() == Some(k),
    }
}

fn deep_equal_data(a: Data, b: &Value) -> bool {
    match a {
        Data::Value(a) => deep_equal(a, b),
        Data::Key(k) => b.as_str() == Some(k),
    }
}

// what `x.constructor` is on a value of the data: Array or Object, unless the object has a key `constructor`
enum Ctor<'v> {
    Array,
    Object,
    Own(&'v Value),
}

fn ctor(v: &Value) -> Ctor<'_> {
    match v {
        Value::Array(_) => Ctor::Array,
        Value::Object(o) => o.get("constructor").map_or(Ctor::Object, Ctor::Own),
        _ => Ctor::Object,
    }
}

/// fast-deep-equal, which Ajv's `const`, `enum` and `uniqueItems` use: two NaN are equal, and objects are equal
/// when they have the same keys with equal values, in any order, and the same `constructor`. (On an object with a key
/// `valueOf` or `toString` fast-deep-equal calls it and throws: the TypeScript checker crashes there; this compares
/// the keys instead.)
pub fn deep_equal(a: &Value, b: &Value) -> bool {
    if a.strict_eq(b) {
        return true;
    }
    if a.is_collection() && b.is_collection() {
        let same = match (ctor(a), ctor(b)) {
            (Ctor::Array, Ctor::Array) | (Ctor::Object, Ctor::Object) => true,
            (Ctor::Own(x), Ctor::Own(y)) => x.strict_eq(y),
            _ => false,
        };
        if !same {
            return false;
        }
        if let (Value::Array(x), Value::Array(y)) = (a, b) {
            return x.len() == y.len() && x.iter().zip(y).all(|(p, q)| deep_equal(p, q));
        }
        let (Value::Object(x), Some(y)) = (a, b.as_object()) else { return false };
        return x.len() == y.len()
            && x.keys().all(|k| y.contains_key(k))
            && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| deep_equal(p, q)));
    }
    matches!((a, b), (Value::Number(x), Value::Number(y)) if x.is_nan() && y.is_nan())
}
