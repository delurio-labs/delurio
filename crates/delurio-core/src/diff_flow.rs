//! The revision diff of a flow graph: what changed in flow.yaml, ui.yaml, the catalog and domain.yaml, keyed by id,
//! and what the flow does differently, found by exploring both revisions. The port of diff-flow.ts.
//!
//! The flow's values are read as they were written (`Value`); a transition is held as `alternatives` gives it, a string
//! spec as `{ target }`, so that `raw(t)` reads its keys as the TypeScript code did.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::compare::{
    Change, Key, Op, Shift, deep_diff, field_changes, join_words, match_ordered, member, moved, op_of, other_keys,
    quote, raw, same, strict_eq, text, union, value_changes, word_changes,
};
use crate::edges::{NodeKind, exit_line, kind_of, plain, press_label, response_line};
use crate::error::{Error, Result};
use crate::graph::{Graph, State, given, humanize, strings, title_of};
use crate::js::{self, Object, Value};
use crate::model::{Exploration, Model, explore, final_paths};
use crate::story::{PathStep, main_story_index, narrate, steps_by_events, steps_to};

/// the API description beside a flow: its files, as one text (`a.yaml, b.yaml`), and the merged document
#[derive(Debug, Clone, PartialEq)]
pub struct FlowDomain {
    pub file: String,
    pub doc: Value,
}

pub struct FlowSide {
    pub g: Graph,
    pub domain: Option<FlowDomain>,
}

fn by_words(by: &str) -> Option<&'static str> {
    match by {
        "human" => Some("a person"),
        "ai" => Some("AI"),
        "agent" => Some("an agent"),
        "ci" => Some("CI"),
        _ => None,
    }
}

/// `BY_WORDS[String(v)] ?? String(v)`
fn by_text(v: Option<&Value>) -> String {
    let t = text(v);
    by_words(&t).map_or(t, str::to_string)
}

/// `String(v ?? fallback)`
fn given_or(v: Option<&Value>, fallback: &str) -> String {
    match v {
        None | Some(Value::Null) => fallback.to_string(),
        Some(v) => js::to_js_string(v),
    }
}

/// `x ?? y` of two values that may be undefined: the first that is neither null nor undefined
fn either<'a>(x: Option<&'a Value>, y: Option<&'a Value>) -> Option<&'a Value> {
    x.filter(|v| !v.is_null()).or(y)
}

fn state_name(g: &Graph, id: &str) -> String {
    if g.flow().state(id).is_none() { id.to_string() } else { quote(&title_of(g.flow(), id)) }
}

fn step_kind(g: &Graph, id: &str) -> String {
    let Some(st) = g.flow().state(id) else { return "a server step".to_string() };
    if st.is_final() {
        return format!("an ending: {}", exit_line(st));
    }
    if st.0.get("goal") == Some(&Value::Bool(true)) {
        return "a goal".to_string();
    }
    if let Some(by) = st.0.get("by") {
        return format!("done by {}", by_text(Some(by)));
    }
    match kind_of(g, id) {
        NodeKind::Screen => "a screen".to_string(),
        NodeKind::Subflow => "another flow".to_string(),
        _ => "a server step".to_string(),
    }
}

fn invoke_words(v: Option<&Value>) -> String {
    let inv = raw(v);
    if let Some(Value::String(a)) = inv.get("action") {
        return format!("the action {a}");
    }
    if let Some(Value::String(f)) = inv.get("flow") {
        return format!("the flow {f}");
    }
    if let Some(Value::String(e)) = inv.get("external") {
        return e.clone();
    }
    "nothing".to_string()
}

/// `t.say ?? t.guard ?? 'otherwise'`
fn rule_words(t: &Value) -> String {
    either(given(t, "say"), given(t, "guard")).map_or_else(|| "otherwise".to_string(), js::to_js_string)
}

// how people know a transition: the button that sends it, its `say`, or the event in words
fn way_words(g: &Graph, state: &str, event: &str, t: &Value, index: usize) -> String {
    if event == "always" {
        return format!("rule {} ({})", index + 1, quote(&rule_words(t)));
    }
    if let Some(press) = press_label(g, state, event) {
        return format!("pressing {}", quote(&press));
    }
    if let Some(say) = t.get("say") {
        return quote(&js::to_js_string(say));
    }
    match event.strip_prefix("FAIL.") {
        Some(rest) => format!("the failure {rest}"),
        None => humanize(event).to_lowercase(),
    }
}

fn way_key(state: &str, event: &str, index: usize, count: usize) -> String {
    if event == "always" {
        format!("state {state}, rule {}", index + 1)
    } else if count > 1 {
        format!("state {state}, on {event} #{}", index + 1)
    } else {
        format!("state {state}, on {event}")
    }
}

fn label_words(g: &Graph, label: Option<&Value>) -> Option<String> {
    let label = label?;
    if let Some(key) = label.as_object().and_then(|o| o.get("key")) {
        return Some(plain(g, &js::to_js_string(key)));
    }
    if let Some(literal) = label.as_object().and_then(|o| o.get("literal")) {
        return Some(js::to_js_string(literal));
    }
    if let Some(one_of) = label.as_object().and_then(|o| o.get("oneOf")) {
        let keys = one_of.as_array().map(Vec::as_slice).unwrap_or(&[]);
        return Some(keys.iter().map(|k| plain(g, &js::to_js_string(k))).collect::<Vec<_>>().join(" / "));
    }
    None
}

fn node_words(g: &Graph, id: &str, n: &Value) -> String {
    let what = humanize(&text(either(n.get("component"), n.get("type")))).to_lowercase();
    match label_words(g, n.get("text")).or_else(|| label_words(g, n.get("name"))) {
        None => format!("{what} {id}"),
        Some(t) => format!("{what} {}", quote(&t)),
    }
}

fn shown_on(n: Option<&Value>) -> Vec<String> {
    let unless = strings(member(n, "unless"));
    strings(member(n, "in")).into_iter().filter(|s| !unless.contains(s)).map(str::to_string).collect()
}

fn value_words(name: &str, cv: Option<&Value>, v: &str) -> String {
    match member(member(cv, "say"), v) {
        Some(said) if said.as_str() != Some("") => quote(&js::to_js_string(said)),
        _ => format!("{name} = {v}"),
    }
}

/// `alternatives(spec ?? [])`: an event's transitions, a string spec as `{ target }`
fn alternatives_of(spec: Option<&Value>) -> Vec<Value> {
    let one = |v: &Value| match v {
        Value::String(s) => Value::Object(Object::from_iter([("target", Value::from(s.as_str()))])),
        other => other.clone(),
    };
    match spec {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(one).collect(),
        Some(v) => vec![one(v)],
    }
}

/// `st.always ?? []`, its rules as they are written
fn always_of(st: Option<&Value>) -> Vec<Value> {
    member(st, "always").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn capital(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.len_utf16() == 1 => c.to_uppercase().chain(chars).collect(),
        _ => s.to_string(),
    }
}

pub struct FlowStructure {
    pub changes: Vec<Change>,
    pub ways: Vec<(String, String)>, // transitions matched on both sides, as the `taken` keys of the explorations: [before, after]
    pub added: Vec<String>,          // `taken` keys of transitions that only the new revision has
}

pub fn flow_structure(b: &FlowSide, a: &FlowSide) -> Result<FlowStructure> {
    let mut out: Vec<Change> = Vec::new();
    let fb = &b.g.flow;
    let fa = &a.g.flow;
    let rb = raw(Some(fb));
    let ra = raw(Some(fa));
    let flow_title =
        |f: &Value| either(f.get("title"), None).map_or_else(|| humanize(&text(f.get("id"))), js::to_js_string);
    let initial = |f: &Value| text(f.get("initial"));

    // the flow itself
    out.extend(field_changes(
        "flow",
        "flow",
        rb,
        ra,
        &["title", "summary", "status", "initial", "id", "ui", "domain", "schema", "kind"],
        &|f, x, y| match f {
            "title" => {
                Some(format!("The flow is now called {} (was {}).", quote(&flow_title(fa)), quote(&flow_title(fb))))
            }
            "summary" => Some(match y {
                None => "The summary was removed.".to_string(),
                Some(y) => format!("New summary: {}", quote(js::trim(&js::to_js_string(y)))),
            }),
            "status" => Some(format!("Status: {} → {}.", given_or(x, "none"), given_or(y, "none"))),
            "initial" => Some(format!(
                "It now starts at {} (was {}).",
                state_name(&a.g, &initial(fa)),
                state_name(&b.g, &initial(fb))
            )),
            _ => None,
        },
    ));
    let (db, da) = (fb.get("decisions"), fa.get("decisions"));
    for id in union(db, da) {
        let x = member(db, &id);
        let y = member(da, &id);
        if same(x, y) {
            continue;
        }
        let key = format!("decision {id}");
        if x.is_none() || y.is_none() {
            let d = either(y, x);
            let say = if y.is_none() {
                format!("No longer open: {}.", quote(&text(member(d, "question"))))
            } else {
                format!(
                    "New open decision: {}; recommended: {}.",
                    quote(&text(member(d, "question"))),
                    text(member(d, "recommend"))
                )
            };
            out.push(Change::new("flow", key, op_of(x, y), Some(say)).before(x).after(y));
            continue;
        }
        let yq = quote(&text(member(y, "question")));
        out.extend(field_changes("flow", &key, raw(x), raw(y), &["question", "recommend", "affects"], &|f, _, _| {
            Some(match f {
                "question" => {
                    format!("An open decision is now worded {yq} (was {}).", quote(&text(member(x, "question"))))
                }
                "recommend" => format!("The recommendation for {yq} is now: {}.", text(member(y, "recommend"))),
                _ => {
                    let affects = member(y, "affects").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
                    let names: Vec<String> = affects.iter().map(|s| state_name(&a.g, &js::to_js_string(s))).collect();
                    format!("The decision {yq} now affects {}.", join_words(&names))
                }
            })
        }));
        other_keys(&key, raw(x), raw(y), &["question", "recommend", "affects"], &mut out);
    }

    // what this view shows
    let sb = given(fb, "scope");
    let sa = given(fa, "scope");
    let (shows_b, shows_a) = (member(sb, "shows"), member(sa, "shows"));
    if !same(shows_b, shows_a) {
        let say = match shows_a {
            None => "The view no longer says what it shows.".to_string(),
            Some(s) => {
                let was = shows_b.map_or_else(String::new, |p| format!(" (was {})", quote(&js::to_js_string(p))));
                format!("What this view shows: {}{was}.", quote(&js::to_js_string(s)))
            }
        };
        out.push(
            Change::new("scope", "scope", op_of(shows_b, shows_a), Some(say))
                .field("shows")
                .before(shows_b)
                .after(shows_a),
        );
    }
    let (world_b, world_a) = (member(sb, "world"), member(sa, "world"));
    if !same(world_b, world_a) {
        let say = if world_a.and_then(Value::as_str) == Some("open") {
            "The view is now an open world: what is not drawn is not ruled out."
        } else {
            "The view is no longer an open world."
        };
        out.push(
            Change::new("scope", "scope", op_of(world_b, world_a), Some(say.to_string()))
                .field("world")
                .before(world_b)
                .after(world_a),
        );
    }
    let oov = value_changes(member(sb, "outOfView"), member(sa, "outOfView"));
    for x in &oov.added {
        let say = format!("Now left out of this view: {}.", quote(&js::to_js_string(x)));
        out.push(Change::new("scope", "scope", Op::Added, Some(say)).field("outOfView").after(Some(x)));
    }
    for x in &oov.removed {
        let say = format!("No longer left out of this view: {}.", quote(&js::to_js_string(x)));
        out.push(Change::new("scope", "scope", Op::Removed, Some(say)).field("outOfView").before(Some(x)));
    }
    other_keys("scope", raw(sb), raw(sa), &["shows", "world", "outOfView"], &mut out);

    // the declared main story
    let norm = |m: Option<&Value>| -> Option<Value> {
        let m = m?;
        let mut o = Object::new();
        match m {
            Value::Array(_) => {
                o.insert("given", Value::Object(Object::new()));
                o.insert("events", m.clone());
            }
            other => {
                o.insert("given", given(other, "given").cloned().unwrap_or_else(|| Value::Object(Object::new())));
                if let Some(events) = other.get("events") {
                    o.insert("events", events.clone());
                }
            }
        }
        Some(Value::Object(o))
    };
    let (main_b, main_a) = (fb.get("main"), fa.get("main"));
    let (nb, na) = (norm(main_b), norm(main_a));
    if !same(nb.as_ref(), na.as_ref()) {
        let say = if main_a.is_none() {
            "The flow no longer declares its main story; the first success exit through a screen is used."
        } else if main_b.is_none() {
            "The flow now declares its main story."
        } else {
            "The declared main story sends other events, or starts from another situation."
        };
        out.push(
            Change::new("main", "main", op_of(main_b, main_a), Some(say.to_string()))
                .before(nb.as_ref())
                .after(na.as_ref()),
        );
    }

    // what it keeps track of
    let (cb, ca) = (fb.get("context"), fa.get("context"));
    for v in union(cb, ca) {
        let x = member(cb, &v);
        let y = member(ca, &v);
        if same(x, y) {
            continue;
        }
        let key = format!("context {v}");
        let values_of = |cv: Option<&Value>| -> Vec<Value> {
            member(cv, "values").and_then(Value::as_array).cloned().unwrap_or_default()
        };
        if x.is_none() || y.is_none() {
            let cv = either(y, x);
            let any = member(cv, "initial").and_then(Value::as_str) == Some("any");
            let values = join_words(
                &values_of(cv).iter().map(|val| value_words(&v, cv, &js::to_js_string(val))).collect::<Vec<_>>(),
            );
            let say = if y.is_some() {
                if any {
                    format!("New in the starting situation: {values}.")
                } else {
                    format!("The flow now keeps track of {v} (it starts as {}).", text(member(cv, "initial")))
                }
            } else if any {
                format!("No longer part of the starting situation: {values}.")
            } else {
                format!("The flow no longer keeps track of {v}.")
            };
            out.push(Change::new("situations", key, op_of(x, y), Some(say)).before(x).after(y));
            continue;
        }
        let (xv, yv) = (member(x, "values"), member(y, "values"));
        if !same(xv, yv) {
            let strs = |cv: Option<&Value>| values_of(cv).iter().map(js::to_js_string).collect::<Vec<_>>();
            let vals = word_changes(&strs(x), &strs(y));
            let mut parts: Vec<String> = Vec::new();
            if !vals.added.is_empty() {
                let words: Vec<String> = vals.added.iter().map(|val| value_words(&v, y, val)).collect();
                parts.push(format!("can now also be {}", join_words(&words)));
            }
            if !vals.removed.is_empty() {
                let words: Vec<String> = vals.removed.iter().map(|val| value_words(&v, x, val)).collect();
                parts.push(format!("can no longer be {}", join_words(&words)));
            }
            if parts.is_empty() {
                parts.push(
                    "lists its values in a new order (the order in which starting situations are tried)".to_string(),
                );
            }
            let say = format!("{v} {}.", parts.join("; "));
            out.push(
                Change::new("situations", key.clone(), Op::Changed, Some(say)).field("values").before(xv).after(yv),
            );
        }
        let (xi, yi) = (member(x, "initial"), member(y, "initial"));
        if !same(xi, yi) {
            let w = |i: Option<&Value>| {
                if i.and_then(Value::as_str) == Some("any") { "any of its values".to_string() } else { text(i) }
            };
            let say = format!("{v} now starts as {} (was {}).", w(yi), w(xi));
            out.push(
                Change::new("situations", key.clone(), Op::Changed, Some(say)).field("initial").before(xi).after(yi),
            );
        }
        let (xs, ys) = (member(x, "say"), member(y, "say"));
        for val in union(xs, ys) {
            let p = member(xs, &val);
            let q = member(ys, &val);
            if strict_eq(p, q) {
                continue;
            }
            // an empty `say` leaves the value out of the starting sentence
            let words = |t: &Value| {
                if t.as_str() == Some("") {
                    "left out of the starting sentence".to_string()
                } else {
                    format!("described as {}", quote(&js::to_js_string(t)))
                }
            };
            let empty = |t: Option<&Value>| t.and_then(Value::as_str) == Some("");
            let say = match (p, q) {
                (_, None) => format!("{v} = {val} is no longer described in words."),
                (p, Some(q)) if p.is_none() || empty(p) || empty(Some(q)) => {
                    format!("{v} = {val} is now {}.", words(q))
                }
                (p, Some(q)) => format!("{} now reads {}.", quote(&text(p)), quote(&js::to_js_string(q))),
            };
            out.push(
                Change::new("situations", key.clone(), op_of(p, q), Some(say))
                    .field(format!("say.{val}"))
                    .before(p)
                    .after(q),
            );
        }
        other_keys(&key, raw(x), raw(y), &["values", "initial", "say"], &mut out);
    }

    // named conditions and effects: formal, so only listed under the exact changes; their consequences show in behaviour
    for (section, word) in [("guards", "guard"), ("effects", "effect")] {
        let (gb, ga) = (fb.get(section), fa.get(section));
        for n in union(gb, ga) {
            let (p, q) = (member(gb, &n), member(ga, &n));
            if !same(p, q) {
                out.push(Change::new("conditions", format!("{word} {n}"), op_of(p, q), None).before(p).after(q));
            }
        }
    }

    // steps
    const STATE_KNOWN: [&str; 15] = [
        "title",
        "description",
        "type",
        "invoke",
        "on",
        "always",
        "redirect",
        "code",
        "carries",
        "leavesPage",
        "by",
        "goal",
        "includes",
        "response",
        "source",
    ];
    const ENDING: [&str; 5] = ["type", "redirect", "code", "carries", "leavesPage"];
    let (stb, sta) = (fb.get("states"), fa.get("states"));
    for id in union(stb, sta) {
        let x = member(stb, &id);
        let y = member(sta, &id);
        let key = format!("state {id}");
        let (Some(xs), Some(ys)) = (x, y) else {
            if x.is_none() {
                let say = format!("New step {} ({}).", state_name(&a.g, &id), step_kind(&a.g, &id));
                out.push(Change::new("steps", key, Op::Added, Some(say)));
            } else {
                out.push(Change::new(
                    "steps",
                    key,
                    Op::Removed,
                    Some(format!("Step {} removed.", state_name(&b.g, &id))),
                ));
            }
            continue;
        };
        let name = state_name(&a.g, &id);
        out.extend(field_changes(
            "steps",
            &key,
            raw(x),
            raw(y),
            &["title", "by", "goal", "description", "invoke", "includes", "response", "source"],
            &|f, p, q| {
                Some(match f {
                    "title" => format!("{} is now called {name}.", state_name(&b.g, &id)),
                    "by" => match q {
                        None => format!("{name} no longer says who does it."),
                        Some(q) => {
                            let was = p.map_or_else(String::new, |p| format!(" (was {})", by_text(Some(p))));
                            format!("{name} is now done by {}{was}.", by_text(Some(q)))
                        }
                    },
                    "goal" => {
                        if q == Some(&Value::Bool(true)) {
                            format!("{name} is now a goal.")
                        } else {
                            format!("{name} is no longer a goal.")
                        }
                    }
                    "description" => match q {
                        None => format!("{name} has no description any more."),
                        Some(q) => format!("{name} has a new description: {}.", quote(&js::to_js_string(q))),
                    },
                    "invoke" => format!("{name} now calls {} (was {}).", invoke_words(q), invoke_words(p)),
                    "response" => match q {
                        None => format!("{name} no longer says what it answers."),
                        Some(q) => {
                            let was = p.map_or_else(String::new, |p| {
                                format!(" (was {})", quote(&response_line(raw(Some(p)))))
                            });
                            format!("{name} now answers {}{was}.", quote(&response_line(raw(Some(q)))))
                        }
                    },
                    _ => {
                        let inc = value_changes(p, q);
                        let covers = |xs: &[Value]| {
                            join_words(&xs.iter().map(|x| quote(&js::to_js_string(x))).collect::<Vec<_>>())
                        };
                        let parts: Vec<String> = [
                            if inc.added.is_empty() {
                                String::new()
                            } else {
                                format!("now also covers {}", covers(&inc.added))
                            },
                            if inc.removed.is_empty() {
                                String::new()
                            } else {
                                format!("no longer covers {}", covers(&inc.removed))
                            },
                        ]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect();
                        let what = if parts.is_empty() {
                            "lists what it covers in a new order".to_string()
                        } else {
                            parts.join("; ")
                        };
                        format!("{name} {what}.")
                    }
                })
            },
        ));
        let ending = |st: Option<&Value>| -> Value {
            let r = raw(st);
            Value::Object(ENDING.iter().filter_map(|f| r.get(f).map(|v| (*f, v.clone()))).collect())
        };
        let (eb, ea) = (ending(x), ending(y));
        if !same(Some(&eb), Some(&ea)) {
            let (xf, yf) = (State(xs).is_final(), State(ys).is_final());
            let say = if yf && xf {
                format!("{name} now ends with: {} (was {}).", exit_line(State(ys)), exit_line(State(xs)))
            } else if yf {
                format!("{name} is now an ending: {}.", exit_line(State(ys)))
            } else if xf {
                format!("{name} is no longer an ending.")
            } else {
                format!("{name} changed its type.")
            };
            out.push(
                Change::new("steps", key.clone(), Op::Changed, Some(say))
                    .field("ending")
                    .before(Some(&eb))
                    .after(Some(&ea)),
            );
        }
        other_keys(&key, raw(x), raw(y), &STATE_KNOWN, &mut out);
    }

    // how it moves on: the alternatives of one event, and the rules of an `always`, are ordered (the first that matches wins).
    // They are paired by their condition, then by their `say`, then by their target, so an inserted rule does not make the
    // rules after it look rewritten; a pair that changed place relative to the other pairs is reported as moved.
    let mut ways: Vec<(String, String)> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    const TRANSITION_KNOWN: [&str; 6] = ["target", "guard", "effect", "say", "starts", "source"];
    let k_guard = |t: &Value| Some(given(t, "guard").map_or_else(String::new, js::to_js_string));
    let k_say = |t: &Value| given(t, "say").map(js::to_js_string);
    // `t.target` as a Map key: undefined is a key of its own, null pairs nothing
    let k_target = |t: &Value| match t.get("target") {
        None => Some("\u{0}undefined".to_string()),
        Some(Value::Null) => None,
        Some(v) => Some(js::to_js_string(v)),
    };
    let target = |t: &Value| text(t.get("target"));
    for id in union(stb, sta) {
        let x = member(stb, &id);
        let y = member(sta, &id);
        let mut lists: Vec<(String, Vec<Value>, Vec<Value>)> = vec![("always".to_string(), always_of(x), always_of(y))];
        let (onb, ona) = (member(x, "on"), member(y, "on"));
        for event in union(onb, ona) {
            let (bl, al) = (alternatives_of(member(onb, &event)), alternatives_of(member(ona, &event)));
            lists.push((event, bl, al));
        }
        for (event, bl, al) in &lists {
            let event = event.as_str();
            let keys: [Key<'_, Value>; 3] = [&k_guard, &k_say, &k_target];
            let pairs = match_ordered(bl, al, &keys);
            let from = |g: &Graph, i: usize, t: &Value| {
                let w = way_words(g, &id, event, t, i);
                if event == "always" {
                    format!("in {}, {w}", state_name(g, &id))
                } else {
                    format!("from {}, {w}", state_name(g, &id))
                }
            };
            let pb: HashSet<usize> = pairs.iter().map(|p| p.0).collect();
            let pa: HashSet<usize> = pairs.iter().map(|p| p.1).collect();
            for (i, t) in al.iter().enumerate() {
                if pa.contains(&i) {
                    continue;
                }
                added.push(format!("{id}|{event}|{i}"));
                let say = format!("New: {} leads to {}.", from(&a.g, i, t), state_name(&a.g, &target(t)));
                out.push(Change::new("ways", way_key(&id, event, i, al.len()), Op::Added, Some(say)).after(Some(t)));
            }
            for (i, t) in bl.iter().enumerate() {
                if pb.contains(&i) {
                    continue;
                }
                let say = if y.is_none() {
                    None
                } else {
                    Some(format!("Removed: {} led to {}.", from(&b.g, i, t), state_name(&b.g, &target(t))))
                };
                out.push(Change::new("ways", way_key(&id, event, i, bl.len()), Op::Removed, say).before(Some(t)));
            }
            for &(i, j) in &pairs {
                ways.push((format!("{id}|{event}|{i}"), format!("{id}|{event}|{j}")));
                let p = &bl[i];
                let q = &al[j];
                let cap = capital(&from(&a.g, j, q));
                let wk = way_key(&id, event, j, al.len());
                out.extend(field_changes("ways", &wk, raw(Some(p)), raw(Some(q)), &TRANSITION_KNOWN, &|f, _, _| {
                    Some(match f {
                        "target" => format!(
                            "{cap} now leads to {} (was {}).",
                            state_name(&a.g, &target(q)),
                            state_name(&b.g, &target(p))
                        ),
                        "guard" => format!("{cap}: the condition changed."),
                        "effect" => format!("{cap}: what it records changed."),
                        "starts" => {
                            if q.get("starts").is_none() {
                                format!("{cap} no longer starts {}.", text(member(p.get("starts"), "flow")))
                            } else if p.get("starts").is_none() {
                                format!("{cap} now starts {}.", text(member(q.get("starts"), "flow")))
                            } else {
                                format!("{cap}: the flow it starts, or what that flow begins with, changed.")
                            }
                        }
                        _ => {
                            let where_ = if event == "always" {
                                format!("In {}, rule {}", state_name(&a.g, &id), j + 1)
                            } else {
                                let bare = match q.get("target") {
                                    Some(t) => Value::Object(Object::from_iter([("target", t.clone())])),
                                    None => Value::Object(Object::new()),
                                };
                                format!("From {}, {}", state_name(&a.g, &id), way_words(&a.g, &id, event, &bare, j))
                            };
                            match (p.get("say"), q.get("say")) {
                                (_, None) => format!(
                                    "{where_} is no longer described in words (was {}).",
                                    quote(&given_or(p.get("say"), ""))
                                ),
                                (None, Some(qs)) => {
                                    format!("{where_} is now described as {}.", quote(&js::to_js_string(qs)))
                                }
                                (Some(ps), Some(qs)) => format!(
                                    "{where_}: {} now reads {}.",
                                    quote(&js::to_js_string(ps)),
                                    quote(&js::to_js_string(qs))
                                ),
                            }
                        }
                    })
                }));
                other_keys(&wk, raw(Some(p)), raw(Some(q)), &TRANSITION_KNOWN, &mut out);
            }
            for (i, j) in moved(&pairs) {
                let t = &al[j];
                let say = if event == "always" {
                    format!(
                        "In {}, {} is now rule {} (was {}); the first rule that matches wins.",
                        state_name(&a.g, &id),
                        quote(&rule_words(t)),
                        j + 1,
                        i + 1
                    )
                } else {
                    format!(
                        "From {}, the alternatives of {} are checked in a new order.",
                        state_name(&a.g, &id),
                        way_words(&a.g, &id, event, t, j)
                    )
                };
                out.push(
                    Change::new("ways", way_key(&id, event, j, al.len()), Op::Moved, Some(say))
                        .before(Some(&Value::from(i + 1)))
                        .after(Some(&Value::from(j + 1))),
                );
            }
        }
    }

    // guarantees
    let by_id = |list: Vec<&Value>, side: &str| -> Result<Object> {
        let mut m = Object::new();
        for x in list {
            let id = text(x.get("id"));
            if m.contains_key(&id) {
                return Err(Error::graph(format!("{side}: the assertion {id} is written twice"), None));
            }
            m.insert(id, x.clone());
        }
        Ok(m)
    };
    let asb = Value::Object(by_id(b.g.flow().assertions(), "before")?);
    let asa = Value::Object(by_id(a.g.flow().assertions(), "after")?);
    for id in union(Some(&asb), Some(&asa)) {
        let x = member(Some(&asb), &id);
        let y = member(Some(&asa), &id);
        if same(x, y) {
            continue;
        }
        let key = format!("assertion {id}");
        let said =
            |v: Option<&Value>| quote(&either(member(v, "say"), None).map_or_else(|| humanize(&id), js::to_js_string));
        match (x, y) {
            (None, y) => out.push(
                Change::new("guarantees", key, Op::Added, Some(format!("New guarantee: {}.", said(y))))
                    .after(member(y, "never")),
            ),
            (x, None) => out.push(
                Change::new("guarantees", key, Op::Removed, Some(format!("Guarantee dropped: {}.", said(x))))
                    .before(member(x, "never")),
            ),
            (x, y) => {
                out.extend(field_changes("guarantees", &key, raw(x), raw(y), &["never", "say"], &|f, _, _| {
                    Some(if f == "never" {
                        format!("The guarantee {} now checks a different condition.", said(y))
                    } else {
                        format!("Guarantee reworded: {} (was {}).", said(y), said(x))
                    })
                }));
                other_keys(&key, raw(x), raw(y), &["id", "never", "say"], &mut out);
            }
        }
    }
    other_keys(
        "flow",
        rb,
        ra,
        &[
            "schema",
            "kind",
            "id",
            "status",
            "ui",
            "title",
            "summary",
            "scope",
            "context",
            "guards",
            "effects",
            "main",
            "initial",
            "states",
            "assertions",
            "decisions",
            "domain",
        ],
        &mut out,
    );

    // what the user sees
    let ub = &b.g.ui;
    let ua = &a.g.ui;
    let (smb, sma) = (ub.get("samples"), ua.get("samples"));
    for k in union(smb, sma) {
        let p = member(smb, &k);
        let q = member(sma, &k);
        if strict_eq(p, q) {
            continue;
        }
        let say = match q {
            None => format!("The sample {k} is gone."),
            Some(q) => {
                let was = p.map_or_else(String::new, |p| format!(" (was {})", quote(&js::to_js_string(p))));
                format!("The sample {k} is now {}{was}.", quote(&js::to_js_string(q)))
            }
        };
        out.push(Change::new("screens", "ui samples", op_of(p, q), Some(say)).field(k).before(p).after(q));
    }
    const NODE_KNOWN: [&str; 12] = [
        "type",
        "component",
        "role",
        "intent",
        "name",
        "text",
        "in",
        "unless",
        "when",
        "disabledIn",
        "emits",
        "source",
    ];
    let (nb_, na_) = (ub.get("nodes"), ua.get("nodes"));
    for id in union(nb_, na_) {
        let x = member(nb_, &id);
        let y = member(na_, &id);
        if same(x, y) {
            continue;
        }
        let key = format!("ui node {id}");
        let on = |g: &Graph, n: Option<&Value>| {
            let states = shown_on(n);
            if states.is_empty() {
                "no screen".to_string()
            } else {
                join_words(&states.iter().map(|s| state_name(g, s)).collect::<Vec<_>>())
            }
        };
        let (Some(xn), Some(yn)) = (x, y) else {
            if let Some(yn) = y {
                let say = format!("New on {}: {}.", on(&a.g, y), node_words(&a.g, &id, yn));
                out.push(Change::new("screens", key, Op::Added, Some(say)));
            } else if let Some(xn) = x {
                let say = format!("Removed from {}: {}.", on(&b.g, x), node_words(&b.g, &id, xn));
                out.push(Change::new("screens", key, Op::Removed, Some(say)));
            }
            continue;
        };
        let what = node_words(&a.g, &id, yn);
        let cap = capital(&what);
        out.extend(field_changes("screens", &key, raw(x), raw(y), &NODE_KNOWN, &|f, p, q| match f {
            "in" | "unless" | "disabledIn" => {
                let list = |n: Option<&Value>| {
                    if f == "disabledIn" {
                        strings(member(n, "disabledIn")).into_iter().map(str::to_string).collect()
                    } else {
                        shown_on(n)
                    }
                };
                let ch = word_changes(&list(x), &list(y));
                let verb = if f == "disabledIn" { "disabled" } else { "shown" };
                let names =
                    |g: &Graph, xs: &[String]| join_words(&xs.iter().map(|s| state_name(g, s)).collect::<Vec<_>>());
                let parts: Vec<String> = [
                    if ch.added.is_empty() {
                        String::new()
                    } else {
                        format!("is now {verb} on {}", names(&a.g, &ch.added))
                    },
                    if ch.removed.is_empty() {
                        String::new()
                    } else {
                        format!("is no longer {verb} on {}", names(&b.g, &ch.removed))
                    },
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                if parts.is_empty() { None } else { Some(format!("{cap} {}.", parts.join("; "))) }
            }
            "text" => Some(format!(
                "{} now reads {}.",
                quote(&label_words(&b.g, xn.get("text")).unwrap_or_default()),
                quote(&label_words(&a.g, yn.get("text")).unwrap_or_default())
            )),
            "name" => Some(format!(
                "The accessible name of the {what} is now {} (was {}).",
                quote(&label_words(&a.g, yn.get("name")).unwrap_or_else(|| "(from another node)".to_string())),
                quote(&label_words(&b.g, xn.get("name")).unwrap_or_else(|| "(from another node)".to_string()))
            )),
            "when" => Some(match q {
                None => format!("{cap} no longer depends on a flag."),
                Some(q) => format!("{cap} is now shown only when {}.", js::to_js_string(q)),
            }),
            "emits" => Some(match q {
                None => format!("{cap} no longer sends an event."),
                Some(q) => {
                    let was = p.map_or_else(String::new, |p| format!(" (was {})", js::to_js_string(p)));
                    format!("{cap} now sends {}{was}.", js::to_js_string(q))
                }
            }),
            "component" | "type" => Some(format!(
                "{cap} is now drawn as {} (was {}).",
                humanize(&given_or(q, "nothing")).to_lowercase(),
                humanize(&given_or(p, "nothing")).to_lowercase()
            )),
            _ => Some(format!("{cap} now has the {f} {} (was {}).", given_or(q, "none"), given_or(p, "none"))),
        }));
        other_keys(&key, raw(x), raw(y), &NODE_KNOWN, &mut out);
    }
    other_keys("ui", raw(Some(ub)), raw(Some(ua)), &["schema", "kind", "id", "samples", "nodes"], &mut out);

    // wording
    for k in union(Some(&b.g.catalog), Some(&a.g.catalog)) {
        let p = member(Some(&b.g.catalog), &k);
        let q = member(Some(&a.g.catalog), &k);
        if strict_eq(p, q) {
            continue;
        }
        let say = if p.is_none() {
            format!("New text: {}.", quote(&plain(&a.g, &k)))
        } else if q.is_none() {
            format!("Text removed: {}.", quote(&plain(&b.g, &k)))
        } else {
            format!("{} now reads {}.", quote(&plain(&b.g, &k)), quote(&plain(&a.g, &k)))
        };
        out.push(Change::new("wording", format!("catalog {k}"), op_of(p, q), Some(say)).before(p).after(q));
    }

    // the API it calls
    match (&b.domain, &a.domain) {
        (None, Some(ad)) => out.push(
            Change::new(
                "domain",
                "domain",
                Op::Added,
                Some(format!("The graph now has an API description ({}).", ad.file)),
            )
            .after(Some(&Value::from(ad.file.as_str()))),
        ),
        (Some(bd), None) => out.push(
            Change::new(
                "domain",
                "domain",
                Op::Removed,
                Some(format!("The API description ({}) is no longer next to the graph.", bd.file)),
            )
            .before(Some(&Value::from(bd.file.as_str()))),
        ),
        (Some(bd), Some(ad)) => deep_diff("domain", "domain", Some(&bd.doc), Some(&ad.doc), &mut out, &[]),
        (None, None) => {}
    }

    Ok(FlowStructure { changes: out, ways, added })
}

pub struct FlowRun<'g> {
    pub g: &'g Graph,
    pub m: Model,
    pub ex: Exploration,
    pub violations: IndexMap<String, usize>, // assertion id -> the first (shortest) situation that breaks it
    pub exits: IndexMap<String, usize>,      // final state -> the first (shortest) situation that reaches it
    pub moves: usize,                        // ways from one reachable situation to another
    pub main: Option<Vec<PathStep>>,
    pub main_broken: bool, // a declared main story whose events cannot be walked
    pub reached: HashSet<String>,
}

fn run(g: &Graph) -> Result<FlowRun<'_>> {
    let m = Model::new(g.flow.clone());
    let ex = explore(&m, None, None)?;
    let mut violations: IndexMap<String, usize> = IndexMap::new();
    for a in g.flow().assertions() {
        let mut hit: Option<usize> = None;
        if !ex.configs.is_empty() {
            match a.get("never") {
                Some(Value::String(src)) => {
                    let test = m.tester(src)?;
                    for (i, c) in ex.configs.iter().enumerate() {
                        if test(&c.state, &c.ctx)? {
                            hit = Some(i);
                            break;
                        }
                    }
                }
                other => {
                    for (i, c) in ex.configs.iter().enumerate() {
                        if m.test_value(other, &c.state, &c.ctx)? {
                            hit = Some(i);
                            break;
                        }
                    }
                }
            }
        }
        if let Some(h) = hit {
            violations.insert(text(a.get("id")), h);
        }
    }
    let exits: IndexMap<String, usize> = final_paths(&m, &ex).into_iter().map(|f| (f.state, f.index)).collect();
    let mut moves = 0;
    for c in &ex.configs {
        moves += m.steps(&c.state, &c.ctx)?.len();
    }
    let main_spec = g.flow().main();
    let declared = match main_spec {
        None => None,
        Some(main) => steps_by_events(&m, main)?,
    };
    let main_broken = main_spec.is_some() && declared.is_none();
    let main = match declared {
        Some(d) => Some(d),
        None => match main_story_index(g, &ex) {
            None => None,
            Some(i) => Some(steps_to(&m, &ex, i)?),
        },
    };
    let mut reached: HashSet<String> = HashSet::new();
    for c in &ex.configs {
        if !reached.contains(&*c.state) {
            reached.insert(c.state.to_string());
        }
    }
    Ok(FlowRun { g, m, ex, violations, exits, moves, main, main_broken, reached })
}

// Runs one side; an `always` that is not exhaustive, an unknown name in a guard and the like stop the diff with the side named.
pub fn run_side<'g>(g: &'g Graph, side: &str) -> Result<FlowRun<'g>> {
    run(g).map_err(|err| Error::graph(format!("{side}: {}", err.message), None))
}

// a path as people read it: the starting situation, then one numbered line per step, the ending last
fn told(r: &FlowRun<'_>, steps: &[PathStep]) -> Vec<String> {
    let story = narrate(r.g, &r.m, steps);
    let last_final = steps.last().and_then(|s| r.g.flow().state(&s.state)).is_some_and(|st| st.is_final());
    let mut lines = story.steps;
    if last_final {
        lines.push(story.result);
    }
    let mut out = vec![format!("When: {}.", story.given)];
    out.extend(lines.iter().enumerate().map(|(i, s)| format!("{}. {s}", i + 1)));
    out
}

fn chain(r: &FlowRun<'_>, steps: &[PathStep]) -> String {
    steps.iter().map(|s| title_of(r.g.flow(), &s.state)).collect::<Vec<_>>().join(" → ")
}

// the same path in both revisions: the same steps taken by the same events, from the same values of what both start with
fn same_path(rb: &FlowRun<'_>, ra: &FlowRun<'_>, b: &[PathStep], a: &[PathStep]) -> bool {
    let walk = |steps: &[PathStep]| {
        steps.iter().map(|s| format!("{}>{}", s.event.as_deref().unwrap_or(""), s.state)).collect::<Vec<_>>().join(" ")
    };
    if walk(b) != walk(a) {
        return false;
    }
    let any =
        |g: &Graph, v: &str| g.flow().context_var(v).and_then(|cv| cv.initial()).and_then(Value::as_str) == Some("any");
    let (Some(b0), Some(a0)) = (b.first(), a.first()) else { return true };
    rb.m.vars.iter().enumerate().filter(|(_, v)| any(rb.g, v) && any(ra.g, v)).all(|(ib, v)| {
        let Some(ia) = ra.m.vars.iter().position(|x| x == v) else { return false };
        let x = b0.ctx.get(ib).map_or(crate::model::Scalar::Undefined, |&c| rb.m.value_at(ib, c));
        let y = a0.ctx.get(ia).map_or(crate::model::Scalar::Undefined, |&c| ra.m.value_at(ia, c));
        x.strict_eq(&y)
    })
}

/// the transition a `taken` key names: its state, event, index and the transition itself
fn transition_at(g: &Graph, taken: &str) -> (String, String, usize, Value) {
    let mut parts = taken.split('|');
    let state = parts.next().unwrap_or("").to_string();
    let event = parts.next().unwrap_or("undefined").to_string();
    let index: Option<usize> = parts.next().and_then(|i| i.parse().ok());
    let st = member(g.flow.get("states"), &state);
    let list = if event == "always" { always_of(st) } else { alternatives_of(member(member(st, "on"), &event)) };
    let t = index.and_then(|i| list.get(i).cloned()).unwrap_or(Value::Null);
    (state, event, index.unwrap_or(0), t)
}

pub fn flow_behaviour(rb: &FlowRun<'_>, ra: &FlowRun<'_>, s: &FlowStructure) -> Result<Vec<Shift>> {
    let mut out: Vec<Shift> = Vec::new();
    let fb = rb.g.flow();
    let fa = ra.g.flow();
    let mut shift =
        |topic: &str, key: &str, before: Value, after: Value, say: String, detail: Vec<String>, worse: bool| {
            out.push(Shift {
                topic: topic.to_string(),
                key: key.to_string(),
                persona: None,
                before,
                after,
                say,
                detail,
                worse,
            });
        };

    // guarantees first: a broken one is what a reviewer must see
    let find = |list: &[&Value], id: &str| list.iter().find(|x| x.get_str("id") == Some(id)).map(|x| (*x).clone());
    let (lb, la) = (fb.assertions(), fa.assertions());
    let said = |id: &str| {
        let (xa, xb) = (find(&la, id), find(&lb, id));
        let say = either(member(xa.as_ref(), "say"), member(xb.as_ref(), "say"));
        quote(&either(say, None).map_or_else(|| humanize(id), js::to_js_string))
    };
    let ids = |list: &[&Value]| Value::Object(list.iter().map(|x| (text(x.get("id")), Value::Null)).collect());
    let at = |r: &FlowRun<'_>, i: usize| -> Result<Value> {
        let steps = steps_to(&r.m, &r.ex, i)?;
        let mut o = Object::new();
        o.insert("state", Value::from(&*r.ex.configs[i].state));
        o.insert("start", Value::Object(steps.first().map_or_else(Object::new, |s| r.m.scenario(&s.ctx))));
        o.insert("events", Value::Array(steps.iter().filter_map(|x| x.event.as_deref().map(Value::from)).collect()));
        Ok(Value::Object(o))
    };
    let violated = |v: Value| Value::Object(Object::from_iter([("violated", v)]));
    for id in union(Some(&ids(&lb)), Some(&ids(&la))) {
        let in_b = lb.iter().any(|x| x.get_str("id") == Some(&id));
        let in_a = la.iter().any(|x| x.get_str("id") == Some(&id));
        let vb = rb.violations.get(&id).copied();
        let va = ra.violations.get(&id).copied();
        match (vb, va) {
            (None, Some(va)) if in_a => {
                let lead = if in_b { "A guarantee no longer holds" } else { "A new guarantee does not hold" };
                let before = Value::from(if in_b { "holds" } else { "absent" });
                let say =
                    format!("{lead}: {}. It breaks in {}:", said(&id), state_name(ra.g, &ra.ex.configs[va].state));
                let detail = told(ra, &steps_to(&ra.m, &ra.ex, va)?);
                shift("guarantee", &id, before, violated(at(ra, va)?), say, detail, true);
            }
            (Some(vb), None) if in_a && in_b => {
                let say = format!("A guarantee now holds: {} (it was broken).", said(&id));
                shift("guarantee", &id, violated(at(rb, vb)?), Value::from("holds"), say, Vec::new(), false);
            }
            (Some(vb), Some(va)) if in_a && in_b => {
                let (pb, pa) = (steps_to(&rb.m, &rb.ex, vb)?, steps_to(&ra.m, &ra.ex, va)?);
                if same_path(rb, ra, &pb, &pa) {
                    continue;
                }
                let say = format!(
                    "Still broken, now first in {}: {}.",
                    state_name(ra.g, &ra.ex.configs[va].state),
                    said(&id)
                );
                shift("guarantee", &id, violated(at(rb, vb)?), violated(at(ra, va)?), say, told(ra, &pa), false);
            }
            (Some(vb), _) if !in_a && in_b => {
                let say = format!("No longer checked, and it was broken: {}.", said(&id));
                shift("guarantee", &id, violated(at(rb, vb)?), Value::from("absent"), say, Vec::new(), false);
            }
            _ => {}
        }
    }

    // the main story
    if rb.main_broken != ra.main_broken {
        if ra.main_broken {
            shift(
                "main",
                "main",
                Value::from("walks"),
                Value::from("broken"),
                "The declared main story can no longer be walked: an event is not possible where it is sent."
                    .to_string(),
                Vec::new(),
                true,
            );
        } else {
            shift(
                "main",
                "main",
                Value::from("broken"),
                Value::from("walks"),
                "The declared main story can be walked again.".to_string(),
                Vec::new(),
                false,
            );
        }
    }
    match (&rb.main, &ra.main) {
        (Some(mb), None) => {
            let c = chain(rb, mb);
            shift(
                "main",
                "main",
                Value::from(c.as_str()),
                Value::Null,
                format!("There is no main story any more (it was: {c})."),
                Vec::new(),
                false,
            );
        }
        (None, Some(ma)) => {
            shift(
                "main",
                "main",
                Value::Null,
                Value::from(chain(ra, ma)),
                "There is now a main story:".to_string(),
                told(ra, ma),
                false,
            );
        }
        (Some(mb), Some(ma)) if !same_path(rb, ra, mb, ma) => {
            let (cb, ca) = (chain(rb, mb), chain(ra, ma));
            let say = format!("The main story changed. It was: {cb}. Now:");
            shift("main", "main", Value::from(cb), Value::from(ca), say, told(ra, ma), false);
        }
        _ => {}
    }

    // endings
    let (stb, sta) = (rb.g.flow.get("states"), ra.g.flow.get("states"));
    let is_final = |st: Option<&Value>| member(st, "type").and_then(Value::as_str) == Some("final");
    let finals: Vec<String> =
        union(stb, sta).into_iter().filter(|id| is_final(member(sta, id)) || is_final(member(stb, id))).collect();
    for id in &finals {
        let ib = rb.exits.get(id).copied();
        let ia = ra.exits.get(id).copied();
        match (ib, ia) {
            (None, Some(ia)) => {
                let line = fa.state(id).map(exit_line).unwrap_or_default();
                let say = format!("New way to end: {} ({line}).", state_name(ra.g, id));
                let detail = told(ra, &steps_to(&ra.m, &ra.ex, ia)?);
                shift("exit", id, Value::from("unreachable"), Value::from("reachable"), say, detail, false);
            }
            (Some(_), None) => {
                let removed = if fa.state(id).is_none() { " (the ending was removed)" } else { "" };
                let say = format!("No longer reachable: {}{removed}.", state_name(rb.g, id));
                shift("exit", id, Value::from("reachable"), Value::from("unreachable"), say, Vec::new(), false);
            }
            (Some(ib), Some(ia)) => {
                let pb = steps_to(&rb.m, &rb.ex, ib)?;
                let pa = steps_to(&ra.m, &ra.ex, ia)?;
                if same_path(rb, ra, &pb, &pa) {
                    continue;
                }
                let nb = pb.len() as isize - 1;
                let na = pa.len() as isize - 1;
                let how = if na == nb {
                    "is now reached another way".to_string()
                } else {
                    format!("is now reached in {na} steps (was {nb})")
                };
                let (cb, ca) = (chain(rb, &pb), chain(ra, &pa));
                let say = format!("{} {how}. It was: {cb}. Now:", capital(&state_name(ra.g, id)));
                shift("path", id, Value::from(cb), Value::from(ca), say, told(ra, &pa), false);
            }
            (None, None) => {}
        }
    }

    // steps no situation reaches, and transitions no situation takes
    for id in fa.state_names() {
        if fa.state(id).is_some_and(|st| st.is_final()) {
            continue;
        }
        let before = if fb.state(id).is_none() { None } else { Some(rb.reached.contains(id)) };
        let after = ra.reached.contains(id);
        let name = state_name(ra.g, id);
        match (before, after) {
            (Some(true), false) => shift(
                "reach",
                id,
                Value::Bool(true),
                Value::Bool(false),
                format!("{} can no longer be reached.", capital(&name)),
                Vec::new(),
                false,
            ),
            (Some(false), true) => shift(
                "reach",
                id,
                Value::Bool(false),
                Value::Bool(true),
                format!("{} can now be reached.", capital(&name)),
                Vec::new(),
                false,
            ),
            (None, false) => shift(
                "reach",
                id,
                Value::Null,
                Value::Bool(false),
                format!("The new step {name} is never reached."),
                Vec::new(),
                false,
            ),
            _ => {}
        }
    }
    let way_name = |g: &Graph, taken: &str| {
        let (state, event, index, t) = transition_at(g, taken);
        let w = way_words(g, &state, &event, &t, index);
        if event == "always" {
            format!("in {}, {w}", state_name(g, &state))
        } else {
            format!("from {}, {w}", state_name(g, &state))
        }
    };
    let state_of = |taken: &str| taken.split('|').next().unwrap_or("").to_string();
    for (kb, ka) in &s.ways {
        let st = state_of(ka);
        if !rb.reached.contains(&st) || !ra.reached.contains(&st) {
            continue;
        }
        let tb = rb.ex.taken.contains(kb.as_str());
        let ta = ra.ex.taken.contains(ka.as_str());
        if tb && !ta {
            let say = format!("{} is never taken any more.", capital(&way_name(ra.g, ka)));
            shift("taken", ka, Value::Bool(true), Value::Bool(false), say, Vec::new(), false);
        } else if !tb && ta {
            let say = format!("{} is now taken (it never was).", capital(&way_name(ra.g, ka)));
            shift("taken", ka, Value::Bool(false), Value::Bool(true), say, Vec::new(), false);
        }
    }
    for ka in &s.added {
        if ra.reached.contains(&state_of(ka)) && !ra.ex.taken.contains(ka.as_str()) {
            let say = format!("The new way {} is never taken.", way_name(ra.g, ka));
            shift("taken", ka, Value::Null, Value::Bool(false), say, Vec::new(), false);
        }
    }

    let nb = rb.ex.configs.len();
    let na = ra.ex.configs.len();
    if na != nb || ra.ex.starts != rb.ex.starts || ra.moves != rb.moves {
        let starts = if ra.ex.starts == rb.ex.starts {
            ra.ex.starts.to_string()
        } else {
            format!("{} → {}", rb.ex.starts, ra.ex.starts)
        };
        let say = if na == nb && ra.ex.starts == rb.ex.starts {
            format!(
                "The same {na} situations, with {} moves between them (was {}): what changed connects situations that were already reachable.",
                ra.moves, rb.moves
            )
        } else {
            format!(
                "Situations the model can reach: {nb} → {na}, from {starts} starting situations; moves between them: {} → {}.",
                rb.moves, ra.moves
            )
        };
        let counts = |n: usize, r: &FlowRun<'_>| {
            Value::Object(Object::from_iter([
                ("situations", Value::from(n)),
                ("starts", Value::from(r.ex.starts)),
                ("moves", Value::from(r.moves)),
            ]))
        };
        shift("situations", "situations", counts(nb, rb), counts(na, ra), say, Vec::new(), false);
    }
    Ok(out)
}

// the sentence for a revision that changes nothing a person can observe
pub fn flow_same_behaviour(r: &FlowRun<'_>) -> String {
    let ends = r.exits.len();
    let guarantees = r.g.flow().assertions().len();
    let parts: Vec<String> = [
        if ends > 0 {
            format!(
                "the same {} reached the same way",
                if ends == 1 { "ending is".to_string() } else { format!("{ends} endings are") }
            )
        } else {
            String::new()
        },
        if guarantees > 0 { format!("the {guarantees} guarantees give the same result") } else { String::new() },
        if r.main.is_some() { "the main story is the same".to_string() } else { String::new() },
    ]
    .into_iter()
    .filter(|x| !x.is_empty())
    .collect();
    let lead = if parts.is_empty() { String::new() } else { format!("{}, ", join_words(&parts)) };
    format!("Nothing: {lead}in the same {} situations.", r.ex.configs.len())
}
