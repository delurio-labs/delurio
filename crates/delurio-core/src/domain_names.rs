//! The names a domain gives one another (SPEC.md §5), each held to what the domain has, all its files read as one: an
//! action's `returns` and every `references` (on an entity's field, or in a body, a query, a result or a refusal's body)
//! name an entity; an entity's `key` and `required` name its own fields; a policy's `do` names actions, and its `read`
//! and `write` entities. A name that the domain does not have is an error, as a flow's `invoke` names are (decided on
//! 16 Nov 2026, d-1116-10). Each finding is given in the file that gives the name, at its line, with a note where another
//! domain beside it has the entity: a domain names only its own, and one domain may be split across files instead.
//! The port of domain-names.ts.

use serde::Serialize;

use crate::domain::DomainPart;
use crate::js::{Object, Value};
use crate::yaml::{key_at, read_yaml};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NamePlace {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NameRule {
    EntityUnknown,
    FieldUnknown,
    PolicyUnknown,
}

impl NameRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            NameRule::EntityUnknown => "entity-unknown",
            NameRule::FieldUnknown => "field-unknown",
            NameRule::PolicyUnknown => "policy-unknown",
        }
    }
}

/// a note of a finding: a place and what is there (`NamePlace & { message }`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameNote {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameProblem {
    pub rule: NameRule,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
    /// where in the domain, by names, not by file or line: entities/Consent/fields/clientId/references
    pub element: String,
    pub notes: Vec<NameNote>,
}

/// another domain that has an entity, and where it gives it (`NamePlace & { domain }`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityPlace {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub domain: String,
}

/// another domain that has an entity of this name, and where it gives it
pub type EntityElsewhere<'a> = dyn Fn(&str) -> Option<EntityPlace> + 'a;

/// the `EntityElsewhere` that knows no other domain: what `checkDomainNames` is given when none is
pub fn nowhere(_entity: &str) -> Option<EntityPlace> {
    None
}

// the keywords of JSON Schema 2020-12 whose value is a schema, a list of schemas, or schemas by name; a value of any
// other keyword (`examples`, `default`, `const`, `enum`) is data, never read for `references`
const ONE: [&str; 11] = [
    "items",
    "contains",
    "not",
    "if",
    "then",
    "else",
    "additionalProperties",
    "propertyNames",
    "unevaluatedItems",
    "unevaluatedProperties",
    "contentSchema",
];
const LIST: [&str; 4] = ["allOf", "anyOf", "oneOf", "prefixItems"];
const BY_NAME: [&str; 4] = ["properties", "patternProperties", "$defs", "dependentSchemas"];

/// every `references` in a schema, with the path to the schema that gives it
fn references_in<'a>(schema: Option<&'a Value>, path: Vec<String>, out: &mut Vec<(Vec<String>, &'a Value)>) {
    let Some(Value::Object(s)) = schema else { return };
    if let Some(name) = s.get("references") {
        out.push((path.clone(), name));
    }
    let with = |extra: &[&str]| {
        let mut p = path.clone();
        p.extend(extra.iter().map(|x| x.to_string()));
        p
    };
    for k in ONE {
        references_in(s.get(k), with(&[k]), out);
    }
    for k in LIST {
        if let Some(Value::Array(items)) = s.get(k) {
            for (i, x) in items.iter().enumerate() {
                references_in(Some(x), with(&[k, &i.to_string()]), out);
            }
        }
    }
    for k in BY_NAME {
        if let Some(Value::Object(o)) = s.get(k) {
            for (name, x) in o.iter() {
                references_in(Some(x), with(&[k, name]), out);
            }
        }
    }
}

/// a schema's path inside the value it sits in, for a message: properties.address.properties.city -> address.city
fn inside(path: &[String]) -> String {
    path.iter()
        .enumerate()
        .map(|(i, s)| {
            if i > 0 && path[i - 1] == "properties" {
                s.as_str()
            } else if s == "properties" {
                ""
            } else {
                s.as_str()
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// a finding of one file: its rule, the path to the name, the message, the element and the entity it names
type Finding<'a> = dyn FnMut(NameRule, Vec<String>, String, String, Option<&str>) + 'a;

fn strs(path: &[&str]) -> Vec<String> {
    path.iter().map(|s| s.to_string()).collect()
}

/// `doc` is the domain's files read as one (`merge_domain`); `parts` the files, each read again here for where it gives
/// a name
pub fn check_domain_names(
    parts: &[DomainPart],
    doc: &Object,
    id: &str,
    elsewhere: &EntityElsewhere<'_>,
) -> Vec<NameProblem> {
    let empty = Object::new();
    let entities = doc.get("entities").and_then(Value::as_object).unwrap_or(&empty);
    let actions = doc.get("actions").and_then(Value::as_object).unwrap_or(&empty);
    let is_entity = |name: &str| entities.contains_key(name);
    let is_action = |name: &str| actions.contains_key(name);
    let fields_of = |entity: &str| entities.get(entity).and_then(|e| e.get("fields")).and_then(Value::as_object);
    let mut out: Vec<NameProblem> = Vec::new();
    for part in parts {
        let Ok(data) = read_yaml(&part.text) else { continue };
        // a file that is not a map, or is another domain's (domain-place), is no part of this one
        let Value::Object(data) = &data else { continue };
        if data.get("id").and_then(Value::as_str).is_some_and(|own| own != id) {
            continue;
        }
        let mut at = |rule: NameRule, path: Vec<String>, message: String, element: String, entity: Option<&str>| {
            let notes = match entity.and_then(elsewhere) {
                None => Vec::new(),
                Some(other) => vec![NameNote {
                    file: other.file,
                    line: other.line,
                    column: other.column,
                    message: format!(
                        "{} is an entity of the domain {}: a domain names only its own entities, and one domain may be split across the files of its directory (SPEC.md §5)",
                        entity.unwrap_or(""),
                        other.domain
                    ),
                }],
            };
            let place = key_at(&part.text, &path);
            out.push(NameProblem {
                rule,
                file: part.file.clone(),
                line: place.line,
                column: place.column,
                message,
                element,
                notes,
            });
        };
        let references =
            |at: &mut Finding<'_>, schema: Option<&Value>, path: Vec<String>, place: &dyn Fn(&str) -> String| {
                let mut found = Vec::new();
                references_in(schema, path.clone(), &mut found);
                for (r_path, name) in found {
                    let Value::String(name) = name else { continue };
                    if is_entity(name) {
                        continue;
                    }
                    let rest = inside(&r_path[path.len()..]);
                    let mut with = r_path.clone();
                    with.push("references".into());
                    at(
                        NameRule::EntityUnknown,
                        with,
                        format!(
                            "{} references {name}, which is not an entity of the domain {id} (SPEC.md §5)",
                            place(&rest)
                        ),
                        format!("{}/references", r_path.join("/")),
                        Some(name),
                    );
                }
            };
        let at_rest = |rest: &str| if rest.is_empty() { String::new() } else { format!(", at {rest},") };

        for (entity, e) in data.get("entities").and_then(Value::as_object).into_iter().flat_map(|o| o.iter()) {
            let Value::Object(e) = e else { continue };
            // an entity whose fields no file gives yet is not held to them
            let fields = fields_of(entity);
            for key in ["key", "required"] {
                let (Some(fields), Some(Value::Array(names))) = (fields, e.get(key)) else { continue };
                for (i, name) in names.iter().enumerate() {
                    let Value::String(name) = name else { continue };
                    if fields.contains_key(name) {
                        continue;
                    }
                    let said = if key == "key" {
                        format!("the key of the entity {entity} names {name}")
                    } else {
                        format!("the entity {entity} requires {name}")
                    };
                    at(
                        NameRule::FieldUnknown,
                        strs(&["entities", entity, key, &i.to_string()]),
                        format!("{said}, which is not one of its fields (SPEC.md §5)"),
                        format!("entities/{entity}/{key}/{name}"),
                        None,
                    );
                }
            }
            for (field, schema) in e.get("fields").and_then(Value::as_object).into_iter().flat_map(|o| o.iter()) {
                references(&mut at, Some(schema), strs(&["entities", entity, "fields", field]), &|rest| {
                    format!("the field {entity}.{field}{}", at_rest(rest))
                });
            }
        }

        for (action, a) in data.get("actions").and_then(Value::as_object).into_iter().flat_map(|o| o.iter()) {
            let Value::Object(a) = a else { continue };
            if let Some(Value::Object(result)) = a.get("result")
                && let Some(Value::String(returns)) = result.get("returns")
                && !is_entity(returns)
            {
                at(
                    NameRule::EntityUnknown,
                    strs(&["actions", action, "result", "returns"]),
                    format!(
                        "the action {action} returns {returns}, which is not an entity of the domain {id} (SPEC.md §5)"
                    ),
                    format!("actions/{action}/result/returns"),
                    Some(returns),
                );
            }
            for section in ["query", "body", "result"] {
                references(&mut at, a.get(section), strs(&["actions", action, section]), &|rest| {
                    format!("the {section} of the action {action}{}", at_rest(rest))
                });
            }
            for (code, r) in a.get("refusals").and_then(Value::as_object).into_iter().flat_map(|o| o.iter()) {
                let Value::Object(r) = r else { continue };
                references(&mut at, r.get("body"), strs(&["actions", action, "refusals", code, "body"]), &|rest| {
                    format!("the body of the refusal {code} of the action {action}{}", at_rest(rest))
                });
            }
        }

        // the policies of this file, by their place in it; the element names the policy by its actor, as the list's
        // order across files is no part of what it is
        for (i, p) in data.get("policies").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let Value::Object(p) = p else { continue };
            let actor = p.get("actor").and_then(Value::as_str).map_or_else(|| i.to_string(), str::to_string);
            for key in ["do", "read", "write"] {
                let Some(Value::Array(names)) = p.get(key) else { continue };
                for (j, name) in names.iter().enumerate() {
                    let Value::String(name) = name else { continue };
                    let known = if key == "do" { is_action(name) } else { is_entity(name) };
                    if known {
                        continue;
                    }
                    let what = if key == "do" { "an action" } else { "an entity" };
                    at(
                        NameRule::PolicyUnknown,
                        strs(&["policies", &i.to_string(), key, &j.to_string()]),
                        format!(
                            "the policy for {actor} names {name} in {key}, which is not {what} of the domain {id} (SPEC.md §5)"
                        ),
                        format!("policies/{actor}/{key}/{name}"),
                        if key == "do" { None } else { Some(name) },
                    );
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js;

    #[test]
    fn a_schema_path_inside_its_value() {
        let p = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(inside(&p(&["properties", "address", "properties", "city"])), "address.city");
        assert_eq!(inside(&p(&["items", "properties", "properties"])), "items.properties");
        assert_eq!(inside(&p(&[])), "");
    }

    fn people(entity: &str) -> Option<EntityPlace> {
        (entity == "Person").then(|| EntityPlace {
            file: "p/domain.yaml".into(),
            line: 5,
            column: 3,
            domain: "people".into(),
        })
    }

    #[test]
    fn names_a_domain_does_not_have() {
        // node --input-type=module -e "import { mergeDomain } from './dist/domain.js'; import { checkDomainNames } from './dist/domain-names.js';
        //   const p = [{ file: 'n.yaml', text: t }]; console.log(JSON.stringify(checkDomainNames(p, mergeDomain(p).doc, 'x', people)))"
        let t = "kind: domain\nid: x\nentities:\n  Note:\n    key: [id, nid]\n    fields:\n      id: { type: string, references: Person }\n      tags: { type: array, items: { properties: { by: { references: Who } } } }\nactions:\n  make:\n    result: { returns: Gone }\n    refusals:\n      BAD: { body: { properties: { at: { references: Person } } } }\npolicies:\n  - { actor: a, do: [make, f], read: [Note, Ghost] }\n  - { do: [h] }\n";
        let parts = [DomainPart { file: "n.yaml".into(), text: t.into() }];
        let doc = crate::domain::merge_domain(&parts).doc;
        let found = check_domain_names(&parts, &doc, "x", &people);
        let said: Vec<(&str, usize, usize, &str, &str, usize)> = found
            .iter()
            .map(|p| (p.rule.as_str(), p.line, p.column, p.message.as_str(), p.element.as_str(), p.notes.len()))
            .collect();
        assert_eq!(
            said,
            [
                (
                    "field-unknown",
                    5,
                    15,
                    "the key of the entity Note names nid, which is not one of its fields (SPEC.md §5)",
                    "entities/Note/key/nid",
                    0
                ),
                (
                    "entity-unknown",
                    7,
                    27,
                    "the field Note.id references Person, which is not an entity of the domain x (SPEC.md §5)",
                    "entities/Note/fields/id/references",
                    1
                ),
                (
                    "entity-unknown",
                    8,
                    57,
                    "the field Note.tags, at items.by, references Who, which is not an entity of the domain x (SPEC.md §5)",
                    "entities/Note/fields/tags/items/properties/by/references",
                    0
                ),
                (
                    "entity-unknown",
                    11,
                    15,
                    "the action make returns Gone, which is not an entity of the domain x (SPEC.md §5)",
                    "actions/make/result/returns",
                    0
                ),
                (
                    "entity-unknown",
                    13,
                    42,
                    "the body of the refusal BAD of the action make, at at, references Person, which is not an entity of the domain x (SPEC.md §5)",
                    "actions/make/refusals/BAD/body/properties/at/references",
                    1
                ),
                (
                    "policy-unknown",
                    15,
                    28,
                    "the policy for a names f in do, which is not an action of the domain x (SPEC.md §5)",
                    "policies/a/do/f",
                    0
                ),
                (
                    "policy-unknown",
                    15,
                    45,
                    "the policy for a names Ghost in read, which is not an entity of the domain x (SPEC.md §5)",
                    "policies/a/read/Ghost",
                    0
                ),
                (
                    "policy-unknown",
                    16,
                    12,
                    "the policy for 1 names h in do, which is not an action of the domain x (SPEC.md §5)",
                    "policies/1/do/h",
                    0
                ),
            ]
        );
        assert_eq!(
            found[1].notes,
            [NameNote {
                file: "p/domain.yaml".into(),
                line: 5,
                column: 3,
                message: "Person is an entity of the domain people: a domain names only its own entities, and one domain may be split across the files of its directory (SPEC.md §5)".into(),
            }]
        );
    }

    #[test]
    fn a_file_of_another_domain_is_no_part_of_this_one() {
        // the parts of domain.rs's test, held to a doc of their own: only note.yaml names what the doc does not have
        let parts = crate::domain::tests::parts();
        let doc = js::json_parse(r#"{"entities":{"Note":{"fields":{"id":{}}}},"actions":{"f":{}}}"#).ok();
        let Some(Value::Object(doc)) = doc else { panic!("JSON") };
        let found = check_domain_names(&parts, &doc, "x", &nowhere);
        let said: Vec<(&str, &str, usize, usize, &str)> =
            found.iter().map(|p| (p.rule.as_str(), p.file.as_str(), p.line, p.column, p.message.as_str())).collect();
        assert_eq!(
            said,
            [(
                "policy-unknown",
                "d/note.yaml",
                10,
                22,
                "the policy for b names g in do, which is not an action of the domain x (SPEC.md §5)"
            )]
        );
    }
}
