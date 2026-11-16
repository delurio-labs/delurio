//! A domain (SPEC.md §1, §5) is every file in its directory that says `kind: domain`, read as one: `domain.yaml` first,
//! then the others by name. `schema`, `kind` and `id` open every file and the id is the same in all; maps merge key by
//! key at any depth; the top-level lists `policies` and `invariants` join in that order; any other value (a string, a
//! number, a list) is given in one file only. A split lets a generated part and a hand-written part sit side by side.
//! The port of domain.ts.

use std::cmp::Ordering;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;
use serde::Serialize;

use crate::js::{self, Object, Value};
use crate::yaml::{key_at, read_yaml};

/// a file of a domain and its text
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainPart {
    pub file: String, // as the caller names it; problems name it the same way
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainPlace {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DomainRule {
    DomainTwice,
    DomainPlace,
}

impl DomainRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            DomainRule::DomainTwice => "domain-twice",
            DomainRule::DomainPlace => "domain-place",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainProblem {
    pub rule: DomainRule,
    pub file: String,
    pub line: usize,
    pub column: usize,
    /// where the value was given first
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<DomainPlace>,
    pub message: String,
    /// the file's name and the JSON pointer of what it is about: user.yaml#/entities/User/fields/email/type
    pub element: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MergedDomain {
    pub id: Option<String>,
    #[serde(serialize_with = "serialize_object")]
    pub doc: Object,
    pub problems: Vec<DomainProblem>,
}

/// an `Object` written as the map it is
pub(crate) fn serialize_object<S: serde::Serializer>(o: &Object, s: S) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = s.serialize_map(Some(o.len()))?;
    for (k, v) in o.iter() {
        map.serialize_entry(k, v)?;
    }
    map.end()
}

/// the files whose name says another kind (SPEC.md §1); only the others are read to see whether they are a domain
pub const OTHER_KINDS: [&str; 10] = [
    "flow.yaml",
    "ui.yaml",
    "nav.yaml",
    "nav.extracted.yaml",
    "binding.yaml",
    "tutorial.yaml",
    "kit.yaml",
    "needs.yaml",
    "needs.gaps.yaml",
    "anchors.lock.yaml",
];
const OWN: [&str; 3] = ["schema", "kind", "id"];
const JOINED: [&str; 2] = ["policies", "invariants"];

/// the names every JavaScript object has from `Object.prototype`: `key in target` is true for them though the map does
/// not give them, so a file that gives one is told it is given twice, as the TypeScript checker tells it
const INHERITED: [&str; 12] = [
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

/// a file that may be part of a domain, by its name
pub fn may_be_domain(name: &str) -> bool {
    name == "domain.yaml" || (name.ends_with(".yaml") && !OTHER_KINDS.contains(&name))
}

static KIND_DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^kind:[ \t]*(?:domain|'domain'|"domain")[ \t]*(?:#.*)?$"#).expect("the pattern compiles")
});

/// `kind: domain` at the top of the file; read from the text, so a file that is not YAML is still found and its own
/// schema check says why. A line ends where JavaScript's multiline `^`/`$` end it: at \n, \r, U+2028 and U+2029.
pub fn is_domain_text(text: &str) -> bool {
    text.split(['\n', '\r', '\u{2028}', '\u{2029}']).any(|line| KIND_DOMAIN.is_match(line))
}

fn base(n: &str) -> &str {
    n.rsplit('/').next().unwrap_or(n)
}

/// the reading order of a domain's files: domain.yaml, then the others by name. (Two domain.yaml, which no caller
/// gives, are kept in name order: the TypeScript comparator puts each before the other.)
pub fn in_reading_order(names: &[String]) -> Vec<String> {
    let mut out = names.to_vec();
    out.sort_by(|a, b| match (base(a) == "domain.yaml", base(b) == "domain.yaml") {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => js::cmp_utf16(a, b),
    });
    out
}

/// a JSON pointer of keys
pub(crate) fn pointer<S: AsRef<str>>(path: &[S]) -> String {
    path.iter().map(|s| format!("/{}", s.as_ref().replace('~', "~0").replace('/', "~1"))).collect()
}

fn name_of(file: &str) -> &str {
    base(file)
}

struct Merge<'a> {
    parts: &'a [DomainPart],
    origin: IndexMap<String, usize>, // the pointer of a value given -> the part that gave it
    problems: Vec<DomainProblem>,
}

impl Merge<'_> {
    fn place(&self, i: usize, path: &[String]) -> DomainPlace {
        let at = key_at(&self.parts[i].text, path);
        DomainPlace { file: self.parts[i].file.clone(), line: at.line, column: at.column }
    }

    // the part that gave the value at `path`, or the map it sits in
    fn giver(&self, path: &[String]) -> usize {
        for n in (1..=path.len()).rev() {
            if let Some(&o) = self.origin.get(&pointer(&path[..n])) {
                return o;
            }
        }
        0
    }

    fn twice(&mut self, i: usize, path: &[String], first: usize, said: &str) {
        let at = self.place(i, path);
        let first_at = self.place(first, path);
        let message = format!(
            "{said} is given in {}:{} too: a domain's files give each value once (SPEC.md §5)",
            name_of(&self.parts[first].file),
            first_at.line
        );
        let element = format!("{}#{}", name_of(&self.parts[i].file), pointer(path));
        self.problems.push(DomainProblem {
            rule: DomainRule::DomainTwice,
            file: at.file,
            line: at.line,
            column: at.column,
            first: Some(first_at),
            message,
            element,
        });
    }

    fn put(&mut self, target: &mut Object, key: &str, value: &Value, path: Vec<String>, i: usize) {
        if !target.contains_key(key) && !INHERITED.contains(&key) {
            target.insert(key, value.clone());
            self.origin.insert(pointer(&path), i);
            return;
        }
        if let (Some(Value::Object(here)), Value::Object(value)) = (target.get_mut(key), value) {
            for (k, v) in value.iter() {
                let mut p = path.clone();
                p.push(k.clone());
                self.put(here, k, v, p, i);
            }
            return;
        }
        let first = self.giver(&path);
        self.twice(i, &path, first, &path.join("."));
    }
}

/// the files of one directory, read as one domain; a file that is not YAML, or not a map, is left to its schema check
pub fn merge_domain(parts: &[DomainPart]) -> MergedDomain {
    let mut doc = Object::new();
    let mut m = Merge { parts, origin: IndexMap::new(), problems: Vec::new() };
    let mut invariants: IndexMap<String, (usize, usize)> = IndexMap::new(); // name -> its part and index
    let mut id: Option<String> = None;
    let mut id_from = 0;

    for (i, part) in parts.iter().enumerate() {
        let Ok(Value::Object(data)) = read_yaml(&part.text) else { continue };
        if let Some(Value::String(own)) = data.get("id") {
            match &id {
                None => {
                    id = Some(own.clone());
                    id_from = i;
                }
                Some(first) if first != own => {
                    let at = m.place(i, &["id".to_string()]);
                    m.problems.push(DomainProblem {
                        rule: DomainRule::DomainPlace,
                        file: at.file,
                        line: at.line,
                        column: at.column,
                        first: None,
                        message: format!(
                            "the id is {own}, and {} beside it is {first}: a directory holds one domain, every file of it with its id (SPEC.md §1)",
                            name_of(&parts[id_from].file)
                        ),
                        element: format!("{}#/id", name_of(&part.file)),
                    });
                    continue;
                }
                Some(_) => {}
            }
        }
        for (key, value) in data.iter() {
            if OWN.contains(&key.as_str()) {
                // doc[key] ??= value
                if doc.get(key).is_none_or(Value::is_null) {
                    doc.insert(key.as_str(), value.clone());
                }
                continue;
            }
            if let Value::Array(items) = value
                && JOINED.contains(&key.as_str())
                && doc.get(key).is_none_or(Value::is_array)
            {
                let mut joined = match doc.get(key) {
                    None => {
                        m.origin.insert(pointer(&[key]), i);
                        Vec::new()
                    }
                    Some(v) => v.as_array().cloned().unwrap_or_default(),
                };
                joined.extend(items.iter().cloned());
                doc.insert(key.as_str(), Value::Array(joined));
                if key != "invariants" {
                    continue;
                }
                for (index, inv) in items.iter().enumerate() {
                    let Some(name) = inv.get_str("id") else { continue };
                    match invariants.get(name) {
                        None => {
                            invariants.insert(name.to_string(), (i, index));
                        }
                        Some(&(seen_part, seen_index)) if seen_part != i => {
                            let at = m.place(i, &["invariants".into(), index.to_string(), "id".into()]);
                            let first_at =
                                m.place(seen_part, &["invariants".into(), seen_index.to_string(), "id".into()]);
                            let message = format!(
                                "the invariant {name} is given in {}:{} too: a domain's files give each invariant once (SPEC.md §5)",
                                name_of(&parts[seen_part].file),
                                first_at.line
                            );
                            m.problems.push(DomainProblem {
                                rule: DomainRule::DomainTwice,
                                file: at.file,
                                line: at.line,
                                column: at.column,
                                first: Some(first_at),
                                message,
                                element: format!("{}#/invariants/{index}", name_of(&part.file)),
                            });
                        }
                        Some(_) => {}
                    }
                }
                continue;
            }
            m.put(&mut doc, key, value, vec![key.clone()], i);
        }
    }
    MergedDomain { id, doc, problems: m.problems }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn domain_files_by_name_and_text() {
        assert!(may_be_domain("domain.yaml"));
        assert!(may_be_domain("user.yaml"));
        assert!(!may_be_domain("flow.yaml"));
        assert!(!may_be_domain("needs.gaps.yaml"));
        assert!(!may_be_domain("user.yml"));
        // node -e "import('./dist/domain.js').then(d => console.log(['kind: domain', 'a: 1\r\nkind: \"domain\" # x', 'kind: domains', ' kind: domain', 'x: 1 kind: domain'].map(d.isDomainText)))"
        let said: Vec<bool> = [
            "kind: domain",
            "a: 1\r\nkind: \"domain\" # x",
            "kind: domains",
            " kind: domain",
            "x: 1\u{2028}kind: domain",
        ]
        .into_iter()
        .map(is_domain_text)
        .collect();
        assert_eq!(said, [true, true, false, false, true]);
    }

    #[test]
    fn domain_yaml_is_read_first() {
        // node -e "import('./dist/domain.js').then(d => console.log(d.inReadingOrder(['b.yaml', 'x/domain.yaml', 'a.yaml', 'B.yaml'])))"
        let names: Vec<String> = ["b.yaml", "x/domain.yaml", "a.yaml", "B.yaml"].map(String::from).to_vec();
        assert_eq!(in_reading_order(&names), ["x/domain.yaml", "B.yaml", "a.yaml", "b.yaml"]);
        assert_eq!(pointer(&["a/b", "c~d"]), "/a~1b/c~0d");
    }

    /// three files of one directory: a value given twice, a name every JavaScript object has, an invariant given twice,
    /// and a file of another domain
    pub(crate) fn parts() -> Vec<DomainPart> {
        let part = |file: &str, text: &str| DomainPart { file: file.into(), text: text.into() };
        vec![
            part(
                "d/domain.yaml",
                "schema: delurio-graph/0\nkind: domain\nid: x\nentities:\n  Note:\n    fields:\n      id: { type: string }\npolicies:\n  - { actor: a, do: [f] }\ninvariants:\n  - { id: kept, rule: r }\n",
            ),
            part(
                "d/note.yaml",
                "kind: domain\nid: x\nentities:\n  Note:\n    fields:\n      id: { type: number }\n      body: { type: string }\n  constructor: { fields: {} }\npolicies:\n  - { actor: b, do: [g] }\ninvariants:\n  - { id: kept, rule: again }\n",
            ),
            part("d/other.yaml", "kind: domain\nid: y\n"),
        ]
    }

    #[test]
    fn files_merged_into_one_domain() {
        // node --input-type=module -e "import { mergeDomain } from './dist/domain.js'; console.log(JSON.stringify(mergeDomain(parts)))"
        // with the parts above (needs DELURIO_YAML_SHIM)
        let m = merge_domain(&parts());
        assert_eq!(m.id.as_deref(), Some("x"));
        assert_eq!(
            js::stringify(&Value::Object(m.doc)),
            r#"{"schema":"delurio-graph/0","kind":"domain","id":"x","entities":{"Note":{"fields":{"id":{"type":"string"},"body":{"type":"string"}}}},"policies":[{"actor":"a","do":["f"]},{"actor":"b","do":["g"]}],"invariants":[{"id":"kept","rule":"r"},{"id":"kept","rule":"again"}]}"#
        );
        let place = |file: &str, line, column| DomainPlace { file: file.into(), line, column };
        let said: Vec<(DomainRule, DomainPlace, Option<DomainPlace>, &str, &str)> = m
            .problems
            .iter()
            .map(|p| {
                (p.rule, place(&p.file, p.line, p.column), p.first.clone(), p.message.as_str(), p.element.as_str())
            })
            .collect();
        assert_eq!(
            said,
            [
                (
                    DomainRule::DomainTwice,
                    place("d/note.yaml", 6, 13),
                    Some(place("d/domain.yaml", 7, 13)),
                    "entities.Note.fields.id.type is given in domain.yaml:7 too: a domain's files give each value once (SPEC.md §5)",
                    "note.yaml#/entities/Note/fields/id/type",
                ),
                (
                    DomainRule::DomainTwice,
                    place("d/note.yaml", 8, 3),
                    Some(place("d/domain.yaml", 4, 1)),
                    "entities.constructor is given in domain.yaml:4 too: a domain's files give each value once (SPEC.md §5)",
                    "note.yaml#/entities/constructor",
                ),
                (
                    DomainRule::DomainTwice,
                    place("d/note.yaml", 12, 7),
                    Some(place("d/domain.yaml", 11, 7)),
                    "the invariant kept is given in domain.yaml:11 too: a domain's files give each invariant once (SPEC.md §5)",
                    "note.yaml#/invariants/0",
                ),
                (
                    DomainRule::DomainPlace,
                    place("d/other.yaml", 2, 1),
                    None,
                    "the id is y, and domain.yaml beside it is x: a directory holds one domain, every file of it with its id (SPEC.md §1)",
                    "other.yaml#/id",
                ),
            ]
        );
    }
}
