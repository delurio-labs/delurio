//! The JSON Schemas of schemas/ (one per kind of graph file, SPEC.md §12) as `check` applies them: a file is validated
//! against its kind's schema, and each failure is placed at its line and column through the YAML document and said in
//! words a writer can act on: the key that is not one, the key it was probably meant to be, why an if/then rule exists
//! (its `$comment`), and the comma that cut a value inside `{ }`. The schemas are built into the binary.
//!
//! The port of schemas.ts. Its validator, Ajv 8.20.0 (with ajv-formats, which these schemas do not use), is `ajv.rs`:
//! the findings depend on Ajv's exact error list and its order. `regexp.rs` writes the schemas' JavaScript patterns
//! for the regex crate.

mod ajv;
mod regexp;

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;
use serde::Serialize;

pub use ajv::{AjvError, Data, Params, SchemaId, Validator};

use crate::domain::{is_domain_text, may_be_domain};
use crate::error::At;
use crate::js::{self, Value};
use crate::nodepath;
use crate::yaml::{self, Document, Node, YamlMap, read_yaml, unsupported};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaKind {
    Flow,
    Ui,
    Domain,
    Nav,
    Catalog,
    Needs,
    Gaps,
}

impl SchemaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SchemaKind::Flow => "flow",
            SchemaKind::Ui => "ui",
            SchemaKind::Domain => "domain",
            SchemaKind::Nav => "nav",
            SchemaKind::Catalog => "catalog",
            SchemaKind::Needs => "needs",
            SchemaKind::Gaps => "gaps",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// One place where a file does not follow its kind's schema.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaFinding {
    /// the offset of an unsupported place, on its finding only (the TypeScript spreads the refusal into it)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    pub line: usize,
    pub column: usize,
    /// the code of an unsupported place, on its finding only
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    pub graph: Option<String>, // the graph's id, when the file or the graph it belongs to says it
    pub element: String, // the file within its graph and the JSON pointer of what failed: ui.yaml#/nodes/deny/onClick
}

/// The format a graph file names in `schema:`; a file that names another is left to the format check.
pub const FORMAT: &str = "delurio-graph/0";
pub const KINDS: [SchemaKind; 7] = [
    SchemaKind::Flow,
    SchemaKind::Ui,
    SchemaKind::Domain,
    SchemaKind::Nav,
    SchemaKind::Catalog,
    SchemaKind::Needs,
    SchemaKind::Gaps,
];

// schemas/<kind>.schema.json, in the order of KINDS
const TEXTS: [&str; 7] = [
    include_str!("../../../../schemas/flow.schema.json"),
    include_str!("../../../../schemas/ui.schema.json"),
    include_str!("../../../../schemas/domain.schema.json"),
    include_str!("../../../../schemas/nav.schema.json"),
    include_str!("../../../../schemas/catalog.schema.json"),
    include_str!("../../../../schemas/needs.schema.json"),
    include_str!("../../../../schemas/gaps.schema.json"),
];

static ROOTS: LazyLock<Vec<Value>> = LazyLock::new(|| {
    TEXTS
        .iter()
        .zip(KINDS)
        .map(|(t, k)| js::json_parse(t).unwrap_or_else(|e| panic!("{}.schema.json: {e}", k.as_str())))
        .collect()
});

// compiled once, on first use; a keyword the validator does not run fails here, as Ajv's strict mode fails to compile
static VALIDATORS: LazyLock<Vec<Validator>> = LazyLock::new(|| {
    ROOTS
        .iter()
        .zip(KINDS)
        .map(|(r, k)| Validator::new(r).unwrap_or_else(|e| panic!("{}.schema.json: {e}", k.as_str())))
        .collect()
});

/// the kind of graph file, by its name, and a domain also by its `kind` (a domain may be split across files, SPEC.md
/// §5): None for a file no schema covers (binding, tutorial, kit, anchors.lock). A file that cannot be read is none
/// (the TypeScript checker threw there).
pub fn schema_kind_of(file: &str) -> Option<SchemaKind> {
    let name = nodepath::basename(file);
    match name.as_str() {
        "flow.yaml" => return Some(SchemaKind::Flow),
        "ui.yaml" => return Some(SchemaKind::Ui),
        "domain.yaml" => return Some(SchemaKind::Domain),
        "needs.yaml" => return Some(SchemaKind::Needs),
        "nav.yaml" | "nav.extracted.yaml" => return Some(SchemaKind::Nav),
        "needs.gaps.yaml" => return Some(SchemaKind::Gaps),
        _ => {}
    }
    if name.ends_with(".json") && nodepath::basename(&nodepath::dirname(file)) == "catalog" {
        return Some(SchemaKind::Catalog);
    }
    if may_be_domain(&name)
        && std::path::Path::new(file).exists()
        && std::fs::read(file).is_ok_and(|bytes| is_domain_text(&String::from_utf8_lossy(&bytes)))
    {
        return Some(SchemaKind::Domain);
    }
    None
}

/// A kind's schema, as written (schemas/<kind>.schema.json).
pub fn root(kind: SchemaKind) -> &'static Value {
    &ROOTS[kind.index()]
}

/// A kind's schema, compiled (schemas.ts's `validators[kind]`): `validate` gives Ajv's raw error list, which
/// `validate_file` simplifies and words.
pub fn validator(kind: SchemaKind) -> &'static Validator {
    &VALIDATORS[kind.index()]
}

// the steps of a JSON pointer, unescaped
fn steps(pointer: &str) -> Vec<String> {
    if pointer.is_empty() {
        return Vec::new();
    }
    pointer[1..].split('/').map(|t| t.replace("~1", "/").replace("~0", "~")).collect()
}

// one step of a JSON pointer, escaped
fn escape(step: &str) -> String {
    step.replace('~', "~0").replace('/', "~1")
}

fn within(path: &str, base: &str) -> bool {
    path == base || path.strip_prefix(base).is_some_and(|rest| rest.starts_with('/'))
}

// what `place` finds: the node at a pointer (None for an empty document) and, with a name, that map and the key's
// value (`Some(None)` for a key without one, None for no such key)
struct Place<'d> {
    at: Option<&'d Node>,
    map: Option<&'d YamlMap>,
    value: Option<Option<&'d Node>>,
}

// the YAML node at a JSON pointer; with `name`, the key `name` of the map there, that map and the key's value
fn place<'d>(doc: &'d Document, pointer: &str, name: Option<&str>) -> Place<'d> {
    let mut node = doc.contents.as_ref();
    for t in steps(pointer) {
        let next = match node {
            Some(Node::Map(m)) => m.items.iter().find(|p| p.key_string() == t).and_then(|p| p.value.as_ref()),
            Some(Node::Seq(s)) => yaml::array_index(&t).and_then(|i| s.items.get(i)),
            _ => None,
        };
        let Some(next) = next else { break };
        node = Some(next);
    }
    let (Some(name), Some(Node::Map(map))) = (name, node) else { return Place { at: node, map: None, value: None } };
    let pair = map.items.iter().find(|p| p.key_string() == name);
    Place { at: pair.and_then(|p| p.key.as_ref()).or(node), map: Some(map), value: pair.map(|p| p.value.as_ref()) }
}

// `x[k]` on a schema value, as the TypeScript reads a `$ref` step
fn step_into<'v>(x: &'v Value, k: &str) -> Option<&'v Value> {
    match x {
        Value::Object(o) => o.get(k),
        Value::Array(a) => array_index(k).and_then(|i| a.get(i)),
        _ => None,
    }
}

fn array_index(k: &str) -> Option<usize> {
    if k.is_empty() || (k.len() > 1 && k.starts_with('0')) || !k.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    k.parse().ok()
}

// a schema and the ones its `$ref`s name, inside the same file
fn chain(root: &'static Value, schema: Option<&'static Value>) -> Vec<&'static Value> {
    let mut out = Vec::new();
    let mut s = schema;
    while let Some(here) = s.filter(|x| x.is_collection()) {
        if out.len() >= 8 {
            break;
        }
        out.push(here);
        s = here.get_str("$ref").filter(|r| r.starts_with("#/")).and_then(|r| ref_at(root, r));
    }
    out
}

const ANCHOR: &str = "#/$defs/sourceAnchor";

fn matches(v: &Validator, pattern: &str, name: &str) -> bool {
    match v.regex(pattern) {
        Some(re) => re.is_match(name),
        None => regexp::compile(pattern).is_ok_and(|re| re.is_match(name)),
    }
}

fn ref_at(root: &'static Value, r: &str) -> Option<&'static Value> {
    r[2..].split('/').try_fold(root, |x, k| step_into(x, k))
}

/// where a source anchor is in a graph file's value, and what it says
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorPlace {
    pub path: Vec<String>, // the keys from the file's top to its `source`
    pub value: String,
}

/// Every source anchor in a graph file's value (SPEC.md §9): each `source` whose schema is the kind's `sourceAnchor`,
/// found by walking the value along its schema, so a key `source` that the schema gives another meaning (a nav graph's
/// `source`, a sample value or a parameter of ui.yaml, a field of a record) is none. Where the schema has
/// alternatives, every one that the value's shape reaches is walked.
pub fn anchors_in(kind: SchemaKind, data: &Value) -> Vec<AnchorPlace> {
    let root = root(kind);
    let mut out: IndexMap<String, AnchorPlace> = IndexMap::new();
    visit(root, validator(kind), Some(root), data, &[], &mut HashSet::new(), &mut out);
    out.into_values().collect()
}

fn visit(
    root: &'static Value,
    v: &Validator,
    schema: Option<&'static Value>,
    value: &Value,
    path: &[String],
    seen: &mut HashSet<*const Value>,
    out: &mut IndexMap<String, AnchorPlace>,
) {
    let Some(s) = schema.filter(|s| s.is_collection()) else { return };
    if !seen.insert(s as *const Value) {
        return;
    }
    if s.get_str("$ref") == Some(ANCHOR) {
        if let Value::String(text) = value {
            out.insert(path.join("\0"), AnchorPlace { path: path.to_vec(), value: text.clone() });
        }
        return;
    }
    if let Some(r) = s.get_str("$ref").filter(|r| r.starts_with("#/")) {
        visit(root, v, ref_at(root, r), value, path, seen, out);
    }
    for k in ["allOf", "anyOf", "oneOf"] {
        for branch in s.get_array(k).map(Vec::as_slice).unwrap_or(&[]) {
            visit(root, v, Some(branch), value, path, seen, out);
        }
    }
    for k in ["then", "else"] {
        visit(root, v, s.get(k), value, path, seen, out);
    }
    let child = |k: &str| {
        let mut p = path.to_vec();
        p.push(k.to_string());
        p
    };
    if let Value::Array(items) = value {
        let prefix = s.get_array("prefixItems").map(Vec::as_slice).unwrap_or(&[]);
        for (i, x) in items.iter().enumerate() {
            let sub = if i < prefix.len() { Some(&prefix[i]) } else { s.get("items") };
            visit(root, v, sub, x, &child(&i.to_string()), &mut HashSet::new(), out);
        }
        return;
    }
    let Value::Object(entries) = value else { return };
    let properties = s.get_object("properties");
    let by_pattern = s.get_object("patternProperties");
    for (k, x) in entries.iter() {
        let sub = match properties.and_then(|p| p.get(k)) {
            Some(sub) => Some(sub),
            None => by_pattern
                .and_then(|ps| ps.iter().find(|(p, _)| matches(v, p, k)).map(|(_, sub)| sub))
                .or_else(|| s.get("additionalProperties")),
        };
        visit(root, v, sub, x, &child(k), &mut HashSet::new(), out);
    }
}

// edits between two names, a swap of two neighbours counting as one (titel, title)
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let mut d: Vec<Vec<usize>> =
        (0..=a.len()).map(|i| std::iter::once(i).chain(std::iter::repeat_n(0, b.len())).collect()).collect();
    for (j, x) in d[0].iter_mut().enumerate().skip(1) {
        *x = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]));
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

const KIND_WORDS: [(&str, &str); 7] = [
    ("object", "an object"),
    ("array", "a list"),
    ("string", "a string"),
    ("integer", "an integer"),
    ("number", "a number"),
    ("boolean", "true or false"),
    ("null", "null"),
];

fn kind_word(t: &str) -> &str {
    KIND_WORDS.iter().find(|(k, _)| *k == t).map_or(t, |(_, w)| w)
}

fn either(xs: &[String]) -> String {
    match xs {
        [] => String::new(),
        [x] => x.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

fn quoted(xs: &[Value]) -> String {
    either(&xs.iter().map(js::stringify).collect::<Vec<_>>())
}

/// An error of Ajv's list as `simplify` keeps it (schemas.ts's `Failure`): with the reason of the if/then rule that
/// gave it, and the types or the alternative keys of the errors at its place merged into it.
struct Failure<'e, 'a> {
    e: &'e AjvError<'a>,
    because: Option<&'static str>, // why the rule exists, from the $comment of the if/then that gave the failure
    alternative: bool,             // a `required` of one alternative of an anyOf or a oneOf
    types: Vec<String>,            // a `type`'s params.type, as a list
    missing: Vec<&'static str>,    // a `required`'s params.missingProperty, as a list
}

fn message(f: &Failure, root: &'static Value, v: &Validator) -> String {
    let e = f.e;
    let parent = v.schema_value(e.node);
    match (e.keyword, &e.params) {
        ("additionalProperties", Params::AdditionalProperty(name))
        | ("unevaluatedProperties", Params::UnevaluatedProperty(name)) => {
            let known: Vec<&str> = chain(root, Some(parent))
                .iter()
                .flat_map(|x| x.get_object("properties").into_iter().flat_map(|p| p.keys().map(String::as_str)))
                .collect();
            let lower = name.to_lowercase();
            let near: Vec<Value> = known
                .into_iter()
                .filter(|k| distance(&k.to_lowercase(), &lower) <= 2.min(1.max(js::len16(k) / 3)))
                .map(Value::from)
                .collect();
            let hint = if near.is_empty() { String::new() } else { format!("; did you mean {}?", quoted(&near)) };
            format!("\"{name}\" is not a key here{hint}")
        }
        ("required", _) => {
            format!("{} is missing", f.missing.iter().map(|k| js::quote_string(k)).collect::<Vec<_>>().join(" or "))
        }
        ("enum", Params::AllowedValues(allowed)) => {
            let allowed = allowed.as_array().map(Vec::as_slice).unwrap_or(&[]);
            if allowed.len() <= 12 {
                format!("must be {}", quoted(allowed))
            } else {
                format!("{} is not one of its {} values", e.data.stringify(), allowed.len())
            }
        }
        ("const", Params::AllowedValue(x)) => format!("must be {}", js::stringify(x)),
        ("type", _) => {
            format!("must be {}", either(&f.types.iter().map(|t| kind_word(t).to_string()).collect::<Vec<_>>()))
        }
        ("pattern", Params::Pattern(p)) => format!("{} does not match {p}", e.data.stringify()),
        ("propertyNames", Params::PropertyName(name)) => {
            let said = chain(root, parent.get("propertyNames")).into_iter().find_map(|x| x.get_str("description"));
            match said {
                None => format!("the name \"{name}\" is not allowed"),
                Some(s) => {
                    let first = s.split(". ").next().unwrap_or("");
                    format!("the name \"{name}\" is not allowed: {}", first.strip_suffix('.').unwrap_or(first))
                }
            }
        }
        ("false schema", _) => "is not allowed here".to_string(),
        ("oneOf", _) => "matches more than one of its forms".to_string(),
        ("minItems" | "maxItems" | "minProperties" | "maxProperties", Params::Limit(n)) => {
            let what = match (e.keyword.ends_with("Items"), *n == 1.0) {
                (true, true) => "item",
                (true, false) => "items",
                (false, true) => "key",
                (false, false) => "keys",
            };
            let n = js::number_to_string(*n);
            if e.keyword.starts_with("min") {
                format!("needs at least {n} {what}")
            } else {
                format!("has more than {n} {what}")
            }
        }
        _ => e.message(),
    }
}

// `/\/(anyOf|oneOf)\/\d+\/required$/`
static ALTERNATIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/(anyOf|oneOf)/[0-9]+/required$").unwrap_or_else(|e| panic!("{e}")));

fn type_names(e: &AjvError) -> Vec<String> {
    match e.params {
        Params::Type(Value::Array(xs)) => xs.iter().map(js::to_js_string).collect(),
        Params::Type(t) => vec![js::to_js_string(t)],
        _ => Vec::new(),
    }
}

// Ajv reports every alternative of an anyOf and every rule around a then; keep what the writer can act on
fn simplify<'e, 'a>(errors: &'e [AjvError<'a>], v: &Validator) -> Vec<Failure<'e, 'a>> {
    // the reason for an if/then rule, from its $comment, goes onto the errors its then or else gave (just before it)
    let mut because: Vec<Option<&'static str>> = vec![None; errors.len()];
    for (i, e) in errors.iter().enumerate() {
        let comment = v.schema_value(e.node).get_str("$comment");
        let Some(comment) = comment.filter(|_| e.keyword == "if") else { continue };
        let base = e.schema_path.strip_suffix("/if").unwrap_or(&e.schema_path);
        let (then, else_) = (format!("{base}/then/"), format!("{base}/else/"));
        for j in (0..i).rev() {
            let x = &errors[j];
            if !within(&x.instance_path, &e.instance_path)
                || !(x.schema_path.starts_with(&then) || x.schema_path.starts_with(&else_))
            {
                break;
            }
            because[j] = Some(comment);
        }
    }
    let wrapper = |x: &AjvError| matches!(x.keyword, "if" | "anyOf" | "oneOf");
    let explained = |i: usize| {
        let e = &errors[i];
        errors.iter().enumerate().any(|(j, x)| {
            j != i
                && !wrapper(x)
                && ((x.instance_path != e.instance_path && within(&x.instance_path, &e.instance_path))
                    || (x.instance_path == e.instance_path && x.keyword != "type"))
        })
    };
    let kept = (0..errors.len()).filter(|&i| {
        let e = &errors[i];
        match (e.keyword, &e.params) {
            ("if", _) => false, // its then or else said it
            ("anyOf", _) | ("oneOf", Params::PassingSchemas(None)) => {
                !explained(i)
                    && !errors
                        .iter()
                        .enumerate()
                        .any(|(j, x)| j != i && x.keyword == "type" && x.instance_path == e.instance_path)
            }
            ("type", _) => !explained(i), // an alternative of the right type is there, and failed further in
            _ if e.property_name.is_some() => false, // the name's own failure; its propertyNames error says it
            _ => true,
        }
    });
    // what is left at one place in one form is one line: the types the alternatives want, the keys of which one is
    // needed
    let mut out: Vec<Failure> = Vec::new();
    for i in kept.collect::<Vec<_>>() {
        let e = &errors[i];
        let alternative = e.keyword == "required" && ALTERNATIVE.is_match(&e.schema_path);
        let twin = out.iter_mut().find(|x| {
            x.e.instance_path == e.instance_path
                && x.e.keyword == e.keyword
                && (e.keyword == "type" || (alternative && x.alternative))
        });
        match twin {
            Some(t) if e.keyword == "type" => {
                // `[...new Set([...twin.params.type, ...[p.type].flat()])]`
                let mut merged: Vec<String> = Vec::new();
                for name in t.types.iter().cloned().chain(type_names(e)) {
                    if !merged.contains(&name) {
                        merged.push(name);
                    }
                }
                t.types = merged;
            }
            Some(t) => {
                if let Params::MissingProperty(k) = e.params {
                    t.missing.push(k);
                }
            }
            None => {
                let types = if e.keyword == "type" { type_names(e) } else { Vec::new() };
                let missing = match e.params {
                    Params::MissingProperty(k) if e.keyword == "required" => vec![k],
                    _ => Vec::new(),
                };
                out.push(Failure { e, because: because[i], alternative, types, missing });
            }
        }
    }
    out
}

// the id of the graph a file belongs to, when the file does not say it: the flow, nav or ui graph beside it
fn graph_id_beside(dir: &str) -> Option<String> {
    for name in ["flow.yaml", "nav.yaml", "ui.yaml"] {
        let file = nodepath::join2(dir, name);
        if !Path::new(&file).exists() {
            continue;
        }
        // a graph file that does not parse names no graph; its own check says why
        let Ok(bytes) = std::fs::read(&file) else { continue };
        if let Ok(value) = read_yaml(&String::from_utf8_lossy(&bytes))
            && let Some(id) = value.get_str("id")
        {
            return Some(id.to_string());
        }
    }
    None
}

/// Every place where the file does not follow its kind's schema; a file written for another format is left to the
/// format check. (A file that cannot be read gives none here; the TypeScript checker threw.)
pub fn validate_file(file: &str) -> Vec<SchemaFinding> {
    let Some(kind) = schema_kind_of(file) else { return Vec::new() };
    let Ok(bytes) = std::fs::read(file) else { return Vec::new() };
    let text = String::from_utf8_lossy(&bytes);
    let refused = if kind == SchemaKind::Catalog { None } else { unsupported(&text) };
    let parsed = yaml::parse_document(&text);
    let doc = &parsed.doc;
    let at = |offset: Option<usize>| parsed.line_pos(offset.unwrap_or(0));
    let own_file = if kind == SchemaKind::Catalog {
        format!("catalog/{}", nodepath::basename(file))
    } else {
        nodepath::basename(file)
    };
    // the graph's id: the file's own once it is read, else the one beside it, looked up at the first failure
    let mut graph: Option<String> = None;
    let failure = |graph_now: &mut Option<String>, place: At, said: String, pointer: &str| -> SchemaFinding {
        if graph_now.is_none() {
            let dir = if kind == SchemaKind::Catalog {
                nodepath::dirname(&nodepath::dirname(file))
            } else {
                nodepath::dirname(file)
            };
            *graph_now = graph_id_beside(&dir);
        }
        SchemaFinding {
            offset: None,
            line: place.line,
            column: place.column,
            code: None,
            message: said,
            graph: graph_now.clone(),
            element: format!("{own_file}#{pointer}"),
        }
    };

    // the first YAML error only: the ones after it mostly follow from it; one before the unsupported place comes first
    let first_error = doc.errors.first();
    if let Some(u) = refused
        && first_error.is_none_or(|e| u.offset < e.pos.0)
    {
        let place = At { line: u.line, column: u.column };
        let mut f = failure(&mut graph, place, u.message, "");
        // the TypeScript spreads the refusal into the finding: its offset and code come along
        f.offset = Some(u.offset);
        f.code = Some(u.code);
        return vec![f];
    }
    if let Some(e) = first_error {
        let said = format!("not YAML: {}", e.message.split('\n').next().unwrap_or(""));
        return vec![failure(&mut graph, at(Some(e.pos.0)), said, "")];
    }
    let data = if kind == SchemaKind::Catalog {
        match js::json_parse(&text) {
            Ok(data) => data,
            Err(err) => {
                let said = format!("not JSON: {}", err.message);
                return vec![failure(&mut graph, At { line: 1, column: 1 }, said, "")];
            }
        }
    } else {
        doc.to_js()
    };
    if kind != SchemaKind::Catalog {
        if data.get_str("schema").is_some_and(|s| s != FORMAT) {
            return Vec::new();
        }
        if let Some(id) = data.get_str("id") {
            graph = Some(id.to_string());
        }
    }

    let v = validator(kind);
    let errors = v.validate(&data);
    if errors.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<SchemaFinding> = Vec::new();
    for f in simplify(&errors, v) {
        let e = f.e;
        let name = match e.params {
            Params::AdditionalProperty(k) | Params::UnevaluatedProperty(k) | Params::PropertyName(k) => Some(k),
            _ => None,
        };
        let place = place(doc, &e.instance_path, name);
        let path = steps(&e.instance_path).join(".");
        let mut said = format!("{}: {}", if path.is_empty() { "the file" } else { &path }, message(&f, root(kind), v));
        let because = f
            .because
            .or_else(|| if e.keyword == "false schema" { None } else { v.schema_value(e.node).get_str("$comment") });
        if let Some(because) = because {
            said.push_str(&format!(" ({because})"));
        }
        // `{ title: Done, and paid }` is a title and a key "and paid": YAML ends a flow mapping's value at the comma
        let empty = match place.value {
            Some(None) => true,
            Some(Some(Node::Scalar(s))) => s.value.is_null(),
            _ => false,
        };
        if name.is_some() && place.map.is_some_and(|m| m.flow) && empty {
            said.push_str("; a comma inside { } ends the value before it: quote that value");
        }
        let pointer = match (name, e.keyword == "required") {
            (Some(name), _) => format!("{}/{}", e.instance_path, escape(name)),
            (None, true) => format!("{}/{}", e.instance_path, escape(&f.missing.join("|"))),
            (None, false) => e.instance_path.clone(),
        };
        let finding = failure(&mut graph, at(place.at.map(|n| n.range()[0])), said, &pointer);
        if !out.iter().any(|x| x.line == finding.line && x.column == finding.column && x.message == finding.message) {
            out.push(finding);
        }
    }
    out
}

#[cfg(test)]
mod tests;
