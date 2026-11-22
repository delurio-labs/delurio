//! What `check` holds in ui.yaml beyond its schema (SPEC.md §4): `focus` and `returnsFocus` give the focus to a node
//! that is shown where the focus goes, `returnsFocus` starts at a dialog, the words a field's `value` takes from the
//! catalog are there, as every other node's are, and the flow's name for its users (`task`) is in every locale. That
//! `value`, `checked` and `script` sit only on the nodes they are for is the schema's; that a `focus` state exists and
//! that `checked` reads the context is the loader's. The port of ui-check.ts; `check_events`, which reads the
//! exploration, is the Rust checker's own.

use std::collections::VecDeque;
use std::sync::LazyLock;

use indexmap::IndexSet;
use regex::Regex;
use serde::Serialize;

use crate::graph::{Graph, Label, OPENER, alternatives, is_dialog, shown_in};
use crate::js::{self, Value};
use crate::model::{Exploration, Model, PathResult, path_to};
use crate::nodepath;
use crate::wireframe::inherited_sample;

/// a problem in ui.yaml: its rule (`focus-not-shown`, `returns-focus-not-dialog`, `task-locale` or `role-missing`), where in ui.yaml
/// it is, and the words that say it; the fields in the order the TypeScript checker built them
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UiProblem {
    pub rule: &'static str,
    /// where in ui.yaml
    pub path: Vec<String>,
    pub element: String,
    pub message: String,
}

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{((?-u:\w)+)\}").expect("the pattern compiles"));

fn listed(xs: &[String]) -> String {
    match xs.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => xs.join(""),
    }
}

/// the targets of a state's transitions, `always` first
fn ways(g: &Graph, s: &str) -> Vec<String> {
    let Some(st) = g.flow().state(s) else { return Vec::new() };
    let mut out: Vec<String> = st.always().iter().map(|a| a.target_text()).collect();
    for (_, spec) in st.on() {
        out.extend(alternatives(spec).iter().map(|a| a.target_text()));
    }
    out
}

/// the states the flow stops in after a transition out of `state` (on `event` only, when it is given): its targets, and
/// past a state with `always` the targets of that state, as far as they go
fn stops_after(g: &Graph, state: &str, event: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: IndexSet<String> = IndexSet::new();
    let mut todo: VecDeque<String> = match event {
        None => ways(g, state).into(),
        Some(e) => g
            .flow()
            .state(state)
            .and_then(|st| st.on_event(e))
            .map(|first| alternatives(first).iter().map(|a| a.target_text()).collect())
            .unwrap_or_default(),
    };
    while let Some(t) = todo.pop_front() {
        if !seen.insert(t.clone()) {
            continue;
        }
        if g.flow().state(&t).is_some_and(|st| st.value().get("always").is_some()) {
            todo.extend(ways(g, &t));
        } else {
            out.push(t);
        }
    }
    out
}

/// the states a dialog closes into: those the flow stops in after a state that shows it, and that do not show it
fn closes_into(g: &Graph, dialog: &str) -> Vec<String> {
    let Some(d) = g.ui().node(dialog) else { return Vec::new() };
    let mut out: IndexSet<String> = IndexSet::new();
    for s in g.flow().state_names() {
        if !shown_in(d, s) {
            continue;
        }
        for t in stops_after(g, s, None) {
            if !shown_in(d, &t) {
                out.insert(t);
            }
        }
    }
    out.into_iter().collect()
}

/// the nodes that open a dialog: each is shown in a state that does not show the dialog, and sends an event that leads
/// from there to a state that shows it (SPEC.md §4, the node that opened a dialog)
fn openers_of(g: &Graph, dialog: &str) -> Vec<String> {
    let d = g.ui().node(dialog);
    let shows_dialog = |s: &str| d.is_some_and(|d| shown_in(d, s));
    let mut out: Vec<String> = Vec::new();
    for (id, n) in g.ui().nodes() {
        let Some(emits) = n.get("emits") else { continue };
        // the event a node sends, as the key `on[emits ?? '']` looks up
        let event = if emits.is_null() { String::new() } else { js::to_js_string(emits) };
        let opens = g.flow().state_names().into_iter().any(|s| {
            shown_in(n, s) && !shows_dialog(s) && stops_after(g, s, Some(&event)).iter().any(|t| shows_dialog(t))
        });
        if opens {
            out.push(id.to_string());
        }
    }
    out
}

/// a value as the key `nodes[v]` looks up, and as a template literal writes it (`undefined` for none)
fn key_text(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), js::to_js_string)
}

/// `basename(p, '.json')`
fn basename_json(p: &str) -> String {
    if p == ".json" {
        return String::new();
    }
    let base = nodepath::basename(p);
    match base.strip_suffix(".json") {
        Some(stem) if !stem.is_empty() => stem.to_string(),
        _ => base,
    }
}

pub fn check_ui(g: &Graph, warn: &mut dyn FnMut(&str)) -> Vec<UiProblem> {
    let ui = g.ui();
    let mut out: Vec<UiProblem> = Vec::new();
    let mut push = |rule: &'static str, path: Vec<String>, element: String, message: String| {
        out.push(UiProblem { rule, path, element, message });
    };
    for (state, node) in ui.focus().into_iter().flat_map(|o| o.iter()) {
        let path = vec!["focus".to_string(), state.clone()];
        let element = format!("focus/{state}");
        let node = js::to_js_string(node);
        match ui.node(&node) {
            None => push(
                "focus-not-shown",
                path,
                element,
                format!("focus.{state} names {node}, which is no node of ui.yaml"),
            ),
            Some(n) if !shown_in(n, state) => push(
                "focus-not-shown",
                path,
                element,
                format!(
                    "focus.{state} gives the focus to {node}, which {state} does not show: a node has focus only where it is shown (its in and unless, SPEC.md §4)"
                ),
            ),
            Some(_) => {}
        }
    }
    for (i, e) in ui.edges().iter().enumerate() {
        let Some(rf) = e.value().get("returnsFocus") else { continue };
        // `const [dialog, node] = e.returnsFocus`: a list's first two items, a string's first two characters; any other
        // value (which the TypeScript checker could not take apart) is passed over
        let (dialog, node): (Option<Value>, Option<Value>) = match rf {
            Value::Array(items) => (items.first().cloned(), items.get(1).cloned()),
            Value::String(s) => {
                let mut chars = s.chars().map(|c| Value::String(c.to_string()));
                (chars.next(), chars.next())
            }
            _ => continue,
        };
        let dialog = key_text(dialog.as_ref());
        let path = vec!["edges".to_string(), i.to_string()];
        let element = format!("edges/returnsFocus/{dialog}");
        let d = ui.node(&dialog);
        if !is_dialog(d) {
            let what = if d.is_none() { "no node of ui.yaml" } else { "not a dialog" };
            push(
                "returns-focus-not-dialog",
                path,
                element,
                format!(
                    "returnsFocus starts at {dialog}, which is {what}: it starts at a page node whose role is dialog or alertdialog (SPEC.md §4)"
                ),
            );
            continue;
        }
        // one node, the nodes that open the dialog, or OPENER: whichever node opens it
        let opener = node.as_ref().is_some_and(|v| v.as_str() == Some(OPENER));
        let several = node.as_ref().is_some_and(Value::is_array) || opener;
        let targets: Vec<String> = match &node {
            _ if opener => openers_of(g, &dialog),
            Some(Value::Array(items)) => items.iter().map(js::to_js_string).collect(),
            other => vec![key_text(other.as_ref())],
        };
        if opener && targets.is_empty() {
            push(
                "focus-not-shown",
                path,
                element,
                format!(
                    "returnsFocus gives the focus of {dialog} back to the node that opened it, but no node opens it: none sends an event that leads from a state without the dialog to one that shows it (SPEC.md §4)"
                ),
            );
            continue;
        }
        let into = closes_into(g, &dialog);
        for t in &targets {
            let at = if several { format!("{element}/{t}") } else { element.clone() };
            let Some(target) = ui.node(t) else {
                push(
                    "focus-not-shown",
                    path.clone(),
                    at,
                    format!("returnsFocus gives the focus of {dialog} to {t}, which is no node of ui.yaml"),
                );
                continue;
            };
            if into.iter().any(|s| shown_in(target, s)) {
                continue;
            }
            let place = if into.is_empty() {
                format!("the dialog {dialog} never closes into a state the flow stops in")
            } else {
                format!("no state {dialog} closes into shows it ({})", listed(&into))
            };
            let who = if opener { format!("back to {t}, which opens it,") } else { format!("to {t},") };
            push(
                "focus-not-shown",
                path.clone(),
                at,
                format!("returnsFocus gives the focus of {dialog} {who} but {place} (SPEC.md §4)"),
            );
        }
    }
    // a field's value in words of the catalog, held as every other node's words are
    let mut words = |v: Label<'_>| {
        // `[...(v.key === undefined ? [] : [v.key]), ...(v.oneOf ?? [])]`, each key as `g.catalog[key]` looks it up: a
        // list's items or a string's characters; any other value (which the TypeScript checker could not spread) is
        // passed over
        let mut keys: Vec<String> = v.value().get("key").map(js::to_js_string).into_iter().collect();
        match v.value().get("oneOf") {
            Some(Value::Array(items)) => keys.extend(items.iter().map(js::to_js_string)),
            Some(Value::String(s)) => keys.extend(s.chars().map(String::from)),
            _ => {}
        }
        for key in keys {
            // a message that is no string is read as missing
            let Some(message) = g.message(&key) else {
                warn(&format!("catalog key {key} is missing"));
                continue;
            };
            for m in PLACEHOLDER.captures_iter(message) {
                let p = &m[1];
                if ui.sample(p).is_none() && inherited_sample(p).is_none() {
                    warn(&format!("no sample value for {{{p}}} in {key}"));
                }
            }
        }
    };
    for (_, n) in ui.nodes() {
        if let Some(v) = n.value_label() {
            words(v);
        }
    }
    // a control with no role: a screen reader, a replay and kit check find a field or an action by its WAI-ARIA role,
    // unless it says it is a native control WAI-ARIA gives no role (`native`), found by its name among its kind
    for (id, n) in ui.nodes() {
        let what = match n.kind() {
            Some("field") => "a field",
            Some("action") => "an action",
            _ => continue,
        };
        if n.get("role").is_some() || n.get("native").is_some() {
            continue;
        }
        push(
            "role-missing",
            vec!["nodes".to_string(), id.to_string()],
            format!("nodes/{id}"),
            format!(
                "{id} is {what} with no role: a screen reader, a replay and kit check find a control by its WAI-ARIA role (textbox, combobox, button, link, ...), and kit check holds only its name and state; a native control WAI-ARIA gives no role (a date, time or file input, a summary) says so with `native` (SPEC.md §4)"
            ),
        );
    }
    // the flow's name for its users, in the catalog of every locale, as the changes note is
    let task = ui.task().and_then(|t| t.get_str("key"));
    if let Some(task) = task
        && !ui.absent()
    {
        let catalog = ui.catalog().unwrap_or("");
        let locales: Vec<String> = match ui.value().get("locales") {
            None | Some(Value::Null) => vec![basename_json(catalog)],
            Some(Value::Array(items)) => items.iter().map(js::to_js_string).collect(),
            Some(Value::String(s)) => s.chars().map(String::from).collect(),
            Some(_) => Vec::new(), // not a list: the TypeScript checker could not go through it
        };
        for locale in locales {
            let file = nodepath::join(&[&g.dir, &nodepath::dirname(catalog), &format!("{locale}.json")]);
            let read = std::fs::read(&file).ok().and_then(|b| js::json_parse(&String::from_utf8_lossy(&b)).ok());
            let name = read.as_ref().and_then(|c| c.get_str(task));
            if name.is_some_and(|n| !js::trim(n).is_empty()) {
                continue;
            }
            push(
                "task-locale",
                vec!["task".to_string()],
                format!("task/{locale}"),
                format!("the flow's name for its users, {task}, is not in the {locale} catalog (SPEC.md §4)"),
            );
        }
    }
    out
}

/// the rule of a node whose event a state that shows it enabled does not handle (SPEC.md §4)
pub const EVENT_UNHANDLED: &str = "event-unhandled";

/// the person leaving the page (SPEC.md §2, Events), which is possible in every state
const NAVIGATE: &str = "NAVIGATE";

/// A node that sends an event (`emits`) in a state that shows it and does not disable it, and that has no transition on
/// that event: where in ui.yaml, what it is about, its words, and, when the node's `when` or `disabledWhen` decided it,
/// the way to the first situation that shows it enabled.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unhandled {
    pub node: String,
    pub state: String,
    pub event: String,
    /// where in ui.yaml: the node
    pub path: Vec<String>,
    pub element: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reached: Option<PathResult>,
}

/// a (node, state) pair that may be one, until a situation of the state shows the node enabled
#[derive(Debug, Clone, Copy)]
struct Pending<'a> {
    node: &'a str,
    state: &'a str,
    event: &'a str,
    when: Option<&'a str>,
    disabled_when: Option<&'a str>,
}

/// An expression a node gives under `key`: Some(None) when it gives none, None when it is no string (the schema's to
/// report, and not read here)
fn expression<'a>(n: crate::graph::UiNode<'a>, key: &str) -> Option<Option<&'a str>> {
    match n.get(key) {
        None => Some(None),
        Some(Value::String(s)) => Some(Some(s.as_str())),
        Some(_) => None,
    }
}

/// whether an expression made ready holds in a situation; None when it cannot be made ready or throws there
fn holds<F: Fn(&str, &[u16]) -> crate::error::Result<bool>>(
    t: &crate::error::Result<F>,
    state: &str,
    ctx: &[u16],
) -> Option<bool> {
    t.as_ref().ok().and_then(|test| test(state, ctx).ok())
}

/// Every node whose event (`emits`) a state that shows it enabled does not handle, once per node and state, in the
/// order of the nodes and then of the states (SPEC.md §4). A node is shown enabled in a situation (a state and a
/// context) the exploration reached when the state is in its `in` and not in its `unless`, its `when` holds in that
/// context, the state is not in its `disabledIn`, and its `disabledWhen` does not hold there. The state handles the
/// event when it has a transition on it in its `on`. Flows have no nested states and no transitions outside their
/// states (SPEC.md §2, Several flows), so a state's `on` is all it handles. Three are not held (SPEC.md §2):
/// - `NAVIGATE`, the person leaving the page, which is always possible: a node that sends it is never one;
/// - an exit (`type: final`), which has no transitions: what a node on its page does is outside the flow (a new run, or
///   another flow);
/// - a state with `always`, which is passed at once (and §4 Focus: "a state with `always` is passed at once"), so
///   nothing is pressed there.
///
/// The exploration is `check`'s own: a flow whose exploration fails does not load, and none of its rules is read; one
/// that has no situation (no start) reaches no state, and, as for its assertions, nothing is found. A `when` or a
/// `disabledWhen` that does not parse keeps the graph from loading; one that throws in a situation decides nothing, and
/// the pair is passed over.
pub fn check_events(g: &Graph, m: &Model, ex: &Exploration) -> Vec<Unhandled> {
    let mut pending: Vec<Pending<'_>> = Vec::new();
    for (node, n) in g.ui().nodes() {
        let Some(event) = n.emits().filter(|&e| e != NAVIGATE) else { continue };
        let (Some(when), Some(disabled_when)) = (expression(n, "when"), expression(n, "disabledWhen")) else {
            continue;
        };
        let disabled_in = n.disabled_in();
        for (state, st) in g.flow().states() {
            let passed = st.is_final() || st.0.get_array("always").is_some();
            if passed || !shown_in(n, state) || disabled_in.contains(&state) || st.on_event(event).is_some() {
                continue;
            }
            pending.push(Pending { node, state, event, when, disabled_when });
        }
    }
    if pending.is_empty() {
        return Vec::new();
    }
    // each one's expressions made ready once; one that does not parse is passed over
    let testers: Vec<_> =
        pending.iter().map(|p| (p.when.map(|s| m.tester(s)), p.disabled_when.map(|s| m.tester(s)))).collect();
    let mut open: Vec<bool> = testers
        .iter()
        .map(|(w, d)| w.as_ref().is_none_or(Result::is_ok) && d.as_ref().is_none_or(Result::is_ok))
        .collect();
    let mut left = open.iter().filter(|&&o| o).count();
    let mut hit: Vec<Option<usize>> = vec![None; pending.len()];
    // the pairs pending in each state, by the state's place in the exploration, found the first time it is reached
    let mut at_state: Vec<Option<Vec<usize>>> = Vec::new();
    let mut ctx = Vec::new();
    let mut i = 0;
    while i < ex.len() && left > 0 {
        let at = ex.state_at(i) as usize;
        if at_state.len() <= at {
            at_state.resize(at + 1, None);
        }
        let here = at_state[at].get_or_insert_with(|| {
            let state = ex.state(i);
            (0..pending.len()).filter(|&k| pending[k].state == state).collect()
        });
        if !here.is_empty() {
            ex.ctx_into(i, &mut ctx);
        }
        let state = ex.state(i);
        for &k in here.iter() {
            if !open[k] {
                continue;
            }
            let (when, disabled_when) = &testers[k];
            let shown = when.as_ref().map_or(Some(true), |t| holds(t, state, &ctx));
            let disabled = disabled_when.as_ref().map_or(Some(false), |t| holds(t, state, &ctx));
            match (shown, disabled) {
                (Some(true), Some(false)) => {
                    hit[k] = Some(i);
                    open[k] = false;
                    left -= 1;
                }
                (Some(_), Some(_)) => {}
                // an expression that throws here decides nothing for the pair
                _ => {
                    open[k] = false;
                    left -= 1;
                }
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    for (p, h) in pending.iter().zip(hit) {
        let Some(h) = h else { continue };
        let Pending { node, state, event, when, disabled_when } = *p;
        let cond = match (when, disabled_when) {
            (None, None) => String::new(),
            (Some(w), None) => format!(" (where its when, {w}, holds)"),
            (None, Some(d)) => format!(" (where its disabledWhen, {d}, does not hold)"),
            (Some(w), Some(d)) => format!(" (where its when, {w}, holds and its disabledWhen, {d}, does not)"),
        };
        let message = format!(
            "node {node} sends {event}, and {state} shows it enabled{cond} but has no on.{event}: a node shown and not disabled can be pressed, so the state handles its event, or the node is disabled there (disabledIn, disabledWhen) or not shown (SPEC.md §4)"
        );
        let reached = (when.is_some() || disabled_when.is_some()).then(|| path_to(m, ex, h));
        out.push(Unhandled {
            node: node.to_string(),
            state: state.to_string(),
            event: event.to_string(),
            path: vec!["nodes".to_string(), node.to_string()],
            element: format!("nodes/{node}/emits/{event}/{state}"),
            message,
            reached,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Error, Result};
    use crate::graph::load_with;
    use crate::model::explore;

    // A flow whose editing page sends SAVE, TOGGLE and BACK_TO_LIST: SAVE leads to saving, where the step runs and busy
    // is true, and past checking (an `always`) to the exit saved, or to failed, where the person may retry; BACK_TO_LIST
    // leaves for the exit left. ready starts from either value; lost is reached by nothing.
    const FLOW: &str = "schema: delurio-graph/0
kind: flow
id: t
initial: editing
context:
  ready: { values: [false, true], initial: any }
  busy: { values: [false, true], initial: false }
effects:
  start: { set: { busy: true } }
states:
  editing:
    on:
      SAVE: { target: saving, effect: [start] }
      TOGGLE: editing
      BACK_TO_LIST: left
  saving:
    invoke: { action: save }
    on:
      DONE: checking
      FAIL: failed
  failed:
    by: human
    on:
      RETRY: saving
  checking:
    always: [saved]
  saved: { type: final }
  left: { type: final }
  lost:
    on:
      SAVE: saving
";

    /// the pairs found for one node, given as the keys of its map in ui.yaml, each as (node, state, event)
    fn found_for(node: &str) -> Vec<Unhandled> {
        let ui = format!(
            "schema: delurio-graph/0\nkind: ui\nid: t\nflow: t\ncatalog: catalog/en.json\nnodes:\n  button: {{ type: action, role: button, {node} }}\n"
        );
        let read = |file: &str| -> Result<String> {
            match file {
                "flow.yaml" => Ok(FLOW.to_string()),
                "ui.yaml" => Ok(ui.clone()),
                "catalog/en.json" => Ok("{}".to_string()),
                _ => Err(Error::graph(format!("no {file}"), None)),
            }
        };
        let g = load_with("t", &read, &|file| matches!(file, "flow.yaml" | "ui.yaml" | "catalog/en.json"))
            .expect("the graph loads");
        let m = Model::new(g.flow.clone());
        let ex = explore(&m, None, None).expect("the flow explores");
        check_events(&g, &m, &ex)
    }

    fn states_for(node: &str) -> Vec<String> {
        found_for(node).into_iter().map(|u| u.state).collect()
    }

    #[test]
    fn a_node_shown_and_enabled_where_its_event_is_not_handled() {
        let found = found_for("emits: SAVE, in: [editing, saving]");
        assert_eq!(found.len(), 1);
        let u = &found[0];
        assert_eq!((u.node.as_str(), u.state.as_str(), u.event.as_str()), ("button", "saving", "SAVE"));
        assert_eq!(u.path, ["nodes", "button"]);
        assert_eq!(u.element, "nodes/button/emits/SAVE/saving");
        assert_eq!(
            u.message,
            "node button sends SAVE, and saving shows it enabled but has no on.SAVE: a node shown and not disabled can be pressed, so the state handles its event, or the node is disabled there (disabledIn, disabledWhen) or not shown (SPEC.md §4)"
        );
        assert_eq!(u.reached, None);
        // shown only where SAVE is handled
        assert!(states_for("emits: SAVE, in: [editing]").is_empty());
        // `unless` takes saving out of `in`
        assert!(states_for("emits: SAVE, in: [editing, saving], unless: [saving]").is_empty());
    }

    #[test]
    fn a_node_disabled_in_the_state_is_not_held_to_it() {
        assert!(states_for("emits: SAVE, in: [editing, saving], disabledIn: [saving]").is_empty());
        assert_eq!(states_for("emits: SAVE, in: [editing, saving], disabledIn: [editing]"), ["saving"]);
    }

    #[test]
    fn a_node_disabled_by_the_context_is_held_where_it_is_not() {
        // busy is true in every situation of saving
        assert!(states_for("emits: SAVE, in: [editing, saving], disabledWhen: busy").is_empty());
        // ready is true in some: the first one is given, with its way there
        let found = found_for("emits: SAVE, in: [editing, saving], disabledWhen: \"not ready\"");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].state, "saving");
        assert!(
            found[0].message.contains("saving shows it enabled (where its disabledWhen, not ready, does not hold)")
        );
        let reached = found[0].reached.as_ref().expect("the way there is given");
        assert_eq!(reached.events, ["SAVE"]);
        assert_eq!(js::stringify(&Value::Object(reached.start.clone())), r#"{"ready":true}"#);
    }

    #[test]
    fn a_node_shown_only_when_its_when_holds() {
        // not shown in saving, where busy is true
        assert!(states_for("emits: SAVE, in: [editing, saving], when: \"not busy\"").is_empty());
        let found = found_for("emits: SAVE, in: [editing, saving], when: ready");
        assert_eq!(found.iter().map(|u| u.state.as_str()).collect::<Vec<_>>(), ["saving"]);
        assert!(found[0].message.contains("saving shows it enabled (where its when, ready, holds)"));
        assert_eq!(found[0].reached.as_ref().map(|r| r.events.clone()), Some(vec!["SAVE".to_string()]));
        // both: shown when ready, disabled when busy, which saving always is
        assert!(states_for("emits: SAVE, in: [editing, saving], when: ready, disabledWhen: busy").is_empty());
    }

    #[test]
    fn a_state_that_handles_the_event() {
        // editing handles TOGGLE, saving does not
        assert!(states_for("emits: TOGGLE, in: [editing]").is_empty());
        assert_eq!(states_for("emits: TOGGLE, in: [editing, saving]"), ["saving"]);
        // a state with `always` is passed at once, and one no situation reaches is never shown
        assert!(states_for("emits: TOGGLE, in: [checking, lost]").is_empty());
        // one finding per node and state, in the order of the states
        assert_eq!(states_for("emits: TOGGLE, in: [failed, saving, editing]"), ["saving", "failed"]);
    }

    #[test]
    fn a_link_back_shown_in_states_that_do_not_handle_it() {
        // the link back to the list on every screen, which only editing handles: the step's failure, where the person
        // is (by: human), does not
        assert_eq!(states_for("emits: BACK_TO_LIST, in: [editing, saving, failed, saved]"), ["saving", "failed"]);
        assert!(states_for("emits: BACK_TO_LIST, in: [editing]").is_empty());
    }

    #[test]
    fn leaving_the_page_is_always_possible() {
        // NAVIGATE is the person leaving the page (SPEC.md §2): no state is held to it
        assert!(states_for("emits: NAVIGATE, in: [editing, saving, failed]").is_empty());
    }

    #[test]
    fn what_a_node_on_an_exit_does_is_outside_the_flow() {
        // an exit has no transitions (SPEC.md §2): a node its page shows starts a new run or another flow
        assert!(states_for("emits: SAVE, in: [saved, left]").is_empty());
        assert_eq!(states_for("emits: SAVE, in: [failed, saved, left]"), ["failed"]);
    }
}
