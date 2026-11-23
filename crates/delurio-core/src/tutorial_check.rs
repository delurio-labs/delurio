//! A flow's tutorial.yaml held to the flow and its ui (SPEC.md §7). First every name a tutorial uses: the flow it is
//! written against, the variables and values of its `scenario` and of each step's `then`, the states of `show`, `cut`
//! and `end`, the nodes of `press`, `reveal` and `focus`, the events of `after`, and each `{word}` its words fill. Then,
//! when every name is one, its steps are replayed on the flow from the scenario as the video plays them, and the first
//! step that no longer holds is where it stops: a tutorial that no longer matches the graph stops at that step.
//! The port of tutorial-check.ts.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::edges::{NodeKind, kind_of};
use crate::error::{Error, Result};
use crate::graph::{Graph, alternatives, given, row_field, shown_in};
use crate::js::{self, Object, Value};
use crate::model::{Ctx, Exploration, Model, Scalar, Step};
use crate::yaml::read_yaml;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TutorialProblem {
    /// where in tutorial.yaml
    pub path: Vec<String>,
    /// where the YAML parser gave it, for a file that cannot be read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    pub message: String,
}

#[derive(Debug, Clone)]
struct Cursor {
    state: Rc<str>,
    ctx: Ctx,
}

/// `Array.isArray(v) ? v : v === undefined ? [] : [v]`
fn names(v: Option<&Value>) -> Vec<&Value> {
    match v {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
    }
}

fn shown(v: &Value) -> String {
    js::stringify(v)
}

/// `String(v)`, undefined when the key is not there
fn text_of(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), js::to_js_string)
}

/// SameValueZero, the equality of `includes`
fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a.strict_eq(b),
    }
}

/// a step that no longer holds, or an error of the model, which is the caller's
enum Fail {
    Stop(Vec<String>, String),
    Error(Error),
}

impl From<Error> for Fail {
    fn from(e: Error) -> Self {
        Fail::Error(e)
    }
}

fn problem(out: &mut Vec<TutorialProblem>, path: Vec<String>, message: String) {
    out.push(TutorialProblem { path, line: None, column: None, message });
}

fn stop<T>(path: Vec<String>, message: String) -> std::result::Result<T, Fail> {
    Err(Fail::Stop(path, message))
}

fn path_with(path: &[String], rest: &[&str]) -> Vec<String> {
    let mut p = path.to_vec();
    p.extend(rest.iter().map(|s| s.to_string()));
    p
}

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{([A-Za-z0-9_]+)\}").expect("the word pattern compiles"));

pub fn check_tutorial(g: &Graph, m: &Model, ex: &Exploration, text: &str) -> Result<Vec<TutorialProblem>> {
    let t = match read_yaml(text) {
        Ok(t) => t,
        Err(err) => {
            return Ok(vec![TutorialProblem {
                path: Vec::new(),
                line: Some(err.line_pos.map_or(1, |p| p.line)),
                column: Some(err.line_pos.map_or(1, |p| p.column)),
                message: format!("tutorial.yaml cannot be read: {}", err.first_line()),
            }]);
        }
    };
    let flow = g.flow();
    let one =
        |path: Vec<String>, message: String| Ok(vec![TutorialProblem { path, line: None, column: None, message }]);
    if !t.is_object() || t.get_str("kind") != Some("tutorial") {
        let path = if t.is_object() { vec!["kind".to_string()] } else { Vec::new() };
        return one(path, "tutorial.yaml is not a tutorial: it says kind: tutorial (SPEC.md §1, §7)".to_string());
    }
    let same_flow = match (t.get("flow"), flow.0.get("id")) {
        (None, None) => true,
        (Some(a), Some(b)) => a.strict_eq(b),
        _ => false,
    };
    if !same_flow {
        return one(
            vec!["flow".to_string()],
            format!(
                "the tutorial is written against {}, and the flow beside it is {} (SPEC.md §7)",
                text_of(t.get("flow")),
                flow.id_text()
            ),
        );
    }
    let stage = given(&t, "stage").cloned().unwrap_or_else(|| Value::from("screen"));
    let diagram = match stage.as_str() {
        Some("screen") => false,
        Some("diagram") => true,
        _ => {
            return one(
                vec!["stage".to_string()],
                format!("stage is screen or diagram, not {} (SPEC.md §7)", js::to_js_string(&stage)),
            );
        }
    };
    let steps: &[Value] = t.get_array("steps").map_or(&[][..], Vec::as_slice);

    let mut out: Vec<TutorialProblem> = Vec::new();
    let ui = g.ui();
    let events: HashSet<&str> = flow.states().flat_map(|(_, st)| st.on().into_iter().map(|(e, _)| e)).collect();
    let mut samples: HashSet<&str> = ui.samples().into_iter().flat_map(|o| o.keys().map(String::as_str)).collect();
    samples.extend(t.get_object("samples").into_iter().flat_map(|o| o.keys().map(String::as_str)));
    let flow_id = flow.id_text();
    let values = |out: &mut Vec<TutorialProblem>, at: Vec<String>, given: Option<&Value>, what: &str| {
        let Some(given) = given else { return };
        let Some(o) = given.as_object() else {
            problem(out, at, format!("{what} is a map of variables to values"));
            return;
        };
        for (name, v) in o.iter() {
            match flow.context_var(name) {
                None => problem(
                    out,
                    path_with(&at, &[name]),
                    format!("{what} names {name}, which is not a variable of {flow_id}"),
                ),
                Some(cv) => {
                    let values = cv.values();
                    if !values.iter().any(|x| same_value_zero(x, v)) {
                        let all: Vec<String> = values.iter().map(|x| shown(x)).collect();
                        problem(
                            out,
                            path_with(&at, &[name]),
                            format!("{what}: {} is not a value of {name} ({})", shown(v), all.join(", ")),
                        );
                    }
                }
            }
        }
    };
    // the start: a value of the scenario's for each variable that starts from any value, and the fixed one of the others
    values(&mut out, vec!["scenario".to_string()], t.get("scenario"), "scenario");
    let empty = Object::new();
    let scenario = t.get_object("scenario").unwrap_or(&empty);
    for v in &m.vars {
        let Some(cv) = flow.context_var(v) else { continue };
        let initial = cv.initial();
        let any = initial.and_then(Value::as_str) == Some("any");
        let given = scenario.get(v);
        if any && given.is_none() {
            problem(
                &mut out,
                vec!["scenario".to_string()],
                format!("scenario gives no value for {v}, and the flow starts from any value of it"),
            );
        } else if let Some(given) = given
            && !any
            && !initial.is_some_and(|i| i.strict_eq(given))
        {
            problem(
                &mut out,
                vec!["scenario".to_string(), v.clone()],
                format!(
                    "scenario gives {v} = {}, and {flow_id} always starts with {v} = {}",
                    shown(given),
                    initial.map_or_else(|| "undefined".to_string(), shown)
                ),
            );
        }
    }
    let words = |out: &mut Vec<TutorialProblem>, at: Vec<String>, s: Option<&Value>, step: &str| {
        let Some(Value::String(s)) = s else { return };
        for c in WORD.captures_iter(s) {
            let name = &c[1];
            if samples.contains(name) {
                continue;
            }
            match ui.node(name) {
                None => problem(
                    out,
                    at.clone(),
                    format!("{step}{{{name}}} is neither a sample value nor a node of ui.yaml"),
                ),
                Some(n) if n.get("text").is_none() => problem(
                    out,
                    at.clone(),
                    format!("{step}{{{name}}} names the node {name}, which has no visible text to say"),
                ),
                Some(_) => {}
            }
        }
    };
    words(&mut out, vec!["title".to_string()], t.get("title"), "title: ");
    for (i, s) in steps.iter().enumerate() {
        if !s.is_object() {
            continue;
        }
        let at = |rest: &[&str]| -> Vec<String> {
            let mut p = vec!["steps".to_string(), i.to_string()];
            p.extend(rest.iter().map(|x| x.to_string()));
            p
        };
        let step = format!("step {}", i + 1);
        for key in ["show", "cut", "end"] {
            if let Some(Value::String(v)) = s.get(key)
                && !flow.has_state(v)
            {
                problem(&mut out, at(&[key]), format!("{step}: {key} {v}, which is not a state of {flow_id}"));
            }
        }
        for key in ["press", "reveal", "focus"] {
            let listed = s.get(key).is_some_and(Value::is_array);
            for (j, id) in names(s.get(key)).into_iter().enumerate() {
                let where_ = if listed { at(&[key, &j.to_string()]) } else { at(&[key]) };
                if diagram {
                    problem(
                        &mut out,
                        where_,
                        format!("{step}: {key} points at a node, and a tutorial on the diagram has none (SPEC.md §7)"),
                    );
                } else if !id.as_str().is_some_and(|id| ui.node(id).is_some()) {
                    problem(
                        &mut out,
                        where_,
                        format!("{step}: {key} {}, which is not a node of ui.yaml", js::to_js_string(id)),
                    );
                }
            }
        }
        if let Some(row) = s.get("row")
            && let Some(message) = row_problem(g, s, row, diagram)
        {
            problem(&mut out, at(&["row"]), format!("{step}: {message}"));
        }
        let listed = s.get("after").is_some_and(Value::is_array);
        for (j, e) in names(s.get("after")).into_iter().enumerate() {
            if !e.as_str().is_some_and(|e| events.contains(e)) {
                let where_ = if listed { at(&["after", &j.to_string()]) } else { at(&["after"]) };
                problem(
                    &mut out,
                    where_,
                    format!("{step}: after {}, which no state of {flow_id} handles", js::to_js_string(e)),
                );
            }
        }
        values(&mut out, at(&["then"]), s.get("then"), &format!("{step}: then"));
        words(&mut out, at(&["say"]), s.get("say"), &format!("{step}: "));
        words(&mut out, at(&["chapter"]), s.get("chapter"), &format!("{step}: "));
        if let Some(card) = s.get("card").filter(|c| c.is_object()) {
            words(&mut out, at(&["card", "title"]), card.get("title"), &format!("{step}: "));
            words(&mut out, at(&["card", "body"]), card.get("body"), &format!("{step}: "));
        }
    }
    if !out.is_empty() {
        return Ok(out);
    }

    let replay = Replay { g, m, ex, then: Vec::new() };
    let done = if diagram { replay.diagram(steps, scenario) } else { replay.screens(steps, scenario) };
    match done {
        Ok(()) => Ok(Vec::new()),
        Err(Fail::Stop(path, message)) => Ok(vec![TutorialProblem { path, line: None, column: None, message }]),
        Err(Fail::Error(e)) => Err(e),
    }
}

/// a step's `row`, the row of the pressed node's sample rows that the step presses (SPEC.md §7): what is wrong with it,
/// when something is; the node itself is looked for with `press`
fn row_problem(g: &Graph, s: &Value, row: &Value, diagram: bool) -> Option<String> {
    let n = row.as_f64().filter(|n| n.fract() == 0.0 && *n >= 1.0);
    let Some(n) = n else {
        return Some(format!("row is the number of a row, from 1, not {}", shown(row)));
    };
    let Some(press) = s.get_str("press") else {
        return Some("row says which row of the pressed node is pressed, and the step presses none".to_string());
    };
    let node = g.ui().node(press).filter(|_| !diagram)?;
    let Some(repeat) = node.repeat() else {
        return Some(format!("press {press} with row {n}, and {press} is not drawn once per item (repeat)"));
    };
    let Some(rows) = g.ui().rows(repeat) else {
        return Some(format!(
            "press {press} with row {n}, and the samples give no rows for its data, {repeat} (SPEC.md §4)"
        ));
    };
    if n as usize > rows.len() {
        let count = if rows.len() == 1 { "1 row".to_string() } else { format!("{} rows", rows.len()) };
        return Some(format!("press {press} with row {n}, and the samples give {count} for {repeat}"));
    }
    None
}

/// a press with a row: the node pressed, the row's number from 1, the row's fields, and where the step gives it
struct Pressed<'v> {
    node: &'v str,
    n: usize,
    row: &'v Object,
    at: Vec<String>,
}

struct Replay<'a> {
    g: &'a Graph,
    m: &'a Model,
    ex: &'a Exploration,
    /// the step's `then`: the variables' positions (None for a name that is not one) and the values
    then: Vec<(Option<usize>, Value)>,
}

impl Replay<'_> {
    fn start(&self, scenario: &Object) -> Result<Cursor> {
        let m = self.m;
        let ctx = (0..m.vars.len())
            .map(|i| match m.initial_code(i) {
                Some(c) => Ok(c),
                None => m.code_of(i, &Scalar::of_option(scenario.get(&m.vars[i]))),
            })
            .collect::<Result<Ctx>>()?;
        Ok(Cursor { state: Rc::from(self.g.flow().initial().unwrap_or("undefined")), ctx })
    }

    // the step's `then`: of the cases its transitions can take, the one with these values; without it, the first
    fn then_of(m: &Model, s: &Value) -> Vec<(Option<usize>, Value)> {
        s.get_object("then")
            .map(|o| o.iter().map(|(name, v)| (m.vars.iter().position(|x| x == name), v.clone())).collect())
            .unwrap_or_default()
    }

    fn pick<'s>(&self, cases: Vec<&'s Step>) -> Option<&'s Step> {
        let fits = |c: &Step| self.then.iter().all(|(i, v)| i.is_some_and(|i| self.m.holds(&c.ctx, i, v)));
        cases.iter().copied().find(|c| fits(c)).or(cases.first().copied())
    }

    fn then_holds(&self, cur: &Cursor, path: &[String], step: &str) -> std::result::Result<(), Fail> {
        for (i, v) in &self.then {
            let holds = i.is_some_and(|i| self.m.holds(&cur.ctx, i, v));
            if !holds {
                let name = i.map_or_else(|| "undefined".to_string(), |i| self.m.vars[i].clone());
                return stop(
                    path_with(path, &["then", &name]),
                    format!("{step}: no case of this step has {name} = {}", shown(v)),
                );
            }
        }
        Ok(())
    }

    // `next`: the event the step gives after this one (SPEC.md §7: a failure taken at a server step the step passes)
    fn take(
        &self,
        cur: &Cursor,
        event: &Value,
        next: Option<&Value>,
        path: &[String],
        step: &str,
    ) -> std::result::Result<Cursor, Fail> {
        self.take_row(cur, event, None, next, path, step)
    }

    // a press with a row takes the case in which each variable its effects set from the row (`row.<field>`) has the
    // row's value, as the row is what was pressed (SPEC.md §4, §7)
    fn take_row(
        &self,
        cur: &Cursor,
        event: &Value,
        pressed: Option<&Pressed<'_>>,
        next: Option<&Value>,
        path: &[String],
        step: &str,
    ) -> std::result::Result<Cursor, Fail> {
        let steps = self.m.steps(&cur.state, &cur.ctx)?;
        let mut cases: Vec<&Step> =
            steps.iter().filter(|x| x.event.as_deref().is_some_and(|e| Some(e) == event.as_str())).collect();
        if let (Some(p), Some(first)) = (pressed, cases.first()) {
            for (var, value) in self.set_from_row(first, p, step)? {
                cases.retain(|c| self.m.holds(&c.ctx, var, &value));
                if cases.is_empty() {
                    let name = &self.m.vars[var];
                    let values = self.g.flow().context_var(name).map(|cv| cv.values()).unwrap_or_default();
                    let not_one = !values.iter().any(|x| same_value_zero(x, &value));
                    let why = if not_one {
                        let all: Vec<String> = values.iter().map(|x| shown(x)).collect();
                        format!("which is not a value of {name} ({})", all.join(", "))
                    } else {
                        "and no case of this press has it".to_string()
                    };
                    return stop(
                        p.at.clone(),
                        format!("{step}: row {} of {} gives {name} = {}, {why}", p.n, p.node, shown(&value)),
                    );
                }
            }
        }
        let hit = self.pick(cases);
        let Some(hit) = hit else {
            return stop(
                path.to_vec(),
                format!("{step}: the flow is in {}, which does not handle {}", cur.state, js::to_js_string(event)),
            );
        };
        self.settle_for(Cursor { state: hit.target.clone(), ctx: hit.ctx.clone() }, next, path, step)
    }

    /// the variables a step's effects set from the pressed row, each with the row's value: read from the transition
    /// the step takes (`<state>|<event>|<alternative>`)
    fn set_from_row(
        &self,
        taken: &Step,
        p: &Pressed<'_>,
        step: &str,
    ) -> std::result::Result<Vec<(usize, Value)>, Fail> {
        let flow = self.g.flow();
        let mut parts = taken.taken.rsplitn(3, '|');
        let (alt, event, state) = (parts.next(), parts.next(), parts.next());
        let way = state
            .and_then(|s| flow.state(s))
            .and_then(|st| st.on_event(event.unwrap_or("")))
            .and_then(|spec| alternatives(spec).get(alt.and_then(|a| a.parse::<usize>().ok())?).copied());
        let mut out: Vec<(usize, Value)> = Vec::new();
        for name in way.map(|w| w.effect()).unwrap_or_default() {
            for (var, v) in flow.effect(name).and_then(|e| e.set()).into_iter().flat_map(|o| o.iter()) {
                let Some(field) = v.as_str().and_then(row_field) else { continue };
                let Some(i) = self.m.vars.iter().position(|x| x == var) else { continue };
                let Some(value) = p.row.get(field) else {
                    return stop(
                        p.at.clone(),
                        format!(
                            "{step}: row {} of {} has no {field}, which the effect {name} reads (row.{field})",
                            p.n, p.node
                        ),
                    );
                };
                out.retain(|(j, _)| *j != i);
                out.push((i, value.clone()));
            }
        }
        Ok(out)
    }

    // a step with no screen passes on its own along its normal way (a transition with no event, then DONE); one with no
    // such way waits for an event the tutorial gives with `after`
    // `wait_for`: an event the step gives next; a server step that handles it otherwise than by its normal way is where
    // the walk waits for it (settle_for)
    fn settle(
        &self,
        cur: Cursor,
        wait_for: Option<&str>,
        path: &[String],
        step: &str,
    ) -> std::result::Result<Cursor, Fail> {
        let mut c = cur;
        let mut hops = 0;
        while hops < 50 && kind_of(self.g, &c.state) == NodeKind::Server {
            let ways = self.m.steps(&c.state, &c.ctx)?;
            if let Some(e) = wait_for
                && e != "DONE"
                && ways.iter().any(|x| x.event.as_deref() == Some(e))
            {
                return Ok(c);
            }
            let next = self.pick(ways.iter().filter(|x| x.event.as_deref().is_none_or(|e| e == "DONE")).collect());
            match next {
                None if !ways.is_empty() => return Ok(c),
                None => {
                    return stop(
                        path.to_vec(),
                        format!("{step}: the flow stops in {}, which has no screen and no way on", c.state),
                    );
                }
                Some(next) => c = Cursor { state: next.target.clone(), ctx: next.ctx.clone() },
            }
            hops += 1;
        }
        Ok(c)
    }

    fn handles(&self, c: &Cursor, event: &str) -> std::result::Result<bool, Fail> {
        Ok(self.m.steps(&c.state, &c.ctx)?.iter().any(|x| x.event.as_deref() == Some(event)))
    }

    // Where the event the step gives next (`next`) is one the screen the server steps lead to does not handle, but one
    // of those server steps does, another way than its normal one (a failure, a refusal), the walk stops at that server
    // step and the event is taken there, so a tutorial can show the screen a failure leads to; a way the screen handles
    // is taken as before (SPEC.md §7)
    fn settle_for(
        &self,
        from: Cursor,
        next: Option<&Value>,
        path: &[String],
        step: &str,
    ) -> std::result::Result<Cursor, Fail> {
        let end = self.settle(from.clone(), None, path, step)?;
        if let Some(event) = next.and_then(Value::as_str)
            && !self.handles(&end, event)?
        {
            let at = self.settle(from, Some(event), path, step)?;
            if self.handles(&at, event)? {
                return Ok(at);
            }
        }
        Ok(end)
    }

    fn screens(mut self, steps: &[Value], scenario: &Object) -> std::result::Result<(), Fail> {
        let g = self.g;
        let ui = g.ui();
        let flow = g.flow();
        let has_screen = |state: &str| ui.nodes().any(|(_, n)| shown_in(n, state));
        let node_get = |id: &str, k: &str| ui.node(id).and_then(|n| n.get(k).cloned());
        let start = self.start(scenario)?;
        // an opening card's first event may come before the first screen: a failure at a server step the start passes
        let opening = steps
            .first()
            .filter(|s| s.get("intro") == Some(&Value::Bool(true)))
            .and_then(|s| names(s.get("after")).first().copied());
        let mut cur = self.settle_for(start, opening, &["scenario".to_string()], "the start")?;
        for (i, s) in steps.iter().enumerate() {
            if !s.is_object() {
                continue;
            }
            let path = vec!["steps".to_string(), i.to_string()];
            let step = format!("step {}", i + 1);
            self.then = Replay::then_of(self.m, s);
            let listed = |key: &str| s.get(key).is_some_and(Value::is_array);
            if s.get("intro") == Some(&Value::Bool(true)) {
                let events = names(s.get("after"));
                for (j, e) in events.iter().enumerate() {
                    let where_ = if listed("after") {
                        path_with(&path, &["after", &j.to_string()])
                    } else {
                        path_with(&path, &["after"])
                    };
                    cur = self.take(&cur, e, events.get(j + 1).copied(), &where_, &step)?;
                }
                self.then_holds(&cur, &path, &step)?;
                continue;
            }
            let mut reveal: Vec<String> = names(s.get("reveal")).into_iter().map(js::to_js_string).collect();
            let press = s.get_str("press");
            if let Some(press) = press
                && node_get(press, "when").is_some()
                && !reveal.iter().any(|r| r == press)
            {
                reveal.push(press.to_string());
            }
            // the state whose screen the step shows; None for a card
            let mut state: Option<Rc<str>> = Some(cur.state.clone());
            let cut = s.get_str("cut");
            let show = s.get_str("show");
            if let Some(wanted) = cut {
                let mut hit = None;
                let mut ctx = Vec::new();
                for i in 0..self.ex.len() {
                    if self.ex.state(i) != wanted {
                        continue;
                    }
                    self.ex.ctx_into(i, &mut ctx);
                    let mut all = true;
                    for id in &reveal {
                        let when = node_get(id, "when");
                        if when.is_none() {
                            continue;
                        }
                        match self.m.test_value(when.as_ref(), wanted, &ctx) {
                            Ok(true) => {}
                            Ok(false) => {
                                all = false;
                                break;
                            }
                            Err(err) => return stop(path_with(&path, &["reveal"]), format!("{step}: {}", err.message)),
                        }
                    }
                    if all {
                        hit = self.ex.config(i);
                        break;
                    }
                }
                let Some(hit) = hit else {
                    let shows =
                        if reveal.is_empty() { String::new() } else { format!(" and shows {}", reveal.join(", ")) };
                    return stop(
                        path_with(&path, &["cut"]),
                        format!("{step}: no situation the flow reaches is in {wanted}{shows}"),
                    );
                };
                cur = Cursor { state: hit.state, ctx: hit.ctx };
                state = Some(cur.state.clone());
            } else if let Some(end) = s.get("end") {
                if let Value::String(end) = end
                    && *end != *cur.state
                {
                    return stop(
                        path_with(&path, &["end"]),
                        format!("{step}: the tutorial ends in {end}, and the flow is in {} here", cur.state),
                    );
                }
                if flow.state(&cur.state).is_some_and(|st| st.is_final()) {
                    state = None;
                } else if !end.is_string() {
                    return stop(
                        path_with(&path, &["end"]),
                        format!("{step}: the flow is in {} here, which is no exit", cur.state),
                    );
                }
                if s.get("card").is_some_and(Value::is_object) {
                    state = None;
                }
            } else if let Some(show) = show {
                if show != &*cur.state {
                    return stop(
                        path_with(&path, &["show"]),
                        format!("{step}: shows {show}, and the flow is in {} here", cur.state),
                    );
                }
            } else if s.get("card").is_some() {
                state = None;
            }
            if let Some(st) = &state
                && !has_screen(st)
            {
                let key = if cut.is_some() {
                    "cut"
                } else if show.is_some() {
                    "show"
                } else {
                    "end"
                };
                return stop(path_with(&path, &[key]), format!("{step}: {st} has no screen to show"));
            }
            for key in ["reveal", "focus"] {
                for (j, id) in names(s.get(key)).into_iter().enumerate() {
                    let where_ =
                        if listed(key) { path_with(&path, &[key, &j.to_string()]) } else { path_with(&path, &[key]) };
                    let id_text = js::to_js_string(id);
                    let n = ui.node(&id_text);
                    let Some(st) = &state else {
                        return stop(where_, format!("{step}: {key} {id_text} on a card, which shows no node"));
                    };
                    if !n.is_some_and(|n| shown_in(n, st)) {
                        return stop(where_, format!("{step}: {key} {id_text}, which {st} does not show"));
                    }
                    if key == "focus"
                        && let Some(when) = n.and_then(|n| n.get("when"))
                        && !reveal.contains(&id_text)
                    {
                        return stop(
                            where_,
                            format!(
                                "{step}: {id_text} is shown only when {}: list it under reveal",
                                js::to_js_string(when)
                            ),
                        );
                    }
                }
            }
            if let Some(press) = press {
                let n = ui.node(press);
                let where_ = path_with(&path, &["press"]);
                if !n.is_some_and(|n| shown_in(n, &cur.state)) {
                    return stop(where_, format!("{step}: press {press}, which {} does not show", cur.state));
                }
                if n.is_some_and(|n| n.disabled_in().contains(&&*cur.state)) {
                    return stop(where_, format!("{step}: press {press}, which is disabled in {}", cur.state));
                }
                let Some(emits) = n.and_then(|n| n.get("emits")) else {
                    return stop(where_, format!("{step}: press {press}, which sends no event"));
                };
                // the row pressed, which the names were held to: the node's sample rows have it
                let rows = n.and_then(|n| n.repeat()).and_then(|r| ui.rows(r));
                let pressed = s.get("row").and_then(Value::as_f64).and_then(|r| {
                    let row = rows?.get((r as usize).checked_sub(1)?)?.as_object()?;
                    Some(Pressed { node: press, n: r as usize, row, at: path_with(&path, &["row"]) })
                });
                let next = names(s.get("after")).first().copied();
                cur = self.take_row(&cur, emits, pressed.as_ref(), next, &where_, &step)?;
            }
            let events = names(s.get("after"));
            for (j, e) in events.iter().enumerate() {
                let where_ = if listed("after") {
                    path_with(&path, &["after", &j.to_string()])
                } else {
                    path_with(&path, &["after"])
                };
                cur = self.take(&cur, e, events.get(j + 1).copied(), &where_, &step)?;
            }
            self.then_holds(&cur, &path, &step)?;
        }
        Ok(())
    }

    // On the diagram no state passes on its own: `show` and `end` walk the flow to their state along its normal way (a
    // transition with no event, then DONE), and every other event is given with `after`
    fn diagram(mut self, steps: &[Value], scenario: &Object) -> std::result::Result<(), Fail> {
        let flow = self.g.flow();
        let mut cur = self.start(scenario)?;
        for (i, s) in steps.iter().enumerate() {
            if !s.is_object() {
                continue;
            }
            let path = vec!["steps".to_string(), i.to_string()];
            let step = format!("step {}", i + 1);
            self.then = Replay::then_of(self.m, s);
            if s.get("intro") == Some(&Value::Bool(true)) {
                continue;
            }
            if let Some(wanted) = s.get_str("cut") {
                let Some(hit) =
                    (0..self.ex.len()).find(|&i| self.ex.state(i) == wanted).and_then(|i| self.ex.config(i))
                else {
                    return stop(
                        path_with(&path, &["cut"]),
                        format!("{step}: no situation the flow reaches is in {wanted}"),
                    );
                };
                cur = Cursor { state: hit.state, ctx: hit.ctx };
            } else if let Some(end) = s.get_str("end") {
                cur = self.walk_to(cur, end, &path_with(&path, &["end"]), &step)?;
                if !flow.state(end).is_some_and(|st| st.is_final()) {
                    return stop(path_with(&path, &["end"]), format!("{step}: ends in {end}, which is no exit"));
                }
            } else if let Some(show) = s.get_str("show") {
                cur = self.walk_to(cur, show, &path_with(&path, &["show"]), &step)?;
            }
            let listed = s.get("after").is_some_and(Value::is_array);
            for (j, e) in names(s.get("after")).into_iter().enumerate() {
                let where_ =
                    if listed { path_with(&path, &["after", &j.to_string()]) } else { path_with(&path, &["after"]) };
                let steps = self.m.steps(&cur.state, &cur.ctx)?;
                let hit = self.pick(
                    steps.iter().filter(|x| x.event.as_deref().is_some_and(|ev| Some(ev) == e.as_str())).collect(),
                );
                let Some(hit) = hit else {
                    return stop(
                        where_,
                        format!("{step}: the flow is in {}, which does not handle {}", cur.state, js::to_js_string(e)),
                    );
                };
                cur = Cursor { state: hit.target.clone(), ctx: hit.ctx.clone() };
            }
            self.then_holds(&cur, &path, &step)?;
        }
        Ok(())
    }

    fn walk_to(&self, cur: Cursor, wanted: &str, path: &[String], step: &str) -> std::result::Result<Cursor, Fail> {
        let from = cur.state.clone();
        let mut cur = cur;
        let mut hops = 0;
        while &*cur.state != wanted {
            let next = if hops < 50 {
                let steps = self.m.steps(&cur.state, &cur.ctx)?;
                self.pick(steps.iter().filter(|x| x.event.as_deref().is_none_or(|e| e == "DONE")).collect())
                    .map(|n| Cursor { state: n.target.clone(), ctx: n.ctx.clone() })
            } else {
                None
            };
            let Some(next) = next else {
                let stops =
                    if cur.state == from { String::new() } else { format!(" (the way stops in {})", cur.state) };
                return stop(
                    path.to_vec(),
                    format!("{step}: {wanted} is not reached from {from} without an event{stops}"),
                );
            };
            cur = next;
            hops += 1;
        }
        Ok(cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::explore;
    use crate::wireframe::tests::exam;

    fn said(g: &Graph, steps: &str) -> Vec<String> {
        let m = Model::new(g.flow.clone());
        let ex = explore(&m, None, None).expect("explored");
        let text = format!("schema: delurio-graph/0\nkind: tutorial\nflow: exam.take\ntitle: Answer\nsteps:\n{steps}");
        let problems = check_tutorial(g, &m, &ex, &text).expect("checked");
        problems.into_iter().map(|p| format!("{}: {}", p.path.join("."), p.message)).collect()
    }

    const PRESS: &str =
        "  - { show: answering }\n  - { press: option, row: ROW }\n  - { press: finish }\n  - { end: done }\n";

    // a page that loads its data, then a message sent: two server steps, each with a failure beside its normal way
    const SEND_FLOW: &str = "schema: delurio-graph/0
kind: flow
id: demo.send
status: proposed
title: Send a message
initial: loading
states:
  loading: { title: Loading, on: { DONE: writing, FAIL.network: offline } }
  offline: { title: Offline, on: { RELOAD: loading } }
  writing: { title: Writing, on: { SEND: sending } }
  sending: { title: Sending, on: { DONE: sent, FAIL.network: failed } }
  sent: { title: Sent, on: { NEW: writing } }
  failed: { title: Not sent, on: { RETRY: sending } }
";
    const SEND_UI: &str = "schema: delurio-graph/0
kind: ui
id: demo.send
flow: demo.send
status: proposed
catalog: catalog/en.json
locales: [en]
nodes:
  offlineText: { type: text, component: Text, role: status, text: { literal: You are offline }, in: [offline] }
  send: { type: action, component: Button, role: button, text: { literal: Send }, in: [writing], emits: SEND }
  sentText: { type: text, component: Text, role: status, text: { literal: Sent }, in: [sent] }
  failedText: { type: text, component: Text, role: alert, text: { literal: Not sent }, in: [failed] }
  retry: { type: action, component: Button, role: button, text: { literal: Try again }, in: [failed], emits: RETRY }
";

    fn said_on_send(steps: &str) -> Vec<String> {
        let read = |f: &str| -> crate::error::Result<String> {
            Ok(match f {
                "flow.yaml" => SEND_FLOW.to_string(),
                "ui.yaml" => SEND_UI.to_string(),
                _ => "{}".to_string(),
            })
        };
        let g = crate::graph::load_with("send", &read, &|f| f != "tutorial.yaml").expect("the flow loads");
        let m = Model::new(g.flow.clone());
        let ex = explore(&m, None, None).expect("explored");
        let text = format!("schema: delurio-graph/0\nkind: tutorial\nflow: demo.send\ntitle: Send\nsteps:\n{steps}");
        let problems = check_tutorial(&g, &m, &ex, &text).expect("checked");
        problems.into_iter().map(|p| format!("{}: {}", p.path.join("."), p.message)).collect()
    }

    // SPEC.md §7: a step's `after` may give a failure that a server step it passes takes, where the screen its normal
    // way leads to does not handle it
    #[test]
    fn a_failure_is_taken_at_the_server_step_the_step_passes() {
        let ok = Vec::<String>::new();
        // the normal way, as before
        assert_eq!(said_on_send("  - { show: writing, press: send }\n  - { show: sent }\n"), ok);
        // the failure, taken at sending, which the press passes on its way to sent
        assert_eq!(said_on_send("  - { show: writing, press: send, after: FAIL.network }\n  - { show: failed }\n"), ok);
        // and again from the screen the failure leads to
        let again = "  - { show: writing, press: send, after: FAIL.network }\n  - { show: failed, press: retry, after: [FAIL.network] }\n  - { show: failed }\n";
        assert_eq!(said_on_send(again), ok);
        // an opening card's failure comes before the first screen, at the server step the start passes
        assert_eq!(said_on_send("  - { intro: true, after: [FAIL.network] }\n  - { show: offline }\n"), ok);
        assert_eq!(said_on_send("  - { intro: true }\n  - { show: writing }\n"), ok);
    }

    #[test]
    fn a_failure_no_server_step_the_step_passes_handles_still_stops_it() {
        // an event no server step on the way handles: only failed does, which the press does not reach
        assert_eq!(
            said_on_send("  - { show: writing, press: send, after: RETRY }\n"),
            ["steps.0.after: step 1: the flow is in sent, which does not handle RETRY"]
        );
        // a step that passes no server step: writing is a screen, and does not handle it
        assert_eq!(
            said_on_send("  - { show: writing, after: FAIL.network }\n"),
            ["steps.0.after: step 1: the flow is in writing, which does not handle FAIL.network"]
        );
        // without an opening card, the start passes loading along its normal way
        assert_eq!(
            said_on_send("  - { show: offline }\n"),
            ["steps.0.show: step 1: shows offline, and the flow is in writing here"]
        );
    }

    #[test]
    fn a_press_with_a_row_takes_the_case_the_row_gives() {
        let g = exam("ListItem", &|u| u);
        let then =
            |row: &str, answer: &str| PRESS.replace("row: ROW", &format!("row: {row}, then: {{ answer: {answer} }}"));
        assert_eq!(said(&g, &then("2", "b")), Vec::<String>::new());
        assert_eq!(said(&g, &then("1", "a")), Vec::<String>::new());
        assert_eq!(said(&g, &then("2", "a")), ["steps.1.then.answer: step 2: no case of this step has answer = \"a\""]);
        // without a row, the first case, as before
        let first = PRESS.replace(", row: ROW", ", then: { answer: none }");
        assert_eq!(said(&g, &first), Vec::<String>::new());
    }

    #[test]
    fn a_row_is_held_to_the_rows_the_samples_give() {
        let g = exam("ListItem", &|u| u);
        let row = |r: &str| said(&g, &PRESS.replace("ROW", r));
        assert_eq!(
            row("3"),
            ["steps.1.row: step 2: press option with row 3, and the samples give 2 rows for question.options"]
        );
        assert_eq!(row("0"), ["steps.1.row: step 2: row is the number of a row, from 1, not 0"]);
        assert_eq!(row("'2'"), ["steps.1.row: step 2: row is the number of a row, from 1, not \"2\""]);
        let other = PRESS.replace("{ press: option, row: ROW }", "{ show: answering, row: 1 }");
        assert_eq!(
            said(&g, &other),
            ["steps.1.row: step 2: row says which row of the pressed node is pressed, and the step presses none"]
        );
        let finish = "  - { press: finish, row: 1 }\n";
        assert_eq!(
            said(&g, finish),
            ["steps.0.row: step 1: press finish with row 1, and finish is not drawn once per item (repeat)"]
        );
        let none = exam("ListItem", &|u| {
            u.replace("  question.options: [ { id: a, text: Eight }, { id: b, text: Six } ]\n", "  option: Any\n")
        });
        assert_eq!(
            said(&none, &PRESS.replace("ROW", "1")),
            [
                "steps.1.row: step 2: press option with row 1, and the samples give no rows for its data, question.options (SPEC.md §4)"
            ]
        );
        // a row whose value the variable does not have, and one without the field the effect reads
        let c = exam("ListItem", &|u| u.replace("{ id: b, text: Six }", "{ id: c, text: Six }"));
        assert_eq!(
            said(&c, &PRESS.replace("ROW", "2")),
            [
                "steps.1.row: step 2: row 2 of option gives answer = \"c\", which is not a value of answer (\"none\", \"a\", \"b\")"
            ]
        );
        let bare = exam("ListItem", &|u| u.replace("{ id: b, text: Six }", "{ text: Six }"));
        assert_eq!(
            said(&bare, &PRESS.replace("ROW", "2")),
            ["steps.1.row: step 2: row 2 of option has no id, which the effect choose reads (row.id)"]
        );
        assert_eq!(said(&bare, &PRESS.replace("ROW", "1")), Vec::<String>::new());
    }
}
