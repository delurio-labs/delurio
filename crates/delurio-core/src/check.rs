//! `check`: every graph under a folder, as a project's CI runs it (graphs/agents/cli.check, SPEC.md §10). It reads the
//! files and nothing else, writes no page, and gives each finding as a GNU error line, or as SARIF 2.1.0 with
//! --format sarif. Exit 0 when clean, 1 when a graph cannot be loaded or is written for a newer format, 2 when an error
//! is found under --strict (and, with --baseline, new since that run). The port of check.ts.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::io::Write;
use std::sync::LazyLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;
use serde::Serialize;

use crate::anchors::check_anchors;
use crate::changes::{Domain, DomainsSeen, check_changes, flow_domain};
use crate::domain::{DomainPart, MergedDomain, in_reading_order, merge_domain};
use crate::domain_names::{EntityPlace, check_domain_names};
use crate::error::{At, Error, ErrorPlace, Result};
use crate::graph::{Graph, alternatives, load, read_error};
use crate::holds::{HOLDS_UNKNOWN, check_holds};
use crate::invokes::check_invokes;
use crate::js::{self, Object, Value};
use crate::kit::resolve_kit;
use crate::model::{Model, check_assertions, explore};
use crate::nav::{LoadedNav, check_goals, findings, load_nav, view_of};
use crate::needs::{check_needs, planned, read_needs};
use crate::nodepath::{self, basename, dirname, join2, relative, resolve1};
use crate::schemas::{SchemaKind, schema_kind_of, validate_file};
use crate::starts::check_starts;
use crate::story::steps_by_events;
use crate::tutorial_check::check_tutorial;
use crate::ui_check::{EVENT_UNHANDLED, check_events, check_ui};
use crate::wireframe::render_screens;
use crate::yaml::{key_at, parsing_once, read_yaml};

pub const CHECK_USAGE: &str = "  delurio check <graphs folder>... [--strict] [--baseline <sarif>] [--format sarif --output <file>] [--app <dir> | --no-app]
    checks every graph under the folders and writes nothing else: GNU error lines on stdout, or SARIF 2.1.0;
    exit 0 clean, 1 a graph cannot be loaded or needs a newer format, 2 an error under --strict (new since --baseline);
    the evidence of needs and the source: anchors are found under --app, the application's repository root
    (default: the working directory); --no-app checks the graphs without the code, and does not look for them";

/// the version of the checker written into SARIF: the package's (`toolVersion` of check.ts reads package.json)
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const FORMAT: &str = "delurio-graph/0";
const SKIP: [&str; 5] = ["viz", "video", "recording", "node_modules", ".git"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Note,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Note => "note",
        }
    }
}

/// a place and what is there: a finding's note
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Note {
    pub file: String, // as given on the command line's side: relative to the working directory
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BaselineState {
    New,
    Unchanged,
}

impl BaselineState {
    pub fn as_str(self) -> &'static str {
        match self {
            BaselineState::New => "new",
            BaselineState::Unchanged => "unchanged",
        }
    }
}

/// a finding of the check, in the order of the TypeScript object's keys
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub rule: String,
    pub level: Level,
    pub file: String, // as given on the command line's side: relative to the working directory
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub graph: String,   // the graph's id
    pub element: String, // what in the graph it is about, for the fingerprint and the logical location
    pub notes: Vec<Note>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_state: Option<BaselineState>,
}

struct Rule {
    id: &'static str,
    text: &'static str,
    level: Level,
}

const fn rule(id: &'static str, text: &'static str, level: Level) -> Rule {
    Rule { id, text, level }
}

const E: Level = Level::Error;
const W: Level = Level::Warning;
const N: Level = Level::Note;

#[rustfmt::skip]
const RULES: &[Rule] = &[
    rule("load", "The graph cannot be loaded", E),
    rule("format-newer", "The graph is written for a newer format than this checker reads", E),
    rule("schema", "A graph file does not follow its kind's schema (SPEC.md)", E),
    rule("domain-twice", "A value of a domain is given in two of its files (SPEC.md §5)", E),
    rule("domain-place", "A domain file is not with the rest of its domain (SPEC.md §1)", E),
    rule("server-fn-http", "A server function is given a route, a query or an operationId, which only an HTTP operation has (SPEC.md §5)", E),
    rule("entity-unknown", "An action's returns or a references names an entity its domain does not have (SPEC.md §5)", E),
    rule("field-unknown", "An entity's key or required names a field the entity does not have (SPEC.md §5)", E),
    rule("assertion-violated", "A guarantee of the flow is broken by a reachable situation", E),
    rule("main-story-unwalkable", "The declared main story cannot be walked", E),
    rule("catalog-key-missing", "A catalog key the screens use is missing from a locale's catalog", E),
    rule("sample-missing", "A catalog message has a placeholder with no sample value", E),
    rule("no-renderer", "A component has no renderer", E),
    rule("kit-uncovered", "The kit does not cover a component or an intent", E),
    rule("list-item-outside-list", "A list item is drawn outside a list", E),
    rule("hand-over", "A started flow cannot begin with what it is given", E),
    rule("changes-note-missing", "A flow whose steps write has no changes note (SPEC.md §4)", E),
    rule("changes-note-locale", "The changes note is missing from a locale's catalog", E),
    rule("changes-note-gap", "A writing flow says why it has no changes note yet: counted, and fails nothing (SPEC.md §4)", N),
    rule("task-locale", "The flow's name for its users is missing from a locale's catalog (SPEC.md §4)", E),
    rule("source-missing", "A state of a captured flow names no code (SPEC.md §9)", E),
    rule("source-unresolved", "A source anchor names a file that is not there, or a line past its end (SPEC.md §9)", E),
    rule("citation-unresolved", "Words that cite the code name a file that is not there, or a line past its end (SPEC.md §9)", W),
    rule("focus-not-shown", "A focus or a returnsFocus gives the focus to a node that is not shown there (SPEC.md §4)", E),
    rule("returns-focus-not-dialog", "A returnsFocus does not start at a dialog (SPEC.md §4)", E),
    rule("event-unhandled", "A node sends an event that a state showing it enabled does not handle (SPEC.md §4)", E),
    rule("during-step-unused", "A node is marked duringStep where no state that runs a step shows it enabled, or it sends NAVIGATE (SPEC.md §4)", W),
    rule("role-missing", "A field or an action gives no role, by which a screen reader finds it (SPEC.md §4)", W),
    rule("state-no-way-out", "A state that is not an exit has no way out (SPEC.md §2)", E),
    rule("state-unreachable", "No situation the flow starts in reaches the state (SPEC.md §2)", E),
    rule("tutorial-mismatch", "A tutorial no longer matches its flow (SPEC.md §7)", E),
    rule("evidence-unresolved", "A need's evidence names what is not there (SPEC.md §13)", E),
    rule("need-no-evidence", "A need that is not missing names no evidence outside the graph (SPEC.md §13)", E),
    rule("need-names-built", "A missing need names a state that is built as its evidence (SPEC.md §13)", W),
    rule("need-unlisted", "A missing need is not in the product's gaps (SPEC.md §13)", E),
    rule("need-twice", "A need's id is given twice in its flow's needs (SPEC.md §13)", E),
    rule("needs-place", "A needs file is beside no flow, or is the needs of another one (SPEC.md §13)", E),
    rule("gap-met", "The gaps list a need that is met (SPEC.md §13)", E),
    rule("gap-unknown", "The gaps list a need that no flow under them has (SPEC.md §13)", E),
    rule("need-gap", "A need that is missing, listed in the gaps: counted, and fails nothing (SPEC.md §13)", N),
    rule("invoke-unknown", "An invoke or a failures entry names an action its flow's domain does not have, or the domain is not found (SPEC.md §5)", E),
    rule("refusal-unknown", "A failure class lists a code that is neither a refusal of its action nor a transport code (SPEC.md §2, §5)", E),
    rule("policy-unknown", "A policy names an action or an entity its domain does not have (SPEC.md §5)", E),
    rule("planned-built", "A planned entity or action names its code (SPEC.md §5)", E),
    rule("graph-warning", "Another warning of the graph", E),
    rule("nav-problem", "A problem of the navigation graph", E),
    rule("nav-note", "A note on the navigation graph", W),
];

/// `LEVEL.get(rule) ?? 'error'`
fn level_of(rule: &str) -> Level {
    RULES.iter().find(|r| r.id == rule).map_or(Level::Error, |r| r.level)
}

// the rules whose errors stop the run (exit 1) however --strict and --baseline read them
const BREAKING: [&str; 2] = ["load", "format-newer"];

/// `count(n, word)`: `1 graph`, `2 graphs`
pub fn count(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// a file's text as `readFileSync(path, 'utf8')` reads it; None when it cannot be read (where the TS crashed, the
/// check reads it as empty)
fn read_text(path: &str) -> Option<String> {
    std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

fn is_dir(path: &str) -> bool {
    std::path::Path::new(path).is_dir()
}

// the 1-based line and column of the first occurrence of `needle`, else the file's start
fn place_of(abs: &str, needle: Option<&str>) -> At {
    let start = At { line: 1, column: 1 };
    let Some(needle) = needle else { return start };
    if !exists(abs) {
        return start;
    }
    let text = read_text(abs).unwrap_or_default();
    for (i, line) in text.split('\n').enumerate() {
        if let Some(c) = line.find(needle) {
            return At { line: i + 1, column: js::utf16_at(line, c) + 1 };
        }
    }
    start
}

/// the names in a folder, as `readdirSync(dir).sort()` gives them
fn sorted_names(dir: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    js::sort_utf16(&mut names);
    names
}

/// every graph folder at or under root, in path order
pub fn graph_dirs(root: &str) -> Vec<String> {
    let mut out = Vec::new();
    graph_dirs_into(root, &mut out);
    out
}

fn graph_dirs_into(root: &str, out: &mut Vec<String>) {
    if exists(&join2(root, "flow.yaml")) || exists(&join2(root, "nav.yaml")) {
        out.push(root.to_string());
    }
    for name in sorted_names(root) {
        if SKIP.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        let sub = join2(root, &name);
        if is_dir(&sub) {
            graph_dirs_into(&sub, out);
        }
    }
}

// every file under root that a schema covers (flow, ui, nav, nav.extracted, domain, catalog, needs, gaps, binding), in
// path order
fn graph_files(root: &str, out: &mut Vec<String>) {
    for name in sorted_names(root) {
        if SKIP.contains(&name.as_str()) || name.starts_with('.') {
            continue;
        }
        let path = join2(root, &name);
        if is_dir(&path) {
            graph_files(&path, out);
        } else if schema_kind_of(&path).is_some() {
            out.push(path);
        }
    }
}

// the format the file says it is written in: None when it is this one
fn newer_format(abs: &str) -> Result<Option<String>> {
    let bytes = std::fs::read(abs).map_err(|e| Error::graph(read_error(&e, abs), None))?;
    let doc = read_yaml(&String::from_utf8_lossy(&bytes))?;
    let s = doc.get_str("schema").unwrap_or(FORMAT);
    Ok(if s == FORMAT { None } else { Some(s.to_string()) })
}

fn rel(abs: &str) -> String {
    let r = relative(&nodepath::cwd(), abs);
    if r.is_empty() { ".".to_string() } else { r }
}

fn result(
    rule: &str,
    file: &str,
    at: At,
    message: impl Into<String>,
    graph: &str,
    element: impl Into<String>,
) -> CheckResult {
    result_with(rule, file, at, message, graph, element, Vec::new())
}

fn result_with(
    rule: &str,
    file: &str,
    at: At,
    message: impl Into<String>,
    graph: &str,
    element: impl Into<String>,
    notes: Vec<Note>,
) -> CheckResult {
    CheckResult {
        rule: rule.to_string(),
        level: level_of(rule),
        file: rel(file),
        line: at.line,
        column: at.column,
        message: message.into(),
        graph: graph.to_string(),
        element: element.into(),
        notes,
        baseline_state: None,
    }
}

/// JavaScript's `\S` without the `u` flag: anything but its white space and line terminators
const S: &str = r"[^\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// JavaScript's `.` without the `s` flag: anything but a line terminator
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
/// JavaScript's `\w` without the `u` flag
const WORD: &str = "[A-Za-z0-9_]";

fn js_regex(pattern: &str) -> Regex {
    Regex::new(&pattern.replace("\\S", S).replace("\\w", WORD).replace('.', DOT)).expect("the pattern compiles")
}

static CATALOG_KEY: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^catalog key (\S+) is missing"));
static NO_SAMPLE: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^no sample value for \{(\w+)\} in (\S+)"));
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^(\S+): ListItem outside a List"));
static KIT_NODE: LazyLock<Regex> = LazyLock::new(|| js_regex(r"\(node (\S+?)\)"));
static NO_RENDERER: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^(\S+): component .* has no wireframe renderer"));
static STARTS: LazyLock<Regex> = LazyLock::new(|| js_regex(r"^(\S+) starts (\S+): (.*)$"));
static CHANGES_LOCALE: LazyLock<Regex> =
    LazyLock::new(|| js_regex(r"^the changes note (\S+) is not in the (\w+) catalog"));

// a warning raised while checking a flow, sorted into its rule and placed in its file
fn from_warning(dir: &str, graph: &str, w: &str) -> CheckResult {
    let flow = join2(dir, "flow.yaml");
    let ui = join2(dir, "ui.yaml");
    if let Some(m) = CATALOG_KEY.captures(w) {
        return result("catalog-key-missing", &ui, place_of(&ui, Some(&m[1])), w, graph, &m[1]);
    }
    if let Some(m) = NO_SAMPLE.captures(w) {
        return result("sample-missing", &ui, place_of(&ui, Some(&m[2])), w, graph, format!("{}/{}", &m[2], &m[1]));
    }
    if let Some(m) = LIST_ITEM.captures(w) {
        return result("list-item-outside-list", &ui, place_of(&ui, Some(&format!("{}:", &m[1]))), w, graph, &m[1]);
    }
    if let Some(m) = KIT_NODE.captures(w)
        && w.starts_with("kit ")
    {
        return result("kit-uncovered", &ui, place_of(&ui, Some(&format!("{}:", &m[1]))), w, graph, &m[1]);
    }
    if let Some(m) = NO_RENDERER.captures(w) {
        return result("no-renderer", &ui, place_of(&ui, Some(&format!("{}:", &m[1]))), w, graph, &m[1]);
    }
    if w.contains("declared main story cannot be walked") {
        return result("main-story-unwalkable", &flow, place_of(&flow, Some("main:")), w, graph, "main");
    }
    if let Some(m) = STARTS.captures(w) {
        let at = place_of(&flow, Some(&format!("flow: {}", &m[2])));
        return result("hand-over", &flow, at, w, graph, format!("{}->{}", &m[1], &m[2]));
    }
    if w.contains("has no changes note") {
        return result("changes-note-missing", &ui, place_of(&ui, Some("samples:")), w, graph, "changes");
    }
    if let Some(m) = CHANGES_LOCALE.captures(w) {
        let element = format!("changes/{}", &m[2]);
        return result("changes-note-locale", &ui, place_of(&ui, Some("changes:")), w, graph, element);
    }
    let file = if exists(&ui) { ui } else { flow };
    result("graph-warning", &file, At { line: 1, column: 1 }, w, graph, w)
}

/// `for (const locale of g.ui.locales ?? [])`, each as a template literal writes it; a value that is no list is none
fn locales_of(g: &Graph) -> Vec<String> {
    g.ui.get_array("locales").map(|a| a.iter().map(js::to_js_string).collect()).unwrap_or_default()
}

// the words of the screens in each locale but the one ui.yaml names (SPEC.md §4): a key the screens use that a locale's
// catalog lacks, at the key's line, as for the catalog ui.yaml names; a locale whose catalog is not there, at `locales`.
// The locale's catalog is lent to the graph while its screens are drawn (`{ ...g, catalog }`), and given back after.
fn check_locales(g: &mut Graph, dir: &str) -> Vec<CheckResult> {
    if g.ui().absent() {
        return Vec::new();
    }
    let ui = join2(dir, "ui.yaml");
    let catalog_name = g.ui().catalog().unwrap_or("").to_string();
    let named = join2(dir, &catalog_name);
    let id = g.flow().id_text();
    let mut out = Vec::new();
    for locale in locales_of(g) {
        let name = join2(&dirname(&catalog_name), &format!("{locale}.json"));
        let abs = join2(dir, &name);
        if abs == named {
            continue;
        }
        let parsed = match std::fs::read(&abs) {
            Err(e) => Err(read_error(&e, &abs)),
            Ok(bytes) => js::json_parse(&String::from_utf8_lossy(&bytes)).map_err(|e| e.message),
        };
        let catalog = match parsed {
            Ok(c) => c,
            Err(message) => {
                let text = read_text(&ui).unwrap_or_default();
                let why = if exists(&abs) { format!("is not JSON: {message}") } else { "does not exist".to_string() };
                out.push(result(
                    "catalog-key-missing",
                    &ui,
                    key_at(&text, &["locales"]),
                    format!("the {locale} catalog, {name}, {why}: every word the screens use is missing in {locale}"),
                    &id,
                    format!("catalog/{locale}"),
                ));
                continue;
            }
        };
        let mut missing: Vec<String> = Vec::new();
        let mut collect = |m: &str| {
            if let Some(c) = CATALOG_KEY.captures(m) {
                let key = &c[1];
                if !missing.iter().any(|k| k == key) {
                    missing.push(key.to_string());
                }
            }
        };
        let other = if catalog.is_collection() { catalog } else { Value::Object(Object::new()) };
        let own = std::mem::replace(&mut g.catalog, other);
        render_screens(g, &mut collect, None);
        check_ui(g, &mut collect);
        g.catalog = own;
        for key in missing {
            out.push(result(
                "catalog-key-missing",
                &ui,
                place_of(&ui, Some(&key)),
                format!("catalog key {key} is missing from the {locale} catalog, {name}"),
                &id,
                format!("{key}/{locale}"),
            ));
        }
    }
    out
}

// the way to a situation as notes at a place: its start, each event sent, and the state it reaches
fn path_notes(file: &str, at: At, start: &Object, events: &[String], state: &str) -> Vec<Note> {
    let start = start.iter().map(|(k, x)| format!("{k} = {}", js::to_js_string(x))).collect::<Vec<_>>().join(", ");
    let note = |message: String| Note { file: rel(file), line: at.line, column: at.column, message };
    let start = if start.is_empty() { "the initial context".to_string() } else { start };
    let mut notes = vec![note(format!("start: {start}"))];
    for (i, e) in events.iter().enumerate() {
        notes.push(note(format!("step {}: {e}", i + 1)));
    }
    notes.push(note(format!("reached: {state}")));
    notes
}

// the states that some transition leads to, whatever its guard
fn targeted(g: &Graph) -> HashSet<&str> {
    let mut out = HashSet::new();
    for (_, st) in g.flow().states() {
        out.extend(st.always().iter().filter_map(|a| a.target()));
        for (_, spec) in st.on() {
            out.extend(alternatives(spec).iter().filter_map(|a| a.target()));
        }
    }
    out
}

fn check_flow(dir: &str, domains: &mut DomainsSeen) -> Result<Vec<CheckResult>> {
    let file = join2(dir, "flow.yaml");
    let mut g = load(dir)?;
    let kit = match resolve_kit(dir, g.ui().kit(), None) {
        Ok(kit) => kit,
        Err(err) => return Err(if g.ui().has_kit() { err.placed(ErrorPlace::at("ui.yaml", &["kit"])) } else { err }),
    };
    let model = Model::new(g.flow.clone());
    let ex = explore(&model, None, None)?;
    let mut out = Vec::new();
    let flow_text = read_text(&file).unwrap_or_default();
    let id = g.flow().id_text();
    // a state the flow stops in for good though it is no exit, and a state no situation reaches (SPEC.md §2)
    let led = targeted(&g);
    for (name, st) in g.flow().states() {
        let at = || key_at(&flow_text, &["states", name]);
        let element = format!("states/{name}");
        let ways = st.0.get_object("on").map_or(0, Object::len) + st.0.get_array("always").map_or(0, Vec::len);
        if !st.is_final() && ways == 0 && st.invoke().is_none() {
            out.push(result(
                "state-no-way-out",
                &file,
                at(),
                format!(
                    "state {name} is not an exit and has no way out: it has no on, always or invoke, so the flow stops there; make it an exit (type: final) or give it a transition (SPEC.md §2)"
                ),
                &id,
                &element,
            ));
        }
        if !ex.at_state.contains_key(name) {
            let why = if led.contains(name) {
                "the transitions that lead to it are never taken: the states they leave are not reached, or their guards never hold"
            } else {
                "no transition leads to it"
            };
            out.push(result(
                "state-unreachable",
                &file,
                at(),
                format!("state {name} is reached by no situation the flow starts in: {why} (SPEC.md §2)"),
                &id,
                &element,
            ));
        }
    }
    let mut warnings: Vec<String> = Vec::new();
    let mut warn = |m: &str| {
        if !warnings.iter().any(|w| w == m) {
            warnings.push(m.to_string());
        }
    };
    for a in check_assertions(&model, &ex)? {
        let Some(v) = &a.violation else { continue };
        let at = place_of(&file, Some(&format!("id: {}", a.id)));
        let notes = path_notes(&file, at, &v.start, &v.events, &v.state);
        let said = a.say.as_ref().unwrap_or(&a.never);
        out.push(result_with(
            "assertion-violated",
            &file,
            at,
            format!("{} is broken: {said}", a.id),
            &id,
            &a.id,
            notes,
        ));
    }
    if let Some(main) = g.flow().main()
        && steps_by_events(&model, main)?.is_none()
    {
        warn("the declared main story cannot be walked: an event is not possible where it is sent");
    }
    render_screens(&g, &mut warn, kit.as_ref());
    if !g.ui().absent() {
        let ui = join2(dir, "ui.yaml");
        let text = read_text(&ui).unwrap_or_default();
        for p in check_ui(&g, &mut warn) {
            out.push(result(p.rule, &ui, key_at(&text, &p.path), p.message, &id, p.element));
        }
        // a node's event is handled in every situation the exploration reached that shows the node enabled (SPEC.md §4)
        for u in check_events(&g, &model, &ex) {
            let at = key_at(&text, &u.path);
            // the state the way reaches: for a node marked duringStep, the one its step's outcome leads to
            let reached = u.lands.as_deref().unwrap_or(&u.state);
            let notes = u.reached.as_ref().map_or_else(Vec::new, |p| path_notes(&ui, at, &p.start, &p.events, reached));
            out.push(result_with(EVENT_UNHANDLED, &ui, at, u.message, &id, u.element, notes));
        }
    }
    out.extend(check_locales(&mut g, dir));
    for h in check_starts(&g, &model, &ex)? {
        for p in &h.problems {
            warn(&format!("{} starts {}: {p}", h.from, h.flow));
        }
    }
    // the flow's domain, looked up once for the rules that read it
    let domain: OnceCell<Option<Domain>> = OnceCell::new();
    let changes = check_changes(&g, || domain.get_or_init(|| flow_domain(&g, Some(&mut *domains))).clone());
    for p in changes.as_ref().map_or(&[][..], |c| c.problems.as_slice()) {
        warn(p);
    }
    for w in &warnings {
        out.push(from_warning(dir, &id, w));
    }
    // a writing flow whose app has no changes message yet says why: a gap of the flow, at the level note (SPEC.md §4)
    if let Some(c) = &changes
        && let Some(missing) = &c.missing
    {
        let ui = join2(dir, "ui.yaml");
        let message = format!(
            "its steps write ({}), and its changes note is missing: {missing}; Rehber does not guide its writing steps (SPEC.md §4)",
            c.writes.join(", ")
        );
        let at = key_at(&read_text(&ui).unwrap_or_default(), &["changes", "missing"]);
        out.push(result("changes-note-gap", &ui, at, message, &id, "changes/missing"));
    }
    for p in check_invokes(&g, || domain.get_or_init(|| flow_domain(&g, Some(&mut *domains))).clone()) {
        out.push(result(p.rule.as_str(), &file, key_at(&flow_text, &p.path), p.message, &id, p.element));
    }
    // what a node holds names a field of the flow's domain (SPEC.md §4)
    let holds = check_holds(&g, || domain.get_or_init(|| flow_domain(&g, Some(&mut *domains))).clone());
    if !holds.is_empty() {
        let ui = join2(dir, "ui.yaml");
        let text = read_text(&ui).unwrap_or_default();
        for p in holds {
            out.push(result(HOLDS_UNKNOWN, &ui, key_at(&text, &p.path), p.message, &id, p.element));
        }
    }
    let tutorial = join2(dir, "tutorial.yaml");
    if exists(&tutorial) {
        let text = read_text(&tutorial).unwrap_or_default();
        for p in check_tutorial(&g, &model, &ex, &text)? {
            let at = match p.line {
                None => key_at(&text, &p.path),
                Some(line) => At { line, column: p.column.unwrap_or(1) },
            };
            let element = format!("tutorial.yaml#/{}", p.path.join("/"));
            out.push(result("tutorial-mismatch", &tutorial, at, p.message, &id, element));
        }
    }
    // a captured flow is read from code, so every state names the code that makes it true; an exit may leave it out, as
    // the transition into it names that code, and so may a state that only missing needs name, which is not built yet
    // (SPEC.md §9, §13)
    if g.flow().status() == Some("captured") {
        // a needs.yaml that cannot be read plans nothing; its schema check says why
        let needs = read_needs(dir).ok().flatten();
        let not_built = planned(needs.as_ref()).states;
        for (name, st) in g.flow().states() {
            if st.has_source() || st.is_final() || not_built.contains_key(name) {
                continue;
            }
            out.push(result(
                "source-missing",
                &file,
                key_at(&flow_text, &["states", name]),
                format!(
                    "state {name} names no code: in a captured flow every state but an exit carries source: <path>:<line> (SPEC.md §9)"
                ),
                &id,
                format!("states/{name}"),
            ));
        }
    }
    Ok(out)
}

// what only an HTTP operation has (SPEC.md §5), and why a server function has none of it
const HTTP_ONLY: [(&str, &str); 3] = [
    (
        "route",
        "a route is an operation that others call over HTTP, and a server function is called only by the product's own pages: give one of them",
    ),
    ("query", "a server function's input is its body, whatever its method"),
    ("operationId", "a server function is no operation of an OpenAPI document"),
];

/// each part of a split domain read on its own, None for one that does not parse
fn parts_read(parts: &[DomainPart]) -> Vec<Option<Value>> {
    parts.iter().map(|p| read_yaml(&p.text).ok()).collect()
}

/// the part that gives `section.name.key` (`d?.[section]?.[name]?.[key] !== undefined`), and where in it
fn given_in(parts: &[DomainPart], read: &[Option<Value>], path: [&str; 3]) -> Option<(String, At)> {
    let i = read.iter().position(|d| {
        d.as_ref().and_then(|d| d.get(path[0])).and_then(|s| s.get(path[1])).and_then(|a| a.get(path[2])).is_some()
    })?;
    Some((parts[i].file.clone(), key_at(&parts[i].text, &path)))
}

/// `String(v)` of a key that may be absent
fn text_of(v: Option<&Value>) -> String {
    v.map_or_else(|| "undefined".to_string(), js::to_js_string)
}

// a server function that is also given what only an HTTP operation has (SPEC.md §5), at its `serverFn` with a note at
// the other key; the two may come from two files of a split domain, so the domain is read as one first
fn check_server_functions(parts: &[DomainPart], doc: &Object, id: &str) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let read = parts_read(parts);
    let Some(actions) = doc.get("actions").and_then(Value::as_object) else { return out };
    for (name, a) in actions.iter() {
        if !a.is_object() || a.get("serverFn").is_none() {
            continue;
        }
        let Some((at_file, at)) = given_in(parts, &read, ["actions", name, "serverFn"]) else { continue };
        for (key, why) in HTTP_ONLY {
            if a.get(key).is_none() {
                continue;
            }
            let other = given_in(parts, &read, ["actions", name, key]);
            let said = match key {
                "route" => format!("a route, {}", text_of(a.get("route"))),
                "query" => "a query".to_string(),
                _ => format!("an operationId, {}", text_of(a.get("operationId"))),
            };
            let notes = other
                .map(|(f, o)| {
                    vec![Note {
                        file: rel(&f),
                        line: o.line,
                        column: o.column,
                        message: format!("its {key} is given here"),
                    }]
                })
                .unwrap_or_default();
            let message = format!(
                "the action {name} is a server function (serverFn: {}) and has {said}: {why} (SPEC.md §5)",
                text_of(a.get("serverFn"))
            );
            out.push(result_with("server-fn-http", &at_file, at, message, id, format!("actions/{name}/{key}"), notes));
        }
    }
    out
}

// an entity or an action that says it is planned, written before its code, and names its code all the same (SPEC.md
// §5): at its `planned`, with a note where its `source` is given, as the two may come from two files of a split domain
fn check_planned(parts: &[DomainPart], doc: &Object, id: &str) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let read = parts_read(parts);
    for (section, word) in [("entities", "entity"), ("actions", "action")] {
        let Some(items) = doc.get(section).and_then(Value::as_object) else { continue };
        for (name, item) in items.iter() {
            let (Some(planned), Some(source)) = (item.get_str("planned"), item.get_str("source")) else { continue };
            let at = given_in(parts, &read, [section, name, "planned"]);
            let src = given_in(parts, &read, [section, name, "source"]);
            let Some((at_file, at)) = at else { continue };
            let notes = src
                .map(|(f, s)| {
                    vec![Note {
                        file: rel(&f),
                        line: s.line,
                        column: s.column,
                        message: "its source is given here".to_string(),
                    }]
                })
                .unwrap_or_default();
            let message = format!(
                "the {word} {name} says it is planned ({planned}), and names its code, {source}: a planned {word} has no code yet; once it is built, take out planned (SPEC.md §5)"
            );
            out.push(result_with(
                "planned-built",
                &at_file,
                at,
                message,
                id,
                format!("{section}/{name}/planned"),
                notes,
            ));
        }
    }
    out
}

// whether a domain's file gives the entity
fn gives_entity(text: &str, entity: &str) -> bool {
    read_yaml(text).ok().and_then(|d| d.get_object("entities").map(|e| e.contains_key(entity))).unwrap_or(false)
}

/// a domain as `checkDomains` reads it: its directory, its files, and the files read as one
struct ReadDomain {
    dir: String,
    parts: Vec<DomainPart>,
    merged: MergedDomain,
}

impl ReadDomain {
    /// `merged.id ?? rel(dir)`
    fn id(&self) -> String {
        self.merged.id.clone().unwrap_or_else(|| rel(&self.dir))
    }
}

// the domains under root, each the domain files of one directory read as one (SPEC.md §5): a value given in two of
// them, a file whose id is not its directory's, a domain whose files sit in two directories, a server function given
// what only an HTTP operation has, and a name of an entity, a field or an action that the domain does not have
fn check_domains(files: &[String]) -> Vec<CheckResult> {
    let mut by_dir: IndexMap<String, Vec<String>> = IndexMap::new();
    for f in files {
        if schema_kind_of(f) == Some(SchemaKind::Domain) {
            by_dir.entry(dirname(f)).or_default().push(f.clone());
        }
    }
    let mut out = Vec::new();
    let mut dirs_of: IndexMap<String, Vec<String>> = IndexMap::new(); // a domain's id -> the directories that hold it
    let domains: Vec<ReadDomain> = by_dir
        .iter()
        .map(|(dir, list)| {
            let parts: Vec<DomainPart> = in_reading_order(list)
                .into_iter()
                .map(|file| DomainPart { text: read_text(&file).unwrap_or_default(), file })
                .collect();
            let merged = merge_domain(&parts);
            ReadDomain { dir: dir.clone(), parts, merged }
        })
        .collect();
    // an entity of another domain, for the note of a name that this one does not have
    let elsewhere = |own: &str, entity: &str| -> Option<EntityPlace> {
        for d in &domains {
            let id = d.id();
            let has = d.merged.doc.get("entities").and_then(Value::as_object).is_some_and(|e| e.contains_key(entity));
            if id == own || !has {
                continue;
            }
            let part = d.parts.iter().find(|p| gives_entity(&p.text, entity)).or(d.parts.first())?;
            let at = key_at(&part.text, &["entities", entity]);
            return Some(EntityPlace { file: part.file.clone(), line: at.line, column: at.column, domain: id });
        }
        None
    };
    for d in &domains {
        let id = d.id();
        for p in &d.merged.problems {
            let notes = p
                .first
                .as_ref()
                .map(|f| {
                    vec![Note {
                        file: rel(&f.file),
                        line: f.line,
                        column: f.column,
                        message: "given first here".to_string(),
                    }]
                })
                .unwrap_or_default();
            let at = At { line: p.line, column: p.column };
            out.push(result_with(p.rule.as_str(), &p.file, at, p.message.clone(), &id, p.element.clone(), notes));
        }
        out.extend(check_server_functions(&d.parts, &d.merged.doc, &id));
        out.extend(check_planned(&d.parts, &d.merged.doc, &id));
        for p in check_domain_names(&d.parts, &d.merged.doc, &id, &|entity: &str| elsewhere(&id, entity)) {
            let notes = p
                .notes
                .iter()
                .map(|n| Note { file: rel(&n.file), line: n.line, column: n.column, message: n.message.clone() })
                .collect();
            let at = At { line: p.line, column: p.column };
            out.push(result_with(p.rule.as_str(), &p.file, at, p.message, &id, p.element, notes));
        }
        if let Some(mid) = &d.merged.id {
            dirs_of.entry(mid.clone()).or_default().push(d.dir.clone());
        }
    }
    for (id, dirs) in &dirs_of {
        for dir in dirs.iter().skip(1) {
            let Some(file) = in_reading_order(by_dir.get(dir).map_or(&[][..], Vec::as_slice)).into_iter().next() else {
                continue;
            };
            let message = format!(
                "the domain {id} is in {}/ too: a domain sits in one directory, so every flow that names it reads all of it (SPEC.md §1)",
                rel(&dirs[0])
            );
            out.push(result(
                "domain-place",
                &file,
                place_of(&file, Some("id:")),
                message,
                id,
                format!("{}#/id", basename(&file)),
            ));
        }
    }
    out
}

fn check_nav(dir: &str) -> Result<Vec<CheckResult>> {
    let file = join2(dir, "nav.yaml");
    let LoadedNav { g, warnings } = load_nav(dir, None)?;
    let views = g.personas.iter().map(|p| view_of(&g, p)).collect::<Result<Vec<_>>>()?;
    let found = findings(&g, &views, &check_goals(&g, &views))?;
    let start = At { line: 1, column: 1 };
    let mut out: Vec<CheckResult> = found
        .into_iter()
        .map(|f| {
            let notes =
                f.detail.into_iter().map(|d| Note { file: rel(&file), line: 1, column: 1, message: d }).collect();
            let rule = if f.level.as_str() == "problem" { "nav-problem" } else { "nav-note" };
            result_with(rule, &file, start, format!("{} ({})", f.say, f.code), &g.id, f.code, notes)
        })
        .collect();
    for w in warnings {
        out.push(result("graph-warning", &file, start, w.clone(), &g.id, w));
    }
    Ok(out)
}

// the level a finding is printed at, and counted at: an error known from the baseline is a warning, as it fails no
// run, but one that stops the run (it cannot be loaded) is an error however often it was seen
fn printed_level(r: &CheckResult) -> Level {
    if r.level == Level::Error
        && r.baseline_state == Some(BaselineState::Unchanged)
        && !BREAKING.contains(&r.rule.as_str())
    {
        Level::Warning
    } else {
        r.level
    }
}

// where a load error is (SPEC.md §10): the place the loader or the model gave it, at the keys it names or the line a
// parser gave, in the file it names; else the line a YAML parser gave; else the start of the graph's own file
fn load_place(dir: &str, file: &str, err: &Error) -> (String, At) {
    let place = err.place.as_ref();
    let at = place.map_or_else(|| file.to_string(), |p| join2(dir, &p.file));
    if let Some(p) = place
        && let Some(line) = p.line
    {
        return (at, At { line, column: p.column.unwrap_or(1) });
    }
    if let Some(pos) = err.line_pos {
        return (at, pos);
    }
    if let Some(path) = place.and_then(|p| p.path.as_ref())
        && exists(&at)
    {
        let found = key_at(&read_text(&at).unwrap_or_default(), path);
        return (at, found);
    }
    (at, At { line: 1, column: 1 })
}

/// SHA-256 of a text's UTF-8 bytes (FIPS 180-4), as `createHash('sha256').update(text)` digests it
fn sha256(text: &str) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = text.as_bytes().to_vec();
    let bits = (msg.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (wi, word) in w.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *wi = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for (k, wi) in K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(*k).wrapping_add(*wi);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 32];
    for (o, x) in out.as_chunks_mut::<4>().0.iter_mut().zip(h) {
        *o = x.to_be_bytes();
    }
    out
}

/// what the finding is about, whatever its line: the first 32 hex digits of the SHA-256 of `rule|graph|element`
fn fingerprint(r: &CheckResult) -> String {
    let digest = sha256(&format!("{}|{}|{}", r.rule, r.graph, r.element));
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// the fingerprints of a SARIF run (a fingerprint that is no string matches no finding); Err with what keeps the file
/// from being read (where the TS crashed)
fn baseline_prints(file: &str) -> std::result::Result<IndexSet<String>, String> {
    let text = std::fs::read(file).map_err(|e| read_error(&e, file))?;
    let doc = js::json_parse(&String::from_utf8_lossy(&text)).map_err(|e| e.message)?;
    let mut out = IndexSet::new();
    for run in doc.get_array("runs").into_iter().flatten() {
        for r in run.get_array("results").into_iter().flatten() {
            if let Some(p) = r.get("fingerprints").and_then(|f| f.get_str("delurio/v1")) {
                out.insert(p.to_string());
            }
        }
    }
    Ok(out)
}

fn obj<const K: usize>(entries: [(&str, Value); K]) -> Value {
    Value::Object(Object::from_iter(entries))
}

fn sarif(results: &[CheckResult], exit_code: i32, said: &[String]) -> Value {
    let used: Vec<&Rule> = RULES.iter().filter(|r| results.iter().any(|x| x.rule == r.id)).collect();
    let rules = used
        .iter()
        .map(|r| {
            obj([
                ("id", r.id.into()),
                ("shortDescription", obj([("text", r.text.into())])),
                ("defaultConfiguration", obj([("level", r.level.as_str().into())])),
            ])
        })
        .collect::<Vec<_>>();
    let mut invocation = Object::new();
    invocation.insert("executionSuccessful", Value::Bool(exit_code != 1));
    invocation.insert("exitCode", Value::Number(f64::from(exit_code)));
    if !said.is_empty() {
        let notes =
            said.iter().map(|text| obj([("level", "note".into()), ("message", obj([("text", text.as_str().into())]))]));
        invocation.insert("toolExecutionNotifications", Value::Array(notes.collect()));
    }
    let results = results
        .iter()
        .map(|r| {
            let index = used.iter().position(|u| u.id == r.rule).map_or(-1.0, |i| i as f64);
            let mut o = Object::new();
            o.insert("ruleId", r.rule.as_str().into());
            o.insert("ruleIndex", Value::Number(index));
            o.insert("level", r.level.as_str().into());
            o.insert("message", obj([("text", r.message.as_str().into())]));
            let physical = obj([
                ("artifactLocation", obj([("uri", r.file.replace('\\', "/").into())])),
                ("region", obj([("startLine", r.line.into()), ("startColumn", r.column.into())])),
            ]);
            let logical = obj([("fullyQualifiedName", format!("{}/{}", r.graph, r.element).into())]);
            o.insert(
                "locations",
                Value::Array(vec![obj([
                    ("physicalLocation", physical),
                    ("logicalLocations", Value::Array(vec![logical])),
                ])]),
            );
            o.insert("fingerprints", obj([("delurio/v1", fingerprint(r).into())]));
            if !r.notes.is_empty() && (r.rule == "assertion-violated" || r.rule == EVENT_UNHANDLED) {
                let locations = r
                    .notes
                    .iter()
                    .map(|n| obj([("location", obj([("message", obj([("text", n.message.as_str().into())]))]))]))
                    .collect();
                let flow = obj([("threadFlows", Value::Array(vec![obj([("locations", Value::Array(locations))])]))]);
                o.insert("codeFlows", Value::Array(vec![flow]));
            }
            if let Some(b) = r.baseline_state {
                o.insert("baselineState", b.as_str().into());
            }
            Value::Object(o)
        })
        .collect();
    let driver = obj([
        ("name", "delurio".into()),
        ("version", VERSION.into()),
        ("informationUri", "https://delur.io".into()),
        ("rules", Value::Array(rules)),
    ]);
    let run = obj([
        ("tool", obj([("driver", driver)])),
        ("automationDetails", obj([("id", "delurio/check/".into())])),
        ("invocations", Value::Array(vec![Value::Object(invocation)])),
        ("results", Value::Array(results)),
    ]);
    obj([
        ("$schema", "https://json.schemastore.org/sarif-2.1.0.json".into()),
        ("version", "2.1.0".into()),
        ("runs", Value::Array(vec![run])),
    ])
}

/// `delurio check <args>`: every text the check reads is parsed once in it (`parsing_once`), however many graphs read it
pub fn check_main(args: &[String]) -> i32 {
    parsing_once(|| check_run(args))
}

fn check_run(args: &[String]) -> i32 {
    let flag = |name: &str| -> Option<&str> {
        let i = args.iter().position(|a| a == name)?;
        args.get(i + 1).map(String::as_str)
    };
    let with_value = ["--baseline", "--format", "--output", "--app"];
    let dirs: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(*i > 0 && with_value.contains(&args[i - 1].as_str())))
        .map(|(_, a)| a)
        .collect();
    if dirs.is_empty() {
        eprintln!("usage:\n{CHECK_USAGE}");
        return 1;
    }
    if let Some(not_folder) = dirs.iter().find(|d| !is_dir(d)) {
        eprintln!("{not_folder} is not a folder\nusage:\n{CHECK_USAGE}");
        return 1;
    }
    // every folder given, each graph and file once though one folder holds another
    let roots: Vec<String> = dirs.iter().map(|d| resolve1(d)).collect();
    let dirs_of_graphs: IndexSet<String> = roots.iter().flat_map(|r| graph_dirs(r)).collect();
    let strict = args.iter().any(|a| a == "--strict");
    let format = flag("--format").unwrap_or("text");
    let output = flag("--output");
    let baseline = flag("--baseline");
    if format != "text" && format != "sarif" {
        eprintln!("--format is text or sarif, not {format}");
        return 1;
    }
    let app_given = flag("--app");
    if let Some(app) = app_given
        && !is_dir(app)
    {
        eprintln!("--app {app} is not a folder\nusage:\n{CHECK_USAGE}");
        return 1;
    }
    // --no-app: the graphs are here without the application's code, so what they name of it is not looked for (SPEC.md §10)
    let no_app = args.iter().any(|a| a == "--no-app");
    if no_app && app_given.is_some() {
        eprintln!(
            "--app names the application's code and --no-app says it is not here: give one of them\nusage:\n{CHECK_USAGE}"
        );
        return 1;
    }
    let app = if no_app { None } else { Some(resolve1(app_given.unwrap_or("."))) };

    let mut results: Vec<CheckResult> = Vec::new();
    let mut domains: DomainsSeen = IndexMap::new(); // the domains found, for every flow that names one
    let mut broken = false;
    for d in &dirs_of_graphs {
        let file = if exists(&join2(d, "flow.yaml")) { join2(d, "flow.yaml") } else { join2(d, "nav.yaml") };
        let checked = newer_format(&file).and_then(|newer| match newer {
            Some(newer) => Ok(Err(newer)),
            None if file.ends_with("flow.yaml") => check_flow(d, &mut domains).map(Ok),
            None => check_nav(d).map(Ok),
        });
        match checked {
            Ok(Ok(found)) => results.extend(found),
            Ok(Err(newer)) => {
                broken = true;
                let message = format!("written in {newer}; this checker reads {FORMAT}: pin a version that reads it");
                results.push(result(
                    "format-newer",
                    &file,
                    place_of(&file, Some("schema:")),
                    message,
                    &rel(d),
                    "schema",
                ));
            }
            Err(err) => {
                broken = true;
                let (at_file, at) = load_place(d, &file, &err);
                results.push(result("load", &at_file, at, err.first_line(), &rel(d), "load"));
            }
        }
    }
    // every graph file against its kind's schema: what an editor with the schemas marks, check reports
    let mut files: IndexSet<String> = IndexSet::new();
    for r in &roots {
        let mut list = Vec::new();
        graph_files(r, &mut list);
        files.extend(list);
    }
    let files: Vec<String> = files.into_iter().collect();
    for file in &files {
        for f in validate_file(file) {
            let graph = f.graph.unwrap_or_else(|| rel(file));
            results.push(result("schema", file, At { line: f.line, column: f.column }, f.message, &graph, f.element));
        }
    }
    results.extend(check_domains(&files));
    // each flow's needs held to their evidence, and the product's gaps to its needs (SPEC.md §13)
    let needs = check_needs(&files, app.as_deref());
    for p in &needs.problems {
        let at = At { line: p.line, column: p.column };
        results.push(result(p.rule.as_str(), &p.file, at, p.message.clone(), &p.graph, p.element.clone()));
    }
    // every source anchor, and every citation of the code in a graph's words, held to the application's files (SPEC.md §9)
    let anchors = check_anchors(&files, app.as_deref());
    for p in &anchors.problems {
        let at = At { line: p.line, column: p.column };
        results.push(result(p.rule.as_str(), &p.file, at, p.message.clone(), &p.graph, p.element.clone()));
    }
    results.sort_by(|a, b| {
        js::locale_compare(&a.file, &b.file)
            .then(a.line.cmp(&b.line))
            .then(a.column.cmp(&b.column))
            .then_with(|| js::locale_compare(&a.rule, &b.rule))
            .then_with(|| js::locale_compare(&a.message, &b.message))
    });

    if let Some(baseline) = baseline {
        let prints = match baseline_prints(baseline) {
            Ok(p) => p,
            Err(message) => {
                eprintln!("error: {message}");
                return 1;
            }
        };
        for r in &mut results {
            r.baseline_state =
                Some(if prints.contains(&fingerprint(r)) { BaselineState::Unchanged } else { BaselineState::New });
        }
    }
    // what --no-app did not look for, said once rather than as a finding each (SPEC.md §10)
    let mut said: Vec<String> = Vec::new();
    if no_app {
        let of = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        let what = [
            count(anchors.anchors, "source anchor"),
            of(anchors.citations, "citation in words", "citations in words"),
            of(needs.not_looked_for, "piece of evidence", "pieces of evidence"),
        ];
        let [anchors, citations, evidence] = &what;
        said.push(format!(
            "--no-app: {anchors}, {citations} and {evidence} name files of the application, which are not here, and were not looked for (SPEC.md §10)"
        ));
    }
    let failing = results.iter().filter(|r| {
        r.level == Level::Error
            && !BREAKING.contains(&r.rule.as_str())
            && r.baseline_state.is_none_or(|b| b == BaselineState::New)
    });
    let exit_code = if broken {
        1
    } else if strict && failing.count() > 0 {
        2
    } else {
        0
    };

    if format == "sarif" {
        let text = format!("{}\n", js::stringify_pretty(&sarif(&results, exit_code, &said), 2));
        match output {
            None => {
                let mut stdout = std::io::stdout().lock();
                let _ = stdout.write_all(text.as_bytes());
                let _ = stdout.flush();
            }
            Some(o) => {
                if let Err(e) = std::fs::write(o, text) {
                    eprintln!("error: {}", read_error(&e, o));
                    return 1;
                }
            }
        }
    }
    if format == "text" || output.is_some() {
        let mut stdout = std::io::BufWriter::new(std::io::stdout().lock());
        for r in &results {
            let _ = writeln!(
                stdout,
                "{}:{}:{}: {}: {} [{}]",
                r.file,
                r.line,
                r.column,
                printed_level(r).as_str(),
                r.message,
                r.rule
            );
            for n in &r.notes {
                let _ = writeln!(stdout, "{}:{}:{}: note: {}", n.file, n.line, n.column, n.message);
            }
        }
        let _ = stdout.flush();
        drop(stdout);
        let graphs = dirs_of_graphs.len();
        let errors = results.iter().filter(|r| printed_level(r) == Level::Error).count();
        let warnings = results.iter().filter(|r| printed_level(r) == Level::Warning).count();
        // the gaps, once a flow has needs: every need that is missing, listed or not, and those the base run did not have
        let fresh =
            results.iter().filter(|r| r.rule == "need-gap" && r.baseline_state == Some(BaselineState::New)).count();
        let gaps = if needs.files == 0 {
            String::new()
        } else {
            let new = if fresh > 0 { format!(" ({fresh} new)") } else { String::new() };
            format!(", {} of {}{new}", count(needs.gaps, "gap"), count(needs.needs, "need"))
        };
        // the changes notes that writing flows say are missing (SPEC.md §4), once there is one
        let notes: Vec<&CheckResult> = results.iter().filter(|r| r.rule == "changes-note-gap").collect();
        let new_notes = notes.iter().filter(|r| r.baseline_state == Some(BaselineState::New)).count();
        let missing_notes = if notes.is_empty() {
            String::new()
        } else {
            let new = if new_notes > 0 { format!(" ({new_notes} new)") } else { String::new() };
            format!(", {}{new}", count(notes.len(), "missing changes note"))
        };
        for s in &said {
            eprintln!("delurio check: note: {s}");
        }
        eprintln!(
            "delurio check: {}, {}, {}{gaps}{missing_notes}; exit {exit_code}",
            count(graphs, "graph"),
            count(errors, "error"),
            count(warnings, "warning")
        );
    }
    exit_code
}

#[cfg(test)]
mod tests {
    use super::*;

    // the digests from `node -e "require('crypto').createHash('sha256').update(s).digest('hex')"`
    #[test]
    fn sha256_as_node_digests() {
        let hex = |s: &str| sha256(s).iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex("load|graphs|load"), "db59e22e5cf0373f80bf3d32148290ba7addb9964916698c26287b16a7590065");
        assert_eq!(hex(&"schema|ğüş|x".repeat(10)), "cac36e264b3a0426807b2f71db94251c50b847f7b9d3ab0a0011da49e3c6845b");
    }

    #[test]
    fn warnings_sorted_into_rules_as_the_patterns_read_them() {
        // JavaScript's `\S` stops at a no-break space, Rust's Unicode `\S` too; its `.` stops at a carriage return
        assert_eq!(CATALOG_KEY.captures("catalog key a.b is missing").map(|c| c[1].to_string()), Some("a.b".into()));
        assert!(STARTS.captures("a starts b: x\ry").is_none());
        assert_eq!(STARTS.captures("a starts b: x y").map(|c| c[2].to_string()), Some("b".into()));
        assert_eq!(KIT_NODE.captures("kit x (node n1) (node n2)").map(|c| c[1].to_string()), Some("n1".into()));
        assert!(NO_SAMPLE.captures("no sample value for {é} in k").is_none());
    }

    #[test]
    fn counted_words() {
        assert_eq!(count(1, "graph"), "1 graph");
        assert_eq!(count(0, "error"), "0 errors");
    }
}
