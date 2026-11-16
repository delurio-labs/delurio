//! wireframe.js: a flow's screens as wireframes (wireframe.ts).

use delurio_core::wireframe::{node_texts, render_check_screens, render_screens, render_stage_screen};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, warner};
use crate::graph::NativeGraph;
use crate::kit::NativeKit;

/// `renderScreens(g, warn, kit)`
#[napi(js_name = "renderScreens")]
pub fn render_screens_of(g: &NativeGraph, warn: Function<String, ()>, kit: Option<&NativeKit>) -> String {
    let mut warn = warner(&warn);
    render_screens(&g.g, &mut warn, kit.map(|k| &k.kit))
}

/// `renderStageScreen(g, state, reveal, warn, kit)`
#[napi(js_name = "renderStageScreen")]
pub fn render_stage_screen_of(
    g: &NativeGraph,
    state: String,
    reveal: Vec<String>,
    warn: Function<String, ()>,
    kit: Option<&NativeKit>,
) -> String {
    let mut warn = warner(&warn);
    let reveal: Vec<&str> = reveal.iter().map(String::as_str).collect();
    render_stage_screen(&g.g, &state, &reveal, &mut warn, kit.map(|k| &k.kit))
}

/// `renderCheckScreens(g, kit, warn)`, as JSON
#[napi(js_name = "renderCheckScreens")]
pub fn render_check_screens_of(g: &NativeGraph, kit: Option<&NativeKit>, warn: Function<String, ()>) -> Result<String> {
    let mut warn = warner(&warn);
    json(&render_check_screens(&g.g, kit.map(|k| &k.kit), &mut warn))
}

/// `nodeTexts(g, id, warn)`
#[napi(js_name = "nodeTexts")]
pub fn node_texts_of(g: &NativeGraph, id: String, warn: Function<String, ()>) -> Vec<String> {
    let mut warn = warner(&warn);
    node_texts(&g.g, &id, &mut warn)
}
