//! The JSON Schemas of schemas/ (one per kind of graph file, SPEC.md §12): the kind of a graph file, each kind's
//! schema compiled, and the source anchors a value holds, found by walking it along its schema.
//!
//! The port of schemas.ts, its first part. Its validator, Ajv 8.20.0 (with ajv-formats, which these schemas do not
//! use), is `ajv.rs`: the findings depend on Ajv's exact error list and its order. `regexp.rs` writes the schemas'
//! JavaScript patterns for the regex crate.

mod ajv;
mod regexp;

use std::collections::HashSet;
use std::sync::LazyLock;

use indexmap::IndexMap;

pub use ajv::{AjvError, Data, Params, SchemaId, Validator};

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

/// A kind's schema, compiled (schemas.ts's `validators[kind]`): `validate` gives Ajv's raw error list.
pub fn validator(kind: SchemaKind) -> &'static Validator {
    &VALIDATORS[kind.index()]
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

#[cfg(test)]
mod tests;
