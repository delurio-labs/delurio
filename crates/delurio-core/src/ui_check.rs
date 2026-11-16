//! What `check` holds in ui.yaml beyond its schema (SPEC.md §4): `focus` and `returnsFocus` give the focus to a node
//! that is shown where the focus goes, `returnsFocus` starts at a dialog, the words a field's `value` takes from the
//! catalog are there, as every other node's are, and the flow's name for its users (`task`) is in every locale. That
//! `value`, `checked` and `script` sit only on the nodes they are for is the schema's; that a `focus` state exists and
//! that `checked` reads the context is the loader's. The port of ui-check.ts.

use std::collections::VecDeque;
use std::sync::LazyLock;

use indexmap::IndexSet;
use regex::Regex;
use serde::Serialize;

use crate::graph::{Graph, Label, OPENER, alternatives, is_dialog, shown_in};
use crate::js::{self, Value};
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
    // a control with no role: a screen reader, a replay and kit check find a field or an action by its WAI-ARIA role
    for (id, n) in ui.nodes() {
        let what = match n.kind() {
            Some("field") => "a field",
            Some("action") => "an action",
            _ => continue,
        };
        if n.get("role").is_some() {
            continue;
        }
        push(
            "role-missing",
            vec!["nodes".to_string(), id.to_string()],
            format!("nodes/{id}"),
            format!(
                "{id} is {what} with no role: a screen reader, a replay and kit check find a control by its WAI-ARIA role (textbox, combobox, button, link, ...), and kit check holds only its name and state (SPEC.md §4)"
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
