//! expr.js: the guard and assertion language (expr.ts). An expression crosses as the object `parseExpr` gives
//! (`{ k: 'lit', v }`, `{ k: 'id', name }`, ...), as JSON.

use delurio_core::expr::{Expr, Op, evaluate, identifiers, parse_expr};
use delurio_core::js::{self, Value};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{json, parse, plain};

/// `parseExpr(src)`, as JSON
#[napi(js_name = "parseExpr")]
pub fn parse_expr_of(src: String) -> Result<String> {
    let e = parse_expr(&src).map_err(plain)?;
    json(&*e)
}

fn boxed(v: Option<&Value>) -> Box<Expr> {
    Box::new(expr_of(v.unwrap_or(&Value::Null)))
}

/// an expression object read back; one of no known kind is the literal undefined (null here), which is what
/// `evaluate` of it gave
pub(crate) fn expr_of(v: &Value) -> Expr {
    match v.get_str("k") {
        Some("lit") => Expr::Lit { v: v.get("v").cloned().unwrap_or(Value::Null) },
        Some("id") => Expr::Id { name: v.get("name").map(js::to_js_string).unwrap_or_default() },
        Some("list") => {
            Expr::List { items: v.get_array("items").map(|a| a.iter().map(expr_of).collect()).unwrap_or_default() }
        }
        Some("not") => Expr::Not { e: boxed(v.get("e")) },
        Some("and") => Expr::And { l: boxed(v.get("l")), r: boxed(v.get("r")) },
        Some("or") => Expr::Or { l: boxed(v.get("l")), r: boxed(v.get("r")) },
        Some("cmp") => {
            let op = match v.get_str("op") {
                Some("==") => Op::Eq,
                Some("!=") => Op::Ne,
                Some("in") => Op::In,
                _ => Op::NotIn,
            };
            Expr::Cmp { op, l: boxed(v.get("l")), r: boxed(v.get("r")) }
        }
        _ => Expr::Lit { v: Value::Null },
    }
}

/// `evaluate(e, lookup)`, as JSON: `lookup` gives each name's value as JSON (a trapped one gives null once it threw)
#[napi(js_name = "evaluate")]
pub fn evaluate_expr(env: Env, e: String, lookup: Function<String, String>) -> Result<String> {
    let e = expr_of(&parse(&e)?);
    let look = |name: &str| -> Value {
        env.run_in_scope(|| lookup.call(name.to_string()))
            .ok()
            .and_then(|text| js::json_parse(&text).ok())
            .unwrap_or(Value::Null)
    };
    json(&evaluate(&e, &look))
}

/// `identifiers(e)`, in the order the expression reads them
#[napi(js_name = "identifiers")]
pub fn identifiers_of(e: String) -> Result<Vec<String>> {
    Ok(identifiers(&expr_of(&parse(&e)?)).into_iter().collect())
}
