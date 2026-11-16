//! kit.js: a design system's templates and styles (kit.ts). A kit `loadKit` read stays in the addon for the object
//! it gave; a kit object a caller built (a copy, say) is read back from its JSON, its templates as their parts.

use delurio_core::js::{self, Object, Value};
use delurio_core::kit::{
    CssTarget, Ctx, Fill, Flag, Kit, KitComponent, Part, Slot, Template, draw_with, kit_css, kit_stage_script,
    load_kit, mark, resolve_kit,
};
use indexmap::IndexMap;
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, ok, parse, warner};

#[napi]
pub struct NativeKit {
    pub(crate) kit: Kit,
}

fn text(v: Option<&Value>) -> String {
    v.map(js::to_js_string).unwrap_or_default()
}

fn part(v: &Value) -> Option<Part> {
    if let Some(lit) = v.get("lit") {
        return Some(Part::Lit { lit: js::to_js_string(lit) });
    }
    if let Some(slot) = v.get_str("slot") {
        let slot = match slot {
            "text" => Slot::Text,
            "name" => Slot::Name,
            "id" => Slot::Id,
            "intent" => Slot::Intent,
            "children" => Slot::Children,
            _ => return None,
        };
        let ctx = match v.get_str("ctx") {
            Some("dq") => Ctx::Dq,
            Some("sq") => Ctx::Sq,
            _ => Ctx::Text,
        };
        return Some(Part::Slot { slot, ctx });
    }
    let flag = match v.get_str("flag")? {
        "disabled" => Flag::Disabled,
        "named" => Flag::Named,
        "conditional" => Flag::Conditional,
        "checked" => Flag::Checked,
        _ => return None,
    };
    Some(Part::Section { flag, negate: v.get("negate").is_some_and(Value::truthy), parts: parts(v.get("parts")) })
}

fn parts(v: Option<&Value>) -> Vec<Part> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(part).collect()).unwrap_or_default()
}

fn template(v: Option<&Value>) -> Template {
    let v = v.cloned().unwrap_or(Value::Null);
    Template { source: text(v.get("source")), parts: parts(v.get("parts")) }
}

fn pairs(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some((js::to_js_string(p.index(0)?), p.index(1).map(js::to_js_string).unwrap_or_default()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// a kit object as `loadKit` gives it, read back
fn kit_of(v: &Value) -> Kit {
    let mut components: IndexMap<String, KitComponent> = IndexMap::new();
    for (name, c) in v.get_object("components").into_iter().flat_map(Object::iter) {
        let roles =
            c.get_object("roles").into_iter().flat_map(Object::iter).map(|(r, t)| (r.clone(), template(Some(t))));
        let intents = c
            .get_object("intents")
            .map(|o| o.iter().map(|(k, w)| (k.clone(), js::to_js_string(w))).collect::<IndexMap<String, String>>());
        components.insert(
            name.clone(),
            KitComponent { template: template(c.get("template")), roles: roles.collect(), intents },
        );
    }
    Kit {
        id: text(v.get("id")),
        title: text(v.get("title")),
        file: text(v.get("file")),
        components,
        light: pairs(v.get("light")),
        dark: pairs(v.get("dark")),
        css: text(v.get("css")),
        zoom: v.get("zoom").and_then(Value::as_f64).unwrap_or(f64::NAN),
    }
}

#[napi]
impl NativeKit {
    /// a kit object a caller built, read from its JSON
    #[napi(constructor)]
    pub fn new(text: String) -> Result<Self> {
        Ok(NativeKit { kit: kit_of(&parse(&text)?) })
    }

    /// the kit as the object `loadKit` gives
    #[napi]
    pub fn json(&self) -> Result<String> {
        json(&self.kit)
    }
}

/// `loadKit(path)`
#[napi(js_name = "loadKit")]
pub fn load_kit_at(path: String) -> Result<NativeKit> {
    ok(load_kit(&path)).map(|kit| NativeKit { kit })
}

/// `resolveKit(graphDir, uiKit, flagValue)`
#[napi(js_name = "resolveKit")]
pub fn resolve_kit_of(
    graph_dir: String,
    ui_kit: Option<String>,
    flag_value: Option<String>,
) -> Result<Option<NativeKit>> {
    ok(resolve_kit(&graph_dir, ui_kit.as_deref(), flag_value.as_deref())).map(|k| k.map(|kit| NativeKit { kit }))
}

/// `kitCss(kit, target)`
#[napi(js_name = "kitCss")]
pub fn kit_css_of(kit: &NativeKit, target: String) -> String {
    kit_css(&kit.kit, if target == "page" { CssTarget::Page } else { CssTarget::Stage })
}

/// `kitStageScript(kit)`
#[napi(js_name = "kitStageScript")]
pub fn kit_stage_script_of(kit: &NativeKit) -> String {
    kit_stage_script(&kit.kit)
}

/// the fill a caller gives: its keys as the template reads them (a key set to undefined is not in the JSON)
fn fill_of(v: &Value) -> Fill {
    let s = |k: &str| v.get(k).map_or_else(|| "undefined".to_string(), js::to_js_string);
    let opt = |k: &str| v.get(k).filter(|x| !x.is_null()).map(js::to_js_string);
    let flag = |k: &str| v.get(k).is_some_and(Value::truthy);
    Fill {
        id: s("id"),
        text: s("text"),
        name: s("name"),
        intent: opt("intent"),
        role: opt("role"),
        children: opt("children"),
        disabled: flag("disabled"),
        named: flag("named"),
        conditional: flag("conditional"),
        checked: flag("checked"),
    }
}

/// `drawWith(kit, component, f, warn)`
#[napi(js_name = "drawWith")]
pub fn draw_with_kit(
    kit: &NativeKit,
    component: String,
    fill: String,
    warn: Function<String, ()>,
) -> Result<Option<String>> {
    let f = fill_of(&parse(&fill)?);
    let mut warn = warner(&warn);
    Ok(draw_with(&kit.kit, &component, &f, &mut warn))
}

/// `mark(html, id, chips)`
#[napi(js_name = "mark")]
pub fn mark_html(html: String, id: String, chips: String) -> String {
    mark(&html, &id, &chips)
}
