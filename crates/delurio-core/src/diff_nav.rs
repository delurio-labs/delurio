//! The revision diff of a navigation graph: pages, entries, personas and goals keyed by id (nav.yaml and the file it
//! extends are compared as one graph), and what changes for each kind of user, found by walking both revisions. The
//! port of diff-nav.ts.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::compare::{
    Change, Key, Op, Shift, deep_diff, field_changes, join_words, match_ordered, member, moved, op_of, other_keys,
    quote, raw, same, strict_eq, text, union, union_keys, word_changes,
};
use crate::error::{Error, Result};
use crate::graph::given;
use crate::js::{self, Object, Value};
use crate::nav::{
    EntrySpec, Finding, Goal, Goalcheck, LoadedNav, NavGraph, Outcome, Page, Persona, PersonaView, Standing,
    check_goals, findings, menu_rows, path_to, view_of,
};

// a side of the diff: the graph and what was found odd while reading it
pub type NavSide = LoadedNav;

fn capital(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.len_utf16() == 1 => c.to_uppercase().chain(chars).collect(),
        _ => s.to_string(),
    }
}

fn kind_words(kind: &str) -> &'static str {
    match kind {
        "screen" => "built",
        "placeholder" => "not built",
        "missing" => "a page that does not exist",
        "redirect" => "a redirect",
        "external" => "another application",
        _ => "undefined",
    }
}

fn standing_words(s: Standing) -> &'static str {
    match s {
        Standing::Ok => "opens",
        Standing::Placeholder => "is not built",
        Standing::Missing => "does not exist",
        Standing::Refused => "refuses them",
        Standing::Unreachable => "cannot be reached",
    }
}

fn standing_now(s: Standing) -> &'static str {
    match s {
        Standing::Ok => "now opens",
        Standing::Placeholder => "is now not built",
        Standing::Missing => "now does not exist",
        Standing::Refused => "now refuses them",
        Standing::Unreachable => "can no longer be reached",
    }
}

fn standing_past(s: Standing) -> &'static str {
    match s {
        Standing::Ok => "opened",
        Standing::Placeholder => "was not built",
        Standing::Missing => "did not exist",
        Standing::Refused => "refused them",
        Standing::Unreachable => "could not be reached",
    }
}

/// `g.catalog[key] ?? key`
fn word(g: &NavGraph, key: &str) -> String {
    given(&g.catalog, key).map_or_else(|| key.to_string(), js::to_js_string)
}

fn page_name(g: &NavGraph, id: Option<&str>) -> String {
    match id {
        None => "no page".to_string(),
        Some(id) => g.pages.get(id).map_or_else(|| id.to_string(), |p| quote(&p.title)),
    }
}

fn from_tokens(s: &Value) -> Vec<String> {
    EntrySpec(s).from_tokens()
}

fn menu_name(g: &NavGraph, frame: &str) -> String {
    g.frames
        .get(frame)
        .and_then(|f| given(f, "menu"))
        .map_or_else(|| frame.to_string(), js::to_js_string)
        .to_lowercase()
}

/// `s.how` as a template literal writes it
fn how(s: &Value) -> String {
    text(s.get("how"))
}

fn from_words(g: &NavGraph, s: &Value) -> String {
    let words: Vec<String> = from_tokens(s)
        .iter()
        .map(|t| {
            let Some(frame) = t.strip_prefix("frame:") else { return page_name(g, Some(t)) };
            let Some(f) = g.frames.get(frame) else { return t.clone() };
            if s.get_str("how") == Some("menu") && f.get("menu").is_some() {
                format!("the {}", menu_name(g, frame))
            } else {
                format!("every page of {}", quote(&text(f.get("title"))))
            }
        })
        .collect();
    join_words(&words)
}

fn entry_words(g: &NavGraph, s: &Value) -> String {
    let label = s.get("label").map_or_else(String::new, |l| quote(&word(g, &js::to_js_string(l))));
    match s.get_str("how") {
        Some("menu") => format!("menu entry {label} in {}", from_words(g, s)),
        Some("picker") => format!("scope picker entry {label} on {}", from_words(g, s)),
        Some("user") => format!("entry {label} in the person’s corner of {}", from_words(g, s)),
        Some("auto") => format!("automatic redirect from {}", from_words(g, s)),
        _ => format!("{} {label} on {}", how(s), from_words(g, s)),
    }
}

fn entry_key(s: &Value) -> String {
    let id = given(s, "id").map_or_else(
        || format!("{} → {} ({})", from_tokens(s).join(", "), text(s.get("to")), how(s)),
        js::to_js_string,
    );
    format!("entry {id}")
}

fn path_words(g: &NavGraph, v: &PersonaView<'_>, page: &str) -> String {
    let words: Vec<&str> =
        path_to(&v.reach, page).into_iter().filter(|e| !e.label.is_empty()).map(|e| e.label.as_str()).collect();
    // a way that starts at a page opened by its address (SPEC.md §6)
    if let Some(typed) = v.reach.get(page).and_then(|r| r.by_address.as_deref()) {
        if typed == page {
            return "by its address".to_string();
        }
        let at = g.pages.get(typed).map_or_else(|| typed.to_string(), |p| quote(&p.title));
        let then = if words.is_empty() { String::new() } else { format!(", then {}", words.join(" → ")) };
        return format!("by the address of {at}{then}");
    }
    if words.is_empty() { "automatic".to_string() } else { words.join(" → ") }
}

fn page_fields(p: &Page) -> Object {
    let mut o = Object::new();
    o.insert("route", Value::from(p.route.as_str()));
    o.insert("title", Value::from(p.title_key.as_deref().unwrap_or(&p.title)));
    o.insert("kind", Value::from(p.kind.as_str()));
    o.insert("frame", Value::from(p.frame.as_str()));
    let put = |o: &mut Object, k: &str, v: &Option<String>| {
        if let Some(v) = v {
            o.insert(k, Value::from(v.as_str()));
        }
    };
    put(&mut o, "opensFor", &p.opens_for);
    put(&mut o, "refusal", &p.refusal);
    if p.by_address {
        o.insert("byAddress", Value::Bool(true));
    }
    put(&mut o, "pageCodes", &p.page_codes);
    put(&mut o, "source", &p.source);
    put(&mut o, "evidence", &p.evidence);
    o
}

/// a persona as the TypeScript object holds it, for comparing
fn persona_value(p: &Persona) -> Value {
    let mut o = Object::new();
    o.insert("id", Value::from(p.id.as_str()));
    o.insert("title", Value::from(p.title.as_str()));
    o.insert("say", Value::from(p.say.as_str()));
    o.insert("facts", Value::Object(p.facts.clone()));
    Value::Object(o)
}

/// `Object.fromEntries(list.map((x) => [key(x), x]))`: JavaScript's key order, the last item of a key kept
fn by_key<T>(list: &[T], key: impl Fn(&T) -> String) -> (Object, HashMap<String, &T>) {
    let mut keys = Object::new();
    let mut items = HashMap::new();
    for x in list {
        let k = key(x);
        keys.insert(k.clone(), Value::Null);
        items.insert(k, x);
    }
    (keys, items)
}

fn page_keys(g: &NavGraph) -> Vec<&str> {
    g.pages.keys().map(String::as_str).collect()
}

fn keys_of(o: &Object) -> Vec<&str> {
    o.keys().map(String::as_str).collect()
}

pub struct NavStructure {
    pub changes: Vec<Change>,
    pub entries: Vec<(usize, usize)>, // entries matched on both sides: [index in the old specs, index in the new specs]
}

pub fn nav_structure(b: &NavSide, a: &NavSide) -> Result<NavStructure> {
    let mut out: Vec<Change> = Vec::new();
    let gb = &b.g;
    let ga = &a.g;

    // the graph itself
    let meta = |g: &NavGraph| -> Object {
        let mut o = Object::new();
        for (k, v) in
            [("id", &g.id), ("title", &g.title), ("status", &g.status), ("summary", &g.summary), ("locale", &g.locale)]
        {
            o.insert(k, Value::from(v.as_str()));
        }
        if let Some(adapter) = &g.adapter {
            o.insert("adapter", Value::from(adapter.as_str()));
        }
        o
    };
    out.extend(field_changes(
        "graph",
        "graph",
        &meta(gb),
        &meta(ga),
        &["title", "summary", "status", "locale", "id", "adapter"],
        &|f, x, y| match f {
            "title" => Some(format!("The graph is now called {} (was {}).", quote(&text(y)), quote(&text(x)))),
            "summary" => Some(format!("New summary: {}", quote(&text(y)))),
            "status" => Some(format!("Status: {} → {}.", text(x), text(y))),
            "locale" => Some(format!("The wording is now in {} (was {}).", text(y), text(x))),
            _ => None,
        },
    ));
    deep_diff("citations", "graph source", Some(&gb.source), Some(&ga.source), &mut out, &[]);

    // frames
    for id in union_keys(&keys_of(&gb.frames), &keys_of(&ga.frames)) {
        let x = gb.frames.get(&id);
        let y = ga.frames.get(&id);
        if same(x, y) {
            continue;
        }
        let key = format!("frame {id}");
        if x.is_none() || y.is_none() {
            let f = y.or(x);
            let title = quote(&text(member(f, "title")));
            let say = if y.is_none() {
                format!("The frame {title} was removed.")
            } else {
                let menu = match member(f, "menu") {
                    None => String::new(),
                    Some(m) => format!(", with the {}", js::to_js_string(m).to_lowercase()),
                };
                format!("New frame: {title}{menu}.")
            };
            out.push(Change::new("frames", key, op_of(x, y), Some(say)));
            continue;
        }
        let (xt, yt) = (quote(&text(member(x, "title"))), quote(&text(member(y, "title"))));
        out.extend(field_changes("frames", &key, raw(x), raw(y), &["title", "menu", "shows"], &|f, p, q| match f {
            "title" => Some(format!("The frame {xt} is now called {yt}.")),
            "menu" => Some(match q {
                None => format!("{yt} no longer carries a menu."),
                Some(q) => {
                    let was =
                        p.map_or_else(String::new, |p| format!(" (was the {})", js::to_js_string(p).to_lowercase()));
                    format!("{yt} now carries the {}{was}.", js::to_js_string(q).to_lowercase())
                }
            }),
            _ => None,
        }));
    }

    // facts and named conditions: formal, listed under the exact changes; who they affect shows under behaviour
    for id in union_keys(&keys_of(&gb.facts), &keys_of(&ga.facts)) {
        let (p, q) = (gb.facts.get(&id), ga.facts.get(&id));
        if !same(p, q) {
            out.push(Change::new("facts", format!("fact {id}"), op_of(p, q), None).before(p).after(q));
        }
    }
    for id in union_keys(&keys_of(&gb.guards), &keys_of(&ga.guards)) {
        let (p, q) = (gb.guards.get(&id), ga.guards.get(&id));
        if !same(p, q) {
            out.push(Change::new("facts", format!("guard {id}"), op_of(p, q), None).before(p).after(q));
        }
    }

    // kinds of user
    let (kb, pb) = by_key(&gb.personas, |p| p.id.clone());
    let (ka, pa) = by_key(&ga.personas, |p| p.id.clone());
    for id in union_keys(&keys_of(&kb), &keys_of(&ka)) {
        let x = pb.get(&id).copied();
        let y = pa.get(&id).copied();
        if same(x.map(persona_value).as_ref(), y.map(persona_value).as_ref()) {
            continue;
        }
        let key = format!("persona {id}");
        let (x, y) = match (x, y) {
            (None, Some(y)) => {
                out.push(Change::new(
                    "people",
                    key,
                    Op::Added,
                    Some(format!("New kind of user: {}, {}.", quote(&y.title), y.say)),
                ));
                continue;
            }
            (Some(x), None) => {
                out.push(Change::new(
                    "people",
                    key,
                    Op::Removed,
                    Some(format!("Kind of user removed: {}.", quote(&x.title))),
                ));
                continue;
            }
            (Some(x), Some(y)) => (x, y),
            (None, None) => continue,
        };
        if x.title != y.title {
            out.push(
                Change::new(
                    "people",
                    key.clone(),
                    Op::Changed,
                    Some(format!("{} is now called {}.", quote(&x.title), quote(&y.title))),
                )
                .field("title")
                .before(Some(&Value::from(x.title.as_str())))
                .after(Some(&Value::from(y.title.as_str()))),
            );
        }
        if x.say != y.say {
            out.push(
                Change::new(
                    "people",
                    key.clone(),
                    Op::Changed,
                    Some(format!("{} is now described as: {}.", quote(&y.title), y.say)),
                )
                .field("say")
                .before(Some(&Value::from(x.say.as_str())))
                .after(Some(&Value::from(y.say.as_str()))),
            );
        }
        for fact in union_keys(&keys_of(&x.facts), &keys_of(&y.facts)) {
            let p = x.facts.get(&fact);
            let q = y.facts.get(&fact);
            if same(p, q) {
                continue;
            }
            let say = if let (Some(Value::Array(pl)), Some(Value::Array(ql))) = (p, q) {
                let strs = |l: &[Value]| l.iter().map(js::to_js_string).collect::<Vec<_>>();
                let ch = word_changes(&strs(pl), &strs(ql));
                let parts: Vec<String> = [
                    if ch.added.is_empty() { String::new() } else { format!("now also has {}", ch.added.join(", ")) },
                    if ch.removed.is_empty() {
                        String::new()
                    } else {
                        format!("no longer has {}", ch.removed.join(", "))
                    },
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
                let what = if parts.is_empty() { "is in a new order".to_string() } else { parts.join("; ") };
                format!("{}: {fact} {what}.", quote(&y.title))
            } else {
                format!("{}: {fact} is now {} (was {}).", quote(&y.title), text(q), text(p))
            };
            out.push(
                Change::new("people", key.clone(), op_of(p, q), Some(say))
                    .field(format!("facts.{fact}"))
                    .before(p)
                    .after(q),
            );
        }
    }

    // pages
    for id in union_keys(&page_keys(gb), &page_keys(ga)) {
        let key = format!("page {id}");
        let (x, y) = match (gb.pages.get(&id), ga.pages.get(&id)) {
            (None, Some(y)) => {
                let kind = if y.kind.as_str() == "screen" {
                    String::new()
                } else {
                    format!(", {}", kind_words(y.kind.as_str()))
                };
                out.push(Change::new(
                    "pages",
                    key,
                    Op::Added,
                    Some(format!("New page {} ({}){kind}.", page_name(ga, Some(&id)), y.route)),
                ));
                continue;
            }
            (Some(x), None) => {
                out.push(Change::new(
                    "pages",
                    key,
                    Op::Removed,
                    Some(format!("Page {} ({}) removed.", page_name(gb, Some(&id)), x.route)),
                ));
                continue;
            }
            (Some(x), Some(y)) => (x, y),
            (None, None) => continue,
        };
        let name = page_name(ga, Some(&id));
        let frame_title = |g: &NavGraph, f: &str| {
            quote(&g.frames.get(f).and_then(|fr| given(fr, "title")).map_or_else(|| f.to_string(), js::to_js_string))
        };
        out.extend(field_changes(
            "pages",
            &key,
            &page_fields(x),
            &page_fields(y),
            &["route", "title", "kind", "frame", "opensFor", "refusal", "byAddress", "pageCodes", "source", "evidence"],
            &|f, p, q| match f {
                "route" => Some(format!("{name} moved to {} (was {}).", text(q), text(p))),
                "title" => Some(format!("{} is now titled {name}.", page_name(gb, Some(&id)))),
                "kind" => Some(format!("{name} is now {} (was {}).", kind_words(&text(q)), kind_words(&text(p)))),
                "frame" => Some(format!(
                    "{name} is now drawn in {} (was {}).",
                    frame_title(ga, &text(q)),
                    frame_title(gb, &text(p))
                )),
                "opensFor" => Some(if q.is_none() {
                    format!("{name} now opens for everyone.")
                } else if p.is_none() {
                    format!("{name} now opens only for some people.")
                } else {
                    format!("Who {name} opens for changed.")
                }),
                "refusal" => Some(match q {
                    None => format!("{name} no longer says how it refuses."),
                    Some(q) => format!("{name} now refuses with: {}.", quote(&js::to_js_string(q))),
                }),
                "byAddress" => Some(if q == Some(&Value::Bool(true)) {
                    format!("{name} is now opened by its address.")
                } else {
                    format!("{name} is no longer opened by its address.")
                }),
                _ => None,
            },
        ));
    }

    // menus, links and buttons: entries are paired by their id or by where they start, where they go, how and their label;
    // what is left by (start, how, label) when the target changed, then by (start, target, how) when the label changed
    let sb = &gb.specs;
    let sa = &ga.specs;
    let label_or_empty = |s: &Value| given(s, "label").map_or_else(String::new, js::to_js_string);
    let k1 = |s: &Value| -> Option<String> {
        Some(given(s, "id").map_or_else(
            || format!("{}|{}|{}|{}", from_tokens(s).join(","), text(s.get("to")), how(s), label_or_empty(s)),
            js::to_js_string,
        ))
    };
    let k2 = |s: &Value| -> Option<String> {
        if s.get("id").is_some() {
            return None;
        }
        Some(format!("{}|{}|{}", from_tokens(s).join(","), how(s), label_or_empty(s)))
    };
    let k3 = |s: &Value| -> Option<String> {
        if s.get("id").is_some() {
            return None;
        }
        Some(format!("{}|{}|{}", from_tokens(s).join(","), text(s.get("to")), how(s)))
    };
    let keys: [Key<'_, Value>; 3] = [&k1, &k2, &k3];
    let pairs = match_ordered(sb, sa, &keys);
    let in_b: HashSet<usize> = pairs.iter().map(|p| p.0).collect();
    let in_a: HashSet<usize> = pairs.iter().map(|p| p.1).collect();
    for (j, s) in sa.iter().enumerate() {
        if in_a.contains(&j) {
            continue;
        }
        let say = format!("New {} → {}.", entry_words(ga, s), page_name(ga, Some(&text(s.get("to")))));
        out.push(Change::new("entries", entry_key(s), Op::Added, Some(say)).after(Some(s)));
    }
    for (i, s) in sb.iter().enumerate() {
        if in_b.contains(&i) {
            continue;
        }
        let say = format!("Removed: the {} → {}.", entry_words(gb, s), page_name(gb, Some(&text(s.get("to")))));
        out.push(Change::new("entries", entry_key(s), Op::Removed, Some(say)).before(Some(s)));
    }
    const ENTRY_KNOWN: [&str; 9] = ["id", "from", "to", "how", "label", "when", "clicks", "data", "back"];
    for &(i, j) in &pairs {
        let (x, y) = (&sb[i], &sa[j]);
        let what = format!("The {}", entry_words(ga, y));
        let key = entry_key(y);
        out.extend(field_changes("entries", &key, raw(Some(x)), raw(Some(y)), &ENTRY_KNOWN, &|f, p, q| match f {
            "to" => Some(format!(
                "{what} now opens {} (was {}).",
                page_name(ga, Some(&text(y.get("to")))),
                page_name(gb, Some(&text(x.get("to"))))
            )),
            "label" => Some(format!("The {} now reads {}.", entry_words(gb, x), quote(&word(ga, &label_or_empty(y))))),
            "when" => Some(format!("Who sees the {} changed.", entry_words(ga, y))),
            "clicks" => {
                let n = |v: Option<&Value>| given_or(v, "1");
                Some(format!("{what} now takes {} clicks (was {}).", n(q), n(p)))
            }
            "data" => Some(if q == Some(&Value::Bool(true)) {
                format!("{what} now needs an item in a list.")
            } else {
                format!("{what} no longer needs an item in a list.")
            }),
            "back" => Some(if q == Some(&Value::Bool(true)) {
                format!("{what} now goes back.")
            } else {
                format!("{what} no longer goes back.")
            }),
            _ => None,
        }));
        other_keys(&key, raw(Some(x)), raw(Some(y)), &ENTRY_KNOWN, &mut out);
    }
    // the order of a menu is what people see; other entries are not ordered for them
    let is_menu = |s: &Value| s.get_str("how") == Some("menu");
    let mut groups: IndexMap<String, Vec<(usize, usize)>> = IndexMap::new();
    for &(i, j) in &pairs {
        if !is_menu(&sb[i]) || !is_menu(&sa[j]) {
            continue;
        }
        groups.entry(from_tokens(&sa[j]).join(",")).or_default().push((i, j));
    }
    for (g, group) in &groups {
        let positions = |list: &[Value]| -> Vec<usize> {
            list.iter()
                .enumerate()
                .filter(|(_, s)| is_menu(s) && from_tokens(s).join(",") == *g)
                .map(|(i, _)| i)
                .collect()
        };
        let pos_b = positions(sb);
        let pos_a = positions(sa);
        // `indexOf(i) + 1`: 0 when it is not there
        let place = |list: &[usize], i: usize| list.iter().position(|&x| x == i).map_or(0, |p| p + 1);
        for (i, j) in moved(group) {
            let s = &sa[j];
            let (was, now) = (place(&pos_b, i), place(&pos_a, j));
            let say = format!(
                "In {}, {} is now entry {now} (was {was}).",
                from_words(ga, s),
                quote(&word(ga, &label_or_empty(s)))
            );
            out.push(
                Change::new("entries", entry_key(s), Op::Moved, Some(say))
                    .before(Some(&Value::from(was)))
                    .after(Some(&Value::from(now))),
            );
        }
    }

    // goals
    let goal_id = |x: &Value| text(x.get("id"));
    let (kgb, goals_b) = by_key(&gb.goals, goal_id);
    let (kga, goals_a) = by_key(&ga.goals, goal_id);
    if kgb.len() != gb.goals.len() || kga.len() != ga.goals.len() {
        return Err(Error::graph("a goal id is written twice", None));
    }
    let who = |g: &NavGraph, ids: Option<&Value>| -> String {
        let ids = ids.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]);
        join_words(
            &ids.iter()
                .map(|id| {
                    quote(
                        &g.personas
                            .iter()
                            .find(|p| id.as_str() == Some(p.id.as_str()))
                            .map_or_else(|| js::to_js_string(id), |p| p.title.clone()),
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    let clicks_word = |within: Option<&Value>| if within == Some(&Value::Number(1.0)) { "click" } else { "clicks" };
    for id in union_keys(&keys_of(&kgb), &keys_of(&kga)) {
        let x = goals_b.get(&id).copied();
        let y = goals_a.get(&id).copied();
        if same(x, y) {
            continue;
        }
        let key = format!("goal {id}");
        let said = |g: Option<&Value>| quote(&text(member(g, "say")));
        match (x, y) {
            (None, y) => {
                let within = member(y, "within");
                out.push(Change::new(
                    "goals",
                    key,
                    Op::Added,
                    Some(format!(
                        "New goal: {}, for {}, within {} {}.",
                        said(y),
                        who(ga, member(y, "for")),
                        text(within),
                        clicks_word(within)
                    )),
                ));
            }
            (x, None) => out.push(Change::new("goals", key, Op::Removed, Some(format!("Goal dropped: {}.", said(x))))),
            (x, y) => {
                let page = |g: &NavGraph, v: Option<&Value>| page_name(g, Some(&text(member(v, "page"))));
                out.extend(field_changes(
                    "goals",
                    &key,
                    raw(x),
                    raw(y),
                    &["say", "page", "for", "within"],
                    &|f, _, _| {
                        Some(match f {
                            "say" => format!("Goal reworded: {} (was {}).", said(y), said(x)),
                            "page" => format!(
                                "The goal {} now means reaching {} (was {}).",
                                said(y),
                                page(ga, y),
                                page(gb, x)
                            ),
                            "for" => format!(
                                "The goal {} is now checked for {} (was {}).",
                                said(y),
                                who(ga, member(y, "for")),
                                who(gb, member(x, "for"))
                            ),
                            _ => {
                                let within = member(y, "within");
                                format!(
                                    "The goal {} now allows {} {} (was {}).",
                                    said(y),
                                    text(within),
                                    clicks_word(within),
                                    text(member(x, "within"))
                                )
                            }
                        })
                    },
                ));
                other_keys(&key, raw(x), raw(y), &["id", "say", "page", "for", "within"], &mut out);
            }
        }
    }

    let nm = word_changes(&gb.not_modeled, &ga.not_modeled);
    for x in &nm.added {
        out.push(
            Change::new(
                "notModeled",
                "notModeled",
                Op::Added,
                Some(format!("Now listed as not modeled: {}.", quote(x))),
            )
            .after(Some(&Value::from(x.as_str()))),
        );
    }
    for x in &nm.removed {
        out.push(
            Change::new(
                "notModeled",
                "notModeled",
                Op::Removed,
                Some(format!("No longer listed as not modeled: {}.", quote(x))),
            )
            .before(Some(&Value::from(x.as_str()))),
        );
    }

    for k in union(Some(&gb.catalog), Some(&ga.catalog)) {
        let p = member(Some(&gb.catalog), &k);
        let q = member(Some(&ga.catalog), &k);
        if strict_eq(p, q) {
            continue;
        }
        let say = match (p, q) {
            (None, q) => format!("New text: {}.", quote(&text(q))),
            (p, None) => format!("Text removed: {}.", quote(&text(p))),
            (p, q) => format!("{} now reads {}.", quote(&text(p)), quote(&text(q))),
        };
        out.push(Change::new("wording", format!("catalog {k}"), op_of(p, q), Some(say)).before(p).after(q));
    }
    Ok(NavStructure { changes: out, entries: pairs })
}

/// `String(v ?? fallback)`
fn given_or(v: Option<&Value>, fallback: &str) -> String {
    match v {
        None | Some(Value::Null) => fallback.to_string(),
        Some(v) => js::to_js_string(v),
    }
}

pub struct NavRun<'g> {
    pub g: &'g NavGraph,
    pub views: Vec<PersonaView<'g>>,
    pub goals: Vec<Goalcheck<'g>>,
    pub found: Vec<Finding>,
}

pub fn run_nav<'g>(g: &'g NavGraph, side: &str) -> Result<NavRun<'g>> {
    let run = || -> Result<NavRun<'g>> {
        let views = g.personas.iter().map(|p| view_of(g, p)).collect::<Result<Vec<_>>>()?;
        let goals = check_goals(g, &views);
        let found = findings(g, &views, &goals)?;
        Ok(NavRun { g, views, goals, found })
    };
    run().map_err(|err| Error::graph(format!("{side}: {}", err.message), None))
}

fn outcome_words(c: Option<&Goalcheck<'_>>) -> String {
    let Some(c) = c else { return "not checked".to_string() };
    let clicks = c.clicks.map_or_else(|| "null".to_string(), js::number_to_string);
    let within = c.goal.within_text();
    match c.outcome {
        Outcome::Ok => {
            let word = if c.goal.within() == Some(&Value::Number(1.0)) { "click" } else { "clicks" };
            format!("ok, {clicks} of {within} {word}")
        }
        Outcome::TooManyClicks => format!("{clicks} clicks, the limit is {within}"),
        other => other.as_str().to_string(),
    }
}

fn same_goal(a: &Goal<'_>, b: &Goal<'_>) -> bool {
    strict_eq(a.0.get("id"), b.0.get("id"))
}

pub fn nav_behaviour(rb: &NavRun<'_>, ra: &NavRun<'_>, s: &NavStructure) -> Result<Vec<Shift>> {
    let mut out: Vec<Shift> = Vec::new();
    let gb = rb.g;
    let ga = ra.g;
    let rows_b = menu_rows(gb, &rb.views)?;
    let rows_a = menu_rows(ga, &ra.views)?;
    let paired_a: HashMap<usize, usize> = s.entries.iter().map(|&(i, j)| (j, i)).collect();
    let paired_b: HashSet<usize> = s.entries.iter().map(|&(i, _)| i).collect();
    let all_pages = union_keys(&page_keys(gb), &page_keys(ga));
    let shown = |cells: &[crate::nav::MenuCell], i: usize| cells.get(i).is_some_and(|c| c.shown);
    let spec_key = |g: &NavGraph, i: usize| entry_key(g.specs.get(i).unwrap_or(&Value::Null));

    for (ia, va) in ra.views.iter().enumerate() {
        let id = &va.persona.id;
        let who = &va.persona.title;
        let ib = rb.views.iter().position(|v| v.persona.id == *id);
        let mut add = |topic: &str, key: &str, before: Value, after: Value, say: String, worse: bool| {
            out.push(Shift {
                topic: topic.to_string(),
                key: format!("{id} {key}"),
                persona: Some(who.clone()),
                before,
                after,
                say,
                detail: Vec::new(),
                worse,
            });
        };
        let reached = |v: &PersonaView<'_>, p: &str| v.standing_of(p).is_some_and(|s| s != Standing::Unreachable);
        let landing = |v: &PersonaView<'_>| v.landing.as_deref().map_or(Value::Null, Value::from);
        let Some(ib) = ib else {
            let n = ga.pages.keys().filter(|p| reached(va, p)).count();
            add(
                "persona",
                "new",
                Value::Null,
                landing(va),
                format!(
                    "A new kind of user: lands on {} and reaches {n} of {} pages.",
                    page_name(ga, va.landing.as_deref()),
                    ga.pages.len()
                ),
                false,
            );
            continue;
        };
        let vb = &rb.views[ib];
        if vb.landing != va.landing {
            add(
                "landing",
                "landing",
                landing(vb),
                landing(va),
                format!(
                    "Lands on {} (was {}).",
                    page_name(ga, va.landing.as_deref()),
                    page_name(gb, vb.landing.as_deref())
                ),
                false,
            );
        }
        let reach_of = |v: &PersonaView<'_>, page: &str| -> Value {
            let mut o = Object::new();
            if let Some(r) = v.reach.get(page) {
                o.insert("clicks", Value::Number(r.clicks));
            }
            if let Some(st) = v.standing_of(page) {
                o.insert("standing", Value::from(st.as_str()));
            }
            Value::Object(o)
        };
        for page in &all_pages {
            let in_b = gb.pages.contains_key(page) && reached(vb, page);
            let in_a = ga.pages.contains_key(page) && reached(va, page);
            let name = if ga.pages.contains_key(page) { page_name(ga, Some(page)) } else { page_name(gb, Some(page)) };
            if !in_b && in_a {
                let Some(st) = va.standing_of(page) else { continue };
                let which = if st == Standing::Ok { String::new() } else { format!(", which {}", standing_words(st)) };
                let clicks = va.reach.get(page.as_str()).map_or(0.0, |r| r.clicks);
                add(
                    "page",
                    page,
                    Value::Null,
                    reach_of(va, page),
                    format!(
                        "Can now reach {name}{which} ({} clicks: {}).",
                        js::number_to_string(clicks),
                        path_words(ga, va, page)
                    ),
                    false,
                );
            } else if in_b && !in_a {
                let removed = if ga.pages.contains_key(page) { "" } else { " (the page was removed)" };
                add(
                    "page",
                    page,
                    reach_of(vb, page),
                    Value::Null,
                    format!("Can no longer reach {name}{removed}."),
                    false,
                );
            } else if in_b && in_a {
                let (Some(st_b), Some(st_a)) = (vb.standing_of(page), va.standing_of(page)) else { continue };
                if st_b != st_a {
                    add(
                        "page",
                        page,
                        Value::from(st_b.as_str()),
                        Value::from(st_a.as_str()),
                        format!("{} {} (it {}).", capital(&name), standing_now(st_a), standing_past(st_b)),
                        false,
                    );
                }
                let cb = vb.reach.get(page.as_str()).map_or(0.0, |r| r.clicks);
                let ca = va.reach.get(page.as_str()).map_or(0.0, |r| r.clicks);
                if cb != ca {
                    add(
                        "clicks",
                        page,
                        Value::Number(cb),
                        Value::Number(ca),
                        format!(
                            "{}: {} → {} {} ({}).",
                            capital(&name),
                            js::number_to_string(cb),
                            js::number_to_string(ca),
                            if ca == 1.0 { "click" } else { "clicks" },
                            path_words(ga, va, page)
                        ),
                        false,
                    );
                }
            }
        }
        // the menu they see
        for row in &rows_a {
            let shown_a = shown(&row.cells, ia);
            let i = paired_a.get(&row.spec).copied();
            let row_b = i.and_then(|i| rows_b.iter().find(|r| r.spec == i));
            let shown_b = row_b.is_some_and(|r| shown(&r.cells, ib));
            let label = quote(&row.entry.label);
            if shown_a && !shown_b {
                let new = if i.is_none() { " (a new entry)" } else { "" };
                add(
                    "menu",
                    &spec_key(ga, row.spec),
                    Value::Bool(false),
                    Value::Bool(true),
                    format!("Their {} now shows {label}{new}.", menu_name(ga, &row.frame)),
                    false,
                );
            } else if !shown_a && shown_b {
                add(
                    "menu",
                    &spec_key(ga, row.spec),
                    Value::Bool(true),
                    Value::Bool(false),
                    format!("Their {} no longer shows {label}.", menu_name(ga, &row.frame)),
                    false,
                );
            }
        }
        for row in &rows_b {
            if paired_b.contains(&row.spec) || !shown(&row.cells, ib) {
                continue;
            }
            add(
                "menu",
                &spec_key(gb, row.spec),
                Value::Bool(true),
                Value::Bool(false),
                format!(
                    "Their {} no longer shows {} (the entry was removed).",
                    menu_name(gb, &row.frame),
                    quote(&row.entry.label)
                ),
                false,
            );
        }
        // what they come to do
        for c in ra.goals.iter().filter(|x| x.persona.id == *id) {
            let before = rb.goals.iter().find(|x| x.persona.id == *id && same_goal(&x.goal, &c.goal));
            if let Some(b) = before
                && b.outcome == c.outcome
                && b.clicks == c.clicks
            {
                continue;
            }
            if before.is_none() && c.outcome == Outcome::Ok {
                continue;
            }
            let worse = c.outcome != Outcome::Ok && before.is_none_or(|b| b.outcome == Outcome::Ok);
            add(
                "goal",
                &format!("goal {}", c.goal.id_text()),
                before.map_or(Value::Null, |b| Value::from(b.outcome.as_str())),
                Value::from(c.outcome.as_str()),
                format!("{}: {} (was {}).", quote(&c.goal.say_text()), outcome_words(Some(c)), outcome_words(before)),
                worse,
            );
        }
    }

    // findings, compared line by line: a finding's headline carries counts, so its detail lines are what is compared. A new
    // line and a gone line that start the same way ("Ders nazırı: …") and are the only such pair are one case that changed.
    let lines =
        |f: &Finding| -> Vec<String> { if f.detail.is_empty() { vec![f.say.clone()] } else { f.detail.clone() } };
    let codes_of = |found: &[Finding]| by_key(found, |f| f.code.clone()).0;
    let (cb, ca) = (codes_of(&rb.found), codes_of(&ra.found));
    let strings = |xs: &[String]| Value::Array(xs.iter().map(|x| Value::from(x.as_str())).collect());
    for code in union_keys(&keys_of(&cb), &keys_of(&ca)) {
        let fb = rb.found.iter().find(|f| f.code == code);
        let fa = ra.found.iter().find(|f| f.code == code);
        let lb = fb.map(lines).unwrap_or_default();
        let la = fa.map(lines).unwrap_or_default();
        let mut added: Vec<String> = la.iter().filter(|l| !lb.contains(l)).cloned().collect();
        let mut resolved: Vec<String> = lb.iter().filter(|l| !la.contains(l)).cloned().collect();
        let head = |l: &str| -> Option<String> { l.find(": ").map(|i| l[..i].to_string()) };
        let mut changed: Vec<String> = Vec::new();
        for l in added.clone() {
            let Some(h) = head(&l) else { continue };
            if added.iter().filter(|x| head(x).as_deref() == Some(h.as_str())).count() > 1 {
                continue;
            }
            let was: Vec<String> =
                resolved.iter().filter(|x| head(x).as_deref() == Some(h.as_str())).cloned().collect();
            if was.len() != 1 {
                continue;
            }
            changed.push(format!("{l} (was: {})", &was[0][h.len() + 2..]));
            added.retain(|x| *x != l);
            resolved.retain(|x| *x != was[0]);
        }
        let Some(level) = fa.or(fb).map(|f| f.level) else { continue };
        let mut push = |say: String, detail: &[String], worse: bool, before: Value, after: Value| {
            out.push(Shift {
                topic: "finding".to_string(),
                key: code.clone(),
                persona: None,
                before,
                after,
                say,
                detail: detail.iter().map(|d| format!("- {d}")).collect(),
                worse,
            });
        };
        match (fb, fa) {
            (None, Some(fa)) => push(
                format!("New {}: {}", fa.level.as_str(), fa.say),
                &fa.detail,
                fa.level == crate::nav::Level::Problem,
                Value::Null,
                strings(&la),
            ),
            (Some(fb), None) => push(
                format!("Resolved {}: {}", fb.level.as_str(), fb.say),
                &fb.detail,
                false,
                strings(&lb),
                Value::Null,
            ),
            (Some(fb), Some(fa)) => {
                let lv = capital(level.as_str());
                if !added.is_empty() {
                    push(
                        format!("{lv}, new cases: {}", fa.say),
                        &added,
                        level == crate::nav::Level::Problem,
                        Value::Null,
                        strings(&added),
                    );
                }
                if !resolved.is_empty() {
                    push(
                        format!("{lv}, cases resolved: {}", fb.say),
                        &resolved,
                        false,
                        strings(&resolved),
                        Value::Null,
                    );
                }
                if !changed.is_empty() {
                    push(
                        format!("{lv}, cases that changed: {}", fa.say),
                        &changed,
                        false,
                        Value::Null,
                        strings(&changed),
                    );
                }
            }
            (None, None) => {}
        }
    }
    Ok(out)
}

pub fn nav_same_behaviour(r: &NavRun<'_>) -> String {
    format!(
        "Nothing: each of the {} kinds of user lands on the same page, reaches the same pages in the same number of clicks and sees the same menu; goals and findings are the same.",
        r.views.len()
    )
}
