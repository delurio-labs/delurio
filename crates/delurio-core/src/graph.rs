//! A graph as `load` reads it from its folder: flow.yaml, ui.yaml and the catalog ui.yaml names (SPEC.md §2-§4). The
//! port of graph.ts. The files' values are kept as they were read (`Value`), every key included, and read through the
//! views below (`Flow`, `State`, `Transition`, `Ui`, `UiNode`, `Label`, `UiEdge`), which give a key of the wrong type
//! as absent: the `schema` rule reports such a file, and the other rules read it as if the key were not there.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::error::{Error, ErrorKind, ErrorPlace, Result};
use crate::expr::{identifiers, parse_expr};
use crate::js::{self, Object, Value};
use crate::nodepath;
use crate::yaml::{line_and_column, read_yaml};

/// a flow, its screens and their words
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    pub dir: String,
    pub flow: Value,
    pub ui: Value,
    pub catalog: Value,
}

impl Graph {
    pub fn flow(&self) -> Flow<'_> {
        Flow(&self.flow)
    }
    pub fn ui(&self) -> Ui<'_> {
        Ui(&self.ui)
    }
    /// the catalog's message for a key, when it is a string
    pub fn message(&self, key: &str) -> Option<&str> {
        self.catalog.get_str(key)
    }
}

/// the strings of a list value; a value that is no list is an empty one, an item that is no string is left out
pub fn strings(v: Option<&Value>) -> Vec<&str> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

/// `v ?? fallback` read as a string: the value when it is a string
fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get_str(key)
}

/// a key's value unless it is null: what `x.key ?? ...` reads
pub fn given<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|x| !x.is_null())
}

#[derive(Debug, Clone, Copy)]
pub struct Flow<'a>(pub &'a Value);

impl<'a> Flow<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn id(&self) -> Option<&'a str> {
        str_of(self.0, "id")
    }
    /// the id as a template literal writes it (`undefined` when there is none)
    pub fn id_text(&self) -> String {
        self.0.get("id").map_or_else(|| "undefined".to_string(), js::to_js_string)
    }
    pub fn title(&self) -> Option<&'a str> {
        str_of(self.0, "title")
    }
    pub fn status(&self) -> Option<&'a str> {
        str_of(self.0, "status")
    }
    pub fn summary(&self) -> Option<&'a str> {
        str_of(self.0, "summary")
    }
    pub fn domain(&self) -> Option<&'a str> {
        str_of(self.0, "domain")
    }
    pub fn initial(&self) -> Option<&'a str> {
        str_of(self.0, "initial")
    }
    /// `flow.states`
    pub fn states_object(&self) -> Option<&'a Object> {
        self.0.get_object("states")
    }
    /// `Object.entries(flow.states)`; after `load`, every state is a map
    pub fn states(&self) -> impl Iterator<Item = (&'a str, State<'a>)> + 'a {
        self.states_object().into_iter().flat_map(|o| o.iter().map(|(k, v)| (k.as_str(), State(v))))
    }
    /// `Object.keys(flow.states)`
    pub fn state_names(&self) -> Vec<&'a str> {
        self.states_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default()
    }
    /// `flow.states[name]`
    pub fn state(&self, name: &str) -> Option<State<'a>> {
        self.states_object().and_then(|o| o.get(name)).map(State)
    }
    pub fn has_state(&self, name: &str) -> bool {
        self.state(name).is_some()
    }
    /// `flow.context` (an object after `load`)
    pub fn context(&self) -> impl Iterator<Item = (&'a str, ContextVar<'a>)> + 'a {
        self.0.get_object("context").into_iter().flat_map(|o| o.iter().map(|(k, v)| (k.as_str(), ContextVar(v))))
    }
    pub fn context_var(&self, name: &str) -> Option<ContextVar<'a>> {
        self.0.get_object("context").and_then(|o| o.get(name)).map(ContextVar)
    }
    pub fn has_context_var(&self, name: &str) -> bool {
        self.0.get_object("context").is_some_and(|o| o.contains_key(name))
    }
    /// `flow.guards`: guard name -> expression
    pub fn guards(&self) -> impl Iterator<Item = (&'a str, &'a Value)> + 'a {
        self.0.get_object("guards").into_iter().flat_map(|o| o.iter().map(|(k, v)| (k.as_str(), v)))
    }
    pub fn guard(&self, name: &str) -> Option<&'a Value> {
        self.0.get_object("guards").and_then(|o| o.get(name))
    }
    pub fn has_guard(&self, name: &str) -> bool {
        self.0.get_object("guards").is_some_and(|o| o.contains_key(name))
    }
    /// `flow.effects`
    pub fn effects(&self) -> impl Iterator<Item = (&'a str, Effect<'a>)> + 'a {
        self.0.get_object("effects").into_iter().flat_map(|o| o.iter().map(|(k, v)| (k.as_str(), Effect(v))))
    }
    pub fn effect(&self, name: &str) -> Option<Effect<'a>> {
        self.0.get_object("effects").and_then(|o| o.get(name)).map(Effect)
    }
    /// `flow.assertions ?? []`
    pub fn assertions(&self) -> Vec<&'a Value> {
        self.0.get_array("assertions").map(|a| a.iter().collect()).unwrap_or_default()
    }
    /// `flow.main`: a list of events, or `{ given, events }`
    pub fn main(&self) -> Option<&'a Value> {
        self.0.get("main")
    }
    pub fn decisions(&self) -> Option<&'a Object> {
        self.0.get_object("decisions")
    }
    pub fn scope(&self) -> Option<&'a Value> {
        self.0.get("scope")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct State<'a>(pub &'a Value);

impl<'a> State<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn title(&self) -> Option<&'a str> {
        str_of(self.0, "title")
    }
    pub fn description(&self) -> Option<&'a str> {
        str_of(self.0, "description")
    }
    /// `type`
    pub fn kind(&self) -> Option<&'a str> {
        str_of(self.0, "type")
    }
    pub fn is_final(&self) -> bool {
        self.kind() == Some("final")
    }
    /// `invoke` (any value: `st.invoke === undefined` asks whether the key is there)
    pub fn invoke(&self) -> Option<&'a Value> {
        self.0.get("invoke")
    }
    /// `Object.entries(st.on ?? {})`: event -> its transition spec
    pub fn on(&self) -> Vec<(&'a str, &'a Value)> {
        self.0.get_object("on").map(|o| o.iter().map(|(k, v)| (k.as_str(), v)).collect()).unwrap_or_default()
    }
    pub fn on_event(&self, event: &str) -> Option<&'a Value> {
        self.0.get_object("on").and_then(|o| o.get(event))
    }
    /// `st.always ?? []`
    pub fn always(&self) -> Vec<Transition<'a>> {
        self.0.get_array("always").map(|a| a.iter().map(Transition::of).collect()).unwrap_or_default()
    }
    pub fn redirect(&self) -> Option<&'a str> {
        str_of(self.0, "redirect")
    }
    pub fn code(&self) -> Option<&'a str> {
        str_of(self.0, "code")
    }
    pub fn carries(&self) -> Vec<&'a str> {
        strings(self.0.get("carries"))
    }
    pub fn leaves_page(&self) -> bool {
        self.0.get("leavesPage").and_then(Value::as_bool) == Some(true)
    }
    pub fn by(&self) -> Option<&'a str> {
        str_of(self.0, "by")
    }
    pub fn goal(&self) -> bool {
        self.0.get("goal").and_then(Value::as_bool) == Some(true)
    }
    pub fn includes(&self) -> Vec<&'a str> {
        strings(self.0.get("includes"))
    }
    pub fn response(&self) -> Option<&'a Object> {
        self.0.get_object("response")
    }
    pub fn source(&self) -> Option<&'a str> {
        str_of(self.0, "source")
    }
    /// `st.source !== undefined`
    pub fn has_source(&self) -> bool {
        self.0.get("source").is_some()
    }
}

/// one way out of a state: `alternatives` of graph.ts gives a string spec as `{ target }`
#[derive(Debug, Clone, Copy)]
pub enum Transition<'a> {
    Target(&'a str),
    Full(&'a Value),
}

impl<'a> Transition<'a> {
    pub fn of(v: &'a Value) -> Self {
        match v {
            Value::String(s) => Transition::Target(s),
            _ => Transition::Full(v),
        }
    }
    pub fn target(&self) -> Option<&'a str> {
        match self {
            Transition::Target(t) => Some(t),
            Transition::Full(v) => v.get_str("target"),
        }
    }
    /// the target as a template literal writes it, and the key `flow.states[w.target]` looks up
    pub fn target_text(&self) -> String {
        match self {
            Transition::Target(t) => (*t).to_string(),
            Transition::Full(v) => v.get("target").map_or_else(|| "undefined".to_string(), js::to_js_string),
        }
    }
    fn field(&self, key: &str) -> Option<&'a Value> {
        match self {
            Transition::Target(_) => None,
            Transition::Full(v) => v.get(key),
        }
    }
    pub fn guard(&self) -> Option<&'a str> {
        self.field("guard").and_then(Value::as_str)
    }
    pub fn effect(&self) -> Vec<&'a str> {
        strings(self.field("effect"))
    }
    pub fn say(&self) -> Option<&'a str> {
        self.field("say").and_then(Value::as_str)
    }
    /// `starts` (any value: `w.starts === undefined` asks whether the key is there)
    pub fn starts(&self) -> Option<&'a Value> {
        self.field("starts")
    }
    /// the transition as JavaScript holds it after `alternatives`: `{ target }` for a string spec
    pub fn to_value(&self) -> Value {
        match self {
            Transition::Target(t) => Value::Object(Object::from_iter([("target", Value::from(*t))])),
            Transition::Full(v) => (*v).clone(),
        }
    }
}

/// `alternatives(spec)`: a spec's transitions, a list of alternatives or one
pub fn alternatives(spec: &Value) -> Vec<Transition<'_>> {
    match spec {
        Value::Array(items) => items.iter().map(Transition::of).collect(),
        v => vec![Transition::of(v)],
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextVar<'a>(pub &'a Value);

impl<'a> ContextVar<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    /// `values`: scalars
    pub fn values(&self) -> Vec<&'a Value> {
        self.0.get_array("values").map(|a| a.iter().collect()).unwrap_or_default()
    }
    /// `initial`: `any` or a scalar
    pub fn initial(&self) -> Option<&'a Value> {
        self.0.get("initial")
    }
    /// `say`: value -> plain words
    pub fn say(&self) -> Option<&'a Object> {
        self.0.get_object("say")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Effect<'a>(pub &'a Value);

impl<'a> Effect<'a> {
    pub fn set(&self) -> Option<&'a Object> {
        self.0.get_object("set")
    }
    pub fn havoc(&self) -> Vec<&'a str> {
        strings(self.0.get("havoc"))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ui<'a>(pub &'a Value);

impl<'a> Ui<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    /// the flow has no screens and no ui.yaml
    pub fn absent(&self) -> bool {
        self.0.get("absent").and_then(Value::as_bool) == Some(true)
    }
    pub fn id(&self) -> Option<&'a str> {
        str_of(self.0, "id")
    }
    pub fn catalog(&self) -> Option<&'a str> {
        str_of(self.0, "catalog")
    }
    pub fn kit(&self) -> Option<&'a str> {
        str_of(self.0, "kit")
    }
    /// `ui.kit !== undefined`
    pub fn has_kit(&self) -> bool {
        self.0.get("kit").is_some()
    }
    pub fn samples(&self) -> Option<&'a Object> {
        self.0.get_object("samples")
    }
    pub fn sample(&self, name: &str) -> Option<&'a Value> {
        self.samples().and_then(|o| o.get(name))
    }
    /// the rows the samples give a repeat's data (`samples.<data path>`, a list of maps of fields, SPEC.md §4): None
    /// when they give none, or an empty list, and the node is drawn once
    pub fn rows(&self, data: &str) -> Option<&'a [Value]> {
        self.sample(data).and_then(Value::as_array).filter(|rows| !rows.is_empty()).map(Vec::as_slice)
    }
    pub fn nodes_object(&self) -> Option<&'a Object> {
        self.0.get_object("nodes")
    }
    /// `Object.entries(ui.nodes)`
    pub fn nodes(&self) -> impl Iterator<Item = (&'a str, UiNode<'a>)> + 'a {
        self.nodes_object().into_iter().flat_map(|o| o.iter().map(|(k, v)| (k.as_str(), UiNode(v))))
    }
    /// `ui.nodes[id]`
    pub fn node(&self, id: &str) -> Option<UiNode<'a>> {
        self.nodes_object().and_then(|o| o.get(id)).map(UiNode)
    }
    pub fn locales(&self) -> Vec<&'a str> {
        strings(self.0.get("locales"))
    }
    /// `ui.changes`
    pub fn changes(&self) -> Option<&'a Value> {
        self.0.get("changes")
    }
    /// `ui.task`
    pub fn task(&self) -> Option<&'a Value> {
        self.0.get("task")
    }
    /// `ui.focus`: state -> node
    pub fn focus(&self) -> Option<&'a Object> {
        self.0.get_object("focus")
    }
    /// `ui.edges ?? []`
    pub fn edges(&self) -> Vec<UiEdge<'a>> {
        self.0.get_array("edges").map(|a| a.iter().map(UiEdge).collect()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UiNode<'a>(pub &'a Value);

impl<'a> UiNode<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    /// `type`
    pub fn kind(&self) -> Option<&'a str> {
        str_of(self.0, "type")
    }
    pub fn component(&self) -> Option<&'a str> {
        str_of(self.0, "component")
    }
    pub fn role(&self) -> Option<&'a str> {
        str_of(self.0, "role")
    }
    pub fn intent(&self) -> Option<&'a str> {
        str_of(self.0, "intent")
    }
    pub fn name(&self) -> Option<Label<'a>> {
        self.0.get("name").filter(|v| v.is_object()).map(Label)
    }
    pub fn text(&self) -> Option<Label<'a>> {
        self.0.get("text").filter(|v| v.is_object()).map(Label)
    }
    pub fn value_label(&self) -> Option<Label<'a>> {
        self.0.get("value").filter(|v| v.is_object()).map(Label)
    }
    /// `in`
    pub fn in_states(&self) -> Vec<&'a str> {
        strings(self.0.get("in"))
    }
    pub fn unless(&self) -> Vec<&'a str> {
        strings(self.0.get("unless"))
    }
    pub fn when(&self) -> Option<&'a str> {
        str_of(self.0, "when")
    }
    pub fn disabled_in(&self) -> Vec<&'a str> {
        strings(self.0.get("disabledIn"))
    }
    pub fn disabled_when(&self) -> Option<&'a str> {
        str_of(self.0, "disabledWhen")
    }
    pub fn emits(&self) -> Option<&'a str> {
        str_of(self.0, "emits")
    }
    pub fn initial_focus(&self) -> Option<&'a str> {
        str_of(self.0, "initialFocus")
    }
    pub fn repeat(&self) -> Option<&'a str> {
        str_of(self.0, "repeat")
    }
    pub fn frame(&self) -> Option<&'a str> {
        str_of(self.0, "frame")
    }
    /// `checked`: true / false, or an expression over the context
    pub fn checked(&self) -> Option<&'a Value> {
        self.0.get("checked")
    }
    pub fn script(&self) -> Option<&'a str> {
        str_of(self.0, "script")
    }
    pub fn source(&self) -> Option<&'a str> {
        str_of(self.0, "source")
    }
    /// any key, as it was read
    pub fn get(&self, key: &str) -> Option<&'a Value> {
        self.0.get(key)
    }
}

/// a node's words: a catalog key and its params, the node that labels it, a literal, or one of several keys
#[derive(Debug, Clone, Copy)]
pub struct Label<'a>(pub &'a Value);

impl<'a> Label<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn key(&self) -> Option<&'a str> {
        str_of(self.0, "key")
    }
    pub fn params(&self) -> Option<&'a Object> {
        self.0.get_object("params")
    }
    pub fn labelled_by(&self) -> Option<&'a str> {
        str_of(self.0, "labelledBy")
    }
    pub fn literal(&self) -> Option<&'a str> {
        str_of(self.0, "literal")
    }
    /// `literal !== undefined`
    pub fn has_literal(&self) -> bool {
        self.0.get("literal").is_some()
    }
    pub fn one_of(&self) -> Option<Vec<&'a str>> {
        self.0.get("oneOf").filter(|v| v.is_array()).map(|v| strings(Some(v)))
    }
}

/// where a returnsFocus gives the focus back: one node, any of a list, or the node that opened the dialog
#[derive(Debug, Clone, PartialEq)]
pub enum FocusTarget<'a> {
    One(&'a str),
    Any(Vec<&'a str>),
}

#[derive(Debug, Clone, Copy)]
pub struct UiEdge<'a>(pub &'a Value);

fn pair(v: Option<&Value>) -> Option<(&str, &str)> {
    let a = v?.as_array()?;
    Some((a.first()?.as_str()?, a.get(1)?.as_str()?))
}

impl<'a> UiEdge<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn labelled_by(&self) -> Option<(&'a str, &'a str)> {
        pair(self.0.get("labelledBy"))
    }
    pub fn triggers(&self) -> Option<(&'a str, &'a str)> {
        pair(self.0.get("triggers"))
    }
    /// `returnsFocus: [dialog, target]`
    pub fn returns_focus(&self) -> Option<(&'a str, FocusTarget<'a>)> {
        let a = self.0.get("returnsFocus")?.as_array()?;
        let dialog = a.first()?.as_str()?;
        let target = match a.get(1)? {
            Value::String(s) => FocusTarget::One(s),
            Value::Array(items) => FocusTarget::Any(items.iter().filter_map(Value::as_str).collect()),
            _ => return None,
        };
        Some((dialog, target))
    }
}

/// in a returnsFocus, the node that was pressed to open the dialog, whichever it is (SPEC.md §4); no node is called so
pub const OPENER: &str = "opener";

/// the roles whose node may say whether it is `checked` (WAI-ARIA 1.2: the roles that support aria-checked)
pub const CHECKABLE: [&str; 6] = ["checkbox", "menuitemcheckbox", "menuitemradio", "option", "radio", "switch"];

/// the field a name reads of the row a repeat node is drawn for, or pressed with: `row.<field>` (SPEC.md §4)
pub fn row_field(name: &str) -> Option<&str> {
    name.strip_prefix("row.").filter(|f| !f.is_empty() && !f.contains('.'))
}

/// a page node drawn over another page, whose closing `returnsFocus` may give the focus back (SPEC.md §4)
pub fn is_dialog(n: Option<UiNode<'_>>) -> bool {
    n.is_some_and(|n| n.kind() == Some("page") && matches!(n.role(), Some("dialog" | "alertdialog")))
}

/// the file a graph names, read as YAML, a YAML error carrying the file it is in
fn read_graph_file(read: &dyn Fn(&str) -> Result<String>, file: &str) -> Result<Value> {
    read(file).and_then(|text| read_yaml(&text)).map_err(|e| e.placed(ErrorPlace::file(file)))
}

/// the place of a transition in flow.yaml: `on.<event>`, the item of a list of alternatives, or an item of `always`
pub fn transition_path(state: &str, st: State<'_>, event: Option<&str>, i: usize) -> Vec<String> {
    let Some(event) = event else { return vec!["states".into(), state.into(), "always".into(), i.to_string()] };
    let listed = st.on_event(event).is_some_and(Value::is_array);
    let mut path = vec!["states".to_string(), state.to_string(), "on".to_string(), event.to_string()];
    if listed {
        path.push(i.to_string());
    }
    path
}

static LINE_COLUMN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"line ([0-9]+) column ([0-9]+)").expect("the pattern compiles"));
static POSITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"position ([0-9]+)").expect("the pattern compiles"));

/// a catalog: a JSON object, read from the file ui.yaml names; one that is not there, or is not JSON, keeps the graph
/// from loading, at the place that names it or at the place the JSON parser gave
fn read_catalog(read: &dyn Fn(&str) -> Result<String>, exists: &dyn Fn(&str) -> bool, ui: &Value) -> Result<Value> {
    let catalog = match ui.get_str("catalog") {
        Some(c) if !c.is_empty() => c,
        _ => {
            return Err(Error::graph(
                "ui.yaml names no catalog: it gives the words of its screens as catalog: catalog/<locale>.json (SPEC.md §4)",
                Some(ErrorPlace::at("ui.yaml", &[])),
            ));
        }
    };
    if !exists(catalog) {
        return Err(Error::graph(
            format!("the catalog {catalog} that ui.yaml names does not exist"),
            Some(ErrorPlace::at("ui.yaml", &["catalog"])),
        ));
    }
    let text = read(catalog).map_err(|e| {
        Error::graph(
            format!("the catalog {catalog} that ui.yaml names cannot be read: {}", e.message),
            Some(ErrorPlace::at("ui.yaml", &["catalog"])),
        )
    })?;
    js::json_parse(&text).map_err(|e| {
        let message = e.message;
        let mut place = ErrorPlace::file(catalog);
        if let Some(m) = LINE_COLUMN.captures(&message) {
            place.line = m[1].parse().ok();
            place.column = m[2].parse().ok();
        } else if let Some(m) = POSITION.captures(&message) {
            let at = line_and_column(&text, m[1].parse().unwrap_or(0));
            place.line = Some(at.line);
            place.column = Some(at.column);
        }
        Error::graph(format!("the catalog {catalog} is not JSON: {message}"), Some(place))
    })
}

static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new("([a-z0-9])([A-Z])").expect("the pattern compiles"));
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new("[_.-]+").expect("the pattern compiles"));

/// 'silentRedirect' -> 'Silent redirect'; used only when the graph gives no title
pub fn humanize(id: &str) -> String {
    let words = CAMEL.replace_all(id, "${1} ${2}");
    let words = SEPARATORS.replace_all(&words, " ").to_lowercase();
    let mut chars = words.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

/// `flow.states[state].title ?? humanize(state)`
pub fn title_of(flow: Flow<'_>, state: &str) -> String {
    match flow.state(state).and_then(|s| given(s.0, "title")) {
        Some(Value::String(t)) => t.clone(),
        Some(v) => js::to_js_string(v),
        None => humanize(state),
    }
}

/// a node is shown in a state when the state is in its `in` and not in its `unless`; `when` is read over the context
pub fn shown_in(n: UiNode<'_>, state: &str) -> bool {
    n.in_states().contains(&state) && !n.unless().contains(&state)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FocusBy {
    ReturnsFocus,
    InitialFocus,
    Focus,
}

/// the node that has focus after a move, and which part of ui.yaml gives it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FocusAfter {
    pub node: String,
    pub by: FocusBy,
    /// the dialog that closed, or the page that opened
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// The node that has focus once the flow has moved from the state `from` (None when it starts) into another state
/// `to`, both states the flow stops in (SPEC.md §4, Focus): a dialog that closes gives it back to its `returnsFocus`
/// node when `to` shows that node; else a page that opens gives it to its `initialFocus`, a dialog's before the page it
/// covers; else `focus.<to>`. None when the graph does not say, and on a move from a state to itself. `pressed` is the
/// node that was pressed to open the dialog, which a replay knows and the graph does not: a returnsFocus that lists
/// several nodes, or says OPENER, gives the focus to it. Without it, a list gives the one node of it that `to` shows,
/// and OPENER, or a list of which `to` shows several, gives None.
pub fn focus_after(ui: Ui<'_>, from: Option<&str>, to: &str, pressed: Option<&str>) -> Option<FocusAfter> {
    if from == Some(to) {
        return None;
    }
    let shown =
        |id: &str, state: Option<&str>| -> bool { state.is_some_and(|s| ui.node(id).is_some_and(|n| shown_in(n, s))) };
    let back = |node: &str, dialog: &str| FocusAfter {
        node: node.to_string(),
        by: FocusBy::ReturnsFocus,
        from: Some(dialog.to_string()),
    };
    for e in ui.edges() {
        let Some((dialog, target)) = e.returns_focus() else { continue };
        if !is_dialog(ui.node(dialog)) {
            continue;
        }
        if !shown(dialog, from) || shown(dialog, Some(to)) {
            continue;
        }
        if let FocusTarget::One(t) = &target
            && *t != OPENER
        {
            if shown(t, Some(to)) {
                return Some(back(t, dialog));
            }
            continue;
        }
        // the node that opened the dialog: the one pressed, when the caller knows it, among those a list allows
        let allowed = |id: &str| match &target {
            FocusTarget::One(_) => true, // OPENER
            FocusTarget::Any(list) => list.contains(&id),
        };
        if let Some(p) = pressed {
            if allowed(p) && shown(p, Some(to)) {
                return Some(back(p, dialog));
            }
            continue;
        }
        let candidates: Vec<&str> = match &target {
            FocusTarget::Any(list) => list.iter().copied().filter(|id| shown(id, Some(to))).collect(),
            FocusTarget::One(_) => Vec::new(),
        };
        if candidates.len() == 1 {
            return Some(back(candidates[0], dialog));
        }
        return None;
    }
    let opened: Vec<(&str, UiNode<'_>)> = ui
        .nodes()
        .filter(|(id, n)| {
            n.kind() == Some("page") && n.0.get("initialFocus").is_some() && shown(id, Some(to)) && !shown(id, from)
        })
        .collect();
    let page = opened.iter().find(|(_, n)| is_dialog(Some(*n))).or(opened.first());
    if let Some((id, n)) = page {
        let node = n.0.get("initialFocus").map(js::to_js_string).unwrap_or_default();
        return Some(FocusAfter { node, by: FocusBy::InitialFocus, from: Some((*id).to_string()) });
    }
    from?;
    let node = ui.focus()?.get(to)?;
    Some(FocusAfter { node: js::to_js_string(node), by: FocusBy::Focus, from: None })
}

/// Node's message for a file that cannot be read, as `readFileSync` throws it
pub fn read_error(err: &std::io::Error, path: &str) -> String {
    let (code, words) = match err.kind() {
        std::io::ErrorKind::NotFound => ("ENOENT", "no such file or directory"),
        std::io::ErrorKind::PermissionDenied => ("EACCES", "permission denied"),
        std::io::ErrorKind::IsADirectory => return "EISDIR: illegal operation on a directory, read".to_string(),
        _ => return err.to_string(),
    };
    format!("{code}: {words}, open '{path}'")
}

/// a graph folder's file, read from the disk as `readFileSync(join(dir, file), 'utf8')` reads it
pub fn read_file(dir: &str, file: &str) -> Result<String> {
    let path = nodepath::join2(dir, file);
    let bytes = std::fs::read(&path).map_err(|e| Error::graph(read_error(&e, &path), None))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `existsSync(join(dir, file))`
pub fn file_exists(dir: &str, file: &str) -> bool {
    std::path::Path::new(&nodepath::join2(dir, file)).exists()
}

/// `load(dir)`: the graph in a folder, read from the disk
pub fn load(dir: &str) -> Result<Graph> {
    load_with(dir, &|file| read_file(dir, file), &|file| file_exists(dir, file))
}

fn names_outside(flow: Flow<'_>, expr: &str) -> std::result::Result<Vec<String>, String> {
    let names = identifiers(&*parse_expr(expr)?);
    Ok(names.into_iter().filter(|id| id != "state" && !flow.has_context_var(id) && !flow.has_guard(id)).collect())
}

/// `load(dir, read, exists)`: `read` and `exists` take a path relative to `dir`; the revision diff passes ones that
/// read a git revision instead of the disk
pub fn load_with(dir: &str, read: &dyn Fn(&str) -> Result<String>, exists: &dyn Fn(&str) -> bool) -> Result<Graph> {
    let mut flow = read_graph_file(read, "flow.yaml")?;
    let Some(f) = flow.as_object_mut() else {
        return Err(Error::graph(
            "flow.yaml is not a map of keys (SPEC.md §2)",
            Some(ErrorPlace::at("flow.yaml", &[])),
        ));
    };
    if !f.get("states").is_some_and(Value::is_object) {
        return Err(Error::graph(
            "flow.yaml has no states: a flow gives them as states: { <state>: { .. } } (SPEC.md §2)",
            Some(ErrorPlace::at("flow.yaml", &["states"])),
        ));
    }
    // a flow with no context, guards or effects leaves the key out; every reader expects an object
    for key in ["context", "guards", "effects"] {
        if f.get(key).is_none_or(Value::is_null) {
            f.insert(key, Value::Object(Object::new()));
        }
    }
    // a flow with no screens (an endpoint, a webhook, a CI job) has no ui.yaml and no catalog
    let mut ui = if exists("ui.yaml") {
        read_graph_file(read, "ui.yaml")?
    } else {
        let mut absent = Object::new();
        absent.insert("absent", Value::Bool(true));
        if let Some(id) = f.get("id") {
            absent.insert("id", id.clone());
        }
        absent.insert("catalog", Value::from(""));
        absent.insert("samples", Value::Object(Object::new()));
        absent.insert("nodes", Value::Object(Object::new()));
        Value::Object(absent)
    };
    let Some(u) = ui.as_object_mut() else {
        return Err(Error::graph("ui.yaml is not a map of keys (SPEC.md §4)", Some(ErrorPlace::at("ui.yaml", &[]))));
    };
    for key in ["nodes", "samples"] {
        if u.get(key).is_none_or(Value::is_null) {
            u.insert(key, Value::Object(Object::new()));
        }
    }
    let catalog = if Ui(&ui).absent() { Value::Object(Object::new()) } else { read_catalog(read, exists, &ui)? };

    let fl = Flow(&flow);
    let initial = flow.get("initial").map_or_else(|| "undefined".to_string(), js::to_js_string);
    if !fl.has_state(&initial) {
        return Err(Error::graph(
            format!("initial state {initial} does not exist"),
            Some(ErrorPlace::at("flow.yaml", &["initial"])),
        ));
    }
    for (name, st) in fl.states() {
        if !st.0.is_object() {
            return Err(Error::graph(
                format!("state {name} is not a map of keys: write it as {name}: {{ .. }} (SPEC.md §2)"),
                Some(ErrorPlace::at("flow.yaml", &["states", name])),
            ));
        }
        let nested = st.0.get("states").is_some();
        if nested || matches!(st.kind(), Some("parallel" | "history")) {
            return Err(Error::graph(
                format!("state {name}: nested, parallel and history states are not supported by graph-viz yet"),
                Some(ErrorPlace::at("flow.yaml", &["states", name, if nested { "states" } else { "type" }])),
            ));
        }
        let mut ways: Vec<(Option<&str>, usize, Transition<'_>)> =
            st.always().into_iter().enumerate().map(|(i, a)| (None, i, a)).collect();
        for (event, spec) in st.on() {
            ways.extend(alternatives(spec).into_iter().enumerate().map(|(i, a)| (Some(event), i, a)));
        }
        for (event, i, w) in ways {
            let path = transition_path(name, st, event, i);
            let at = |extra: &[&str]| {
                let mut p = path.clone();
                p.extend(extra.iter().map(|s| s.to_string()));
                Some(ErrorPlace { file: "flow.yaml".into(), path: Some(p), line: None, column: None })
            };
            let target = w.target_text();
            if !fl.has_state(&target) {
                return Err(Error::graph(format!("state {name} goes to {target}, which does not exist"), at(&[])));
            }
            let Some(starts) = w.starts() else { continue };
            let Some(started) = starts.get_str("flow") else {
                return Err(Error::graph(format!("state {name}: `starts` names no flow"), at(&["starts"])));
            };
            for (k, v) in starts.get_object("given").into_iter().flat_map(|o| o.iter()) {
                let Value::String(v) = v else { continue };
                let place = at(&["starts", "given", k]);
                match names_outside(fl, v) {
                    Err(message) => {
                        return Err(Error::graph(
                            format!("state {name} starts {started} with {k} = {v}: {message}"),
                            place,
                        ));
                    }
                    Ok(outside) => {
                        if let Some(id) = outside.first() {
                            return Err(Error::graph(
                                format!(
                                    "state {name} starts {started} with {k} = {v}: {id} is not a variable or a guard of this flow (a value is quoted: '{id}')"
                                ),
                                place,
                            ));
                        }
                    }
                }
            }
        }
    }
    let ui_view = Ui(&ui);
    for (id, node) in ui_view.nodes() {
        for key in ["in", "unless", "disabledIn"] {
            for (i, s) in strings(node.get(key)).into_iter().enumerate() {
                if !fl.has_state(s) {
                    return Err(Error::graph(
                        format!("ui node {id} names the unknown state {s}"),
                        Some(ErrorPlace::at("ui.yaml", &["nodes", id, key, &i.to_string()])),
                    ));
                }
            }
        }
        // `checked` is an expression over the context (SPEC.md §3), read when the node is shown; a repeat node's may
        // read the row it is drawn for (`row.<field>`)
        if let Some(Value::String(checked)) = node.checked() {
            let place = Some(ErrorPlace::at("ui.yaml", &["nodes", id, "checked"]));
            let repeat = node.repeat().is_some();
            match names_outside(fl, checked) {
                Err(message) => return Err(Error::graph(format!("ui node {id}: checked: {message}"), place)),
                Ok(outside) => {
                    if let Some(n) = outside.iter().find(|n| !repeat || row_field(n).is_none()) {
                        let row = if row_field(n).is_some() {
                            ": a node reads the row it is drawn for (row.<field>) only when it has repeat"
                        } else {
                            ""
                        };
                        return Err(Error::graph(
                            format!(
                                "ui node {id}: checked: {checked} names {n}, which is not a variable or a guard of the flow{row}"
                            ),
                            place,
                        ));
                    }
                }
            }
        }
    }
    for s in ui_view.focus().into_iter().flat_map(|o| o.keys()) {
        if !fl.has_state(s) {
            return Err(Error::graph(
                format!("ui focus names the unknown state {s}"),
                Some(ErrorPlace::at("ui.yaml", &["focus", s])),
            ));
        }
    }
    disabled_when_reads(fl, ui_view)?;
    Ok(Graph { dir: dir.to_string(), flow, ui, catalog })
}

/// a node's `disabledWhen` is an expression over the context (SPEC.md §3, §4), read where the node is shown: one that
/// does not parse, or names what is not a variable, a guard or `state`, keeps the graph from loading, as `checked` does
fn disabled_when_reads(flow: Flow<'_>, ui: Ui<'_>) -> Result<()> {
    for (id, node) in ui.nodes() {
        let Some(Value::String(expr)) = node.get("disabledWhen") else { continue };
        match names_outside(flow, expr) {
            Err(message) => return Err(Error::graph(format!("ui node {id}: disabledWhen: {message}"), None)),
            Ok(outside) => {
                if let Some(n) = outside.first() {
                    return Err(Error::graph(
                        format!(
                            "ui node {id}: disabledWhen: {expr} names {n}, which is not a variable or a guard of the flow"
                        ),
                        None,
                    ));
                }
            }
        }
    }
    Ok(())
}

/// whether an error is one that keeps a graph from loading (`loadError` of cli.ts): every error of this crate is
pub fn is_load_error(e: &Error) -> bool {
    matches!(e.kind, ErrorKind::Graph | ErrorKind::Model | ErrorKind::Kit | ErrorKind::Yaml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanized_names() {
        assert_eq!(humanize("silentRedirect"), "Silent redirect");
        assert_eq!(humanize("code_issued.ok"), "Code issued ok");
        assert_eq!(humanize("aBC"), "A bc");
        assert_eq!(humanize(""), "");
    }
}
