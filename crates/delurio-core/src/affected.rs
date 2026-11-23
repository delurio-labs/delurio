//! Which graphs a change touches, each with the reasons it is touched (SPEC.md §9, What a change touches): the flows
//! and the navigation graphs to check or replay for a pull request, a branch or the working tree, so that a
//! repository's CI walks only those. `delurio affected` reads the change from a checkout; `affected` here is the
//! selection itself, pure over what it is given: the changed paths, with their hunks when they are known, possibly
//! cut short, and the repository's files on each side of the change (`Tree`). A caller without a checkout, which has
//! the graphs' texts and the paths a host says a pull request changes, selects as the command does on a checkout.
//!
//! A change touches a graph by:
//! - its own files: anything in the graph's folder, its bindings and catalogs included (`viz/`, `video/` and the anchor
//!   lock are written by the tools and touch nothing);
//! - a source anchor (§9): a changed file that an anchor of the graph names. Line-precise when the anchor names a line
//!   and the change's lines are known (the anchored line, or a line next to it, is in a changed hunk, before or after
//!   the change); file-level when the anchor names a whole file, or the lines are not known. An anchor of the domain
//!   touches the flows that invoke the action it is on (or an action that names the entity it is on);
//! - the domain: the flows that invoke an action the change adds, removes or changes, or one that a changed entity,
//!   policy or invariant names;
//! - a binding (any file `is_binding_name` names, anywhere in the repository): its graph; a base binding, every binding
//!   that extends it; a helper of a binding (a file its commands or data name, a file of its folder, a file of the
//!   folder its commands run from): its graph; a binding that enters through a graph whose files changed, or one of
//!   whose nodes draws a flow whose files changed: its graph;
//! - the graphs' shared files: a catalog or a kit a graph names outside its own folder touches every graph naming it.
//!
//! Anything unknown touches every graph, and says why: no merge base, a list of changed files that was cut short, a
//! graph file that does not load, a file of the graphs folder that belongs to no graph. A changed file that touches no
//! graph is listed as such. The JSON is `delurio-affected/0`.
//!
//! It is the selection delurio's replay makes, with the same rules and the same words, but for two rules that belong
//! to delurio's own repository and not to the format: the citation forms of its code comments, and its engine's own
//! files.

use std::collections::{HashMap, HashSet};

use indexmap::{IndexMap, IndexSet};

use crate::domain::{DomainPart, in_reading_order, is_domain_text, may_be_domain, merge_domain};
use crate::js::{self, Object, Value};
use crate::nodepath::{basename, dirname, join2, normalize};
use crate::schemas::{SchemaKind, anchors_in, is_binding_name};
use crate::yaml::{parsing_once, read_yaml};

pub const AFFECTED_USAGE: &str =
    "  delurio affected --base <ref> [--head <ref>] [--graphs <dir>] [--repo <checkout>] [--json] [--all]
    which graphs the change from the merge base of <ref> and --head touches, each with why, and the changed files
    that touch none (--json: for machines); without --head, the working tree and the files git does not ignore.
    --graphs is the graphs folder (default: graphs), --repo the checkout (default: the working directory); --all
    touches every graph";

/// the format of the JSON `affected` answers in
pub const SCHEMA: &str = "delurio-affected/0";

// ---- the change ----

/// A changed region of a file, as `git diff -U0` gives it: lines `start`..`start + lines - 1` of each side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// repository-relative; a deleted file's path as it was
    pub path: String,
    /// a renamed file's old path
    pub from: Option<String>,
    pub status: ChangeStatus,
    /// None when the changed lines are not known: anchors on the file are then held at file level
    pub hunks: Option<Vec<Hunk>>,
}

/// The repository's files on one side of the change.
pub trait Tree {
    /// every file at or under `dir` (repository-relative, "" for the whole tree), sorted
    fn files(&self, dir: &str) -> Vec<String>;
    /// a file's text, or None when it is not there (or is not text)
    fn read(&self, path: &str) -> Option<String>;
    /// reads these files ahead, at once (one `git cat-file` for a commit)
    fn preload(&self, _paths: &[String]) {}
}

/// what was compared, for the answer
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub base: Option<String>,
    pub head: String,
}

pub struct Change<'t> {
    /// the graphs folder, repository-relative ("" for the repository's root)
    pub graphs: String,
    pub head: &'t dyn Tree,
    /// the files before the change; None when they cannot be read
    pub base: Option<&'t dyn Tree>,
    /// None when the change cannot be read; `why` says why
    pub files: Option<Vec<ChangedFile>>,
    /// false when the list of files was cut short; `why` may say how
    pub complete: bool,
    pub why: Option<String>,
    /// touch every graph, for this reason (asked for)
    pub everything: Option<String>,
    /// what was compared; None: no base, and the working tree
    pub label: Option<Label>,
}

impl<'t> Change<'t> {
    /// a change read whole, with nothing asked for
    pub fn new(graphs: &str, head: &'t dyn Tree, base: Option<&'t dyn Tree>, files: Option<Vec<ChangedFile>>) -> Self {
        Change {
            graphs: graphs.to_string(),
            head,
            base,
            files,
            complete: true,
            why: None,
            everything: None,
            label: None,
        }
    }
}

/// The files given as texts: what a caller without a checkout has.
pub struct MapTree {
    all: Vec<String>,
    texts: HashMap<String, String>,
}

impl MapTree {
    pub fn new(files: impl IntoIterator<Item = (String, String)>) -> Self {
        let texts: HashMap<String, String> = files.into_iter().collect();
        let mut all: Vec<String> = texts.keys().cloned().collect();
        js::sort_utf16(&mut all);
        MapTree { all, texts }
    }
}

impl Tree for MapTree {
    fn files(&self, dir: &str) -> Vec<String> {
        self.all.iter().filter(|p| inside(dir, p)).cloned().collect()
    }
    fn read(&self, path: &str) -> Option<String> {
        self.texts.get(path).cloned()
    }
}

// ---- the answer ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Graph,
    Anchor,
    Domain,
    Binding,
    Helper,
    Entered,
    Drawn,
    Shared,
    All,
}

impl Rule {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rule::Graph => "graph",
            Rule::Anchor => "anchor",
            Rule::Domain => "domain",
            Rule::Binding => "binding",
            Rule::Helper => "helper",
            Rule::Entered => "entered",
            Rule::Drawn => "drawn",
            Rule::Shared => "shared",
            Rule::All => "all",
        }
    }
}

/// for an anchor: whether the changed lines were compared with the anchored one
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    Line,
    File,
}

impl Precision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Precision::Line => "line",
            Precision::File => "file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub rule: Rule,
    /// the changed file, when one is the cause
    pub file: Option<String>,
    pub precision: Option<Precision>,
    pub say: String,
}

impl Reason {
    fn new(rule: Rule, file: Option<&str>, say: String) -> Self {
        Reason { rule, file: file.map(str::to_string), precision: None, say }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphKind {
    Flow,
    Nav,
}

impl GraphKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GraphKind::Flow => "flow",
            GraphKind::Nav => "nav",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub id: String,
    /// repository-relative
    pub dir: String,
    pub kind: GraphKind,
    pub reasons: Vec<Reason>,
    /// the repository's bindings of the graph, repository-relative
    pub bindings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Affected {
    pub base: Option<String>,
    pub head: String,
    /// why every graph is touched; None when the change was read file by file
    pub all: Option<String>,
    pub graphs: Vec<Touched>,
    /// how many graphs the change does not touch
    pub untouched: usize,
    /// the changed files that touch no graph
    pub unmapped: Vec<String>,
    pub changed: usize,
}

impl Affected {
    /// the answer as `delurio-affected/0`
    pub fn to_value(&self) -> Value {
        let text = |s: &Option<String>| s.as_deref().map_or(Value::Null, Value::from);
        let strings = |l: &[String]| Value::Array(l.iter().map(|s| Value::from(s.as_str())).collect());
        let graphs = self.graphs.iter().map(|g| {
            let reasons = g.reasons.iter().map(|r| {
                let mut o = Object::new();
                o.insert("rule", Value::from(r.rule.as_str()));
                o.insert("file", text(&r.file));
                if let Some(p) = r.precision {
                    o.insert("precision", Value::from(p.as_str()));
                }
                o.insert("say", Value::from(r.say.as_str()));
                Value::Object(o)
            });
            let mut o = Object::new();
            o.insert("id", Value::from(g.id.as_str()));
            o.insert("dir", Value::from(g.dir.as_str()));
            o.insert("kind", Value::from(g.kind.as_str()));
            o.insert("reasons", Value::Array(reasons.collect()));
            o.insert("bindings", strings(&g.bindings));
            Value::Object(o)
        });
        let mut o = Object::new();
        o.insert("schema", Value::from(SCHEMA));
        o.insert("base", text(&self.base));
        o.insert("head", Value::from(self.head.as_str()));
        o.insert("all", text(&self.all));
        o.insert("graphs", Value::Array(graphs.collect()));
        o.insert("untouched", Value::from(self.untouched));
        o.insert("unmapped", strings(&self.unmapped));
        o.insert("changed", Value::from(self.changed));
        Value::Object(o)
    }

    /// the answer as JSON, two spaces deep, and a line break
    pub fn to_json(&self) -> String {
        format!("{}\n", js::stringify_pretty(&self.to_value(), 2))
    }

    /// the answer in words: each graph touched with its reasons, then the changed files that touch none
    pub fn to_text(&self) -> String {
        let mut lines = Vec::new();
        let since = self.base.as_ref().map_or_else(String::new, |b| format!(" since {}", js::slice16(b, 0, Some(12))));
        match &self.all {
            Some(all) => {
                lines.push(format!("every graph ({}){since}: {all}", self.graphs.len()));
                for g in &self.graphs {
                    lines.push(format!("  {} ({})", g.dir, g.id));
                }
            }
            None => {
                lines.push(format!(
                    "{} touched{since} by {}; {} not touched",
                    plural(self.graphs.len(), "graph"),
                    plural(self.changed, "changed file"),
                    self.untouched
                ));
                for g in &self.graphs {
                    lines.push(format!(
                        "  {} ({}){}",
                        g.dir,
                        g.id,
                        if g.bindings.is_empty() { ", not bound" } else { "" }
                    ));
                    for r in &g.reasons {
                        let precision = match r.precision {
                            None => "",
                            Some(Precision::Line) => " [line-precise]",
                            Some(Precision::File) => " [file-level]",
                        };
                        lines.push(format!("    - {}{precision}", r.say));
                    }
                }
            }
        }
        if !self.unmapped.is_empty() {
            lines.push(format!("touching no graph: {}", self.unmapped.join(", ")));
        }
        format!("{}\n", lines.join("\n"))
    }
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 { format!("{n} {one}") } else { format!("{n} {one}s") }
}

// ---- paths ----

/// whether `path` is `dir` or under it; every relative path is under "" (the repository's root)
pub fn inside(dir: &str, path: &str) -> bool {
    if dir.is_empty() {
        !path.starts_with('/')
    } else {
        path == dir || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
    }
}

/// a path under the graphs folder, from it
fn from_folder<'p>(graphs_dir: &str, path: &'p str) -> &'p str {
    if graphs_dir.is_empty() { path } else { path.get(graphs_dir.len() + 1..).unwrap_or("") }
}

const GENERATED_DIRS: [&str; 2] = ["viz", "video"];

/// under the graphs folder, inside a generated folder (viz/, video/) or the anchor lock: written by the tools
fn generated(graphs_dir: &str, path: &str) -> bool {
    if !inside(graphs_dir, path) {
        return false;
    }
    if basename(path) == "anchors.lock.yaml" {
        return true;
    }
    from_folder(graphs_dir, path).split('/').any(|s| GENERATED_DIRS.contains(&s))
}

/// a file in a folder no graph is read from: a hidden one, node_modules, or one the tools write
fn skipped_path(graphs_dir: &str, path: &str) -> bool {
    let segs: Vec<&str> = from_folder(graphs_dir, path).split('/').collect();
    segs[..segs.len() - 1].iter().any(|s| s.starts_with('.') || *s == "node_modules" || GENERATED_DIRS.contains(s))
}

/// words for people, which nothing reads
fn is_doc(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = basename(path);
    [".md", ".mdx", ".txt", ".rst"].iter().any(|e| lower.ends_with(e))
        || name.starts_with("LICENSE")
        || name.starts_with("NOTICE")
}

/// a file of a domain's folder that may be part of it, by its name
fn may_be_domain_name(name: &str) -> bool {
    may_be_domain(name) && !is_binding_name(name)
}

/// the graphs folder as given, without a leading `./` or `/` and a trailing `/`
fn trim_graphs(graphs: &str) -> String {
    let rest = graphs.strip_prefix('.').unwrap_or(graphs);
    let lead = if rest.starts_with('/') { rest.trim_start_matches('/') } else { graphs };
    lead.trim_end_matches('/').to_string()
}

/// a file a graph names by a path relative to its own file (a catalog, a kit), repository-relative
fn from_graph(graph_file: &str, reference: &str) -> String {
    normalize(&join2(&dirname(graph_file), reference))
}

// ---- the graphs of the head ----

#[derive(Debug, Clone)]
struct Anchor {
    file: String,      // the graph file, repository-relative
    at: String,        // states.paying.on.PAY
    segs: Vec<String>, // the keys from the file's top to its `source`
    path: String,
    line: Option<u64>,
}

/// `<path>:<line>` or a bare `<path>`, as `check` reads an anchor; anything else is no anchor a machine follows
fn anchor_target(value: &str) -> Option<(&str, Option<u64>)> {
    let (path, line) = match value.find(':') {
        None => (value, None),
        Some(i) => (&value[..i], Some(&value[i + 1..])),
    };
    if path.is_empty() || path.chars().any(js::is_js_space) {
        return None;
    }
    match line {
        None => Some((path, None)),
        Some(l) if !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()) => {
            Some((path, Some(l.parse::<u64>().unwrap_or(u64::MAX))))
        }
        Some(_) => None,
    }
}

/// The order a graph file's anchors are given in, and so the reasons they give: by place, in this order, and the
/// anchors of one place in the file's order. `*` is any key of a map, `[]` any item of a list; a place a schema adds
/// later comes last.
const ANCHOR_ORDER: [(SchemaKind, &[&str]); 4] = [
    (
        SchemaKind::Flow,
        &[
            "source",
            "states.*.source",
            "states.*.on.*.source",
            "states.*.on.*.[].source",
            "states.*.always.[].source",
            "assertions.[].source",
        ],
    ),
    (SchemaKind::Ui, &["source", "nodes.*.source"]),
    (SchemaKind::Nav, &["pages.*.source", "frames.*.source", "entries.[].source"]),
    (
        SchemaKind::Domain,
        &[
            "source",
            "entities.*.source",
            "entities.*.fields.*.source",
            "actions.*.source",
            "actions.*.body.source",
            "actions.*.query.source",
            "actions.*.result.source",
            "actions.*.refusals.*.source",
            "actions.*.refusals.*.body.source",
            "invariants.[].source",
            "policies.[].source",
        ],
    ),
];

/// where an anchor's place comes in `ANCHOR_ORDER`
fn place_rank(kind: SchemaKind, segs: &[String]) -> usize {
    let places = ANCHOR_ORDER.iter().find(|(k, _)| *k == kind).map_or(&[][..], |(_, p)| *p);
    let fits = |place: &str| {
        let steps: Vec<&str> = place.split('.').collect();
        steps.len() == segs.len()
            && steps.iter().zip(segs).all(|(step, seg)| match *step {
                "*" => true,
                "[]" => !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()),
                literal => literal == seg,
            })
    };
    places.iter().position(|p| fits(p)).unwrap_or(places.len())
}

/// every anchor of a graph file: each `source` its kind's schema gives as one (SPEC.md §9), inside the repository
fn anchors_of(kind: SchemaKind, file: &str, doc: &Value) -> Vec<Anchor> {
    let mut places = anchors_in(kind, doc);
    places.sort_by_key(|p| place_rank(kind, &p.path)); // stable: one place's anchors keep the file's order
    let mut out = Vec::new();
    for place in places {
        let Some((path, line)) = anchor_target(&place.value) else { continue };
        let path = normalize(path);
        let path = path.strip_prefix("./").map_or_else(|| path.clone(), str::to_string);
        if path.starts_with("..") || path.starts_with('/') {
            continue;
        }
        let at = place.path[..place.path.len().saturating_sub(1)].join(".");
        out.push(Anchor {
            file: file.to_string(),
            at: if at.is_empty() { "(top)".to_string() } else { at },
            segs: place.path,
            path,
            line,
        });
    }
    out
}

#[derive(Debug)]
struct GraphInfo {
    kind: GraphKind,
    id: String,
    dir: String,
    domain: Option<String>, // a flow's
    actions: HashSet<String>,
    anchors: Vec<Anchor>,
    /// files outside its folder that it reads: its catalogs, its kit's files
    shared: HashSet<String>,
    kit_dir: Option<String>,
}

#[derive(Debug)]
struct DomainInfo {
    id: String,
    dir: String,
    files: Vec<String>,
    doc: Object,
    anchors: Vec<Anchor>,
}

#[derive(Debug)]
struct Model {
    graphs: Vec<GraphInfo>,
    domains: Vec<DomainInfo>,
    broken: Vec<(String, String)>, // a file that does not load, and why
}

impl Model {
    fn flows(&self) -> impl Iterator<Item = (usize, &GraphInfo)> {
        self.graphs.iter().enumerate().filter(|(_, g)| g.kind == GraphKind::Flow)
    }

    /// the flows of a domain: those that name it, and with one domain, those that name none
    fn flows_of_domain(&self, d: &DomainInfo) -> Vec<usize> {
        self.flows()
            .filter(|(_, g)| {
                g.domain.as_deref() == Some(d.id.as_str()) || (g.domain.is_none() && self.domains.len() == 1)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn invoking(&self, d: &DomainInfo, action: &str) -> Vec<usize> {
        self.flows_of_domain(d).into_iter().filter(|&i| self.graphs[i].actions.contains(action)).collect()
    }
}

/// a graph file's value, when it is a map; a file that is not YAML or not a map is broken, and says why
fn read_map(tree: &dyn Tree, file: &str, broken: &mut Vec<(String, String)>) -> Option<Value> {
    let text = tree.read(file)?;
    match read_yaml(&text) {
        Ok(doc @ Value::Object(_)) => Some(doc),
        Ok(_) => {
            broken.push((file.to_string(), "it is not a map".to_string()));
            None
        }
        Err(e) => {
            broken.push((file.to_string(), e.message.split('\n').next().unwrap_or_default().to_string()));
            None
        }
    }
}

fn actions_of(flow: &Value) -> HashSet<String> {
    let mut out = HashSet::new();
    for state in flow.get_object("states").map(|s| s.values().collect::<Vec<_>>()).unwrap_or_default() {
        if let Some(action) = state.get_object("invoke").and_then(|i| i.get("action")).and_then(Value::as_str) {
            out.insert(action.to_string());
        }
    }
    out
}

/// the files outside a graph's folder that a graph file names: its catalog and its locales' catalogs, its kit
fn shared_of(tree: &dyn Tree, file: &str, doc: &Value, dir: &str) -> (HashSet<String>, Option<String>) {
    let mut shared = HashSet::new();
    let mut kit_dir = None;
    if let Some(catalog) = doc.get_str("catalog") {
        let catalog = from_graph(file, catalog);
        let folder = dirname(&catalog);
        let locales: Vec<String> =
            doc.get_array("locales").map(|l| l.iter().map(js::to_js_string).collect()).unwrap_or_default();
        let mut files = vec![catalog];
        files.extend(locales.iter().map(|l| join2(&folder, &format!("{l}.json"))));
        shared.extend(files.into_iter().filter(|f| !inside(dir, f)));
    }
    if let Some(kit) = doc.get_str("kit") {
        let kit = from_graph(file, kit);
        if !inside(dir, &kit) {
            let folder = if kit.ends_with(".yaml") { dirname(&kit) } else { kit };
            shared.extend(tree.files(&folder));
            kit_dir = Some(folder);
        }
    }
    (shared, kit_dir)
}

fn load_model(tree: &dyn Tree, graphs_dir: &str) -> Model {
    let files: Vec<String> = tree.files(graphs_dir).into_iter().filter(|f| !skipped_path(graphs_dir, f)).collect();
    let present: HashSet<&str> = files.iter().map(String::as_str).collect();
    let mut broken = Vec::new();
    let mut graphs = Vec::new();
    let mut domain_parts: IndexMap<String, Vec<String>> = IndexMap::new();
    for file in &files {
        let name = basename(file);
        let dir = dirname(file);
        let id_of = |doc: &Value| doc.get_str("id").map_or_else(|| dir.clone(), str::to_string);
        if name == "flow.yaml" {
            let Some(doc) = read_map(tree, file, &mut broken) else { continue };
            let ui_file = join2(&dir, "ui.yaml");
            let ui = if present.contains(ui_file.as_str()) { read_map(tree, &ui_file, &mut broken) } else { None };
            let (shared, kit_dir) = match &ui {
                Some(ui) => shared_of(tree, &ui_file, ui, &dir),
                None => (HashSet::new(), None),
            };
            let mut anchors = anchors_of(SchemaKind::Flow, file, &doc);
            if let Some(ui) = &ui {
                anchors.extend(anchors_of(SchemaKind::Ui, &ui_file, ui));
            }
            graphs.push(GraphInfo {
                kind: GraphKind::Flow,
                id: id_of(&doc),
                domain: doc.get_str("domain").map(str::to_string),
                actions: actions_of(&doc),
                dir: dir.clone(),
                anchors,
                shared,
                kit_dir,
            });
        } else if name == "nav.yaml" {
            let Some(doc) = read_map(tree, file, &mut broken) else { continue };
            let extracted_file = join2(&dir, "nav.extracted.yaml");
            let extracted = if present.contains(extracted_file.as_str()) {
                read_map(tree, &extracted_file, &mut broken)
            } else {
                None
            };
            let (shared, kit_dir) = shared_of(tree, file, &doc, &dir);
            let mut anchors = anchors_of(SchemaKind::Nav, file, &doc);
            if let Some(extracted) = &extracted {
                anchors.extend(anchors_of(SchemaKind::Nav, &extracted_file, extracted));
            }
            graphs.push(GraphInfo {
                kind: GraphKind::Nav,
                id: id_of(&doc),
                domain: None,
                actions: HashSet::new(),
                dir: dir.clone(),
                anchors,
                shared,
                kit_dir,
            });
        } else if may_be_domain_name(&name)
            && tree.read(file).is_some_and(|text| name == "domain.yaml" || is_domain_text(&text))
        {
            domain_parts.entry(dir).or_default().push(file.clone());
        }
    }
    let mut domains = Vec::new();
    for (dir, parts) in domain_parts {
        let docs: Vec<(String, Option<Value>)> =
            parts.iter().map(|f| (f.clone(), read_map(tree, f, &mut broken))).collect();
        if docs.iter().any(|(_, d)| d.is_none()) {
            continue;
        }
        let read: Vec<DomainPart> = in_reading_order(&parts)
            .into_iter()
            .map(|file| DomainPart { text: tree.read(&file).unwrap_or_default(), file })
            .collect();
        let doc = merge_domain(&read).doc;
        let anchors = docs
            .iter()
            .flat_map(|(file, d)| d.as_ref().map(|d| anchors_of(SchemaKind::Domain, file, d)).unwrap_or_default())
            .collect();
        domains.push(DomainInfo {
            id: doc.get("id").and_then(Value::as_str).map_or_else(|| dir.clone(), str::to_string),
            dir,
            files: parts,
            doc,
            anchors,
        });
    }
    Model { graphs, domains, broken }
}

/// a domain's files at one side of the change, read as one; None when it has none there or one does not load
fn domain_at(tree: &dyn Tree, dir: &str) -> Option<Object> {
    let names: Vec<String> = tree
        .files(dir)
        .into_iter()
        .filter(|f| dirname(f) == dir)
        .filter(|f| {
            let name = basename(f);
            may_be_domain_name(&name) && tree.read(f).is_some_and(|text| name == "domain.yaml" || is_domain_text(&text))
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for file in in_reading_order(&names) {
        let text = tree.read(&file).unwrap_or_default();
        if !matches!(read_yaml(&text), Ok(Value::Object(_))) {
            return None;
        }
        parts.push(DomainPart { file, text });
    }
    Some(merge_domain(&parts).doc)
}

// ---- the bindings of the repository ----

#[derive(Debug)]
struct BindingInfo {
    file: String,
    graph: Option<String>, // the id of the flow or nav graph it binds; None for a base others extend
    extends: Option<String>,
    dir: String,
    root: String,
    /// the files its commands, data sources and attachments name
    refs: HashSet<String>,
    /// graph folders and binding files its scenarios enter through
    enters: Vec<String>,
    /// flow folders and binding files whose screens its nodes draw (`draws`)
    draws: Vec<String>,
}

fn root_of(raw: &IndexMap<String, Value>, file: &str, seen: &mut Vec<String>) -> String {
    let Some(doc) = raw.get(file).filter(|_| !seen.iter().any(|s| s == file)) else { return dirname(file) };
    if let Some(root) = doc.get_str("root") {
        return normalize(&join2(&dirname(file), root));
    }
    if let Some(base) = doc.get_str("extends") {
        seen.push(file.to_string());
        return root_of(raw, &normalize(&join2(&dirname(file), base)), seen);
    }
    dirname(file)
}

struct Refs<'a> {
    dir: &'a str,
    root: &'a str,
    refs: HashSet<String>,
    enters: Vec<String>,
    draws: Vec<String>,
}

impl Refs<'_> {
    // a path a binding names that reads as a file: neither absolute, nor with a placeholder, nor a bare word
    fn add(&mut self, from_root: bool, p: &str) {
        if p.starts_with('/') || p.contains('{') || !(p.contains('.') || p.contains('/')) {
            return;
        }
        let f = normalize(&join2(if from_root { self.root } else { self.dir }, p));
        if !f.starts_with("..") {
            self.refs.insert(f);
        }
    }

    fn walk(&mut self, v: &Value, key: Option<&str>) {
        match v {
            Value::Array(items) => {
                if key == Some("run") {
                    for a in items.iter().filter_map(Value::as_str) {
                        self.add(true, a);
                    }
                }
                for x in items {
                    self.walk(x, None);
                }
            }
            Value::Object(o) => {
                for (k, x) in o.iter() {
                    match (k.as_str(), x) {
                        ("json" | "dotenv", Value::String(s)) => self.add(true, s),
                        ("file", Value::String(s)) => self.add(false, s),
                        ("file", Value::Array(items)) => {
                            for a in items.iter().filter_map(Value::as_str) {
                                self.add(false, a);
                            }
                        }
                        ("enter", Value::Object(e)) => {
                            for to in ["nav", "flow", "bind"].iter().filter_map(|n| e.get(n).and_then(Value::as_str)) {
                                self.enters.push(normalize(&join2(self.dir, to)));
                            }
                        }
                        ("draws", Value::Object(d)) => {
                            for to in ["flow", "bind"].iter().filter_map(|n| d.get(n).and_then(Value::as_str)) {
                                self.draws.push(normalize(&join2(self.dir, to)));
                            }
                        }
                        _ => self.walk(x, Some(k)),
                    }
                }
            }
            _ => {}
        }
    }
}

fn load_bindings(tree: &dyn Tree) -> Vec<BindingInfo> {
    let mut raw: IndexMap<String, Value> = IndexMap::new();
    for file in tree.files("") {
        if !is_binding_name(&basename(&file)) {
            continue;
        }
        // a binding that does not load binds nothing; its replay says why
        if let Some(text) = tree.read(&file)
            && let Ok(doc @ Value::Object(_)) = read_yaml(&text)
            && doc.get_str("kind") == Some("binding")
        {
            raw.insert(file, doc);
        }
    }
    let mut out: Vec<BindingInfo> = raw
        .iter()
        .map(|(file, doc)| {
            let dir = dirname(file);
            let root = root_of(&raw, file, &mut Vec::new());
            let mut refs = Refs { dir: &dir, root: &root, refs: HashSet::new(), enters: Vec::new(), draws: Vec::new() };
            refs.walk(doc, None);
            let (refs, enters, draws) = (refs.refs, refs.enters, refs.draws);
            BindingInfo {
                file: file.clone(),
                graph: doc.get_str("flow").or_else(|| doc.get_str("nav")).map(str::to_string),
                extends: doc.get_str("extends").map(|e| normalize(&join2(&dir, e))),
                dir,
                root,
                refs,
                enters,
                draws,
            }
        })
        .collect();
    out.sort_by(|a, b| js::cmp_utf16(&a.file, &b.file));
    out
}

/// a binding and every binding that extends it, however far down, that binds a graph: what a change to it reaches
fn users_of<'b>(bindings: &'b [BindingInfo], start: &str) -> Vec<&'b BindingInfo> {
    let mut users: HashSet<&str> = HashSet::from([start]);
    let mut grew = true;
    while grew {
        grew = false;
        for b in bindings {
            if !users.contains(b.file.as_str()) && b.extends.as_deref().is_some_and(|e| users.contains(e)) {
                users.insert(&b.file);
                grew = true;
            }
        }
    }
    bindings.iter().filter(|b| users.contains(b.file.as_str()) && b.graph.is_some()).collect()
}

// ---- the domain ----

const META_KEYS: [&str; 10] =
    ["schema", "kind", "id", "title", "summary", "status", "description", "source", "notes", "decisions"];

/// `JSON.stringify(v ?? null)`
fn json(v: Option<&Value>) -> String {
    v.map_or_else(|| "null".to_string(), js::stringify)
}

/// whether `word` stands in `text` as a word: neither a letter, a digit nor `_` (ASCII) on either side
fn word_in(text: &str, word: &str) -> bool {
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = text.as_bytes();
    (0..=text.len()).filter(|&i| text.is_char_boundary(i)).any(|i| {
        text[i..].starts_with(word)
            && (i == 0 || !is_word(bytes[i - 1]))
            && bytes.get(i + word.len()).is_none_or(|&b| !is_word(b))
    })
}

fn note(actions: &mut IndexMap<String, String>, name: &str, why: String) {
    if !actions.contains_key(name) {
        actions.insert(name.to_string(), why);
    }
}

/// the actions of a domain a change to it touches, each with why, and why every flow of it is, when the change cannot
/// be told apart
fn domain_changes(before: Option<&Object>, after: &Object) -> (IndexMap<String, String>, Option<String>) {
    let mut actions = IndexMap::new();
    let Some(before) = before else {
        return (actions, Some("the domain is new, or its earlier files do not load".to_string()));
    };
    let empty = Object::new();
    let map_of =
        |doc: &Object, key: &str| -> Object { doc.get(key).and_then(Value::as_object).unwrap_or(&empty).clone() };
    let list_of =
        |doc: &Object, key: &str| -> Vec<Value> { doc.get(key).and_then(Value::as_array).cloned().unwrap_or_default() };
    let (a0, a1) = (map_of(before, "actions"), map_of(after, "actions"));
    let names: IndexSet<&String> = a0.keys().chain(a1.keys()).collect();
    for name in names {
        if json(a0.get(name)) != json(a1.get(name)) {
            let why = if !a0.contains_key(name) {
                format!("adds the action {name}")
            } else if !a1.contains_key(name) {
                format!("removes the action {name}")
            } else {
                format!("changes the action {name}")
            };
            note(&mut actions, name, why);
        }
    }
    let mut all_actions = a0.clone();
    for (k, v) in a1.iter() {
        all_actions.insert(k.as_str(), v.clone());
    }
    let (e0, e1) = (map_of(before, "entities"), map_of(after, "entities"));
    let (p0, p1) = (list_of(before, "policies"), list_of(after, "policies"));
    let entities: IndexSet<&String> = e0.keys().chain(e1.keys()).collect();
    for entity in entities {
        if json(e0.get(entity)) == json(e1.get(entity)) {
            continue;
        }
        for (name, action) in all_actions.iter() {
            if word_in(&js::stringify(action), entity) {
                note(&mut actions, name, format!("changes the entity {entity}, which the action {name} names"));
            }
        }
        for policy in p0.iter().chain(&p1) {
            let lists = |k: &str| policy.get_array(k).is_some_and(|l| l.iter().any(|x| x.as_str() == Some(entity)));
            if !(lists("read") || lists("write")) {
                continue;
            }
            for name in policy.get_array("do").into_iter().flatten().filter_map(Value::as_str) {
                note(
                    &mut actions,
                    name,
                    format!("changes the entity {entity}, which a policy of {name} reads or writes"),
                );
            }
        }
    }
    // the items of a list that are on one side of the change only
    let side_only = |l0: &[Value], l1: &[Value]| -> Vec<Value> {
        let j0: Vec<String> = l0.iter().map(js::stringify).collect();
        let j1: Vec<String> = l1.iter().map(js::stringify).collect();
        let only0 = l0.iter().zip(&j0).filter(|(_, j)| !j1.contains(j)).map(|(v, _)| v.clone());
        let only1 = l1.iter().zip(&j1).filter(|(_, j)| !j0.contains(j)).map(|(v, _)| v.clone());
        only0.chain(only1).collect()
    };
    for policy in side_only(&p0, &p1) {
        for name in policy.get_array("do").into_iter().flatten().filter_map(Value::as_str) {
            note(&mut actions, name, format!("changes a policy that lists {name}"));
        }
    }
    for invariant in side_only(&list_of(before, "invariants"), &list_of(after, "invariants")) {
        let id = invariant.get_str("id").unwrap_or("an invariant");
        let text = js::stringify(&invariant);
        for name in all_actions.keys().filter(|n| word_in(&text, n)) {
            note(&mut actions, name, format!("changes the invariant {id}, which names {name}"));
        }
    }
    let keys: IndexSet<&String> = before.keys().chain(after.keys()).collect();
    for key in keys {
        if META_KEYS.contains(&key.as_str())
            || ["actions", "entities", "policies", "invariants"].contains(&key.as_str())
        {
            continue;
        }
        if json(before.get(key)) != json(after.get(key)) {
            return (actions, Some(format!("the domain's {key} changed")));
        }
    }
    (actions, None)
}

/// the actions an anchor of a domain stands for: its action's, or those naming its entity, or those its policy or its
/// invariant names; None for the whole domain
fn anchor_actions(a: &Anchor, doc: &Object, file_doc: Option<&Value>) -> Option<Vec<String>> {
    let empty = Object::new();
    let actions = doc.get("actions").and_then(Value::as_object).unwrap_or(&empty);
    let naming = |text: &str| actions.keys().filter(|n| word_in(text, n)).cloned().collect::<Vec<_>>();
    let (Some(top), Some(key)) = (a.segs.first(), a.segs.get(1)) else { return None };
    match top.as_str() {
        "actions" => Some(vec![key.clone()]),
        "entities" => Some(actions.keys().filter(|n| word_in(&json(actions.get(n.as_str())), key)).cloned().collect()),
        "policies" | "invariants" if file_doc.is_some_and(Value::is_object) => {
            let item = file_doc
                .and_then(|d| d.get_array(top))
                .and_then(|l| crate::yaml::array_index(key).and_then(|i| l.get(i)));
            if top == "policies"
                && let Some(dos) = item.and_then(|i| i.get_array("do"))
            {
                return Some(dos.iter().filter_map(Value::as_str).map(str::to_string).collect());
            }
            Some(naming(&json(item)))
        }
        _ => None,
    }
}

// ---- the selection ----

/// whether a hunk changes the anchored line, or a line just before or after it, on either side of the change
fn hunk_touches(h: &Hunk, line: u64) -> bool {
    let near = |start: u64, count: u64| {
        if count == 0 {
            line == start || line == start.saturating_add(1)
        } else {
            line >= start && line < start.saturating_add(count)
        }
    };
    near(h.old_start, h.old_lines) || near(h.new_start, h.new_lines)
}

/// the files to read ahead on one side: the graphs folder's YAML and JSON, and every binding
fn preload(tree: &dyn Tree, graphs_dir: &str) {
    let mut wanted: IndexSet<String> = tree
        .files(graphs_dir)
        .into_iter()
        .filter(|p| !skipped_path(graphs_dir, p) && [".yaml", ".yml", ".json"].iter().any(|e| p.ends_with(e)))
        .collect();
    wanted.extend(tree.files("").into_iter().filter(|p| is_binding_name(&basename(p))));
    tree.preload(&wanted.into_iter().collect::<Vec<_>>());
}

struct Run<'m> {
    model: &'m Model,
    bindings: &'m [BindingInfo],
    touched: HashMap<String, Vec<Reason>>, // graph dir -> reasons
    unmapped: Vec<String>,
    label: Label,
    changed: usize,
}

impl Run<'_> {
    fn touch(&mut self, g: Option<usize>, reason: Reason) -> bool {
        let Some(g) = g else { return false };
        let list = self.touched.entry(self.model.graphs[g].dir.clone()).or_default();
        if !list.iter().any(|r| r.say == reason.say && r.file == reason.file) {
            list.push(reason);
        }
        true
    }

    fn answer(&self, all: Option<String>) -> Affected {
        let mut chosen: Vec<&GraphInfo> =
            self.model.graphs.iter().filter(|g| all.is_some() || self.touched.contains_key(&g.dir)).collect();
        chosen.sort_by(|a, b| js::cmp_utf16(&a.dir, &b.dir));
        let graphs: Vec<Touched> = chosen
            .into_iter()
            .map(|g| Touched {
                id: g.id.clone(),
                dir: g.dir.clone(),
                kind: g.kind,
                reasons: match &all {
                    Some(all) => vec![Reason::new(Rule::All, None, all.clone())],
                    None => self.touched.get(&g.dir).cloned().unwrap_or_default(),
                },
                bindings: self
                    .bindings
                    .iter()
                    .filter(|b| b.graph.as_deref() == Some(g.id.as_str()))
                    .map(|b| b.file.clone())
                    .collect(),
            })
            .collect();
        let mut unmapped = if all.is_some() { Vec::new() } else { self.unmapped.clone() };
        js::sort_utf16(&mut unmapped);
        Affected {
            base: self.label.base.clone(),
            head: self.label.head.clone(),
            untouched: self.model.graphs.len() - graphs.len(),
            all,
            graphs,
            unmapped,
            changed: self.changed,
        }
    }
}

/// Which graphs `change` touches, and why. Pure: everything it reads comes through the change's trees, each text
/// parsed once however often a side reads it.
pub fn affected(change: &Change<'_>) -> Affected {
    parsing_once(|| select(change))
}

fn select(change: &Change<'_>) -> Affected {
    let graphs_dir = trim_graphs(&change.graphs);
    for tree in std::iter::once(change.head).chain(change.base) {
        preload(tree, &graphs_dir);
    }
    let model = load_model(change.head, &graphs_dir);
    let bindings = load_bindings(change.head);
    let mut run = Run {
        model: &model,
        bindings: &bindings,
        touched: HashMap::new(),
        unmapped: Vec::new(),
        label: change.label.clone().unwrap_or(Label { base: None, head: "the working tree".to_string() }),
        changed: change.files.as_ref().map_or(0, Vec::len),
    };
    let why = change.why.as_deref();

    if let Some(everything) = &change.everything {
        return run.answer(Some(everything.clone()));
    }
    let Some(files) = &change.files else {
        return run
            .answer(Some(format!("the change cannot be read{}", why.map_or(String::new(), |w| format!(": {w}")))));
    };
    if !change.complete {
        return run.answer(Some(format!(
            "the list of changed files was cut short{}, so what else changed is not known",
            why.map_or(String::new(), |w| format!(" ({w})"))
        )));
    }
    if let Some((file, why)) = model.broken.first() {
        return run.answer(Some(format!("{file} does not load ({why}), so what it names is not known")));
    }

    let mut by_dir: HashMap<&str, usize> = HashMap::new();
    let mut by_id: HashMap<&str, usize> = HashMap::new();
    for (i, g) in model.graphs.iter().enumerate() {
        by_dir.insert(&g.dir, i);
        by_id.insert(&g.id, i);
    }
    let graph_of = |id: Option<&str>| id.and_then(|id| by_id.get(id).copied());
    let mut files_touched: HashSet<&str> = HashSet::new(); // graph dirs touched by their own or shared files
    // the folders that held a graph before the change: a file of a graph that is gone touches nothing
    let mut gone: Option<Vec<String>> = None;

    for f in files {
        let paths: Vec<&str> = std::iter::once(f.path.as_str()).chain(f.from.as_deref()).collect();
        let file = Some(f.path.as_str());
        let mut hit = false;
        let said = match f.status {
            ChangeStatus::Deleted => "deleted",
            ChangeStatus::Added => "added",
            ChangeStatus::Renamed => "renamed",
            ChangeStatus::Modified => "changed",
        };

        // the graphs folder
        for &p in paths.iter().filter(|p| inside(&graphs_dir, p)) {
            let name = basename(p);
            if generated(&graphs_dir, p) || is_doc(p) || name == "needs.gaps.yaml" || name.starts_with('.') {
                continue; // written by the tools, words for people, or the needs' check: nothing a replay reads
            }
            let mut owner: Option<usize> = None;
            for (i, g) in model.graphs.iter().enumerate() {
                if inside(&g.dir, p) && owner.is_none_or(|o| g.dir.len() > model.graphs[o].dir.len()) {
                    owner = Some(i);
                }
            }
            if let Some(o) = owner {
                let dir = &model.graphs[o].dir;
                let own = p.get(dir.len() + 1..).unwrap_or("");
                let say = if is_binding_name(&name) {
                    format!("its binding {own} is {said}")
                } else {
                    format!("its own file {own} is {said}")
                };
                hit = run.touch(owner, Reason::new(Rule::Graph, file, say)) || hit;
                files_touched.insert(dir);
            }
            let mut shared = false;
            for (i, g) in model.graphs.iter().enumerate() {
                let in_kit = g.kit_dir.as_deref().is_some_and(|k| inside(k, p));
                if Some(i) != owner && (g.shared.contains(p) || in_kit) {
                    shared = true;
                    let what = if in_kit { "its kit" } else { "a catalog it names" };
                    hit = run.touch(Some(i), Reason::new(Rule::Shared, file, format!("{p} is {said}: {what}"))) || hit;
                    files_touched.insert(&g.dir);
                }
            }
            let domain = model
                .domains
                .iter()
                .find(|d| d.files.iter().any(|x| x == p) || (dirname(p) == d.dir && may_be_domain_name(&name)));
            if let Some(d) = domain {
                let before = change.base.and_then(|b| domain_at(b, &d.dir));
                let (actions, every) = domain_changes(before.as_ref(), &d.doc);
                if let Some(every) = every {
                    for g in model.flows_of_domain(d) {
                        let say = format!("{every}: every flow of {}", d.id);
                        hit = run.touch(Some(g), Reason::new(Rule::Domain, file, say)) || hit;
                    }
                }
                for (action, why) in &actions {
                    for g in model.invoking(d, action) {
                        let say = format!("{p} {why}, which it invokes");
                        hit = run.touch(Some(g), Reason::new(Rule::Domain, file, say)) || hit;
                    }
                }
            }
            if owner.is_none() && !shared && domain.is_none() && !is_binding_name(&name) {
                let gone = gone.get_or_insert_with(|| {
                    change.base.map_or_else(Vec::new, |b| {
                        b.files(&graphs_dir)
                            .iter()
                            .filter(|x| ["flow.yaml", "nav.yaml", "domain.yaml"].contains(&basename(x).as_str()))
                            .map(|x| dirname(x))
                            .collect()
                    })
                });
                if !gone.iter().any(|d| inside(d, p) && !by_dir.contains_key(d.as_str())) {
                    return run.answer(Some(format!("{p} is in the graphs folder and belongs to no graph")));
                }
            }
        }

        // bindings, wherever they are (one inside its graph's folder is that graph's own file, above)
        for &p in paths.iter().filter(|p| is_binding_name(&basename(p))) {
            for b in users_of(&bindings, p) {
                let Some(g) = graph_of(b.graph.as_deref()) else { continue };
                if b.file == p && inside(&model.graphs[g].dir, p) {
                    continue;
                }
                let say = if b.file == p {
                    format!("its binding {p} is {said}")
                } else {
                    format!("its binding {} extends {p}, which is {said}", b.file)
                };
                hit = run.touch(Some(g), Reason::new(Rule::Binding, file, say)) || hit;
            }
        }

        // the helpers of bindings outside the graphs folder: what they name, what sits beside them, and the folder
        // their commands run from
        let helper: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|p| !is_binding_name(&basename(p)) && !is_doc(p) && !inside(&graphs_dir, p))
            .collect();
        if !helper.is_empty() {
            // per graph, only the closest tie: a file its binding names, else one beside it, else one where it runs
            let mut ties: IndexMap<&str, (u8, Vec<String>)> = IndexMap::new();
            for b in &bindings {
                let (strength, why) = if helper.iter().any(|p| b.refs.contains(*p)) {
                    (0, format!("its binding {} names {}", b.file, f.path))
                } else if helper.iter().any(|p| dirname(p) == b.dir) {
                    (1, format!("{} sits beside its binding {}", f.path, b.file))
                } else if helper.iter().any(|p| dirname(p) == b.root) {
                    (2, format!("{} sits in {}, where its binding {} runs its commands", f.path, b.root, b.file))
                } else {
                    continue;
                };
                for user in users_of(&bindings, &b.file) {
                    let Some(id) = user.graph.as_deref() else { continue };
                    match ties.get_mut(id) {
                        None => {
                            ties.insert(id, (strength, vec![why.clone()]));
                        }
                        Some(had) if strength < had.0 => *had = (strength, vec![why.clone()]),
                        Some(had) if strength == had.0 && !had.1.contains(&why) => had.1.push(why.clone()),
                        Some(_) => {}
                    }
                }
            }
            for (id, (_, whys)) in ties {
                for why in whys {
                    hit = run.touch(graph_of(Some(id)), Reason::new(Rule::Helper, file, why)) || hit;
                }
            }
        }

        // the anchors
        let anchor_hit = |a: &Anchor| -> Option<(Precision, String)> {
            if !paths.contains(&a.path.as_str()) {
                return None;
            }
            if f.status != ChangeStatus::Modified {
                return Some((Precision::File, format!("{} is {said}", a.path)));
            }
            let Some(line) = a.line else {
                return Some((Precision::File, format!("{} changed, and the anchor names the whole file", a.path)));
            };
            let Some(hunks) = &f.hunks else {
                return Some((
                    Precision::File,
                    format!("{} changed; its changed lines are not known, so line {line} is taken as changed", a.path),
                ));
            };
            hunks
                .iter()
                .any(|h| hunk_touches(h, line))
                .then(|| (Precision::Line, format!("line {line} of {} changed", a.path)))
        };
        for (i, g) in model.graphs.iter().enumerate() {
            for a in &g.anchors {
                if let Some((precision, how)) = anchor_hit(a) {
                    let say = format!("{how}: the anchor of {} in {}", a.at, basename(&a.file));
                    let reason = Reason { precision: Some(precision), ..Reason::new(Rule::Anchor, file, say) };
                    hit = run.touch(Some(i), reason) || hit;
                }
            }
        }
        for d in &model.domains {
            let mut file_docs: HashMap<&str, Option<Value>> = HashMap::new();
            for a in &d.anchors {
                let Some((precision, how)) = anchor_hit(a) else { continue };
                let file_doc = file_docs
                    .entry(a.file.as_str())
                    .or_insert_with(|| read_yaml(&change.head.read(&a.file).unwrap_or_default()).ok());
                let names = anchor_actions(a, &d.doc, file_doc.as_ref());
                let targets: Vec<usize> = match &names {
                    None => model.flows_of_domain(d),
                    Some(names) => {
                        let unique: IndexSet<usize> = names.iter().flat_map(|n| model.invoking(d, n)).collect();
                        unique.into_iter().collect()
                    }
                };
                let invokes = if names.is_none() { "" } else { ", which it invokes" };
                for g in targets {
                    let say = format!("{how}: the anchor of {} in {}{invokes}", a.at, d.id);
                    let reason = Reason { precision: Some(precision), ..Reason::new(Rule::Anchor, file, say) };
                    hit = run.touch(Some(g), reason) || hit;
                }
            }
        }

        if !hit {
            run.unmapped.push(f.path.clone());
        }
    }

    // a binding that enters through a graph whose own files changed walks those files too, and one whose node draws
    // a flow's screen checks that flow's nodes
    for b in &bindings {
        let Some(g) = graph_of(b.graph.as_deref()) else { continue };
        let ties = b
            .enters
            .iter()
            .map(|e| (e, Rule::Entered, "enters through"))
            .chain(b.draws.iter().map(|d| (d, Rule::Drawn, "draws")));
        for (e, rule, how) in ties {
            let other = by_dir.get(e.as_str()).map(|&i| &model.graphs[i]);
            let other_changed =
                other.is_some_and(|x| files_touched.contains(x.dir.as_str()) && x.dir != model.graphs[g].dir);
            let bind_changed = files.iter().any(|f| f.path == *e || f.from.as_deref() == Some(e));
            if other_changed || bind_changed {
                let say = format!("its binding {} {how} {e}, whose files changed", b.file);
                run.touch(Some(g), Reason::new(rule, None, say));
            }
        }
    }
    run.answer(None)
}

// ---- a patch ----

/// a path as git writes it in a patch: in double quotes, with C escapes, when it has a byte it does not write as it is
fn unquote(p: &str) -> String {
    let Some(inner) = p.strip_prefix('"').and_then(|q| q.strip_suffix('"')) else { return p.to_string() };
    let mut bytes = Vec::new();
    let mut it = inner.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match it.next() {
            Some(d @ b'0'..=b'7') => {
                let mut n = u32::from(d - b'0');
                for _ in 0..2 {
                    if let Some(&d @ b'0'..=b'7') = it.peek() {
                        n = n * 8 + u32::from(d - b'0');
                        it.next();
                    }
                }
                bytes.push(n as u8);
            }
            Some(b'a') => bytes.push(0x07),
            Some(b'b') => bytes.push(0x08),
            Some(b't') => bytes.push(b'\t'),
            Some(b'n') => bytes.push(b'\n'),
            Some(b'v') => bytes.push(0x0b),
            Some(b'f') => bytes.push(0x0c),
            Some(b'r') => bytes.push(b'\r'),
            Some(other) => bytes.push(other),
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// `@@ -<old>[,<n>] +<new>[,<n>] @@`
fn hunk_of(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, _) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |r: &str| -> Option<(u64, u64)> {
        let (start, count) = r.split_once(',').map_or((r, None), |(s, c)| (s, Some(c)));
        let number = |s: &str| {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) { None } else { s.parse::<u64>().ok() }
        };
        Some((number(start)?, count.map_or(Some(1), number)?))
    };
    let ((old_start, old_lines), (new_start, new_lines)) = (range(old)?, range(new)?);
    Some(Hunk { old_start, old_lines, new_start, new_lines })
}

/// `git diff -U0`'s hunks of each file, by its path after the change (and before, for a deletion). The `---` and
/// `+++` lines are read only in a file's header, before its first hunk: a removed line `-- x` is no header.
pub fn parse_hunks(patch: &str) -> IndexMap<String, Vec<Hunk>> {
    let mut out: IndexMap<String, Vec<Hunk>> = IndexMap::new();
    let mut current: Option<String> = None;
    let mut old_path: Option<String> = None;
    let mut header = false;
    for line in patch.split('\n') {
        if line.starts_with("diff --git ") {
            current = None;
            old_path = None;
            header = true;
        } else if header && let Some(p) = line.strip_prefix("--- ") {
            let p = unquote(p);
            old_path = (p != "/dev/null").then(|| p.strip_prefix("a/").unwrap_or(&p).to_string());
        } else if header && let Some(p) = line.strip_prefix("+++ ") {
            let p = unquote(p);
            let path =
                if p == "/dev/null" { old_path.clone() } else { Some(p.strip_prefix("b/").unwrap_or(&p).to_string()) };
            if let Some(path) = path {
                out.entry(path.clone()).or_default();
                current = Some(path);
            }
        } else if line.starts_with("@@ ") {
            header = false;
            if let (Some(path), Some(h)) = (&current, hunk_of(line)) {
                out.entry(path.clone()).or_default().push(h);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
