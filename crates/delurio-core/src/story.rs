//! A flow's stories: the steps to a situation the exploration reached, the main story the graph declares, and either
//! told in plain words. The port of story.ts.

use std::rc::Rc;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::edges::{NodeKind, kind_of, press_label};
use crate::error::Result;
use crate::graph::{Graph, Transition, alternatives, given, title_of};
use crate::js::{self, Value};
use crate::model::{Ctx, Exploration, Model, Shown, Step};

#[derive(Debug, Clone, PartialEq)]
pub struct PathStep {
    pub state: Rc<str>,
    pub ctx: Ctx,
    /// the event that led here
    pub event: Option<Rc<str>>,
    /// the transition that led here: `${state}|${event or "always"}|${alternative}`
    pub taken: Option<Rc<str>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Story {
    pub given: String,
    pub steps: Vec<String>,
    pub result: String,
}

pub fn steps_to(m: &Model, ex: &Exploration, index: usize) -> Result<Vec<PathStep>> {
    let mut chain: Vec<usize> = Vec::new();
    let mut i = Some(index);
    while let Some(k) = i.filter(|&k| k < ex.configs.len()) {
        chain.push(k);
        i = ex.configs[k].parent_index();
    }
    chain.reverse();
    let mut out = Vec::with_capacity(chain.len());
    for (k, &ci) in chain.iter().enumerate() {
        let c = &ex.configs[ci];
        let mut taken = None;
        if k > 0 {
            let prev = &ex.configs[chain[k - 1]];
            let joined = m.joined(&c.ctx);
            let hit = m
                .steps(&prev.state, &prev.ctx)?
                .into_iter()
                .find(|s| s.target == c.state && s.event == c.event && m.joined(&s.ctx) == joined);
            taken = hit.map(|s| s.taken);
        }
        out.push(PathStep { state: c.state.clone(), ctx: c.ctx.clone(), event: c.event.clone(), taken });
    }
    Ok(out)
}

// The main story the graph declares: from a starting situation, send the events in order (`always` rules run by themselves).
pub fn steps_by_events(m: &Model, main: &Value) -> Result<Option<Vec<PathStep>>> {
    // a main that is neither a list nor a map with events (the TS crashed on it) sends no event
    let (events, given): (&[Value], Option<&js::Object>) = match main {
        Value::Array(items) => (items, None),
        other => (other.get_array("events").map_or(&[][..], Vec::as_slice), other.get_object("given")),
    };
    let initial: Rc<str> = Rc::from(m.flow().initial().unwrap_or("undefined"));
    for init in m.initials() {
        let fits = given.into_iter().flat_map(|o| o.iter()).all(|(k, v)| match m.vars.iter().position(|x| x == k) {
            Some(i) => m.holds(&init, i, v),
            None => false,
        });
        if !fits {
            continue;
        }
        let mut path = vec![PathStep { state: initial.clone(), ctx: init, event: None, taken: None }];
        if walk(m, &mut path, events, 0)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

// Depth first, in document order, so the first walk that sends every event wins; when an effect leaves several
// situations (havoc), one that cannot go on gives way to the next. `path` is the walk when it is true.
fn walk(m: &Model, path: &mut Vec<PathStep>, events: &[Value], i: usize) -> Result<bool> {
    if path.len() > 500 {
        return Ok(false);
    }
    let Some(cur) = path.last() else { return Ok(false) };
    let steps = m.steps(&cur.state, &cur.ctx)?;
    let autos: Vec<&Step> = steps.iter().filter(|s| s.event.is_none()).collect();
    let next: Vec<&Step> = if !autos.is_empty() {
        autos.clone()
    } else if let Some(Value::String(e)) = events.get(i) {
        steps.iter().filter(|s| s.event.as_deref() == Some(e.as_str())).collect()
    } else {
        Vec::new()
    };
    if next.is_empty() {
        return Ok(i == events.len());
    }
    for s in next {
        path.push(PathStep {
            state: s.target.clone(),
            ctx: s.ctx.clone(),
            event: s.event.clone(),
            taken: Some(s.taken.clone()),
        });
        if walk(m, path, events, if autos.is_empty() { i + 1 } else { i })? {
            return Ok(true);
        }
        path.pop();
    }
    Ok(false)
}

// The first success exit reachable through at least one screen: the story a person expects to read first.
pub fn main_story_index(g: &Graph, ex: &Exploration) -> Option<usize> {
    let mut screen: std::collections::HashMap<&str, bool> = std::collections::HashMap::new();
    let mut via_screen: Vec<bool> = Vec::with_capacity(ex.configs.len());
    for c in &ex.configs {
        let here = *screen.entry(&c.state).or_insert_with(|| kind_of(g, &c.state) == NodeKind::Screen);
        let v = here || c.parent_index().is_some_and(|p| via_screen[p]);
        via_screen.push(v);
    }
    for (name, st) in g.flow().states() {
        if !st.is_final() || st.redirect() != Some("success") {
            continue;
        }
        if let Some(i) = ex.configs.iter().enumerate().position(|(idx, c)| &*c.state == name && via_screen[idx]) {
            return Some(i);
        }
    }
    None
}

fn say_of(g: &Graph, taken: Option<&str>) -> Option<String> {
    let taken = taken?;
    let mut parts = taken.split('|');
    let (state, event, idx) = (parts.next()?, parts.next()?, parts.next().unwrap_or(""));
    let st = g.flow().state(state)?;
    let n: Option<usize> = idx.parse().ok();
    let a = if event == "always" {
        st.0.get_array("always").and_then(|a| a.get(n?)).map(Transition::of)
    } else {
        // `.at(Number(idx))`: an index that is no number is the first
        let alts = st.on_event(event).map(alternatives).unwrap_or_default();
        alts.get(n.unwrap_or(0)).copied()
    };
    match a? {
        Transition::Full(v) => given(v, "say").map(js::to_js_string),
        Transition::Target(_) => None,
    }
}

/// `s.charAt(0).toUpperCase() + s.slice(1)`: the first UTF-16 unit, so a letter outside the BMP stays as it is
fn capital(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.len_utf16() == 1 => c.to_uppercase().chain(chars).collect(),
        _ => s.to_string(),
    }
}

pub fn narrate(g: &Graph, m: &Model, steps: &[PathStep]) -> Story {
    let flow = g.flow();
    let mut parts: Vec<String> = Vec::new();
    if let Some(start) = steps.first() {
        for (i, v) in m.vars.iter().enumerate() {
            let Some(cv) = flow.context_var(v) else { continue };
            if cv.initial().and_then(Value::as_str) != Some("any") {
                continue;
            }
            let value = start.ctx.get(i).map_or(crate::model::Scalar::Undefined, |&c| m.value_at(i, c));
            let text = value.text();
            match cv.say().and_then(|s| s.get(&text)) {
                None => parts.push(format!("{v} is {text}")),
                Some(Value::String(s)) if s.is_empty() => {}
                // `join` writes a null as nothing
                Some(Value::Null) => parts.push(String::new()),
                Some(said) => parts.push(js::to_js_string(said)),
            }
        }
    }
    let mut lines = Vec::new();
    for (k, s) in steps.iter().enumerate() {
        if flow.state(&s.state).is_some_and(|st| st.is_final()) {
            continue;
        }
        let mut line = title_of(flow, &s.state);
        if let Some(next) = steps.get(k + 1) {
            let say = say_of(g, next.taken.as_deref());
            let press = next.event.as_deref().and_then(|e| press_label(g, &s.state, e));
            if let Some(press) = press {
                line += &format!(": the user presses “{press}”");
            } else if let Some(say) = say {
                if next.event.as_deref().is_some_and(|e| e.starts_with("FAIL")) {
                    line += &format!(": it fails, because {say}");
                } else {
                    line += &format!(": {say}");
                }
            }
        }
        lines.push(line);
    }
    Story {
        given: if parts.is_empty() { "Any request".to_string() } else { capital(&parts.join(", ")) },
        steps: lines,
        result: steps.last().map_or_else(String::new, |last| title_of(flow, &last.state)),
    }
}

impl Serialize for Shown<'_, PathStep> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let x = self.value;
        let mut st = s.serialize_struct("PathStep", 4)?;
        st.serialize_field("state", &*x.state)?;
        st.serialize_field("ctx", &self.model.show(&x.ctx[..]))?;
        st.serialize_field("event", &x.event.as_deref())?;
        st.serialize_field("taken", &x.taken.as_deref())?;
        st.end()
    }
}

impl Serialize for Shown<'_, [PathStep]> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.value.iter().map(|x| self.model.show(x)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capital_as_javascript_writes_it() {
        // node -e "const c = (s) => s.charAt(0).toUpperCase() + s.slice(1); console.log(c('ßa'), c('ımm'), c('𐐨x'), c(''))"
        assert_eq!(capital("ßa"), "SSa");
        assert_eq!(capital("ımm"), "Imm");
        assert_eq!(capital("𐐨x"), "𐐨x");
        assert_eq!(capital(""), "");
    }
}
