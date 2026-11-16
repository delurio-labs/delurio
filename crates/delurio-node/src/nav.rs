//! nav.js: navigation graphs (nav.ts). A graph `loadNav` read stays in the addon for the object it gave; one a caller
//! built is read back from its JSON. Personas, views and goal checks a caller hands back come as JSON, each view's
//! `reach` as the entries of its Map.

use delurio_core::js::{self, Object, Value};
use delurio_core::nav::{
    self, Entry, Goal, Goalcheck, How, MenuCell, NavGraph, Outcome, Page, PageKind, Persona, PersonaView, Reach,
    Standing, check_goals, findings, frame_level, frame_reached, guard_codes, holds, load_nav_with, menu_rows,
    refused_for, view_of, walk,
};
use indexmap::IndexMap;
use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Serialize;

use crate::conv::{ReadOut, json, ok, parse, reader};

#[napi]
pub struct NativeNav {
    pub(crate) g: NavGraph,
    warnings: Vec<String>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).map(js::to_js_string).unwrap_or_default()
}

fn opt(v: &Value, k: &str) -> Option<String> {
    v.get(k).filter(|x| !x.is_null()).map(js::to_js_string)
}

fn flag(v: &Value, k: &str) -> bool {
    v.get(k).is_some_and(Value::truthy)
}

fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn object(v: &Value, k: &str) -> Object {
    v.get_object(k).cloned().unwrap_or_default()
}

fn list(v: &Value, k: &str) -> Vec<Value> {
    v.get_array(k).cloned().unwrap_or_default()
}

fn page_kind(s: &str) -> PageKind {
    match s {
        "placeholder" => PageKind::Placeholder,
        "missing" => PageKind::Missing,
        "redirect" => PageKind::Redirect,
        "external" => PageKind::External,
        _ => PageKind::Screen,
    }
}

fn how(s: &str) -> How {
    match s {
        "auto" => How::Auto,
        "menu" => How::Menu,
        "picker" => How::Picker,
        "user" => How::User,
        "link" => How::Link,
        _ => How::Button,
    }
}

fn standing(s: &str) -> Standing {
    match s {
        "placeholder" => Standing::Placeholder,
        "missing" => Standing::Missing,
        "refused" => Standing::Refused,
        "unreachable" => Standing::Unreachable,
        _ => Standing::Ok,
    }
}

fn outcome(s: &str) -> Outcome {
    match s {
        "too many clicks" => Outcome::TooManyClicks,
        "not built" => Outcome::NotBuilt,
        "does not exist" => Outcome::DoesNotExist,
        "refused" => Outcome::Refused,
        "cannot reach" => Outcome::CannotReach,
        _ => Outcome::Ok,
    }
}

fn page_of(v: &Value) -> Page {
    Page {
        id: s(v, "id"),
        route: s(v, "route"),
        title: s(v, "title"),
        title_key: opt(v, "titleKey"),
        kind: page_kind(&s(v, "kind")),
        frame: s(v, "frame"),
        source: opt(v, "source"),
        opens_for: opt(v, "opensFor"),
        page_codes: opt(v, "pageCodes"),
        evidence: opt(v, "evidence"),
        refusal: opt(v, "refusal"),
        by_address: flag(v, "byAddress"),
    }
}

fn entry_of(v: &Value) -> Entry {
    Entry {
        id: s(v, "id"),
        spec: num(v, "spec") as usize,
        from: s(v, "from"),
        of_frame: opt(v, "ofFrame"),
        to: s(v, "to"),
        how: how(&s(v, "how")),
        label: s(v, "label"),
        label_key: opt(v, "labelKey"),
        when: opt(v, "when"),
        clicks: num(v, "clicks"),
        data: flag(v, "data"),
        back: flag(v, "back"),
        order: num(v, "order") as usize,
    }
}

fn persona_of(v: &Value) -> Persona {
    Persona { id: s(v, "id"), title: s(v, "title"), say: s(v, "say"), facts: object(v, "facts") }
}

/// a navigation graph object, as `loadNav` gives it, read back
fn nav_of(v: &Value) -> NavGraph {
    NavGraph {
        dir: s(v, "dir"),
        adapter: opt(v, "adapter"),
        frames: object(v, "frames"),
        menu_frames: list(v, "menuFrames").iter().map(js::to_js_string).collect(),
        id: s(v, "id"),
        title: s(v, "title"),
        status: s(v, "status"),
        summary: s(v, "summary"),
        locale: s(v, "locale"),
        source: v.get("source").cloned().unwrap_or(Value::Object(Object::new())),
        facts: object(v, "facts"),
        guards: object(v, "guards"),
        pages: v.get_object("pages").into_iter().flat_map(Object::iter).map(|(k, p)| (k.clone(), page_of(p))).collect(),
        entries: list(v, "entries").iter().map(entry_of).collect(),
        specs: list(v, "specs"),
        personas: list(v, "personas").iter().map(persona_of).collect(),
        goals: list(v, "goals"),
        not_modeled: list(v, "notModeled").iter().map(js::to_js_string).collect(),
        catalog: v.get("catalog").cloned().unwrap_or(Value::Object(Object::new())),
    }
}

#[napi]
impl NativeNav {
    /// a navigation graph object a caller built, read from its JSON
    #[napi(constructor)]
    pub fn new(text: String) -> Result<Self> {
        Ok(NativeNav { g: nav_of(&parse(&text)?), warnings: Vec::new() })
    }

    /// the graph as the object `loadNav` gives
    #[napi]
    pub fn json(&self) -> Result<String> {
        json(&self.g)
    }

    #[napi]
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.clone()
    }
}

/// `loadNav(dir, locale, read)`
#[napi(js_name = "loadNav")]
pub fn load_nav_of(dir: String, locale: Option<String>, read: Function<String, ReadOut>) -> Result<NativeNav> {
    let read = reader(&read);
    let loaded = ok(load_nav_with(&dir, locale.as_deref(), &read))?;
    Ok(NativeNav { g: loaded.g, warnings: loaded.warnings })
}

/// views a caller hands back: the personas and the entries their reach goes through, owned here for the views to
/// borrow
struct Views {
    personas: Vec<Persona>,
    vias: Vec<Vec<Option<Entry>>>,
    raw: Vec<Value>,
}

impl Views {
    fn read(text: &str) -> Result<Views> {
        let raw = match parse(text)? {
            Value::Array(items) => items,
            _ => Vec::new(),
        };
        let personas = raw.iter().map(|v| persona_of(v.get("persona").unwrap_or(&Value::Null))).collect();
        let vias = raw
            .iter()
            .map(|v| {
                list(v, "reach")
                    .iter()
                    .map(|pair| pair.index(1).and_then(|r| r.get("via")).filter(|x| x.is_object()).map(entry_of))
                    .collect()
            })
            .collect();
        Ok(Views { personas, vias, raw })
    }

    fn views(&self) -> Vec<PersonaView<'_>> {
        self.raw
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let reach = list(v, "reach")
                    .iter()
                    .enumerate()
                    .map(|(k, pair)| {
                        let key = pair.index(0).map(js::to_js_string).unwrap_or_default();
                        let r = pair.index(1).cloned().unwrap_or(Value::Null);
                        let reach = Reach {
                            page: s(&r, "page"),
                            clicks: num(&r, "clicks"),
                            via: self.vias[i].get(k).and_then(Option::as_ref),
                            prev: opt(&r, "prev"),
                            uses_data: flag(&r, "usesData"),
                            by_address: opt(&r, "byAddress"),
                        };
                        (key, reach)
                    })
                    .collect();
                let standing = v
                    .get_object("standing")
                    .into_iter()
                    .flat_map(Object::iter)
                    .map(|(k, x)| (k.clone(), standing(&js::to_js_string(x))))
                    .collect();
                PersonaView { persona: &self.personas[i], reach, standing, landing: opt(v, "landing") }
            })
            .collect()
    }
}

/// the position of an entry in a list of entries, by identity: what js/nav.js gives back as the caller's own object
fn position(list: &[Entry], e: &Entry) -> Option<usize> {
    list.iter().position(|x| std::ptr::eq(x, e))
}

/// a reach as JSON, its entry as its position in the graph's entries
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReachOut<'a> {
    page: &'a str,
    clicks: f64,
    via: Option<usize>,
    prev: &'a Option<String>,
    uses_data: bool,
    by_address: &'a Option<String>,
}

fn reach_out<'a>(g: &NavGraph, r: &'a Reach<'a>) -> ReachOut<'a> {
    ReachOut {
        page: &r.page,
        clicks: r.clicks,
        via: r.via.and_then(|e| position(&g.entries, e)),
        prev: &r.prev,
        uses_data: r.uses_data,
        by_address: &r.by_address,
    }
}

/// a view as JSON: its reach and its standing as entries, so js/nav.js gives a Map and an object of the same order
#[derive(Serialize)]
struct ViewOut<'a> {
    reach: Vec<(&'a str, ReachOut<'a>)>,
    standing: Vec<(&'a str, Standing)>,
    landing: &'a Option<String>,
}

fn view_out<'a>(g: &NavGraph, v: &'a PersonaView<'a>) -> ViewOut<'a> {
    ViewOut {
        reach: v.reach.iter().map(|(k, r)| (k.as_str(), reach_out(g, r))).collect(),
        standing: v.standing.iter().map(|(k, s)| (k.as_str(), *s)).collect(),
        landing: &v.landing,
    }
}

/// a menu row as JSON, its entry as its position in the graph's entries
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RowOut<'a> {
    spec: usize,
    frame: &'a str,
    entry: Option<usize>,
    frame_known: &'a [bool],
    cells: &'a [MenuCell],
}

/// a goal check as JSON: the goal as its position in the graph's goals, the persona as the position of its view
#[derive(Serialize)]
struct GoalOut {
    goal: Option<usize>,
    persona: Option<usize>,
    outcome: Outcome,
    clicks: Option<f64>,
}

/// `holds(g, p, expr)`
#[napi(js_name = "holds")]
pub fn holds_for(g: &NativeNav, persona: String, expr: Option<String>) -> Result<bool> {
    let p = persona_of(&parse(&persona)?);
    ok(holds(&g.g, &p, expr.as_deref()))
}

/// `refusedFor(g, p, page)`
#[napi(js_name = "refusedFor")]
pub fn refused_for_page(g: &NativeNav, persona: String, page: String) -> Result<bool> {
    let p = persona_of(&parse(&persona)?);
    ok(refused_for(&g.g, &p, &page))
}

/// `walk(g, p)`, as JSON entries
#[napi(js_name = "walk")]
pub fn walk_for(g: &NativeNav, persona: String) -> Result<String> {
    let p = persona_of(&parse(&persona)?);
    let reach = ok(walk(&g.g, &p))?;
    json(&reach.iter().map(|(k, r)| (k.as_str(), reach_out(&g.g, r))).collect::<Vec<_>>())
}

/// `pathTo(reach, page)`, the reach as JSON entries; the entries of the way as JSON
#[napi(js_name = "navPathTo")]
pub fn path_to_page(reach: String, page: String) -> Result<String> {
    let raw = match parse(&reach)? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let vias: Vec<Option<Entry>> = raw
        .iter()
        .map(|pair| pair.index(1).and_then(|r| r.get("via")).filter(|x| x.is_object()).map(entry_of))
        .collect();
    let map: IndexMap<String, Reach<'_>> = raw
        .iter()
        .enumerate()
        .map(|(k, pair)| {
            let r = pair.index(1).cloned().unwrap_or(Value::Null);
            let key = pair.index(0).map(js::to_js_string).unwrap_or_default();
            (
                key,
                Reach {
                    page: s(&r, "page"),
                    clicks: num(&r, "clicks"),
                    via: vias[k].as_ref(),
                    prev: opt(&r, "prev"),
                    uses_data: flag(&r, "usesData"),
                    by_address: opt(&r, "byAddress"),
                },
            )
        })
        .collect();
    let way: Vec<Option<usize>> = nav::path_to(&map, &page)
        .into_iter()
        .map(|e| vias.iter().position(|v| v.as_ref().is_some_and(|v| std::ptr::eq(v, e))))
        .collect();
    json(&way)
}

/// `viewOf(g, p)`, as JSON
#[napi(js_name = "viewOf")]
pub fn view_of_persona(g: &NativeNav, persona: String) -> Result<String> {
    let p = persona_of(&parse(&persona)?);
    let v = ok(view_of(&g.g, &p))?;
    json(&view_out(&g.g, &v))
}

/// `frameReached(g, v, frame)`, the view as JSON
#[napi(js_name = "frameReached")]
pub fn frame_reached_by(g: &NativeNav, view: String, frame: String) -> Result<bool> {
    let views = Views::read(&format!("[{view}]"))?;
    let v = views.views();
    Ok(v.first().is_some_and(|v| frame_reached(&g.g, v, &frame)))
}

/// `menuRows(g, views)`, as JSON
#[napi(js_name = "menuRows")]
pub fn menu_rows_of(g: &NativeNav, views: String) -> Result<String> {
    let views = Views::read(&views)?;
    let v = views.views();
    let rows = ok(menu_rows(&g.g, &v))?;
    let out: Vec<RowOut<'_>> = rows
        .iter()
        .map(|r| RowOut {
            spec: r.spec,
            frame: &r.frame,
            entry: position(&g.g.entries, r.entry),
            frame_known: &r.frame_known,
            cells: &r.cells,
        })
        .collect();
    json(&out)
}

/// `checkGoals(g, views)`, as JSON
#[napi(js_name = "checkGoals")]
pub fn check_goals_of(g: &NativeNav, views: String) -> Result<String> {
    let views = Views::read(&views)?;
    let v = views.views();
    let out: Vec<GoalOut> = check_goals(&g.g, &v)
        .iter()
        .map(|c| GoalOut {
            goal: g.g.goals.iter().position(|x| std::ptr::eq(x, c.goal.0)),
            persona: views.personas.iter().position(|x| std::ptr::eq(x, c.persona)),
            outcome: c.outcome,
            clicks: c.clicks,
        })
        .collect();
    json(&out)
}

/// `findings(g, views, goals)`, as JSON
#[napi(js_name = "findings")]
pub fn findings_of(g: &NativeNav, views: String, goals: String) -> Result<String> {
    let views = Views::read(&views)?;
    let v = views.views();
    let raw = match parse(&goals)? {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    let goal_values: Vec<Value> = raw.iter().map(|x| x.get("goal").cloned().unwrap_or(Value::Null)).collect();
    let personas: Vec<Persona> = raw.iter().map(|x| persona_of(x.get("persona").unwrap_or(&Value::Null))).collect();
    let checks: Vec<Goalcheck<'_>> = raw
        .iter()
        .enumerate()
        .map(|(i, x)| Goalcheck {
            goal: Goal(&goal_values[i]),
            persona: &personas[i],
            outcome: outcome(&s(x, "outcome")),
            clicks: x.get("clicks").and_then(Value::as_f64),
        })
        .collect();
    json(&ok(findings(&g.g, &v, &checks))?)
}

/// `guardCodes(g, src, depth)`
#[napi(js_name = "guardCodes")]
pub fn guard_codes_of(g: &NativeNav, src: String, depth: Option<u32>) -> Result<Vec<String>> {
    ok(guard_codes(&g.g, &src, depth.unwrap_or(0) as usize)).map(|set| set.into_iter().collect())
}

/// `frameLevel(e)`, the entry as JSON
#[napi(js_name = "frameLevel")]
pub fn frame_level_of(entry: String) -> Result<bool> {
    Ok(frame_level(&entry_of(&parse(&entry)?)))
}
