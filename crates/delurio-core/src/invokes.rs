//! What a flow invokes, held to its domain (SPEC.md §2, §5): `invoke: { action }` names an action of the domain the
//! flow names, and for a state that invokes an action, `failures.<state>.from` does too and each class under it lists
//! refusals of that action. A state that invokes an `external` or a `flow` is not held to the domain. The domain is
//! looked up by id among the graphs beside the flow, all its files read as one, as the `changes` rule finds it. A class
//! may also list a transport code, a failure that is no answer of the action's own (SPEC.md §5). Each finding is an
//! error, which fails --strict (decided on 16 Nov 2026, d-1116-10). The port of invokes.ts.

use serde::Serialize;

use crate::changes::Domain;
use crate::graph::{Graph, given};
use crate::js::{self, Object, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvokeRule {
    InvokeUnknown,
    RefusalUnknown,
}

impl InvokeRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            InvokeRule::InvokeUnknown => "invoke-unknown",
            InvokeRule::RefusalUnknown => "refusal-unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InvokeProblem {
    pub rule: InvokeRule,
    pub path: Vec<String>, // where in flow.yaml
    pub element: String,
    pub message: String,
}

/// the key that names a class's action in `failures.<state>`; every other key is a class
const FROM: &str = "from";

/// the codes a failure class may list though its action has no such refusal (SPEC.md §5): no answer came back, none
/// came in time, or the server failed with a status the action does not list
pub const TRANSPORT_CODES: [&str; 3] = ["network", "timeout", "5xx"];

fn strs(path: &[&str]) -> Vec<String> {
    path.iter().map(|s| s.to_string()).collect()
}

/// `Object.entries(classes)` of a failure's classes: a map's keys, or a list's indices
fn entries(classes: &Value) -> Vec<(String, &Value)> {
    match classes {
        Value::Object(o) => o.iter().map(|(k, v)| (k.clone(), v)).collect(),
        Value::Array(items) => items.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        _ => Vec::new(),
    }
}

/// `domain_of` finds the flow's domain, as for `check_changes`
pub fn check_invokes(g: &Graph, domain_of: impl FnOnce() -> Option<Domain>) -> Vec<InvokeProblem> {
    let flow = g.flow();
    let invoked: Vec<(&str, &str)> = flow
        .states()
        .filter_map(|(name, st)| st.invoke().and_then(|i| i.get_str("action")).map(|a| (name, a)))
        .collect();
    let Some(named) = g.flow.get("domain") else { return Vec::new() };
    if invoked.is_empty() {
        return Vec::new();
    }
    let domain_text = js::to_js_string(named);
    let empty = Object::new();
    let failures = given(&g.flow, "failures").and_then(Value::as_object).unwrap_or(&empty);
    let Some(domain) = domain_of() else {
        return vec![InvokeProblem {
            rule: InvokeRule::InvokeUnknown,
            path: strs(&["domain"]),
            element: "domain".into(),
            message: format!(
                "the flow names the domain {domain_text}, which no domain beside it is (under the nearest folder called graphs), so what its steps invoke is not checked (SPEC.md §5)"
            ),
        }];
    };
    let actions = domain.actions().unwrap_or(&empty);
    let has = |name: &str| actions.contains_key(name);
    let mut out = Vec::new();
    for (state, action) in &invoked {
        if has(action) {
            continue;
        }
        out.push(InvokeProblem {
            rule: InvokeRule::InvokeUnknown,
            path: strs(&["states", state, "invoke", "action"]),
            element: format!("states/{state}/invoke"),
            message: format!(
                "state {state} invokes {action}, which is not an action of the domain {domain_text} (SPEC.md §5)"
            ),
        });
    }
    for (state, classes) in failures.iter() {
        let action = flow.state(state).and_then(|st| st.invoke()).and_then(|i| i.get_str("action"));
        let Some(action) = action else { continue };
        if !classes.is_collection() {
            continue;
        }
        let named = classes.get_str(FROM).unwrap_or(action);
        if !has(named) {
            // an action the state's own invoke names is reported there
            if named != action {
                out.push(InvokeProblem {
                    rule: InvokeRule::InvokeUnknown,
                    path: strs(&["failures", state, FROM]),
                    element: format!("failures/{state}/from"),
                    message: format!(
                        "failures.{state} comes from {named}, which is not an action of the domain {domain_text} (SPEC.md §2, §5)"
                    ),
                });
            }
            continue;
        }
        let refusals = domain.action(named).and_then(|a| a.refusals()).unwrap_or(&empty);
        for (cls, codes) in entries(classes) {
            let Value::Array(codes) = codes else { continue };
            if cls == FROM {
                continue;
            }
            for (i, code) in codes.iter().enumerate() {
                let Value::String(code) = code else { continue };
                if refusals.contains_key(code) || TRANSPORT_CODES.contains(&code.as_str()) {
                    continue;
                }
                out.push(InvokeProblem {
                    rule: InvokeRule::RefusalUnknown,
                    path: strs(&["failures", state, &cls, &i.to_string()]),
                    element: format!("failures/{state}/{cls}/{code}"),
                    message: format!(
                        "FAIL.{cls} of {state} stands for {code}, which is not a refusal of {named} in the domain {domain_text}, nor network, timeout or 5xx (SPEC.md §2, §5)"
                    ),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_flow_invokes_held_to_its_domain() {
        // node --input-type=module -e "import { checkInvokes } from './dist/invokes.js'; import { load } from './dist/graph.js';
        //   const q = load('examples/qauth-authorize'); console.log(JSON.stringify([checkInvokes(q), checkInvokes(q, () => custom), checkInvokes(q, () => null)]))"
        let q = crate::graph::load(&crate::changes::tests::example("qauth-authorize")).expect("the example loads");
        assert_eq!(check_invokes(&q, || crate::changes::flow_domain(&q, None)), []);
        let Ok(Value::Object(custom)) = js::json_parse(r#"{"actions":{"decide":{"refusals":{"CSRF_MISMATCH":{}}}}}"#)
        else {
            panic!("JSON")
        };
        let found = check_invokes(&q, || Some(Domain(custom)));
        let said: Vec<(&str, String, &str)> =
            found.iter().map(|p| (p.rule.as_str(), p.path.join("."), p.element.as_str())).collect();
        assert_eq!(
            said,
            [
                ("invoke-unknown", "states.validating.invoke.action".into(), "states/validating/invoke"),
                ("invoke-unknown", "states.issuing.invoke.action".into(), "states/issuing/invoke"),
                ("refusal-unknown", "failures.deciding.unsafe.1".into(), "failures/deciding/unsafe/REQUEST_EXPIRED"),
                ("refusal-unknown", "failures.deciding.server.0".into(), "failures/deciding/server/server_error"),
            ]
        );
        assert_eq!(
            found[2].message,
            "FAIL.unsafe of deciding stands for REQUEST_EXPIRED, which is not a refusal of decide in the domain qauth.authorize, nor network, timeout or 5xx (SPEC.md §2, §5)"
        );
        let none = check_invokes(&q, || None);
        assert_eq!(none.len(), 1);
        assert_eq!(
            none[0].message,
            "the flow names the domain qauth.authorize, which no domain beside it is (under the nearest folder called graphs), so what its steps invoke is not checked (SPEC.md §5)"
        );
    }
}
