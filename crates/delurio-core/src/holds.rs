//! What a node of a flow's screens holds, held to its domain (SPEC.md §4, §5): `holds: <Entity>.<field>` names a field
//! of an entity of the domain the flow names, the value the node shows or the control takes. The domain is looked up as
//! the `changes` rule and the invoke rule find it, among the graphs beside the flow, all its files read as one. Each
//! finding is an error, `holds-unknown`, which fails --strict, as `invoke-unknown` does. A node that holds a field whose
//! `sensitivity` is `credential` is masked wherever the flow's screens are recorded (SPEC.md §7): `masked_nodes` names
//! them for a recorder.

use crate::changes::Domain;
use crate::graph::Graph;
use crate::js::{self, Value};

pub const HOLDS_UNKNOWN: &str = "holds-unknown";

/// the sensitivity of a field that a recording never shows (SPEC.md §5, §7)
pub const MASKED_SENSITIVITY: &str = "credential";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoldsProblem {
    pub path: Vec<String>, // where in ui.yaml
    pub element: String,
    pub message: String,
}

/// each node that says what it holds, with its entity and field, in the order of `nodes`; a value that is not
/// `<Entity>.<field>` is left to the `schema` rule
fn held(g: &Graph) -> Vec<(&str, &str, &str)> {
    g.ui()
        .nodes()
        .filter_map(|(name, n)| {
            let holds = n.value().get("holds").and_then(Value::as_str)?;
            let (entity, field) = holds.split_once('.')?;
            (!entity.is_empty() && !field.is_empty()).then_some((name, entity, field))
        })
        .collect()
}

/// the field `entity.field` of the domain; Some(None) when the entity gives no fields yet, which holds nothing to them
/// (SPEC.md §5, The names it gives)
fn field_of<'d>(domain: &'d Domain, entity: &str, field: &str) -> Result<Option<&'d Value>, &'static str> {
    let entity = domain.doc().get("entities").and_then(|e| e.get(entity)).ok_or("entity")?;
    let Some(fields) = entity.get("fields").and_then(Value::as_object) else { return Ok(None) };
    fields.get(field).map(Some).ok_or("field")
}

/// `domain_of` finds the flow's domain, called at most once, as for `check_changes`
pub fn check_holds(g: &Graph, domain_of: impl FnOnce() -> Option<Domain>) -> Vec<HoldsProblem> {
    let held = held(g);
    let Some(&(first, ..)) = held.first() else { return Vec::new() };
    let at =
        |node: &str| (vec!["nodes".to_string(), node.to_string(), "holds".to_string()], format!("nodes/{node}/holds"));
    let Some(named) = g.flow.get("domain") else {
        let (path, element) = at(first);
        return vec![HoldsProblem {
            path,
            element,
            message: format!(
                "node {first} holds a field, and the flow names no domain, so what its nodes hold is not checked (SPEC.md §4)"
            ),
        }];
    };
    let domain_text = js::to_js_string(named);
    let Some(domain) = domain_of() else {
        let (path, element) = at(first);
        return vec![HoldsProblem {
            path,
            element,
            message: format!(
                "the flow names the domain {domain_text}, which no domain beside it is (under the nearest folder called graphs), so what its nodes hold is not checked (SPEC.md §4)"
            ),
        }];
    };
    let mut out = Vec::new();
    for (node, entity, field) in held {
        let message = match field_of(&domain, entity, field) {
            Ok(_) => continue,
            Err("entity") => format!(
                "node {node} holds {entity}.{field}, and {entity} is not an entity of the domain {domain_text} (SPEC.md §4)"
            ),
            Err(_) => format!(
                "node {node} holds {entity}.{field}, and {field} is not a field of {entity} in the domain {domain_text} (SPEC.md §4)"
            ),
        };
        let (path, element) = at(node);
        out.push(HoldsProblem { path, element, message });
    }
    out
}

/// the nodes a recording masks (SPEC.md §7): each node that holds a field whose `sensitivity` is `credential`, in the
/// order of `nodes`. None of them when the domain is not found, as `check` reports it.
pub fn masked_nodes(g: &Graph, domain: Option<&Domain>) -> Vec<String> {
    let Some(domain) = domain else { return Vec::new() };
    held(g)
        .into_iter()
        .filter(|(_, entity, field)| {
            matches!(field_of(domain, entity, field), Ok(Some(f))
                if f.get("sensitivity").and_then(Value::as_str) == Some(MASKED_SENSITIVITY))
        })
        .map(|(node, ..)| node.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(text: &str) -> Domain {
        let Ok(Value::Object(o)) = js::json_parse(text) else { panic!("JSON") };
        Domain(o)
    }

    fn with_holds(pairs: &[(&str, &str)]) -> Graph {
        let mut g = crate::graph::load(&crate::changes::tests::example("qauth-authorize")).expect("the example loads");
        let Some(Value::Object(nodes)) = g.ui.get_mut("nodes") else { panic!("nodes") };
        for (node, holds) in pairs {
            let Some(Value::Object(n)) = nodes.get_mut(node) else { panic!("node {node}") };
            n.insert("holds", Value::from(*holds));
        }
        g
    }

    #[test]
    fn what_a_node_holds_is_held_to_its_domain() {
        let q = crate::graph::load(&crate::changes::tests::example("qauth-authorize")).expect("the example loads");
        assert_eq!(check_holds(&q, || crate::changes::flow_domain(&q, None)), []);

        let g = with_holds(&[("account", "Session.email"), ("redirectNote", "Client.host")]);
        let found = check_holds(&g, || crate::changes::flow_domain(&g, None));
        let said: Vec<(String, &str)> = found.iter().map(|p| (p.path.join("."), p.element.as_str())).collect();
        assert_eq!(
            said,
            [
                ("nodes.redirectNote.holds".to_string(), "nodes/redirectNote/holds"),
                ("nodes.account.holds".to_string(), "nodes/account/holds"),
            ]
        );
        assert_eq!(
            found[0].message,
            "node redirectNote holds Client.host, and host is not a field of Client in the domain qauth.authorize (SPEC.md §4)"
        );
        assert_eq!(
            found[1].message,
            "node account holds Session.email, and Session is not an entity of the domain qauth.authorize (SPEC.md §4)"
        );

        let none = check_holds(&g, || None);
        assert_eq!(none.len(), 1);
        assert_eq!(
            none[0].message,
            "the flow names the domain qauth.authorize, which no domain beside it is (under the nearest folder called graphs), so what its nodes hold is not checked (SPEC.md §4)"
        );
        // an entity that gives no fields yet holds nothing to them
        let bare = domain(r#"{"entities":{"Session":{},"Client":{"fields":{"host":{"type":"string"}}}}}"#);
        assert_eq!(check_holds(&g, || Some(bare)), []);
    }

    #[test]
    fn a_recording_masks_the_nodes_that_hold_a_credential() {
        let g = with_holds(&[("title", "Client.name"), ("account", "AuthorizationCode.value")]);
        let d = crate::changes::flow_domain(&g, None);
        assert_eq!(masked_nodes(&g, d.as_ref()), ["account"]);
        assert_eq!(masked_nodes(&g, None), Vec::<String>::new());
        let personal = domain(r#"{"entities":{"AuthorizationCode":{"fields":{"value":{"sensitivity":"personal"}}}}}"#);
        assert_eq!(masked_nodes(&g, Some(&personal)), Vec::<String>::new());
    }
}
