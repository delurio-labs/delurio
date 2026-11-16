//! The edges a flow's diagrams draw, merged by their ends, and the words of their nodes: what a person presses, where
//! an exit leaves them, a step's response in one line. The port of edges.ts.

use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::{Captures, Regex};
use serde::Serialize;

use crate::graph::{Graph, State, Transition, alternatives, given, humanize, shown_in};
use crate::js::{self, Object, Value};
use crate::model::Exploration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    User,
    System,
    Route,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VizEdge {
    pub from: String,
    pub to: String,
    /// plain words, one entry per merged transition
    pub labels: Vec<String>,
    pub kind: EdgeKind,
    /// ends in a state where the user leaves the page
    pub leave: bool,
    /// no reachable configuration ever takes it
    pub unused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Server,
    Screen,
    Subflow,
    Exit,
}

pub const BY: [(&str, &str); 4] = [("human", "person"), ("ai", "AI"), ("agent", "agent"), ("ci", "CI")];

/// `BY[by]`
pub fn by_word(by: &str) -> Option<&'static str> {
    BY.iter().find(|(k, _)| *k == by).map(|(_, w)| *w)
}

pub fn kind_of(g: &Graph, name: &str) -> NodeKind {
    // a state that is not there (the TS crashed) is read as one that has no type and no invoke
    let st = g.flow().state(name);
    if st.is_some_and(|s| s.is_final()) {
        return NodeKind::Exit;
    }
    if st.and_then(|s| s.invoke()).is_some_and(|i| i.get("flow").is_some()) {
        return NodeKind::Subflow;
    }
    let shown = g.ui().nodes().any(|(_, n)| shown_in(n, name));
    if shown { NodeKind::Screen } else { NodeKind::Server }
}

/// `list.join(sep)`: a null item is empty; a value that is no list is read as its text
fn join_js(v: Option<&Value>, sep: &str) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|x| if x.is_null() { String::new() } else { js::to_js_string(x) })
            .collect::<Vec<_>>()
            .join(sep),
        Some(other) => js::to_js_string(other),
    }
}

pub fn exit_line(st: State<'_>) -> String {
    if st.leaves_page() {
        return "user leaves the page".to_string();
    }
    let redirect = st.0.get("redirect");
    if redirect.and_then(Value::as_str) == Some("never") {
        return "no redirect, page shown".to_string();
    }
    if redirect.and_then(Value::as_str) == Some("success") {
        return format!("redirect with {}", join_js(given(st.0, "carries"), " + "));
    }
    if redirect.is_none() && st.0.get("code").is_none() {
        return "ends here".to_string();
    }
    // a code that is no string (the TS crashed on it) is read as its text
    let code = given(st.0, "code").map(js::to_js_string).unwrap_or_default();
    let codes: Vec<&str> = code.split('|').map(js::trim).filter(|c| !c.is_empty()).collect();
    if codes.len() == 1 {
        format!("redirect, error={}", codes[0])
    } else {
        format!("redirect, error ({} codes)", codes.len())
    }
}

// a step's `response` in one line: the status, the code and the redirect first, then the other fields
pub fn response_line(r: &Object) -> String {
    let mut parts: Vec<String> = ["status", "code", "redirect"]
        .iter()
        .filter_map(|&k| {
            let v = r.get(k)?;
            Some(if k == "redirect" { format!("redirect {}", js::to_js_string(v)) } else { js::to_js_string(v) })
        })
        .collect();
    for (k, v) in r.iter() {
        if ["status", "code", "redirect"].contains(&k.as_str()) {
            continue;
        }
        parts.push(format!("{k}: {}", if let Value::String(s) = v { s.clone() } else { js::stringify(v) }));
    }
    parts.join(" · ")
}

pub fn node_line(g: &Graph, name: &str) -> String {
    let st = g.flow().state(name);
    let kind = kind_of(g, name);
    if kind == NodeKind::Exit
        && let Some(st) = st
    {
        return exit_line(st);
    }
    if kind == NodeKind::Subflow {
        let flow = st.and_then(|s| s.invoke()).and_then(|i| i.get("flow"));
        return format!("sign-in flow · {}", flow.map_or_else(|| "undefined".to_string(), js::to_js_string));
    }
    let by = st.and_then(|s| s.0.get("by")).and_then(|b| by_word(&js::to_js_string(b)));
    if st.is_some_and(|s| s.goal()) {
        return format!("{name} · goal");
    }
    format!("{name} · {}", by.unwrap_or(if kind == NodeKind::Screen { "screen" } else { "server step" }))
}

static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([A-Za-z0-9_]+)\}").expect("the placeholder pattern compiles"));

// visible text for a catalog key, with {placeholders} filled from the samples
pub fn plain(g: &Graph, key: &str) -> String {
    // a message that is no string (the TS crashed on it) is read as its text
    let raw = match given(&g.catalog, key) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => js::to_js_string(other),
        None => key.to_string(),
    };
    PLACEHOLDER
        .replace_all(&raw, |c: &Captures<'_>| {
            let p = &c[1];
            g.ui().sample(p).filter(|v| !v.is_null()).map_or_else(|| p.to_string(), js::to_js_string)
        })
        .into_owned()
}

// the button or link text a person presses to send `event` while in `state`, if there is one
pub fn press_label(g: &Graph, state: &str, event: &str) -> Option<String> {
    for (_, n) in g.ui().nodes() {
        if n.get("emits").and_then(Value::as_str) != Some(event) || !n.in_states().contains(&state) {
            continue;
        }
        if let Some(key) = n.text().and_then(|t| t.0.get("key")) {
            return Some(plain(g, &js::to_js_string(key)));
        }
    }
    None
}

pub fn wrap(text: &str, max: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && js::len16(&line) + 1 + js::len16(word) > max {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else if line.is_empty() {
            line = word.to_string();
        } else {
            line.push(' ');
            line.push_str(word);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

struct Group {
    from: String,
    to: String,
    labels: Vec<String>,
    kinds: Vec<EdgeKind>,
    used: bool,
}

pub fn collect_edges(g: &Graph, ex: &Exploration) -> Vec<VizEdge> {
    let mut groups: IndexMap<String, Group> = IndexMap::new();
    let mut add = |from: &str, to: &str, label: Option<String>, kind: EdgeKind, used: bool| {
        let grp = groups.entry(format!("{from}>{to}")).or_insert_with(|| Group {
            from: from.to_string(),
            to: to.to_string(),
            labels: Vec::new(),
            kinds: Vec::new(),
            used: false,
        });
        if let Some(label) = label
            && !grp.labels.contains(&label)
        {
            grp.labels.push(label);
        }
        if !grp.kinds.contains(&kind) {
            grp.kinds.push(kind);
        }
        grp.used = grp.used || used;
    };
    let field = |a: &Transition<'_>, k: &str| -> Option<Value> {
        match a {
            Transition::Full(v) => v.get(k).cloned(),
            Transition::Target(_) => None,
        }
    };
    for (name, st) in g.flow().states() {
        for (i, a) in st.always().into_iter().enumerate() {
            let label = [field(&a, "say"), field(&a, "guard")]
                .into_iter()
                .flatten()
                .find(|v| !v.is_null())
                .map_or_else(|| "otherwise".to_string(), |v| js::to_js_string(&v));
            add(
                name,
                &a.target_text(),
                Some(label),
                EdgeKind::Route,
                ex.taken.contains(&*format!("{name}|always|{i}")),
            );
        }
        for (event, spec) in st.on() {
            let press = press_label(g, name, event);
            for (i, a) in alternatives(spec).into_iter().enumerate() {
                let say = field(&a, "say");
                let (label, kind) = if let Some(press) = &press {
                    (Some(format!("“{press}”")), EdgeKind::User)
                } else if event.starts_with("FAIL") {
                    let label = match &say {
                        Some(s) => format!("fails: {}", js::to_js_string(s)),
                        None => humanize(event),
                    };
                    (Some(label), EdgeKind::Fail)
                } else {
                    (say.filter(|s| !s.is_null()).map(|s| js::to_js_string(&s)), EdgeKind::System)
                };
                let target = a.target_text();
                let used = ex.taken.contains(&*format!("{name}|{event}|{i}"));
                add(name, &target, label, kind, used);
                if let Some(starts) = a.starts() {
                    let flow = starts.get("flow").map_or_else(|| "undefined".to_string(), js::to_js_string);
                    add(name, &target, Some(format!("starts {flow}")), kind, used);
                }
            }
        }
    }
    let order = [EdgeKind::User, EdgeKind::System, EdgeKind::Route, EdgeKind::Fail];
    groups
        .into_values()
        .map(|grp| VizEdge {
            kind: order.into_iter().find(|k| grp.kinds.contains(k)).unwrap_or(EdgeKind::System),
            leave: g.flow().state(&grp.to).is_some_and(|s| s.leaves_page()),
            unused: !grp.used,
            from: grp.from,
            to: grp.to,
            labels: grp.labels,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_wrap_at_words() {
        // node -e "import('./dist/edges.js').then(e => console.log(JSON.stringify(e.wrap('a bb ccc dddd eeeee', 6))))"
        assert_eq!(wrap("a bb ccc dddd eeeee", 6), ["a bb", "ccc", "dddd", "eeeee"]);
        assert_eq!(wrap("", 6), Vec::<String>::new());
        assert_eq!(wrap("çok güzel şey", 5), ["çok", "güzel", "şey"]);
    }

    #[test]
    fn a_response_in_one_line() {
        // node -e "import('./dist/edges.js').then(e => console.log(e.responseLine({ body: { a: 1 }, redirect: '/x', status: 302, note: 'n' })))"
        let mut r = Object::new();
        r.insert("body", Value::Object(Object::from_iter([("a", Value::Number(1.0))])));
        r.insert("redirect", Value::from("/x"));
        r.insert("status", Value::Number(302.0));
        r.insert("note", Value::from("n"));
        assert_eq!(response_line(&r), "302 · redirect /x · body: {\"a\":1} · note: n");
    }
}
