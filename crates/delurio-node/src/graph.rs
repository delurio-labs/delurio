//! graph.js: `load`, and the readers of a graph's parts (graph.ts).

use delurio_core::graph::{
    self, CHECKABLE, Graph, OPENER, State, Ui, UiNode, alternatives, focus_after, humanize, is_dialog, shown_in,
    title_of, transition_path,
};
use delurio_core::js::{self, Object, Value};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{ReadOut, exister, json, ok, parse, parse_opt, reader};

/// a graph the addon holds for the JavaScript object it was read into (js/internal.js keeps the pair)
#[napi]
pub struct NativeGraph {
    pub(crate) g: Graph,
}

#[napi]
impl NativeGraph {
    /// a graph object a caller built (a copy of a loaded one, say), read from its JSON
    #[napi(constructor)]
    pub fn new(text: String) -> Result<Self> {
        let v = parse(&text)?;
        let field = |k: &str| v.get(k).cloned().unwrap_or(Value::Null);
        let dir = v.get("dir").map(js::to_js_string).unwrap_or_default();
        Ok(NativeGraph { g: Graph { dir, flow: field("flow"), ui: field("ui"), catalog: field("catalog") } })
    }

    /// the graph as the object `load` gave
    #[napi]
    pub fn json(&self) -> Result<String> {
        let mut o = Object::new();
        o.insert("dir", Value::from(self.g.dir.as_str()));
        o.insert("flow", self.g.flow.clone());
        o.insert("ui", self.g.ui.clone());
        o.insert("catalog", self.g.catalog.clone());
        json(&Value::Object(o))
    }
}

/// any value of a graph file the addon holds for its JavaScript object (a flow, read once for many calls)
#[napi]
pub struct NativeValue {
    pub(crate) v: Value,
}

#[napi]
impl NativeValue {
    #[napi(constructor)]
    pub fn new(text: String) -> Result<Self> {
        Ok(NativeValue { v: parse(&text)? })
    }
}

/// OPENER and CHECKABLE
#[napi]
pub fn graph_constants() -> Result<String> {
    let mut o = Object::new();
    o.insert("OPENER", Value::from(OPENER));
    o.insert("CHECKABLE", Value::Array(CHECKABLE.iter().map(|s| Value::from(*s)).collect()));
    json(&Value::Object(o))
}

/// `load(dir, read, exists)`
#[napi]
pub fn load(dir: String, read: Function<String, ReadOut>, exists: Function<String, u32>) -> Result<NativeGraph> {
    let read = reader(&read);
    let exists = exister(&exists);
    ok(graph::load_with(&dir, &read, &exists)).map(|g| NativeGraph { g })
}

/// `alternatives(spec)`, as JSON
#[napi]
pub fn alternatives_of(spec: Option<String>) -> Result<String> {
    let spec = parse_opt(spec)?.unwrap_or(Value::Null);
    // `alternatives(undefined)` is `[undefined]` in the TypeScript: a list of one item that is no transition
    let out: Vec<Value> = alternatives(&spec).iter().map(|t| t.to_value()).collect();
    json(&out)
}

/// `titleOf(flow, state)`
#[napi]
pub fn title_of_state(flow: &NativeValue, state: String) -> String {
    title_of(graph::Flow(&flow.v), &state)
}

#[napi(js_name = "humanize")]
pub fn humanize_id(id: String) -> String {
    humanize(&id)
}

/// `shownIn(n, state)`
#[napi]
pub fn shown_in_state(node: String, state: String) -> Result<bool> {
    let n = parse(&node)?;
    Ok(shown_in(UiNode(&n), &state))
}

/// `isDialog(n)`
#[napi]
pub fn is_dialog_node(node: Option<String>) -> Result<bool> {
    let n = parse_opt(node)?;
    Ok(is_dialog(n.as_ref().filter(|v| v.is_object()).map(UiNode)))
}

/// `focusAfter(ui, from, to, pressed)`, as JSON (null when nothing has the focus)
#[napi]
pub fn focus_after_step(ui: String, from: Option<String>, to: String, pressed: Option<String>) -> Result<String> {
    let ui = parse(&ui)?;
    match focus_after(Ui(&ui), from.as_deref(), &to, pressed.as_deref()) {
        Some(f) => json(&f),
        None => Ok("null".into()),
    }
}

/// `transitionPath(state, st, event, i)`
#[napi]
pub fn transition_path_of(state: String, st: String, event: Option<String>, i: u32) -> Result<Vec<String>> {
    let st = parse(&st)?;
    Ok(transition_path(&state, State(&st), event.as_deref(), i as usize))
}
