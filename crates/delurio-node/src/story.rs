//! story.js: a flow's stories (story.ts). Steps cross as lists, one entry per step, their contexts as places in the
//! model's tables, one after the other.

use std::rc::Rc;

use delurio_core::js::Value;
use delurio_core::story::{PathStep, main_story_index, narrate, steps_by_events, steps_to};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, ok, parse_opt};
use crate::graph::NativeGraph;
use crate::model::{NativeExploration, NativeModel};

#[napi(object)]
pub struct PathSteps {
    pub state: Vec<String>,
    pub event: Vec<Option<String>>,
    pub taken: Vec<Option<String>>,
    pub ctx: Uint16Array,
}

fn out(steps: Vec<PathStep>) -> PathSteps {
    let mut o = PathSteps { state: Vec::new(), event: Vec::new(), taken: Vec::new(), ctx: Uint16Array::new(vec![]) };
    let mut ctx: Vec<u16> = Vec::new();
    for s in steps {
        o.state.push(s.state.to_string());
        o.event.push(s.event.map(|e| e.to_string()));
        o.taken.push(s.taken.map(|t| t.to_string()));
        ctx.extend_from_slice(&s.ctx);
    }
    o.ctx = Uint16Array::new(ctx);
    o
}

/// steps a caller hands in (an engine's own extended path, say), each context `n` places long
pub(crate) fn path_steps(steps: &PathSteps, n: usize) -> Vec<PathStep> {
    (0..steps.state.len())
        .map(|k| PathStep {
            state: Rc::from(steps.state[k].as_str()),
            ctx: steps.ctx.get(k * n..(k + 1) * n).unwrap_or(&[]).to_vec().into_boxed_slice(),
            event: steps.event.get(k).cloned().flatten().map(|e| Rc::from(e.as_str())),
            taken: steps.taken.get(k).cloned().flatten().map(|t| Rc::from(t.as_str())),
        })
        .collect()
}

/// `stepsTo(m, ex, index)`
#[napi(js_name = "stepsTo")]
pub fn steps_to_config(m: &NativeModel, ex: &NativeExploration, index: u32) -> Result<PathSteps> {
    ok(steps_to(&m.m, &ex.ex, index as usize)).map(out)
}

/// `stepsByEvents(m, main)`
#[napi(js_name = "stepsByEvents")]
pub fn steps_by_main(m: &NativeModel, main: Option<String>) -> Result<Option<PathSteps>> {
    let main = parse_opt(main)?.unwrap_or(Value::Null);
    ok(steps_by_events(&m.m, &main)).map(|s| s.map(out))
}

/// `mainStoryIndex(g, ex)`
#[napi(js_name = "mainStoryIndex")]
pub fn main_story_of(g: &NativeGraph, ex: &NativeExploration) -> Option<u32> {
    main_story_index(&g.g, &ex.ex).map(|i| i as u32)
}

/// `narrate(g, m, steps)`, as JSON
#[napi(js_name = "narrate")]
pub fn narrate_steps(g: &NativeGraph, m: &NativeModel, steps: PathSteps) -> Result<String> {
    let steps = path_steps(&steps, m.m.vars.len());
    json(&narrate(&g.g, &m.m, &steps))
}
