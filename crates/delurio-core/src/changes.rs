//! `changes:` in ui.yaml (SPEC.md §4): a flow whose steps write something tells its users, in its own note, what it
//! changes and what follows, or says why it has none yet (`missing`, a gap; decided on 16 Nov 2026, d-1116-11). An
//! action writes when its route or its server function is not a GET, when a policy lets someone `write` with it, or
//! when it is a step inside the service (neither a route nor a server function) that returns an entity; all three are
//! read from the flow's domain, looked up by id among the graphs beside this one, all its files read as one (SPEC.md
//! §5). A GET that a policy lists is held to what it does: it writes only when one of its `sideEffects` is more than an
//! audit record of the call (`audit:<event>`), as a policy names reads and writes together (SPEC.md §4, decided on 16
//! Nov 2026, d-1116-12). A flow with no screens, or whose domain is not found, is not checked. The port of changes.ts.

use indexmap::{IndexMap, IndexSet};
use serde::Serialize;

use crate::domain::{DomainPart, in_reading_order, is_domain_text, may_be_domain, merge_domain, serialize_object};
use crate::graph::{Graph, given};
use crate::js::{self, Object, Value};
use crate::nodepath;

/// an action of a domain, read through: its keys as the domain gives them
#[derive(Debug, Clone, Copy)]
pub struct DomainAction<'a>(pub &'a Value);

impl<'a> DomainAction<'a> {
    pub fn value(&self) -> &'a Value {
        self.0
    }
    /// `route`, when it is a string
    pub fn route(&self) -> Option<&'a str> {
        self.0.get_str("route")
    }
    /// `serverFn`, when it is a string
    pub fn server_fn(&self) -> Option<&'a str> {
        self.0.get_str("serverFn")
    }
    /// `result.returns`, any value (`a.result.returns !== undefined` asks whether the key is there)
    pub fn returns(&self) -> Option<&'a Value> {
        self.0.get_object("result").and_then(|r| r.get("returns"))
    }
    /// `sideEffects`, when it is a list
    pub fn side_effects(&self) -> Option<&'a Vec<Value>> {
        self.0.get_array("sideEffects")
    }
    /// `refusals`, when it is a map
    pub fn refusals(&self) -> Option<&'a Object> {
        self.0.get_object("refusals")
    }
}

/// a domain as a flow finds it: the document of all its files read as one (`merge_domain`), every key kept
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Domain(#[serde(serialize_with = "serialize_object")] pub Object);

impl Domain {
    pub fn doc(&self) -> &Object {
        &self.0
    }
    pub fn id(&self) -> Option<&str> {
        self.0.get("id").and_then(Value::as_str)
    }
    /// `domain.actions`, when it is a map
    pub fn actions(&self) -> Option<&Object> {
        self.0.get("actions").and_then(Value::as_object)
    }
    /// `domain.actions[name]`, an action the domain gives
    pub fn action(&self, name: &str) -> Option<DomainAction<'_>> {
        self.actions().and_then(|a| a.get(name)).map(DomainAction)
    }
    /// `domain.policies ?? []`, when it is a list
    pub fn policies(&self) -> Vec<&Value> {
        self.0.get("policies").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default()
    }
}

/// what `check_changes` finds of a flow
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangesCheck {
    pub writes: Vec<String>, // the actions the flow invokes that write
    pub key: Option<String>,
    pub missing: Option<String>, // why a flow whose steps write has no note yet: a gap, which fails nothing (SPEC.md §4)
    pub problems: Vec<String>,
}

/// the domains a run has found, by the root searched and the id: `seen` of `flowDomain`
pub type DomainsSeen = IndexMap<String, Option<Domain>>;

const SKIP: [&str; 4] = ["viz", "video", "node_modules", ".git"];

/// the root to search: the nearest folder called `graphs` above the graph, else the graph's parent
fn root_of(dir: &str) -> String {
    let mut d = dir.to_string();
    for _ in 0..5 {
        if nodepath::basename(&d) == "graphs" {
            return d;
        }
        d = nodepath::dirname(&d);
    }
    nodepath::dirname(dir)
}

/// whether a domain's merged id is the id a flow names (`merged.id === id`: a flow's `domain:` left empty is null)
fn is_id(merged: Option<&str>, id: &Value) -> bool {
    match (merged, id) {
        (None, Value::Null) => true,
        (Some(a), Value::String(b)) => a == b,
        _ => false,
    }
}

fn is_file(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
}

fn is_dir(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_dir())
}

/// `readdirSync(dir)`: the names in a folder, in the order Node gives them on Linux (libuv sorts them by their bytes);
/// a folder that cannot be read has none (the TypeScript checker threw there)
fn read_dir(dir: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> =
        entries.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

fn domain_under(root: &str, id: &Value, depth: usize) -> Option<Domain> {
    let names = read_dir(root);
    let candidates: Vec<String> =
        names.iter().filter(|name| may_be_domain(name) && is_file(&nodepath::join2(root, name))).cloned().collect();
    let parts: Vec<DomainPart> = in_reading_order(&candidates)
        .into_iter()
        .filter_map(|name| {
            let file = nodepath::join2(root, &name);
            let text = String::from_utf8_lossy(&std::fs::read(&file).ok()?).into_owned();
            Some(DomainPart { file, text })
        })
        .filter(|p| is_domain_text(&p.text))
        .collect();
    if !parts.is_empty() {
        let merged = merge_domain(&parts);
        if is_id(merged.id.as_deref(), id) {
            return Some(Domain(merged.doc));
        }
    }
    if depth == 0 {
        return None;
    }
    for name in &names {
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let sub = nodepath::join2(root, name);
        if !is_dir(&sub) {
            continue;
        }
        if let Some(found) = domain_under(&sub, id, depth - 1) {
            return Some(found);
        }
    }
    None
}

/// the domain the flow names, looked up by id among the graphs beside it; None when it names none or it is not found.
/// `seen` keeps what a run has found, so that the flows that name one domain look it up once
pub fn flow_domain(g: &Graph, seen: Option<&mut DomainsSeen>) -> Option<Domain> {
    let named = g.flow.get("domain")?;
    let root = root_of(&g.dir);
    let key = format!("{root}\0{}", js::to_js_string(named));
    match seen {
        Some(seen) => {
            if let Some(found) = seen.get(&key) {
                return found.clone();
            }
            let found = domain_under(&root, named, 4);
            seen.insert(key, found.clone());
            found
        }
        None => domain_under(&root, named, 4),
    }
}

/// the HTTP method of an action's route or server function; None for a step inside the service. (A route that is not
/// a string, on which the TypeScript checker threw, is read as absent.)
fn method_of(a: Option<DomainAction<'_>>) -> Option<String> {
    let a = a?;
    let call = a.route().or_else(|| a.server_fn())?;
    let first = js::trim(call).split(js::is_js_space).next().unwrap_or("");
    Some(first.to_uppercase())
}

/// a side effect that records the call and changes nothing the flow's users are told of: `audit:<event>` (SPEC.md §5)
fn is_audit(s: &Value) -> bool {
    s.as_str().is_some_and(|s| s.trim_start_matches(js::is_js_space).starts_with("audit:"))
}

/// a GET that a writing policy lists writes only when it does more than record that it was called: a policy names its
/// actor's reads and writes together, and a GET reads (SPEC.md §4)
fn writes_through_policy(a: Option<DomainAction<'_>>) -> bool {
    method_of(a).as_deref() != Some("GET")
        || a.and_then(|a| a.side_effects()).is_some_and(|effects| effects.iter().any(|s| !is_audit(s)))
}

/// `(p.write ?? []).length > 0`: a list or a text that is not empty
fn has_length(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    }
}

/// `basename(p, ext)` of Node: the base name, the extension taken off unless it is the whole name
fn basename_without(p: &str, ext: &str) -> String {
    if !ext.is_empty() && js::len16(ext) <= js::len16(p) && p == ext {
        return String::new();
    }
    let base = nodepath::basename(p);
    match base.strip_suffix(ext) {
        Some(stem) if !ext.is_empty() && !stem.is_empty() => stem.to_string(),
        _ => base,
    }
}

/// `domain_of` finds the flow's domain, called at most once: a caller that reads the domain for another rule as well
/// hands over the one it found (the TypeScript default is `|| flow_domain(g, None)`)
pub fn check_changes(g: &Graph, domain_of: impl FnOnce() -> Option<Domain>) -> Option<ChangesCheck> {
    if g.ui().absent() {
        return None;
    }
    let domain = domain_of()?;
    let mut writing: IndexSet<String> = IndexSet::new();
    for (name, a) in domain.actions().into_iter().flat_map(|o| o.iter()) {
        let a = DomainAction(a);
        let method = method_of(Some(a));
        let writes = match &method {
            Some(m) => m != "GET",
            None => a.returns().is_some(),
        };
        if writes {
            writing.insert(name.clone());
        }
    }
    for p in domain.policies() {
        if !has_length(p.get("write")) {
            continue;
        }
        for name in p.get_array("do").into_iter().flatten().filter_map(Value::as_str) {
            if writes_through_policy(domain.action(name)) {
                writing.insert(name.to_string());
            }
        }
    }
    let mut writes: Vec<String> = IndexSet::<String>::from_iter(
        g.flow()
            .states()
            .filter_map(|(_, s)| s.invoke().and_then(|i| i.get_str("action")))
            .filter(|a| writing.contains(*a))
            .map(str::to_string),
    )
    .into_iter()
    .collect();
    js::sort_utf16(&mut writes);
    let changes = g.ui().changes();
    let key = changes.and_then(|c| given(c, "key")).map(js::to_js_string);
    let said = changes.and_then(|c| c.get("missing"));
    // the note a writing flow says it has no message for yet; a flow that writes nothing is missing none
    let missing = match said {
        Some(Value::String(s)) if !writes.is_empty() && key.is_none() && !js::trim(s).is_empty() => {
            Some(js::trim(s).to_string())
        }
        _ => None,
    };
    let mut problems = Vec::new();
    if !writes.is_empty() && key.is_none() && missing.is_none() {
        problems.push(format!("its steps write ({}), but ui.yaml has no changes note (SPEC.md §4)", writes.join(", ")));
    }
    if let Some(key) = &key {
        let catalog_file = g.ui().catalog().unwrap_or("");
        let locales: Vec<String> = match g.ui().value().get("locales") {
            Some(Value::Array(items)) => items.iter().map(js::to_js_string).collect(),
            _ => vec![basename_without(catalog_file, ".json")],
        };
        for locale in locales {
            let file = nodepath::join(&[&g.dir, &nodepath::dirname(catalog_file), &format!("{locale}.json")]);
            // a catalog that cannot be read, or is not JSON, is none here (the TypeScript checker threw there)
            let catalog = std::fs::read(&file).ok().and_then(|b| js::json_parse(&String::from_utf8_lossy(&b)).ok());
            let note = catalog.as_ref().and_then(|c| c.get(key));
            if !note.and_then(Value::as_str).is_some_and(|n| !js::trim(n).is_empty()) {
                problems.push(format!("the changes note {key} is not in the {locale} catalog"));
            }
        }
    }
    Some(ChangesCheck { writes, key, missing, problems })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::graph::Graph;

    #[test]
    fn methods_of_routes_and_server_functions() {
        let action = |route: &str| {
            let mut o = Object::new();
            o.insert("route", Value::from(route));
            Value::Object(o)
        };
        // node -e "for (const r of ['post /x', '  GET\t/y', '', 'patch']) console.log(JSON.stringify(r.trim().split(/\s+/)[0]?.toUpperCase()))"
        for (route, method) in [("post /x", "POST"), ("  GET\t/y", "GET"), ("", ""), ("patch", "PATCH")] {
            assert_eq!(method_of(Some(DomainAction(&action(route)))).as_deref(), Some(method));
        }
        assert_eq!(method_of(Some(DomainAction(&Value::Null))), None);
        assert!(is_audit(&Value::from("  audit:note.read")));
        assert!(!is_audit(&Value::from("audit note")));
    }

    #[test]
    fn node_basename_with_an_extension() {
        // node -e "const p=require('path'); console.log(JSON.stringify(['catalog/en.json','.json','/a/.json','x/a.json.json','json'].map(x=>p.basename(x,'.json'))))"
        let got: Vec<String> = ["catalog/en.json", ".json", "/a/.json", "x/a.json.json", "json"]
            .iter()
            .map(|p| basename_without(p, ".json"))
            .collect();
        assert_eq!(got, ["en", "", ".json", "a.json", "json"]);
    }

    #[test]
    fn the_root_a_domain_is_looked_for_under() {
        assert_eq!(root_of("/r/graphs/a/b"), "/r/graphs");
        assert_eq!(root_of("/r/x/a/b"), "/r/x/a");
        assert_eq!(root_of("/r/graphs/1/2/3/4/5"), "/r/graphs/1/2/3/4"); // five folders up at most
    }

    /// a folder of the examples, as an absolute path
    pub(crate) fn example(dir: &str) -> String {
        nodepath::resolve(&[env!("CARGO_MANIFEST_DIR"), "../../examples", dir])
    }

    fn domain(json: &str) -> Domain {
        match js::json_parse(json) {
            Ok(Value::Object(o)) => Domain(o),
            _ => panic!("a domain is a JSON object"),
        }
    }

    #[test]
    fn a_flow_that_writes_and_its_note() {
        // node --input-type=module -e "import { checkChanges } from './dist/changes.js'; import { load } from './dist/graph.js';
        //   const g = load('examples/notes/restore'); console.log(JSON.stringify(checkChanges(g)))", and so on for each below
        let mut g = crate::graph::load(&example("notes/restore")).expect("the example loads");
        let found = flow_domain(&g, None).expect("the notes domain is found");
        assert_eq!(found.id(), Some("example.notes"));
        let said = check_changes(&g, || Some(found.clone())).expect("checked");
        assert_eq!(said.writes, ["restoreNote"]);
        assert_eq!(said.key.as_deref(), Some("Changes.note"));
        assert!(said.missing.is_none() && said.problems.is_empty());

        // a GET through a writing policy writes when a side effect is more than an audit; a lower-case method is one
        let custom = domain(
            r#"{"id":"example.notes","actions":{"readNote":{"serverFn":"get","sideEffects":["audit:x","told"]},"restoreNote":{"serverFn":" POST"},"step":{"result":{"returns":null}}},"policies":[{"write":[],"do":["restoreNote"]},{"write":"x","do":["readNote","ghost"]}]}"#,
        );
        assert_eq!(
            check_changes(&g, || Some(custom)).map(|c| c.writes),
            Some(vec!["readNote".into(), "restoreNote".into()])
        );
        let audited = domain(
            r#"{"actions":{"readNote":{"serverFn":"GET","sideEffects":["audit:x"]},"restoreNote":{"serverFn":"GET"}},"policies":[{"write":["Note"],"do":["readNote"]}]}"#,
        );
        assert_eq!(check_changes(&g, || Some(audited)).map(|c| c.writes), Some(vec![]));

        let deleting = || Some(domain(r#"{"actions":{"restoreNote":{"route":"DELETE /x"}}}"#));
        let changes = |g: &mut Graph, json: &str| {
            if let Some(ui) = g.ui.as_object_mut() {
                ui.insert("changes", js::json_parse(json).expect("JSON"));
                ui.insert("locales", js::json_parse(r#"["en","tr"]"#).expect("JSON"));
            }
        };
        changes(&mut g, r#"{"missing":"  not yet  "}"#);
        let said = check_changes(&g, deleting).expect("checked");
        assert_eq!((said.key, said.missing.as_deref(), said.problems.len()), (None, Some("not yet"), 0));
        changes(&mut g, r#"{"key":"Nope","missing":"x"}"#);
        assert_eq!(
            check_changes(&g, deleting).map(|c| c.problems),
            Some(vec![
                "the changes note Nope is not in the en catalog".into(),
                "the changes note Nope is not in the tr catalog".into()
            ])
        );
        changes(&mut g, "{}");
        assert_eq!(
            check_changes(&g, deleting).map(|c| c.problems),
            Some(vec!["its steps write (restoreNote), but ui.yaml has no changes note (SPEC.md §4)".into()])
        );
        // a flow with no screens is not checked, nor one whose domain is not found
        let archive = crate::graph::load(&example("notes/archive")).expect("the example loads");
        assert_eq!(check_changes(&archive, || unreachable!()), None);
        assert_eq!(check_changes(&g, || None), None);
    }

    #[test]
    fn a_domain_is_looked_up_once_a_run() {
        let g = crate::graph::load(&example("notes/edit")).expect("the example loads");
        let mut seen = DomainsSeen::new();
        let first = flow_domain(&g, Some(&mut seen));
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen.keys().next().map(String::as_str),
            Some(format!("{}\0example.notes", example("notes")).as_str())
        );
        assert_eq!(flow_domain(&g, Some(&mut seen)), first);
        assert_eq!(first.as_ref().and_then(|d| d.action("editNote")).and_then(|a| a.server_fn()), Some("POST"));
    }
}
