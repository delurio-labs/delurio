//! `delurio diff`: what a revision of a graph changes, for a PR description (SPEC.md §10). Each side is a graph
//! directory on disk or the same directory at a git revision (read with `git show <commit>:<path>`, nothing is
//! checked out). No model, no network: the same two revisions give the same bytes. The port of diff.ts.

use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};

use indexmap::{IndexMap, IndexSet};

use crate::compare::{Change, Shift, join_words, text};
use crate::diff_flow::{FlowDomain, FlowSide, flow_behaviour, flow_same_behaviour, flow_structure, run_side};
use crate::diff_nav::{NavSide, nav_behaviour, nav_same_behaviour, nav_structure, run_nav};
use crate::domain::{DomainPart, in_reading_order, is_domain_text, may_be_domain, merge_domain};
use crate::error::{Error, Result};
use crate::graph::{Graph, given, humanize, load_with, read_error};
use crate::js::{self, Object, Value};
use crate::nav::load_nav_with;
use crate::nodepath;

pub const DIFF_USAGE: &str = "  node src/cli.ts diff <old dir> <new dir> [--base <rev>] [--head <rev>] [--json] [--fail-on-behaviour] [--fail-on-regression]
  node src/cli.ts diff <graph dir> --base <rev> [--head <rev>] ...
    what the new revision changes, as markdown for a PR description (--json: for machines); --base reads the old side
    from a git revision, --head the new one (default: the directory on disk). Works for flow and navigation graphs.
    --fail-on-behaviour: exit 2 when what the graph does changed;  --fail-on-regression: exit 2 when a guarantee newly
    fails, a goal newly fails or a problem is new";

fn graph_error(message: impl Into<String>) -> Error {
    Error::graph(message, None)
}

/// where a side's files are read from
enum Origin {
    /// the directory on disk
    Dir(String),
    /// the directory at a commit of the repository at `top`, `rel` its path there
    Git { top: String, commit: String, rel: String, short: String },
}

// One side of the diff. `read`, `exists`, `peek` and `list` take paths relative to the graph directory; every file read
// is kept, so the header can say which files differ, and a file only peeked at is not.
pub struct Source {
    pub place: String,       // the directory as given
    pub rev: Option<String>, // the revision as shown, None for the directory on disk
    origin: Origin,
    pub texts: RefCell<IndexMap<String, String>>,
}

impl Source {
    /// a file of the git side, by its path in the repository
    fn path(&self, file: &str) -> Result<String> {
        let Origin::Git { rel, .. } = &self.origin else { return Ok(file.to_string()) };
        let p = nodepath::normalize(&nodepath::join2(rel, file));
        if p.starts_with("../") {
            return Err(graph_error(format!("{file} is outside the repository")));
        }
        Ok(p)
    }

    pub fn exists(&self, file: &str) -> Result<bool> {
        match &self.origin {
            Origin::Dir(dir) => Ok(std::path::Path::new(&nodepath::join2(dir, file)).exists()),
            Origin::Git { top, commit, .. } => {
                Ok(git(&["cat-file", "-e", &format!("{commit}:{}", self.path(file)?)], top).is_some())
            }
        }
    }

    pub fn read(&self, file: &str) -> Result<String> {
        let text = match &self.origin {
            Origin::Dir(dir) => {
                let path = nodepath::join2(dir, file);
                if !std::path::Path::new(&path).exists() {
                    return Err(graph_error(format!("{path} does not exist")));
                }
                read_text(&path)?
            }
            Origin::Git { top, commit, short, .. } => {
                let path = self.path(file)?;
                match git(&["show", &format!("{commit}:{path}")], top) {
                    Some(t) => t,
                    None => return Err(graph_error(format!("{path} does not exist at {short}"))),
                }
            }
        };
        self.texts.borrow_mut().insert(nodepath::normalize(file), text.clone());
        Ok(text)
    }

    pub fn peek(&self, file: &str) -> Result<String> {
        match &self.origin {
            Origin::Dir(dir) => read_text(&nodepath::join2(dir, file)),
            Origin::Git { top, commit, .. } => {
                Ok(git(&["show", &format!("{commit}:{}", self.path(file)?)], top).unwrap_or_default())
            }
        }
    }

    // the files (not directories) in a directory, by name; none when it does not exist
    pub fn list(&self, sub: &str) -> Result<Vec<String>> {
        let mut names = match &self.origin {
            Origin::Dir(dir) => {
                let path = nodepath::join2(dir, sub);
                if !std::path::Path::new(&path).is_dir() {
                    return Ok(Vec::new());
                }
                let entries = std::fs::read_dir(&path).map_err(|e| graph_error(read_error(&e, &path)))?;
                let mut names = Vec::new();
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let full = nodepath::join2(&path, &name);
                    match std::fs::metadata(&full) {
                        Ok(m) if m.is_file() => names.push(name),
                        Ok(_) => {}
                        Err(_) => return Err(graph_error(format!("ENOENT: no such file or directory, stat '{full}'"))),
                    }
                }
                names
            }
            Origin::Git { top, commit, .. } => {
                // blobs only: `ls-tree` names a directory a tree
                let Some(tree) = git(&["ls-tree", &format!("{commit}:{}", self.path(sub)?)], top) else {
                    return Ok(Vec::new());
                };
                tree.split('\n')
                    .filter(|l| is_blob_line(l))
                    .map(|l| l[l.find('\t').map_or(0, |i| i + 1)..].to_string())
                    .collect()
            }
        };
        js::sort_utf16(&mut names);
        Ok(names)
    }
}

/// `/^\d+ blob /`
fn is_blob_line(l: &str) -> bool {
    let digits = l.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0 && l[digits..].starts_with(" blob ")
}

/// `readFileSync(path, 'utf8')`
fn read_text(path: &str) -> Result<String> {
    let bytes = std::fs::read(path).map_err(|e| graph_error(read_error(&e, path)))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `git <args>` in `cwd`: its output, None when it fails
fn git(args: &[&str], cwd: &str) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(cwd).stdin(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// the repository's top folder, as `git rev-parse --show-toplevel` in the working directory gives it
fn toplevel() -> Option<String> {
    let top = git(&["rev-parse", "--show-toplevel"], &nodepath::cwd())?;
    let top = js::trim(&top).to_string();
    if top.is_empty() { None } else { Some(top) }
}

/// `relative(top, resolve(dir))` and whether it is outside `top`
fn inside(top: &str, dir: &str) -> (String, bool) {
    let rel = nodepath::relative(top, &nodepath::resolve1(dir));
    let outside = rel.starts_with("..") || rel.starts_with('/');
    (rel, !outside)
}

// a directory inside the repository is named from its root, as a PR reader knows it; one outside as given
fn shown(dir: &str) -> String {
    let Some(top) = toplevel() else { return dir.to_string() };
    match inside(&top, dir) {
        (_, false) => dir.to_string(),
        (rel, true) if rel.is_empty() => ".".to_string(),
        (rel, true) => rel,
    }
}

fn dir_source(dir: &str) -> Result<Source> {
    if !std::path::Path::new(dir).exists() {
        return Err(graph_error(format!("{dir} does not exist")));
    }
    Ok(Source {
        place: shown(dir),
        rev: None,
        origin: Origin::Dir(dir.to_string()),
        texts: RefCell::new(IndexMap::new()),
    })
}

fn git_source(rev: &str, dir: &str) -> Result<Source> {
    let Some(top) = toplevel() else {
        return Err(graph_error(format!(
            "--base and --head read a git revision, and {} is not in a git repository",
            nodepath::cwd()
        )));
    };
    let commit = git(&["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")], &top)
        .map(|c| js::trim(&c).to_string())
        .unwrap_or_default();
    if commit.is_empty() {
        return Err(graph_error(format!("{rev} is not a revision of {top}")));
    }
    let (rel, ok) = inside(&top, dir);
    if !ok {
        return Err(graph_error(format!("{dir} is outside the repository {top}")));
    }
    let short = js::slice16(&commit, 0, Some(7)).to_string(); // fixed length, so the output does not change as the repository grows
    let shown_rev = if commit.starts_with(rev) { short.clone() } else { format!("{rev} ({short})") };
    let at = if rel.is_empty() { "." } else { rel.as_str() };
    if git(&["cat-file", "-e", &format!("{commit}:{at}")], &top).is_none() {
        return Err(graph_error(format!(
            "{rel} does not exist at {shown_rev}; a new graph has no diff, render it instead"
        )));
    }
    Ok(Source {
        place: at.to_string(),
        rev: Some(shown_rev),
        origin: Origin::Git { top, commit, rel, short },
        texts: RefCell::new(IndexMap::new()),
    })
}

// The API description of a flow: the domain beside it, else the shared ../domain/ of a capture layout, else
// ../../domain/ of graphs/ (graphs/<part>/<flow>); all its files read as one (SPEC.md §5). When the flow names its
// domain, the files must be that domain.
fn find_domain(src: &Source, g: &Graph) -> Result<Option<FlowDomain>> {
    for dir in [".", "../domain", "../../domain"] {
        let names: Vec<String> = src
            .list(dir)?
            .into_iter()
            .filter(|n| may_be_domain(n))
            .map(|n| if dir == "." { n } else { format!("{dir}/{n}") })
            .collect();
        let mut files = Vec::new();
        for f in in_reading_order(&names) {
            if is_domain_text(&src.peek(&f)?) {
                files.push(f);
            }
        }
        if files.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        for file in &files {
            parts.push(DomainPart { file: file.clone(), text: src.read(file)? });
        }
        let merged = merge_domain(&parts);
        if let Some(Value::String(named)) = g.flow.get("domain")
            && merged.id.as_deref() != Some(named.as_str())
        {
            return Err(graph_error(format!(
                "flow.yaml names the domain {named}, but {} {} {}",
                files.join(", "),
                if files.len() == 1 { "is" } else { "are" },
                merged.id.as_deref().unwrap_or("null")
            )));
        }
        return Ok(Some(FlowDomain { file: files.join(", "), doc: Value::Object(merged.doc) }));
    }
    Ok(None)
}

enum Loaded {
    Flow(Box<FlowSide>),
    Nav(Box<NavSide>),
}

impl Loaded {
    fn kind(&self) -> &'static str {
        match self {
            Loaded::Flow(_) => "flow",
            Loaded::Nav(_) => "nav",
        }
    }
}

fn load_side(src: &Source, name: &str, locale: Option<&str>) -> Result<Loaded> {
    let inner = || -> Result<Option<Loaded>> {
        if src.exists("flow.yaml")? {
            // `exists` of a git side throws for a file outside the repository: the first such error is the one load gave
            let thrown: RefCell<Option<Error>> = RefCell::new(None);
            let exists = |f: &str| match src.exists(f) {
                Ok(b) => b,
                Err(e) => {
                    thrown.borrow_mut().get_or_insert(e);
                    false
                }
            };
            let g = load_with(&src.place, &|f| src.read(f), &exists);
            if let Some(e) = thrown.into_inner() {
                return Err(e);
            }
            let g = g?;
            let domain = find_domain(src, &g)?;
            return Ok(Some(Loaded::Flow(Box::new(FlowSide { g, domain }))));
        }
        if src.exists("nav.yaml")? {
            return Ok(Some(Loaded::Nav(Box::new(load_nav_with(&src.place, locale, &|f| src.read(f))?))));
        }
        Ok(None)
    };
    match inner() {
        Err(err) => Err(graph_error(format!("{name} ({}): {}", label(src), err.message))),
        Ok(Some(loaded)) => Ok(loaded),
        Ok(None) => Err(graph_error(format!("{name} ({}) has neither flow.yaml nor nav.yaml", label(src)))),
    }
}

fn label(s: &Source) -> String {
    match &s.rev {
        None => s.place.clone(),
        Some(rev) => format!("{} at {rev}", s.place),
    }
}

pub struct Report<'s> {
    pub kind: &'static str, // flow or nav
    pub title: String,
    pub before: &'s Source,
    pub after: &'s Source,
    pub structure: Vec<Change>,
    pub behaviour: Vec<Shift>,
    pub same: String, // the sentence when nothing observable changed
}

const HEADINGS: [(&str, &str); 21] = [
    ("flow", "The flow"),
    ("graph", "The graph"),
    ("scope", "What this view shows"),
    ("main", "The main story"),
    ("steps", "Steps"),
    ("ways", "How it moves on"),
    ("guarantees", "Guarantees"),
    ("situations", "What it keeps track of"),
    ("conditions", "Conditions and effects"),
    ("screens", "What the user sees"),
    ("people", "Kinds of user"),
    ("pages", "Pages"),
    ("entries", "Menus, links and buttons"),
    ("goals", "Goals"),
    ("frames", "Frames"),
    ("facts", "Facts and conditions"),
    ("notModeled", "Not modeled"),
    ("wording", "Wording"),
    ("domain", "The API it calls (its domain)"),
    ("other", "Other keys"),
    ("citations", "Source citations"),
];

// markdown: escape what would turn plain words into markup
fn md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn tick(s: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in s.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if s.starts_with('`') || s.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{s}{pad}{fence}")
}

fn value(v: Option<&Value>) -> String {
    v.map_or_else(|| "(none)".to_string(), |v| tick(&js::stringify(v)))
}

fn exact_line(c: &Change) -> String {
    let what = match &c.field {
        None => c.key.clone(),
        Some(f) => format!("{} › {f}", c.key),
    };
    let short = |v: Option<&Value>| -> String {
        let Some(v) = v else { return String::new() };
        let t = js::stringify(v);
        if js::len16(&t) <= 160 { format!(": {}", tick(&t)) } else { String::new() }
    };
    match c.op.as_str() {
        "added" => format!("- {} added{}", tick(&what), short(c.after.as_ref())),
        "removed" => format!("- {} removed{}", tick(&what), short(c.before.as_ref())),
        "moved" => format!("- {} moved from {} to {}", tick(&what), text(c.before.as_ref()), text(c.after.as_ref())),
        _ => format!("- {}: {} → {}", tick(&what), value(c.before.as_ref()), value(c.after.as_ref())),
    }
}

fn plural(k: usize, w: &str) -> String {
    format!("{k} {w}{}", if k == 1 { "" } else { "s" })
}

fn distinct(xs: Vec<String>) -> Vec<String> {
    xs.into_iter().collect::<IndexSet<String>>().into_iter().collect()
}

// the line that stands for the changes of an area that have no sentence of their own
fn summary_line(area: &str, items: &[&Change]) -> String {
    let n = items.len();
    if area == "conditions" || area == "facts" {
        let guards = items.iter().filter(|c| c.key.starts_with("guard ")).count();
        let other = n - guards;
        let parts: Vec<String> = [
            if guards > 0 { plural(guards, "named condition") } else { String::new() },
            if other > 0 { plural(other, if area == "facts" { "fact" } else { "effect" }) } else { String::new() },
        ]
        .into_iter()
        .filter(|x| !x.is_empty())
        .collect();
        return format!("{} changed; the exact text is under the exact changes.", join_words(&parts));
    }
    if area == "domain" {
        let one = |section: &str| match section {
            "entities" => Some("entity"),
            "actions" => Some("action"),
            "policies" => Some("policy"),
            "invariants" => Some("invariant"),
            "relations" => Some("relation"),
            "helpers" => Some("helper"),
            "permissions" => Some("permission"),
            "jobs" => Some("job"),
            _ => None,
        };
        let place = |c: &&Change| -> String {
            let field = c.field.as_deref().unwrap_or("");
            let mut parts = field.split('.');
            let section = parts.next().unwrap_or("");
            match parts.next() {
                None => section.to_string(),
                Some(name) => format!("{} {name}", one(section).unwrap_or(section)),
            }
        };
        let places = distinct(items.iter().map(place).collect());
        return format!("{} in the API description: {}.", plural(n, "change"), join_words(&places));
    }
    if area == "ways" {
        return format!("{} on from removed steps, under the exact changes.", plural(n, "way"));
    }
    if area == "citations" {
        return format!("{} changed.", plural(n, "source citation"));
    }
    if area == "other" {
        let keys = distinct(
            items
                .iter()
                .map(|c| c.field.as_deref().unwrap_or(&c.key).split('.').next().unwrap_or("").to_string())
                .collect(),
        );
        return format!("{} in keys this tool does not read: {}.", plural(n, "change"), keys.join(", "));
    }
    format!("{}, under the exact changes.", plural(n, "more change"))
}

/// the files read on either side, the new side's first: `[...new Set([...after, ...before])]`
fn files_of(r: &Report<'_>) -> Vec<String> {
    let after = r.after.texts.borrow();
    let before = r.before.texts.borrow();
    distinct(after.keys().chain(before.keys()).cloned().collect())
}

fn changed_files(r: &Report<'_>, files: &[String]) -> Vec<String> {
    let after = r.after.texts.borrow();
    let before = r.before.texts.borrow();
    files.iter().filter(|f| before.get(*f) != after.get(*f)).cloned().collect()
}

pub fn render_markdown(r: &Report<'_>) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("## {}: what this revision changes", md(&r.title)));
    out.push(String::new());
    let kind = if r.kind == "flow" { "Flow graph" } else { "Navigation graph" };
    let at = |s: &Source| s.rev.as_deref().map_or_else(|| "the directory on disk".to_string(), tick);
    out.push(if r.before.place == r.after.place {
        format!("{kind} {}: {} → {}.", tick(&r.after.place), at(r.before), at(r.after))
    } else {
        format!("{kind}: {} → {}.", tick(&label(r.before)), tick(&label(r.after)))
    });
    let files = files_of(r);
    let changed = changed_files(r, &files);
    let differ =
        if changed.is_empty() { "no file differs".to_string() } else { format!("changed: {}", changed.join(", ")) };
    out.push(format!("Compared {}; {differ}.", files.join(", ")));
    out.push(String::new());

    out.push("### What it does differently".to_string());
    out.push(String::new());
    if r.behaviour.is_empty() {
        out.push(md(&r.same));
    } else if r.kind == "flow" {
        for s in &r.behaviour {
            out.extend(item(s));
        }
    } else {
        let people = distinct(r.behaviour.iter().filter_map(|s| s.persona.clone()).collect());
        for who in &people {
            let items: Vec<&Shift> = r.behaviour.iter().filter(|s| s.persona.as_ref() == Some(who)).collect();
            if items.len() > 8 {
                out.push(format!("<details><summary><b>{}</b>: {} changes</summary>", md(who), items.len()));
                out.push(String::new());
                for s in &items {
                    out.extend(item(s));
                }
                out.push(String::new());
                out.push("</details>".to_string());
                out.push(String::new());
            } else {
                out.push(format!("**{}**", md(who)));
                out.push(String::new());
                for s in &items {
                    out.extend(item(s));
                }
                out.push(String::new());
            }
        }
        let rest: Vec<&Shift> = r.behaviour.iter().filter(|s| s.persona.is_none()).collect();
        if !rest.is_empty() {
            out.push("**Findings**".to_string());
            out.push(String::new());
            for s in rest {
                out.extend(item(s));
            }
        }
    }
    out.push(String::new());

    out.push("### What changed in the graph".to_string());
    out.push(String::new());
    if r.structure.is_empty() {
        out.push(
            if changed.is_empty() {
                "Nothing."
            } else {
                "Nothing: the files differ only in comments, layout or key order."
            }
            .to_string(),
        );
    }
    for (area, heading) in HEADINGS {
        let items: Vec<&Change> = r.structure.iter().filter(|c| c.area == area).collect();
        if items.is_empty() {
            continue;
        }
        let silent: Vec<&Change> = items.iter().copied().filter(|c| c.say.is_none()).collect();
        out.push(format!("**{heading}**"));
        out.push(String::new());
        for c in &items {
            if let Some(say) = &c.say {
                out.push(format!("- {}", md(say)));
            }
        }
        if !silent.is_empty() {
            out.push(format!("- {}", md(&summary_line(area, &silent))));
        }
        out.push(String::new());
    }
    if !r.structure.is_empty() {
        out.push(format!("<details><summary>Exact changes, by id ({})</summary>", r.structure.len()));
        out.push(String::new());
        for c in &r.structure {
            out.push(exact_line(c));
        }
        out.push(String::new());
        out.push("</details>".to_string());
    }
    let joined = out.join("\n");
    // `.replace(/\n{3,}/g, '\n\n')`
    let mut squeezed = String::with_capacity(joined.len());
    let mut newlines = 0;
    for c in joined.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                squeezed.push(c);
            }
        } else {
            newlines = 0;
            squeezed.push(c);
        }
    }
    format!("{}\n", squeezed.trim_end_matches(js::is_js_space))
}

fn item(s: &Shift) -> Vec<String> {
    let head = if s.worse { format!("**{}**", md(&s.say)) } else { md(&s.say) };
    let mut out = vec![format!("- {head}")];
    out.extend(s.detail.iter().map(|d| format!("  {}", md(d))));
    out
}

pub fn render_json(r: &Report<'_>) -> String {
    let side = |s: &Source| -> Value {
        let mut o = Object::new();
        o.insert("path", Value::from(s.place.as_str()));
        o.insert("revision", s.rev.as_deref().map_or(Value::Null, Value::from));
        o.insert("files", Value::Array(s.texts.borrow().keys().map(|k| Value::from(k.as_str())).collect()));
        Value::Object(o)
    };
    let files = files_of(r);
    let mut o = Object::new();
    o.insert("schema", Value::from("delurio-graph-diff/0"));
    o.insert("kind", Value::from(r.kind));
    o.insert("title", Value::from(r.title.as_str()));
    o.insert("before", side(r.before));
    o.insert("after", side(r.after));
    o.insert("changedFiles", Value::Array(changed_files(r, &files).iter().map(|f| Value::from(f.as_str())).collect()));
    o.insert("behaviourChanged", Value::Bool(!r.behaviour.is_empty()));
    o.insert("regressions", Value::from(r.behaviour.iter().filter(|s| s.worse).count()));
    o.insert("behaviour", Value::Array(r.behaviour.iter().map(Shift::to_value).collect()));
    o.insert("structure", Value::Array(r.structure.iter().map(Change::to_value).collect()));
    format!("{}\n", js::stringify_pretty(&Value::Object(o), 2))
}

pub fn compare<'s>(before: &'s Source, after: &'s Source, locale: Option<&str>) -> Result<Report<'s>> {
    let b = load_side(before, "before", locale)?;
    let a = load_side(after, "after", locale)?;
    match (&b, &a) {
        (Loaded::Flow(bs), Loaded::Flow(as_)) => {
            let s = flow_structure(bs, as_)?;
            let rb = run_side(&bs.g, &format!("before ({})", label(before)))?;
            let ra = run_side(&as_.g, &format!("after ({})", label(after)))?;
            let fa = &as_.g.flow;
            let title = given(fa, "title").map_or_else(|| humanize(&text(fa.get("id"))), js::to_js_string);
            let behaviour = flow_behaviour(&rb, &ra, &s)?;
            Ok(Report {
                kind: "flow",
                title,
                before,
                after,
                structure: s.changes,
                behaviour,
                same: flow_same_behaviour(&ra),
            })
        }
        (Loaded::Nav(bs), Loaded::Nav(as_)) => {
            for w in &bs.warnings {
                eprintln!("warning: before: {w}");
            }
            for w in &as_.warnings {
                eprintln!("warning: after: {w}");
            }
            let s = nav_structure(bs, as_)?;
            let rb = run_nav(&bs.g, &format!("before ({})", label(before)))?;
            let ra = run_nav(&as_.g, &format!("after ({})", label(after)))?;
            let behaviour = nav_behaviour(&rb, &ra, &s)?;
            Ok(Report {
                kind: "nav",
                title: as_.g.title.clone(),
                before,
                after,
                structure: s.changes,
                behaviour,
                same: nav_same_behaviour(&ra),
            })
        }
        _ => Err(graph_error(format!(
            "before ({}) is a {} graph and after ({}) a {} graph",
            label(before),
            b.kind(),
            label(after),
            a.kind()
        ))),
    }
}

fn diff(args: &[String]) -> Result<i32> {
    const WITH_VALUE: [&str; 3] = ["--base", "--head", "--locale"];
    let dirs: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, x)| {
            let prev = if *i == 0 { "" } else { args[i - 1].as_str() };
            !x.starts_with("--") && !WITH_VALUE.contains(&prev)
        })
        .map(|(_, x)| x)
        .collect();
    let flag = |name: &str| -> Option<&str> {
        let i = args.iter().position(|a| a == name)?;
        args.get(i + 1).map(String::as_str)
    };
    let has = |name: &str| args.iter().any(|a| a == name);
    let base = flag("--base");
    let head = flag("--head");
    for name in WITH_VALUE {
        if has(name) && flag(name).is_none_or(|v| v.starts_with("--")) {
            return Err(graph_error(format!("{name} needs a value")));
        }
    }
    if dirs.is_empty() || dirs.len() > 2 || (dirs.len() == 1 && base.is_none() && head.is_none()) {
        eprintln!("usage:\n{DIFF_USAGE}");
        return Ok(1);
    }
    let (old_dir, new_dir) = if dirs.len() == 2 { (dirs[0], dirs[1]) } else { (dirs[0], dirs[0]) };
    let before = match base {
        None => dir_source(old_dir)?,
        Some(rev) => git_source(rev, old_dir)?,
    };
    let after = match head {
        None => dir_source(new_dir)?,
        Some(rev) => git_source(rev, new_dir)?,
    };
    let report = compare(&before, &after, flag("--locale"))?;
    let text = if has("--json") { render_json(&report) } else { render_markdown(&report) };
    let mut stdout = std::io::stdout().lock();
    // all of it, before the process exits; a closed pipe is the reader's choice
    let _ = stdout.write_all(text.as_bytes()).and_then(|()| stdout.flush());
    let regressions = report.behaviour.iter().filter(|s| s.worse).count();
    if has("--fail-on-regression") && regressions > 0 {
        return Ok(2);
    }
    if has("--fail-on-behaviour") && !report.behaviour.is_empty() {
        return Ok(2);
    }
    Ok(0)
}

/// `delurio diff <args>`: the exit code; an error that keeps a side from loading is written as cli.ts writes it
pub fn diff_main(args: &[String]) -> i32 {
    match diff(args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {}", err.message);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_escapes_and_code_spans() {
        assert_eq!(md("a_b *c* [d] <e> `f` \\"), "a\\_b \\*c\\* \\[d\\] \\<e> \\`f\\` \\\\");
        assert_eq!(tick("plain"), "`plain`");
        assert_eq!(tick("a `b` c"), "``a `b` c``");
        assert_eq!(tick("`x``"), "``` `x`` ```");
    }

    #[test]
    fn blob_lines_of_ls_tree() {
        assert!(is_blob_line("100644 blob 0123abcd\tflow.yaml"));
        assert!(!is_blob_line("040000 tree 0123abcd\tdomain"));
        assert!(!is_blob_line(" blob x"));
    }
}
