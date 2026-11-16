//! `starts:` on a transition: this flow starts another one and does not wait for it (SPEC.md §2, "Several flows").
//! The started flow is looked up by id among the graphs beside this one. When it is found, every situation in which the
//! transition is taken must give it a beginning it accepts; when it is not written yet, that is a note, not a failure.
//! The port of starts.ts.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::error::Result;
use crate::graph::Graph;
use crate::graph::{Transition, alternatives, given};
use crate::js::{self, Object, Value};
use crate::model::{Exploration, Model, Scalar};
use crate::nodepath;
use crate::yaml::read_yaml;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HandOver {
    /// the state the transition leaves
    pub from: String,
    /// None for an `always` rule
    pub event: Option<String>,
    pub flow: String,
    pub given: Value,
    pub found: bool,
    /// what the started flow cannot begin with; empty when it fits
    pub problems: Vec<String>,
}

const SKIP: [&str; 4] = ["viz", "video", "node_modules", ".git"];

// the root to search: the nearest folder called `graphs` above the graph, else the graph's parent
fn root_of(dir: &str) -> String {
    let mut d = dir.to_string();
    for _ in 0..5 {
        if nodepath::basename(&d) == "graphs" {
            return d;
        }
        d = nodepath::dirname(&d);
    }
    nodepath::dirname(dir)
}

// every flow under root by its id; a flow.yaml that cannot be read is left out, as its own check reports it
fn flows_under(root: &str, depth: usize, out: &mut HashMap<String, Value>) {
    let file = nodepath::join2(root, "flow.yaml");
    if Path::new(&file).exists() {
        // a flow.yaml that is no file (the TS crashed on it) is left out too
        let f = std::fs::read(&file).ok().and_then(|b| read_yaml(&String::from_utf8_lossy(&b)).ok());
        if let Some(mut f) = f
            && let Some(id) = f.get_str("id").map(str::to_string)
        {
            if let Some(o) = f.as_object_mut()
                && o.get("context").is_none_or(Value::is_null)
            {
                o.insert("context", Value::Object(Object::new()));
            }
            out.insert(id, f);
        }
    }
    if depth == 0 {
        return;
    }
    // readdirSync's order: by bytes; a folder that cannot be read, or an entry that cannot be stat'ed, is passed over
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let mut names: Vec<String> =
        entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for name in names {
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let sub = nodepath::join2(root, &name);
        if std::fs::metadata(&sub).is_ok_and(|m| m.is_dir()) {
            flows_under(&sub, depth - 1, out);
        }
    }
}

pub fn check_starts(g: &Graph, m: &Model, ex: &Exploration) -> Result<Vec<HandOver>> {
    let mut out: Vec<HandOver> = Vec::new();
    let mut flows: Option<HashMap<String, Value>> = None;
    for (name, st) in g.flow().states() {
        let mut ways: Vec<(Option<&str>, usize, Transition<'_>)> =
            st.always().into_iter().enumerate().map(|(i, t)| (None, i, t)).collect();
        for (event, spec) in st.on() {
            ways.extend(alternatives(spec).into_iter().enumerate().map(|(i, t)| (Some(event), i, t)));
        }
        for (event, i, t) in ways {
            let Some(starts) = t.starts() else { continue };
            let flows = flows.get_or_insert_with(|| {
                let mut f = HashMap::new();
                flows_under(&root_of(&g.dir), 4, &mut f);
                f
            });
            let given_v = given(starts, "given").cloned().unwrap_or_else(|| Value::Object(Object::new()));
            let flow = starts.get("flow").map_or_else(|| "undefined".to_string(), js::to_js_string);
            // the flows are found by their ids, which are strings: a flow named by anything else is none of them
            let started = starts.get("flow").and_then(Value::as_str).and_then(|f| flows.get(f));
            let mut hand = HandOver {
                from: name.to_string(),
                event: event.map(str::to_string),
                flow,
                given: given_v.clone(),
                found: started.is_some(),
                problems: Vec::new(),
            };
            let Some(started) = started else {
                out.push(hand);
                continue;
            };
            let problem = |hand: &mut HandOver, p: String| {
                if !hand.problems.contains(&p) {
                    hand.problems.push(p);
                }
            };
            let started_id = started.get("id").map_or_else(|| "undefined".to_string(), js::to_js_string);
            let context = started.get_object("context");
            let entries: Vec<(&String, &Value)> = given_v.as_object().map(|o| o.iter().collect()).unwrap_or_default();
            for (k, _) in &entries {
                if context.is_none_or(|c| !c.contains_key(k.as_str())) {
                    problem(&mut hand, format!("{k} is not a variable of {started_id}"));
                }
            }
            let taken = format!("{name}|{}|{i}", event.unwrap_or("always"));
            let mut ctx = Vec::new();
            for c in 0..ex.len() {
                if ex.state(c) != name {
                    continue;
                }
                ex.ctx_into(c, &mut ctx);
                for s in m.steps(name, &ctx)? {
                    if *s.taken != *taken {
                        continue;
                    }
                    for (k, expr) in &entries {
                        let Some(cv) = context.and_then(|c| c.get(k.as_str())) else { continue };
                        let v = match expr {
                            Value::String(e) => m.scalar_value(e, name, &s.ctx)?,
                            other => Scalar::of(other),
                        };
                        let values = cv.get_array("values").map(Vec::as_slice).unwrap_or(&[]);
                        let initial = cv.get("initial");
                        if !values.iter().any(|x| v.same_value_zero(x)) {
                            // JSON.stringify(undefined) is no text, and the template literal writes it as undefined
                            let shown =
                                if matches!(v, Scalar::Undefined) { v.text() } else { js::stringify(&v.to_value()) };
                            problem(
                                &mut hand,
                                format!("{k} would be {shown}, which is not a value of {started_id}'s {k}"),
                            );
                        } else if initial.and_then(Value::as_str) != Some("any")
                            && !initial.map_or(matches!(v, Scalar::Undefined), |x| v.is(x))
                        {
                            let begins = initial.map_or_else(|| "undefined".to_string(), js::to_js_string);
                            problem(
                                &mut hand,
                                format!("{started_id} begins with {k} = {begins}, and here it would be {}", v.text()),
                            );
                        }
                    }
                }
            }
            out.push(hand);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_the_nearest_graphs_folder() {
        assert_eq!(root_of("/a/graphs/x/y"), "/a/graphs");
        assert_eq!(root_of("/a/b/c"), "/a/b");
        assert_eq!(root_of("/a/graphs/1/2/3/4/5"), "/a/graphs/1/2/3/4");
    }
}
