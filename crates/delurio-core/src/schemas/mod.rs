//! The JSON Schemas of schemas/ (one per kind of graph file, SPEC.md §12): the part of the port of schemas.ts that the
//! anchors rule reads. `schema_kind_of` is the port; `anchors_in` here walks the schemas as schemas.ts does, a stand-in
//! for the port of the schemas module, which replaces it.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use indexmap::IndexMap;
use regex::Regex;

use crate::domain::{is_domain_text, may_be_domain};
use crate::js::{self, Value};
use crate::nodepath;

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
    pub fn as_str(&self) -> &'static str {
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
}

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

/// where a source anchor is in a graph file's value, and what it says
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorPlace {
    pub path: Vec<String>, // the keys from the file's top to its `source`
    pub value: String,
}

fn parsed(text: &str) -> Value {
    js::json_parse(text).expect("the schemas compiled in are JSON")
}

static ROOTS: LazyLock<[Value; 7]> = LazyLock::new(|| {
    [
        parsed(include_str!("../../../../schemas/flow.schema.json")),
        parsed(include_str!("../../../../schemas/ui.schema.json")),
        parsed(include_str!("../../../../schemas/domain.schema.json")),
        parsed(include_str!("../../../../schemas/nav.schema.json")),
        parsed(include_str!("../../../../schemas/catalog.schema.json")),
        parsed(include_str!("../../../../schemas/needs.schema.json")),
        parsed(include_str!("../../../../schemas/gaps.schema.json")),
    ]
});

fn root_of(kind: SchemaKind) -> &'static Value {
    let at = match kind {
        SchemaKind::Flow => 0,
        SchemaKind::Ui => 1,
        SchemaKind::Domain => 2,
        SchemaKind::Nav => 3,
        SchemaKind::Catalog => 4,
        SchemaKind::Needs => 5,
        SchemaKind::Gaps => 6,
    };
    &ROOTS[at]
}

const ANCHOR: &str = "#/$defs/sourceAnchor";

static PATTERNS: LazyLock<Mutex<HashMap<String, Option<Regex>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn matches(pattern: &str, name: &str) -> bool {
    let Ok(mut patterns) = PATTERNS.lock() else { return false };
    patterns
        .entry(pattern.to_string())
        .or_insert_with(|| Regex::new(pattern).ok())
        .as_ref()
        .is_some_and(|re| re.is_match(name))
}

fn ref_at(root: &'static Value, r: &str) -> Option<&'static Value> {
    r[2..].split('/').try_fold(root, |x, k| x.get(k))
}

fn visit(
    root: &'static Value,
    schema: Option<&'static Value>,
    value: &Value,
    path: Vec<String>,
    seen: &mut HashSet<*const Value>,
    out: &mut IndexMap<String, AnchorPlace>,
) {
    let Some(schema) = schema else { return };
    let Value::Object(s) = schema else { return };
    if !seen.insert(schema as *const Value) {
        return;
    }
    let r = s.get("$ref").and_then(Value::as_str);
    if r == Some(ANCHOR) {
        if let Value::String(v) = value {
            out.insert(path.join("\0"), AnchorPlace { path, value: v.clone() });
        }
        return;
    }
    if let Some(r) = r
        && r.starts_with("#/")
    {
        visit(root, ref_at(root, r), value, path.clone(), seen, out);
    }
    for k in ["allOf", "anyOf", "oneOf"] {
        for branch in s.get(k).and_then(Value::as_array).into_iter().flatten() {
            visit(root, Some(branch), value, path.clone(), seen, out);
        }
    }
    for k in ["then", "else"] {
        visit(root, s.get(k), value, path.clone(), seen, out);
    }
    if let Value::Array(items) = value {
        let prefix = s.get("prefixItems").and_then(Value::as_array);
        for (i, x) in items.iter().enumerate() {
            let sub = match prefix {
                Some(p) if i < p.len() => p.get(i),
                _ => s.get("items"),
            };
            let mut p = path.clone();
            p.push(i.to_string());
            visit(root, sub, x, p, &mut HashSet::new(), out);
        }
        return;
    }
    let Value::Object(value) = value else { return };
    let properties = s.get("properties").and_then(Value::as_object);
    let by_pattern = s.get("patternProperties").and_then(Value::as_object);
    for (k, x) in value.iter() {
        let sub = match properties.and_then(|p| p.get(k)) {
            Some(sub) => Some(sub),
            None => by_pattern
                .and_then(|o| o.iter().find(|(p, _)| matches(p, k)).map(|(_, sub)| sub))
                .filter(|sub| !sub.is_null())
                .or_else(|| s.get("additionalProperties")),
        };
        let mut p = path.clone();
        p.push(k.clone());
        visit(root, sub, x, p, &mut HashSet::new(), out);
    }
}

/// every source anchor in a graph file's value (SPEC.md §9): each `source` whose schema is the kind's `sourceAnchor`,
/// found by walking the value along its schema, so a key `source` that the schema gives another meaning (a nav graph's
/// `source`, a sample value or a parameter of ui.yaml, a field of a record) is none. Where the schema has alternatives,
/// every one that the value's shape reaches is walked.
pub fn anchors_in(kind: SchemaKind, data: &Value) -> Vec<AnchorPlace> {
    let root = root_of(kind);
    let mut out = IndexMap::new();
    visit(root, Some(root), data, Vec::new(), &mut HashSet::new(), &mut out);
    out.into_values().collect()
}
