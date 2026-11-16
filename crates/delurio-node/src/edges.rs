//! edges.js: the edges a flow's diagrams draw and the words of their nodes (edges.ts).

use delurio_core::edges::{BY, collect_edges, exit_line, kind_of, node_line, plain, press_label, response_line, wrap};
use delurio_core::graph::State;
use delurio_core::js::{Object, Value};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, parse, parse_opt};
use crate::graph::NativeGraph;
use crate::model::NativeExploration;

/// `BY`, as JSON
#[napi]
pub fn edges_constants() -> Result<String> {
    let by: Object = BY.iter().map(|(k, w)| (*k, Value::from(*w))).collect();
    json(&Value::Object(by))
}

/// `kindOf(g, name)`
#[napi(js_name = "kindOf")]
pub fn kind_of_state(g: &NativeGraph, name: String) -> Result<String> {
    // the kind as its JSON string, without the quotes
    Ok(json(&kind_of(&g.g, &name))?.trim_matches('"').to_string())
}

/// `exitLine(st)`
#[napi(js_name = "exitLine")]
pub fn exit_line_of(st: Option<String>) -> Result<String> {
    let st = parse_opt(st)?.unwrap_or(Value::Null);
    Ok(exit_line(State(&st)))
}

/// `responseLine(r)`
#[napi(js_name = "responseLine")]
pub fn response_line_of(r: String) -> Result<String> {
    let r = parse(&r)?;
    Ok(response_line(r.as_object().unwrap_or(&Object::new())))
}

/// `nodeLine(g, name)`
#[napi(js_name = "nodeLine")]
pub fn node_line_of(g: &NativeGraph, name: String) -> String {
    node_line(&g.g, &name)
}

/// `plain(g, key)`
#[napi(js_name = "plain")]
pub fn plain_text(g: &NativeGraph, key: String) -> String {
    plain(&g.g, &key)
}

/// `pressLabel(g, state, event)`
#[napi(js_name = "pressLabel")]
pub fn press_label_of(g: &NativeGraph, state: String, event: String) -> Option<String> {
    press_label(&g.g, &state, &event)
}

/// `wrap(text, max)`
#[napi(js_name = "wrap")]
pub fn wrap_text(text: String, max: f64) -> Vec<String> {
    // `line.length + 1 + word.length > max` with a max below 0 or not a number: every word on a line of its own, or
    // all on one
    let max = if max.is_nan() {
        usize::MAX
    } else if max < 0.0 {
        0
    } else {
        max.min(usize::MAX as f64) as usize
    };
    wrap(&text, max)
}

/// `collectEdges(g, ex)`, as JSON
#[napi(js_name = "collectEdges")]
pub fn collect_edges_of(g: &NativeGraph, ex: &NativeExploration) -> Result<String> {
    json(&collect_edges(&g.g, &ex.ex))
}
