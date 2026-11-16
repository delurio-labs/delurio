//! needs.js: a flow's needs and a product's gaps (needs.ts).

use delurio_core::js::{Object, Value};
use delurio_core::needs::{GAPS_FILE, NEEDS_FILE, Needs, check_needs, planned, read_needs};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, ok, parse_opt};

/// NEEDS_FILE and GAPS_FILE
#[napi]
pub fn needs_constants() -> Result<String> {
    let mut o = Object::new();
    o.insert("NEEDS_FILE", Value::from(NEEDS_FILE));
    o.insert("GAPS_FILE", Value::from(GAPS_FILE));
    json(&Value::Object(o))
}

/// `readNeeds(dir)`, as JSON (null when the folder has none)
#[napi(js_name = "readNeeds")]
pub fn read_needs_in(dir: String) -> Result<String> {
    match ok(read_needs(&dir))? {
        Some(n) => json(n.value()),
        None => Ok("null".into()),
    }
}

/// `planned(needs)`, as JSON entries of its two Maps
#[napi(js_name = "planned")]
pub fn planned_of(needs: Option<String>) -> Result<String> {
    let needs = parse_opt(needs)?.filter(|v| !v.is_null()).map(Needs);
    json(&planned(needs.as_ref()))
}

/// `checkNeeds(files, app)`, as JSON
#[napi(js_name = "checkNeeds")]
pub fn check_needs_of(files: Vec<String>, app: Option<String>) -> Result<String> {
    json(&check_needs(&files, app.as_deref()))
}
