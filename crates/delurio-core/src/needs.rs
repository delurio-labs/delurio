//! A flow's needs and a product's gaps (SPEC.md §13). needs.yaml, beside a flow, is a closed list of what must exist for
//! the flow's users: each need names the evidence that it does, or says `missing` and why. needs.gaps.yaml, at the
//! root of a product's graphs, lists every need of its flows that is missing. `check_needs` holds each piece of
//! evidence to what it names, each missing need to the gaps file above it, and each entry of a gaps file to a need
//! under its folder that is still missing, so the list only shrinks on its own. `planned` gives a tool that walks a flow
//! on the app the states and nodes that only missing needs are drawn as, for it to leave out. The port of needs.ts.

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde::Serialize;

use crate::anchors::{AppFiles, split_line};
use crate::error::{Error, Result};
use crate::graph::read_error;
use crate::js::{self, Object, Value};
use crate::nodepath;
use crate::yaml::{key_at, read_yaml};

pub const NEEDS_FILE: &str = "needs.yaml";
pub const GAPS_FILE: &str = "needs.gaps.yaml";

const FORMAT: &str = "delurio-graph/0";

/// a piece of evidence, as needs.yaml gives it, read through: each key when it is a string
#[derive(Debug, Clone, Copy)]
pub struct Evidence<'a>(pub &'a Value);

impl<'a> Evidence<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    /// `<path>[:<line>]`, from the application's repository root (SPEC.md §9)
    pub fn source(&self) -> Option<&'a str> {
        self.0.get_str("source")
    }
    pub fn test(&self) -> Option<&'a str> {
        self.0.get_str("test")
    }
    /// with source or test: a name the file holds
    pub fn name(&self) -> Option<&'a str> {
        self.0.get_str("name")
    }
    pub fn scenario(&self) -> Option<&'a str> {
        self.0.get_str("scenario")
    }
    /// with scenario: the binding, when it is not the flow's own binding.yaml
    pub fn binding(&self) -> Option<&'a str> {
        self.0.get_str("binding")
    }
    pub fn state(&self) -> Option<&'a str> {
        self.0.get_str("state")
    }
    pub fn node(&self) -> Option<&'a str> {
        self.0.get_str("node")
    }
    pub fn setting(&self) -> Option<&'a str> {
        self.0.get_str("setting")
    }
    /// `in`, with setting: the file that sets it
    pub fn in_file(&self) -> Option<&'a str> {
        self.0.get_str("in")
    }
}

/// a need, as needs.yaml gives it, read through
#[derive(Debug, Clone, Copy)]
pub struct Need<'a>(pub &'a Value);

impl<'a> Need<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    pub fn id(&self) -> Option<&'a str> {
        self.0.get_str("id")
    }
    pub fn need(&self) -> Option<&'a str> {
        self.0.get_str("need")
    }
    /// `evidence ?? []`, when it is a list
    pub fn evidence(&self) -> Vec<Evidence<'a>> {
        self.0.get_array("evidence").map(|a| a.iter().map(Evidence).collect()).unwrap_or_default()
    }
    pub fn missing(&self) -> Option<&'a str> {
        self.0.get_str("missing")
    }
    pub fn note(&self) -> Option<&'a str> {
        self.0.get_str("note")
    }
}

/// a flow's needs.yaml as `read_needs` gives it: the file's value, every key kept, a map with a list of needs
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Needs(pub Value);

impl Needs {
    pub fn value(&self) -> &Value {
        &self.0
    }
    pub fn schema(&self) -> Option<&str> {
        self.0.get_str("schema")
    }
    pub fn kind(&self) -> Option<&str> {
        self.0.get_str("kind")
    }
    /// the flow's id
    pub fn id(&self) -> Option<&str> {
        self.0.get_str("id")
    }
    pub fn status(&self) -> Option<&str> {
        self.0.get_str("status")
    }
    pub fn needs(&self) -> Vec<Need<'_>> {
        self.0.get_array("needs").map(|a| a.iter().map(Need).collect()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NeedRule {
    EvidenceUnresolved,
    NeedNoEvidence,
    NeedNamesBuilt,
    NeedUnlisted,
    NeedGap,
    NeedTwice,
    NeedsPlace,
    GapMet,
    GapUnknown,
}

impl NeedRule {
    pub fn as_str(&self) -> &'static str {
        match self {
            NeedRule::EvidenceUnresolved => "evidence-unresolved",
            NeedRule::NeedNoEvidence => "need-no-evidence",
            NeedRule::NeedNamesBuilt => "need-names-built",
            NeedRule::NeedUnlisted => "need-unlisted",
            NeedRule::NeedGap => "need-gap",
            NeedRule::NeedTwice => "need-twice",
            NeedRule::NeedsPlace => "needs-place",
            NeedRule::GapMet => "gap-met",
            NeedRule::GapUnknown => "gap-unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedProblem {
    pub rule: NeedRule,
    pub file: String, // as found: absolute when the check was given absolute folders
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub graph: String, // the flow's id for a needs file, the product's for a gaps file
    /// what it is about, stable when lines move: needs/<id>/evidence/<kind>:<value>, gaps/<flow>/<need>
    pub element: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsCheck {
    pub problems: Vec<NeedProblem>,
    pub files: usize, // the needs files read
    pub needs: usize,
    pub gaps: usize, // the needs that say missing
    /// under --no-app: the pieces of evidence that name a file of the application, not looked for
    pub not_looked_for: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Source,
    Test,
    Scenario,
    State,
    Node,
    Setting,
}

const KINDS: [Kind; 6] = [Kind::Source, Kind::Test, Kind::Scenario, Kind::State, Kind::Node, Kind::Setting];

impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Kind::Source => "source",
            Kind::Test => "test",
            Kind::Scenario => "scenario",
            Kind::State => "state",
            Kind::Node => "node",
            Kind::Setting => "setting",
        }
    }
    /// what the graph draws: it shows where a need is, and alone meets nothing
    fn in_graph(&self) -> bool {
        matches!(self, Kind::State | Kind::Node)
    }
}

/// a text that says something: a string that is not blank
fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !js::trim(s).is_empty())
}

fn kind_of(e: &Object) -> Option<Kind> {
    KINDS.into_iter().find(|k| e.get(k.as_str()).is_some_and(Value::is_string))
}

fn is_missing(n: &Object) -> bool {
    text(n.get("missing")).is_some()
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

/// a file's text as `readFileSync(file, 'utf8')` gives it; None when it cannot be read
fn read_text(file: &str) -> Option<String> {
    std::fs::read(file).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// a graph file read: its text and its value, a map
struct Read {
    text: String,
    doc: Object,
}

/// the value of a graph file, or None when it cannot be read: the schema rule says why
fn read_doc(file: &str) -> Option<Read> {
    let raw = read_text(file)?;
    let Ok(Value::Object(doc)) = read_yaml(&raw) else { return None };
    if doc.get("schema").and_then(Value::as_str).is_some_and(|s| s != FORMAT) {
        return None;
    }
    Some(Read { text: raw, doc })
}

/// the needs.yaml beside a flow, read; None when there is none. A file that cannot be read, or is not YAML, is an error
pub fn read_needs(dir: &str) -> Result<Option<Needs>> {
    let file = nodepath::join2(dir, NEEDS_FILE);
    if !exists(&file) {
        return Ok(None);
    }
    let bytes = std::fs::read(&file).map_err(|e| Error::graph(read_error(&e, &file), None))?;
    let doc = read_yaml(&String::from_utf8_lossy(&bytes))?;
    Ok(if doc.get_array("needs").is_some() { Some(Needs(doc)) } else { None })
}

/// what a walk leaves out: a state, or a node of ui.yaml, -> the missing needs drawn as it
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Planned {
    #[serde(serialize_with = "entries")]
    pub states: IndexMap<String, Vec<String>>,
    #[serde(serialize_with = "entries")]
    pub nodes: IndexMap<String, Vec<String>>,
}

/// a map written as its entries, `[...map]`
fn entries<S: serde::Serializer>(m: &IndexMap<String, Vec<String>>, s: S) -> std::result::Result<S::Ok, S::Error> {
    s.collect_seq(m.iter())
}

/// what a tool that walks the flow on the app leaves out (SPEC.md §13): the states and the nodes that missing needs are
/// drawn as and that no met need names, each with the ids of those needs
pub fn planned(needs: Option<&Needs>) -> Planned {
    let mut out = Planned::default();
    let mut built_states: IndexSet<String> = IndexSet::new();
    let mut built_nodes: IndexSet<String> = IndexSet::new();
    let items = needs.and_then(|n| n.0.get_array("needs")).map(|a| a.as_slice()).unwrap_or_default();
    for n in items {
        let Value::Object(n) = n else { continue };
        let Some(id) = n.get("id").and_then(Value::as_str) else { continue };
        for e in n.get("evidence").and_then(Value::as_array).into_iter().flatten() {
            let Value::Object(e) = e else { continue };
            for (key, into, built) in
                [("state", &mut out.states, &mut built_states), ("node", &mut out.nodes, &mut built_nodes)]
            {
                let Some(name) = e.get(key).and_then(Value::as_str) else { continue };
                if !is_missing(n) {
                    built.insert(name.to_string());
                } else {
                    into.entry(name.to_string()).or_default().push(id.to_string());
                }
            }
        }
    }
    for name in &built_states {
        out.states.shift_remove(name);
    }
    for name in &built_nodes {
        out.nodes.shift_remove(name);
    }
    out
}

/// the nearest gaps file at or above a folder, up to the repository's root (the first folder that holds .git)
fn nearest_gaps(dir: &str) -> Option<String> {
    let mut d = dir.to_string();
    loop {
        let file = nodepath::join2(&d, GAPS_FILE);
        if exists(&file) {
            return Some(file);
        }
        let up = nodepath::dirname(&d);
        if exists(&nodepath::join2(&d, ".git")) || up == d {
            return None;
        }
        d = up;
    }
}

/// `[\w$]`: what may not stand right before or after a name a file holds
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// the text holds the name, not as a part of a longer word: an identifier, or a test's name in quotes
fn holds(body: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(i) = body[from..].find(name).map(|i| i + from) {
        let before = body[..i].chars().next_back();
        let after = body[i + name.len()..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
            return true;
        }
        from = i + body[i..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// a line of the file sets the variable and is not commented out: NAME=, export NAME=, NAME: or - NAME=
fn sets(body: &str, name: &str) -> bool {
    let Ok(re) = Regex::new(&format!(r"^[ \t]*(?:-[ \t]*)?(?:export[ \t]+)?{}[ \t]*[=:]", regex::escape(name))) else {
        return false;
    };
    // a line starts at the text's start and after each line terminator, as JavaScript's multiline `^` reads them
    let starts = std::iter::once(0).chain(
        body.char_indices()
            .filter(|(_, c)| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            .map(|(i, c)| i + c.len_utf8()),
    );
    starts.into_iter().any(|at| re.is_match(&body[at..]))
}

/// what the flow beside a needs file has of its nodes
enum Nodes {
    Unread,    // flow.yaml is not there, or ui.yaml cannot be read
    NoScreens, // the flow has no ui.yaml
    Of(IndexSet<String>),
}

struct Context<'a> {
    app: String,
    code: &'a AppFiles, // the application's files, as a source: anchor reads them (SPEC.md §9)
    look_for: bool,     // false under --no-app: the application's files are not here
    dir: String,        // the flow's folder
    flow_id: Option<String>,
    states: Option<IndexSet<String>>, // None when flow.yaml cannot be read: its own check says why
    built: IndexMap<String, String>,  // the states of a captured flow that name their code, with their source
    nodes: Nodes,
}

fn file_at(ctx: &Context<'_>, path: &str) -> Option<String> {
    let abs = if path.starts_with('/') { path.to_string() } else { nodepath::join2(&ctx.app, path) };
    std::fs::metadata(&abs).is_ok_and(|m| m.is_file()).then_some(abs)
}

/// a binding's scenarios, those of the bindings it extends included (each named relative to the one that extends it)
fn scenarios_of(file: &str, seen: &mut IndexSet<String>) -> IndexSet<String> {
    let mut out = IndexSet::new();
    if seen.contains(file) || !exists(file) {
        return out;
    }
    seen.insert(file.to_string());
    let Some(read) = read_doc(file) else { return out };
    if let Some(Value::Object(s)) = read.doc.get("scenarios") {
        out.extend(s.keys().cloned());
    }
    if let Some(Value::String(extends)) = read.doc.get("extends") {
        out.extend(scenarios_of(&nodepath::join2(&nodepath::dirname(file), extends), seen));
    }
    out
}

/// whether a piece of evidence names a file of the application by its path from the repository's root: a source, a
/// test, the file a setting is set in, a binding named by its path (SPEC.md §13)
fn names_app_file(kind: Kind, e: &Object) -> bool {
    matches!(kind, Kind::Source | Kind::Test | Kind::Setting)
        || (kind == Kind::Scenario && text(e.get("binding")).is_some())
}

/// why a piece of evidence does not resolve, or None when it does (or when what it names cannot be read, which is
/// reported where it is)
fn unresolved(ctx: &Context<'_>, kind: Kind, e: &Object) -> Option<String> {
    let value = e.get(kind.as_str()).and_then(Value::as_str).unwrap_or("");
    let name = text(e.get("name"));
    match kind {
        Kind::Source => {
            if let Some(why) = ctx.code.unresolved(value) {
                return Some(format!("source {} {}", why.subject, why.reason));
            }
            let (path, _) = split_line(value);
            let name = name?;
            let body = read_text(&ctx.code.abs(path)).unwrap_or_default();
            (!holds(&body, name)).then(|| format!("source {path} holds no {name}"))
        }
        Kind::Test => {
            let Some(abs) = file_at(ctx, value) else { return Some(format!("test {value} does not exist")) };
            let name = name?;
            let body = read_text(&abs).unwrap_or_default();
            (!holds(&body, name)).then(|| format!("test {value} holds no test named \"{name}\""))
        }
        Kind::Scenario => {
            let given = text(e.get("binding"));
            let abs = match given {
                None => Some(nodepath::join2(&ctx.dir, "binding.yaml")),
                Some(g) => file_at(ctx, g),
            };
            let said = match given {
                None => "the flow's own binding.yaml".to_string(),
                Some(g) => format!("the binding {g}"),
            };
            let Some(abs) = abs.filter(|a| exists(a)) else {
                return Some(format!("scenario {value}: {said} does not exist"));
            };
            let Some(read) = read_doc(&abs) else { return Some(format!("scenario {value}: {said} cannot be read")) };
            if read.doc.get("kind").and_then(Value::as_str) != Some("binding") {
                return Some(format!("scenario {value}: {said} is not a binding (kind: binding)"));
            }
            if let Some(flow_id) = &ctx.flow_id {
                let bound = read.doc.get("flow");
                if !bound.is_some_and(|b| b.as_str() == Some(flow_id)) {
                    let bound = bound.filter(|b| !b.is_null()).map_or_else(|| "no flow".to_string(), js::to_js_string);
                    return Some(format!("scenario {value}: {said} binds {bound}, not {flow_id}"));
                }
            }
            let scenarios = scenarios_of(&abs, &mut IndexSet::new());
            if scenarios.contains(value) {
                return None;
            }
            let mut listed: Vec<String> = scenarios.into_iter().collect();
            js::sort_utf16(&mut listed);
            let has = if listed.is_empty() { String::new() } else { format!(" (it has {})", listed.join(", ")) };
            Some(format!("scenario {value} is not a scenario of {said}{has}"))
        }
        Kind::State => match &ctx.states {
            Some(states) if !states.contains(value) => {
                Some(format!("state {value} is not a state of {}", ctx.flow_id.as_deref().unwrap_or("the flow")))
            }
            _ => None,
        },
        Kind::Node => match &ctx.nodes {
            Nodes::NoScreens => {
                Some(format!("node {value}: the flow has no screens, as there is no ui.yaml beside it"))
            }
            Nodes::Of(nodes) if !nodes.contains(value) => {
                Some(format!("node {value} is not a node of the flow's ui.yaml"))
            }
            _ => None,
        },
        Kind::Setting => {
            let at = text(e.get("in"))?; // the schema says it is missing
            let Some(abs) = file_at(ctx, at) else { return Some(format!("setting {value}: {at} does not exist")) };
            let body = read_text(&abs).unwrap_or_default();
            (!sets(&body, value)).then(|| {
                format!("setting {value} is not set in {at}: no line sets it, or the one that does is commented out")
            })
        }
    }
}

/// the states of a captured flow that name their code (SPEC.md §9), which are therefore built, each with its anchor
fn built_states(flow: Option<&Object>) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    let Some(flow) = flow else { return out };
    if flow.get("status").and_then(Value::as_str) != Some("captured") {
        return out;
    }
    let Some(Value::Object(states)) = flow.get("states") else { return out };
    for (name, st) in states.iter() {
        if let Some(source) = st.get_str("source") {
            out.insert(name.clone(), source.to_string());
        }
    }
    out
}

/// the nodes of a ui.yaml, or None when it cannot be read
fn nodes_of(file: &str) -> Option<IndexSet<String>> {
    let read = read_doc(file)?;
    Some(read.doc.get("nodes").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default())
}

/// a gaps file as it lists the missing needs
struct Listed {
    id: String,
    gaps: IndexMap<String, Vec<String>>, // a flow's id -> the ids of its needs listed
    text: String,
}

/// gaps files, each read once per run; None when one cannot be read (the schema rule says why)
#[derive(Default)]
struct GapsReader {
    seen: HashMap<String, Option<Listed>>,
}

impl GapsReader {
    fn get(&mut self, file: &str) -> Option<&Listed> {
        if !self.seen.contains_key(file) {
            let listed = read_doc(file).map(|read| {
                let mut gaps = IndexMap::new();
                if let Some(Value::Object(o)) = read.doc.get("gaps") {
                    for (flow, ids) in o.iter() {
                        let ids =
                            ids.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
                        gaps.insert(flow.clone(), ids.unwrap_or_default());
                    }
                }
                let id = read
                    .doc
                    .get("id")
                    .and_then(Value::as_str)
                    .map_or_else(|| nodepath::basename(&nodepath::dirname(file)), str::to_string);
                Listed { id, gaps, text: read.text }
            });
            self.seen.insert(file.to_string(), listed);
        }
        self.seen.get(file).and_then(Option::as_ref)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Met,
    Missing,
    Unproven,
}

fn strs(path: &[&str]) -> Vec<String> {
    path.iter().map(|s| s.to_string()).collect()
}

/// every needs file and gaps file among `files` (the graph files under the folders checked), with the application's
/// repository root that evidence paths are relative to; with `app` None (--no-app), evidence that names a file of the
/// application is counted and not looked for, and the rest is held as before
pub fn check_needs(files: &[String], app: Option<&str>) -> NeedsCheck {
    let mut out = NeedsCheck::default();
    let mut gaps_of = GapsReader::default();
    let code = AppFiles::new(app.unwrap_or("."));
    // each needs file's flow and the state of each of its needs, by the gaps file that covers it; None for a needs
    // file that cannot be read, whose entries are then not judged
    let mut covered: HashMap<String, HashMap<String, Option<IndexMap<String, State>>>> = HashMap::new();
    let mut cover = |gaps_file: Option<&str>, flow: &str, states: Option<IndexMap<String, State>>| {
        let Some(gaps_file) = gaps_file else { return };
        covered.entry(gaps_file.to_string()).or_default().insert(flow.to_string(), states);
    };
    let problem = |out: &mut NeedsCheck,
                   rule: NeedRule,
                   file: &str,
                   body: &str,
                   path: &[String],
                   message: String,
                   graph: &str,
                   element: String| {
        let at = key_at(body, path);
        out.problems.push(NeedProblem {
            rule,
            file: file.to_string(),
            line: at.line,
            column: at.column,
            message,
            graph: graph.to_string(),
            element,
        });
    };

    for file in files.iter().filter(|f| nodepath::basename(f) == NEEDS_FILE) {
        let dir = nodepath::dirname(file);
        let read = read_doc(file).filter(|r| r.doc.get("needs").is_some_and(Value::is_array));
        let Some(read) = read else {
            let beside = read_doc(&nodepath::join2(&dir, "flow.yaml"));
            if let Some(id) = beside.as_ref().and_then(|b| b.doc.get("id")).and_then(Value::as_str) {
                cover(nearest_gaps(&dir).as_deref(), id, None);
            }
            continue;
        };
        out.files += 1;
        let body = &read.text;
        let flow_file = nodepath::join2(&dir, "flow.yaml");
        let flow = if exists(&flow_file) { read_doc(&flow_file) } else { None };
        let flow_id = flow.as_ref().and_then(|f| f.doc.get("id")).and_then(Value::as_str).map(str::to_string);
        let own_id = read.doc.get("id").and_then(Value::as_str).map(str::to_string);
        let graph = flow_id.clone().or_else(|| own_id.clone()).unwrap_or_else(|| nodepath::basename(&dir));
        if !exists(&flow_file) {
            problem(
                &mut out,
                NeedRule::NeedsPlace,
                file,
                body,
                &strs(&["id"]),
                "needs.yaml is beside no flow: it sits in a flow's folder, beside its flow.yaml, and lists that flow's needs (SPEC.md §13)".into(),
                &graph,
                "needs".into(),
            );
        } else if let (Some(flow_id), Some(own_id)) = (&flow_id, &own_id)
            && own_id != flow_id
        {
            problem(
                &mut out,
                NeedRule::NeedsPlace,
                file,
                body,
                &strs(&["id"]),
                format!(
                    "needs.yaml is the needs of {own_id}, and the flow beside it is {flow_id}: its id is the id of the flow beside it (SPEC.md §13)"
                ),
                &graph,
                "needs/id".into(),
            );
        }
        let ui_file = nodepath::join2(&dir, "ui.yaml");
        let ctx = Context {
            app: app.unwrap_or(".").to_string(),
            code: &code,
            look_for: app.is_some(),
            dir: dir.clone(),
            flow_id: flow_id.clone(),
            states: flow.as_ref().map(|f| {
                f.doc.get("states").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default()
            }),
            built: built_states(flow.as_ref().map(|f| &f.doc)),
            nodes: if !exists(&flow_file) {
                Nodes::Unread
            } else if exists(&ui_file) {
                nodes_of(&ui_file).map_or(Nodes::Unread, Nodes::Of)
            } else {
                Nodes::NoScreens
            },
        };
        let key = flow_id.clone().or_else(|| own_id.clone()).unwrap_or_else(|| graph.clone()); // the flow, as the gaps name it
        let gaps_file = nearest_gaps(&dir);
        // the ids the gaps file above lists under this flow; None when there is none, or it cannot be read
        let listed: Option<Vec<String>> =
            gaps_file.as_ref().and_then(|f| gaps_of.get(f)).map(|l| l.gaps.get(&key).cloned().unwrap_or_default());
        let mut states: IndexMap<String, State> = IndexMap::new();
        let mut first_at: IndexMap<String, usize> = IndexMap::new();

        for (i, item) in read.doc.get("needs").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let Value::Object(item) = item else { continue };
            let Some(id) = item.get("id").and_then(Value::as_str) else { continue };
            out.needs += 1;
            let at = strs(&["needs", &i.to_string()]);
            let with = |extra: &[&str]| {
                let mut p = at.clone();
                p.extend(extra.iter().map(|s| s.to_string()));
                p
            };
            let first = first_at.get(id).copied();
            match first {
                Some(first) => {
                    let line = key_at(body, &["needs".to_string(), first.to_string(), "id".to_string()]).line;
                    problem(
                        &mut out,
                        NeedRule::NeedTwice,
                        file,
                        body,
                        &with(&["id"]),
                        format!(
                            "need {id} is given twice: it is given first at line {line}, and a need's id comes once in its flow (SPEC.md §13)"
                        ),
                        &graph,
                        format!("needs/{id}/twice"),
                    );
                }
                None => {
                    first_at.insert(id.to_string(), i);
                }
            }
            let evidence = item.get("evidence").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            let mut outside = false;
            let mut kinds = 0;
            for (j, e) in evidence.iter().enumerate() {
                let Value::Object(e) = e else { continue };
                let Some(kind) = kind_of(e) else { continue }; // the schema says it names no kind
                kinds += 1;
                if !kind.in_graph() {
                    outside = true;
                }
                if !ctx.look_for && names_app_file(kind, e) {
                    out.not_looked_for += 1;
                    continue;
                }
                let what = e.get(kind.as_str()).map(js::to_js_string).unwrap_or_default();
                if let Some(why) = unresolved(&ctx, kind, e) {
                    problem(
                        &mut out,
                        NeedRule::EvidenceUnresolved,
                        file,
                        body,
                        &with(&["evidence", &j.to_string(), kind.as_str()]),
                        format!("need {id}: {why} (SPEC.md §13)"),
                        &graph,
                        format!("needs/{id}/evidence/{}:{what}", kind.as_str()),
                    );
                }
                // a missing need names only what is not built: a walk leaves out a state that only missing needs name
                let built = if kind == Kind::State && is_missing(item) { ctx.built.get(&what) } else { None };
                if let Some(built) = built {
                    problem(
                        &mut out,
                        NeedRule::NeedNamesBuilt,
                        file,
                        body,
                        &with(&["evidence", &j.to_string(), kind.as_str()]),
                        format!(
                            "need {id} is missing, and names state {what}, which is built: it names its code (source: {built}). A walk on the app leaves out a state that only missing needs name, so a missing need names only what is not built yet; say in its note that it is about {what} (SPEC.md §13)"
                        ),
                        &graph,
                        format!("needs/{id}/built/state:{what}"),
                    );
                }
            }
            let missing = is_missing(item);
            let state = if missing {
                State::Missing
            } else if outside {
                State::Met
            } else {
                State::Unproven
            };
            if first.is_none() {
                states.insert(id.to_string(), state);
            }
            if state == State::Unproven && kinds > 0 && kinds == evidence.len() {
                problem(
                    &mut out,
                    NeedRule::NeedNoEvidence,
                    file,
                    body,
                    &with(&["evidence"]),
                    format!(
                        "need {id} is not missing and names no evidence outside the graph: what it is drawn as shows nothing built; name a source, a test, a scenario or a setting, or say why it is missing (SPEC.md §13)"
                    ),
                    &graph,
                    format!("needs/{id}"),
                );
            }
            if !missing {
                continue;
            }
            out.gaps += 1;
            let why = text(item.get("missing")).unwrap_or("");
            let Some(ids) = &listed else {
                problem(
                    &mut out,
                    NeedRule::NeedUnlisted,
                    file,
                    body,
                    &with(&["missing"]),
                    format!(
                        "need {id} is missing, and there is no {GAPS_FILE} above it: list it in the product's gaps, at the root of its graphs (SPEC.md §13)"
                    ),
                    &graph,
                    format!("needs/{id}"),
                );
                continue;
            };
            if !ids.iter().any(|x| x == id) {
                problem(
                    &mut out,
                    NeedRule::NeedUnlisted,
                    file,
                    body,
                    &with(&["missing"]),
                    format!(
                        "need {id} is missing, and {GAPS_FILE} does not list it under {key}: list it there, or name its evidence and take out missing (SPEC.md §13)"
                    ),
                    &graph,
                    format!("needs/{id}"),
                );
            } else {
                problem(
                    &mut out,
                    NeedRule::NeedGap,
                    file,
                    body,
                    &with(&["missing"]),
                    format!("need {id} is missing: {why}"),
                    &graph,
                    format!("needs/{id}"),
                );
            }
        }

        cover(gaps_file.as_deref(), &key, Some(states));
    }

    // each gaps file among the files checked, held to the needs under its folder that it covers
    for file in files.iter().filter(|f| nodepath::basename(f) == GAPS_FILE) {
        let Some(listed) = gaps_of.get(file) else { continue };
        let by_flow = covered.get(file.as_str());
        for (flow, ids) in &listed.gaps {
            let states = match by_flow.and_then(|b| b.get(flow)) {
                Some(None) => continue,
                Some(Some(states)) => states,
                None => {
                    problem(
                        &mut out,
                        NeedRule::GapUnknown,
                        file,
                        &listed.text,
                        &strs(&["gaps", flow]),
                        format!(
                            "{GAPS_FILE} lists gaps of {flow}, which is no flow with a needs.yaml under its folder: take them out, or name the flow by its id (SPEC.md §13)"
                        ),
                        &listed.id,
                        format!("gaps/{flow}"),
                    );
                    continue;
                }
            };
            for (k, id) in ids.iter().enumerate() {
                let at = strs(&["gaps", flow, &k.to_string()]);
                match states.get(id) {
                    None => problem(
                        &mut out,
                        NeedRule::GapUnknown,
                        file,
                        &listed.text,
                        &at,
                        format!(
                            "{GAPS_FILE} lists {id} of {flow}, which is no need of its needs.yaml: take it out, or name the need by its id (SPEC.md §13)"
                        ),
                        &listed.id,
                        format!("gaps/{flow}/{id}"),
                    ),
                    Some(State::Met) => problem(
                        &mut out,
                        NeedRule::GapMet,
                        file,
                        &listed.text,
                        &at,
                        format!(
                            "{GAPS_FILE} lists {id} of {flow}, which is met now: take it out, as the list only shrinks (SPEC.md §13)"
                        ),
                        &listed.id,
                        format!("gaps/{flow}/{id}"),
                    ),
                    Some(_) => {}
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_examples_needs_and_gaps() {
        // node --input-type=module -e "import { checkNeeds } from './dist/needs.js'; console.log(JSON.stringify(checkNeeds(files, process.cwd())))"
        let root = nodepath::resolve(&[env!("CARGO_MANIFEST_DIR"), "../.."]);
        let files = [
            nodepath::join2(&root, "examples/notes/needs.gaps.yaml"),
            nodepath::join2(&root, "examples/notes/restore/needs.yaml"),
        ];
        for (app, not_looked_for) in [(Some(root.as_str()), 0), (None, 7)] {
            let said = check_needs(&files, app);
            assert_eq!((said.files, said.needs, said.gaps, said.not_looked_for), (1, 4, 1, not_looked_for));
            let p: Vec<(&str, &str, usize, usize, &str, &str)> = said
                .problems
                .iter()
                .map(|p| (p.rule.as_str(), p.file.as_str(), p.line, p.column, p.graph.as_str(), p.element.as_str()))
                .collect();
            assert_eq!(
                p,
                [("need-gap", files[1].as_str(), 47, 5, "example.restoreNote", "needs/says-when-it-is-erased")]
            );
            assert_eq!(
                said.problems[0].message,
                "need says-when-it-is-erased is missing: archived notes are not erased yet; the second phase erases them after NOTES_KEEP_ARCHIVED_DAYS"
            );
        }
    }

    #[test]
    fn what_a_walk_leaves_out() {
        // the CI step "Evidence is found under --app, and a walk leaves out what only missing needs draw"
        let entries = |m: &IndexMap<String, Vec<String>>| -> Vec<(String, Vec<String>)> {
            m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };
        let dir = |d: &str| nodepath::resolve(&[env!("CARGO_MANIFEST_DIR"), "../../examples", d]);
        let restore = read_needs(&dir("notes/restore")).expect("it reads");
        let p = planned(restore.as_ref());
        assert_eq!(entries(&p.nodes), [("keptFor".to_string(), vec!["says-when-it-is-erased".to_string()])]);
        assert!(p.states.is_empty());
        assert_eq!(read_needs(&dir("notes/edit")).expect("it reads"), None);
        let shared = js::json_parse(
            r#"{"kind":"needs","id":"x","needs":[{"id":"built","need":"A","evidence":[{"state":"a"},{"test":"t.js"}]},{"id":"later","need":"B","evidence":[{"state":"a"},{"state":"b"},{"node":"n"}],"missing":"the second phase"},{"id":"after","need":"C","evidence":[{"state":"b"}],"missing":"the third phase"}]}"#,
        )
        .expect("JSON");
        let p = planned(Some(&Needs(shared)));
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(entries(&p.states), [("b".to_string(), ids(&["later", "after"]))]);
        assert_eq!(entries(&p.nodes), [("n".to_string(), ids(&["later"]))]);
    }

    #[test]
    fn names_held_and_settings_set() {
        assert!(holds("function restoreNote() {}", "restoreNote"));
        assert!(!holds("function restoreNotes() {}", "restoreNote"));
        assert!(!holds("$restoreNote", "restoreNote"));
        assert!(holds("aa aaa aa", "aaa"));
        assert!(holds("test('restores an archived note', ...)", "restores an archived note"));
        assert!(sets("A=1\nNOTES_SESSION_SECRET=x", "NOTES_SESSION_SECRET"));
        assert!(sets("  - export KEY: v", "KEY"));
        assert!(sets("x\rKEY=1", "KEY"));
        assert!(!sets("# KEY=1", "KEY"));
        assert!(!sets("KEYS=1", "KEY"));
    }
}
