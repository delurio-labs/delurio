//! The guard and assertion language: a small, fixed subset, interpreted without eval.
//!   or / and / not, == / != / in / not in, parentheses, [lists], 'strings', numbers, true / false / null, names.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

use indexmap::IndexSet;
use regex::Regex;
use serde::Serialize;

use crate::js::{self, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Op {
    #[serde(rename = "==")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
    #[serde(rename = "in")]
    In,
    #[serde(rename = "not in")]
    NotIn,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "k", rename_all = "lowercase")]
pub enum Expr {
    Lit { v: Value },
    Id { name: String },
    List { items: Vec<Expr> },
    Not { e: Box<Expr> },
    And { l: Box<Expr>, r: Box<Expr> },
    Or { l: Box<Expr>, r: Box<Expr> },
    Cmp { op: Op, l: Box<Expr>, r: Box<Expr> },
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<Expr>>> = RefCell::new(HashMap::new());
}

static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*(?:==|!=|[()\[\],]|'[^']*'|"[^"]*"|-?[0-9]+(?:\.[0-9]+)?|[A-Za-z_][A-Za-z0-9_.]*)"#)
        .expect("the token pattern compiles")
});

fn tokenize(src: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        match TOKEN.find(&src[i..]) {
            None => {
                if js::trim(&src[i..]).is_empty() {
                    break;
                }
                return Err(format!("cannot read \"{}\" in: {src}", &src[i..]));
            }
            Some(m) => {
                out.push(js::trim(m.as_str()).to_string());
                i += m.end();
            }
        }
    }
    Ok(out)
}

struct Reader<'a> {
    src: &'a str,
    toks: Vec<String>,
    i: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<&str> {
        self.toks.get(self.i).map(String::as_str)
    }
    fn next(&mut self) -> Option<String> {
        let t = self.toks.get(self.i).cloned();
        self.i += 1;
        t
    }
    fn expect(&mut self, t: &str) -> Result<(), String> {
        if self.next().as_deref() != Some(t) {
            return Err(format!("expected \"{t}\" in: {}", self.src));
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<Expr, String> {
        let Some(t) = self.next() else { return Err(format!("unexpected end in: {}", self.src)) };
        if t == "(" {
            let e = self.or_expr()?;
            self.expect(")")?;
            return Ok(e);
        }
        if t == "[" {
            let mut items = Vec::new();
            while self.peek() != Some("]") {
                items.push(self.atom()?);
                if self.peek() == Some(",") {
                    self.next();
                }
            }
            self.expect("]")?;
            return Ok(Expr::List { items });
        }
        if t.starts_with('\'') || t.starts_with('"') {
            let inner = if t.len() >= 2 { &t[1..t.len() - 1] } else { "" };
            return Ok(Expr::Lit { v: Value::String(inner.to_string()) });
        }
        let first = t.as_bytes()[0];
        let second = t.as_bytes().get(1).copied();
        if first.is_ascii_digit() || (first == b'-' && second.is_some_and(|c| c.is_ascii_digit())) {
            return Ok(Expr::Lit { v: Value::Number(t.parse::<f64>().unwrap_or(f64::NAN)) });
        }
        match t.as_str() {
            "true" => return Ok(Expr::Lit { v: Value::Bool(true) }),
            "false" => return Ok(Expr::Lit { v: Value::Bool(false) }),
            "null" => return Ok(Expr::Lit { v: Value::Null }),
            _ => {}
        }
        if (first.is_ascii_alphabetic() || first == b'_') && !["and", "or", "not", "in"].contains(&t.as_str()) {
            return Ok(Expr::Id { name: t });
        }
        Err(format!("unexpected \"{t}\" in: {}", self.src))
    }

    fn cmp_expr(&mut self) -> Result<Expr, String> {
        let l = self.atom()?;
        match self.peek() {
            Some(p @ ("==" | "!=")) => {
                let op = if p == "==" { Op::Eq } else { Op::Ne };
                self.next();
                Ok(Expr::Cmp { op, l: Box::new(l), r: Box::new(self.atom()?) })
            }
            Some("in") => {
                self.next();
                Ok(Expr::Cmp { op: Op::In, l: Box::new(l), r: Box::new(self.atom()?) })
            }
            Some("not") if self.toks.get(self.i + 1).map(String::as_str) == Some("in") => {
                self.i += 2;
                Ok(Expr::Cmp { op: Op::NotIn, l: Box::new(l), r: Box::new(self.atom()?) })
            }
            _ => Ok(l),
        }
    }

    fn not_expr(&mut self) -> Result<Expr, String> {
        if self.peek() == Some("not") {
            self.next();
            return Ok(Expr::Not { e: Box::new(self.not_expr()?) });
        }
        self.cmp_expr()
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut l = self.not_expr()?;
        while self.peek() == Some("and") {
            self.next();
            l = Expr::And { l: Box::new(l), r: Box::new(self.not_expr()?) };
        }
        Ok(l)
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut l = self.and_expr()?;
        while self.peek() == Some("or") {
            self.next();
            l = Expr::Or { l: Box::new(l), r: Box::new(self.and_expr()?) };
        }
        Ok(l)
    }
}

/// the expression of a guard or an assertion; the error is the message the TS parser threw
pub fn parse_expr(src: &str) -> Result<Rc<Expr>, String> {
    if let Some(hit) = CACHE.with(|c| c.borrow().get(src).cloned()) {
        return Ok(hit);
    }
    let toks = tokenize(src)?;
    let mut r = Reader { src, toks, i: 0 };
    let e = r.or_expr()?;
    if r.i != r.toks.len() {
        let t = r.toks.get(r.i).cloned().unwrap_or_else(|| "undefined".to_string());
        return Err(format!("unexpected \"{t}\" in: {src}"));
    }
    let e = Rc::new(e);
    CACHE.with(|c| c.borrow_mut().insert(src.to_string(), Rc::clone(&e)));
    Ok(e)
}

/// the expression's value, its names read through `lookup`
pub fn evaluate(e: &Expr, lookup: &dyn Fn(&str) -> Value) -> Value {
    match e {
        Expr::Lit { v } => v.clone(),
        Expr::Id { name } => lookup(name),
        Expr::List { items } => Value::Array(items.iter().map(|x| evaluate(x, lookup)).collect()),
        Expr::Not { e } => Value::Bool(!evaluate(e, lookup).truthy()),
        Expr::And { l, r } => Value::Bool(evaluate(l, lookup).truthy() && evaluate(r, lookup).truthy()),
        Expr::Or { l, r } => Value::Bool(evaluate(l, lookup).truthy() || evaluate(r, lookup).truthy()),
        Expr::Cmp { op, l, r } => {
            let l = evaluate(l, lookup);
            let r = evaluate(r, lookup);
            match op {
                Op::Eq => Value::Bool(l.strict_eq(&r)),
                Op::Ne => Value::Bool(!l.strict_eq(&r)),
                Op::In | Op::NotIn => {
                    let in_list = r.as_array().is_some_and(|items| items.iter().any(|x| x.strict_eq(&l)));
                    Value::Bool(if *op == Op::In { in_list } else { !in_list })
                }
            }
        }
    }
}

/// the expression written out again in the language it was read from, every compound part in parentheses, so that
/// `parse_expr` reads back the same tree
pub fn source(e: &Expr) -> String {
    let part = |x: &Expr| match x {
        Expr::Lit { .. } | Expr::Id { .. } | Expr::List { .. } => source(x),
        _ => format!("({})", source(x)),
    };
    match e {
        Expr::Lit { v: Value::String(s) } if s.contains('\'') => format!("\"{s}\""),
        Expr::Lit { v: Value::String(s) } => format!("'{s}'"),
        Expr::Lit { v } => js::to_js_string(v),
        Expr::Id { name } => name.clone(),
        Expr::List { items } => format!("[{}]", items.iter().map(part).collect::<Vec<_>>().join(", ")),
        Expr::Not { e } => format!("not {}", part(e)),
        Expr::And { l, r } => format!("{} and {}", part(l), part(r)),
        Expr::Or { l, r } => format!("{} or {}", part(l), part(r)),
        Expr::Cmp { op, l, r } => {
            let op = match op {
                Op::Eq => "==",
                Op::Ne => "!=",
                Op::In => "in",
                Op::NotIn => "not in",
            };
            format!("{} {op} {}", part(l), part(r))
        }
    }
}

/// An expression that holds where `e` comes out `want` whatever the names that `unknown` picks out hold. Those names
/// are read as unknown, as in Kleene's three-valued logic: a part that reads one decides nothing, and `not`, `and`
/// and `or` keep what the rest decides. With no such name in it, the result is `e` itself (or `not e`). So a repeat
/// node's `disabledWhen` that reads its place among the rows (SPEC.md §4) disables it in a situation only where it
/// holds in every row.
pub fn surely(e: &Expr, unknown: &dyn Fn(&str) -> bool, want: bool) -> Expr {
    if !identifiers(e).iter().any(|n| unknown(n)) {
        return if want { e.clone() } else { Expr::Not { e: Box::new(e.clone()) } };
    }
    let both = |l: &Expr, r: &Expr, w: bool| (Box::new(surely(l, unknown, w)), Box::new(surely(r, unknown, w)));
    match e {
        Expr::Not { e } => surely(e, unknown, !want),
        Expr::And { l, r } if want => {
            let (l, r) = both(l, r, true);
            Expr::And { l, r }
        }
        Expr::And { l, r } => {
            let (l, r) = both(l, r, false);
            Expr::Or { l, r }
        }
        Expr::Or { l, r } if want => {
            let (l, r) = both(l, r, true);
            Expr::Or { l, r }
        }
        Expr::Or { l, r } => {
            let (l, r) = both(l, r, false);
            Expr::And { l, r }
        }
        // a name, a comparison or a list that reads an unknown name: neither surely true nor surely false
        _ => Expr::Lit { v: Value::Bool(false) },
    }
}

/// every name the expression reads, in the order it reads them
pub fn identifiers(e: &Expr) -> IndexSet<String> {
    let mut out = IndexSet::new();
    collect(e, &mut out);
    out
}

fn collect(e: &Expr, out: &mut IndexSet<String>) {
    match e {
        Expr::Id { name } => {
            out.insert(name.clone());
        }
        Expr::List { items } => items.iter().for_each(|x| collect(x, out)),
        Expr::Not { e } => collect(e, out),
        Expr::And { l, r } | Expr::Or { l, r } | Expr::Cmp { l, r, .. } => {
            collect(l, out);
            collect(r, out);
        }
        Expr::Lit { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(name: &str) -> Value {
        match name {
            "a" => Value::from("x"),
            "n" => Value::Number(2.0),
            "t" => Value::Bool(true),
            _ => Value::Null,
        }
    }

    #[test]
    fn reads_and_evaluates() {
        let e = parse_expr("a == 'x' and not (n in [1, 3]) or t").unwrap();
        assert_eq!(evaluate(&e, &ctx), Value::Bool(true));
        assert_eq!(identifiers(&e).into_iter().collect::<Vec<_>>(), ["a", "n", "t"]);
        assert_eq!(evaluate(&parse_expr("n not in [2]").unwrap(), &ctx), Value::Bool(false));
        assert_eq!(evaluate(&parse_expr("[1] == [1]").unwrap(), &ctx), Value::Bool(false));
        assert_eq!(evaluate(&parse_expr("-2.5").unwrap(), &ctx), Value::Number(-2.5));
    }

    #[test]
    fn errors_as_the_ts_parser_words_them() {
        // node --input-type=module -e "import {parseExpr} from './dist/expr.js'; try { parseExpr(x) } catch (e) { console.log(e.message) }"
        assert_eq!(parse_expr("a ==").unwrap_err(), "unexpected end in: a ==");
        assert_eq!(parse_expr("a = b").unwrap_err(), "cannot read \" = b\" in: a = b");
        assert_eq!(parse_expr("[1, 2").unwrap_err(), "unexpected end in: [1, 2");
        assert_eq!(parse_expr("a b").unwrap_err(), "unexpected \"b\" in: a b");
        assert_eq!(parse_expr("(a").unwrap_err(), "expected \")\" in: (a");
        assert_eq!(parse_expr("and").unwrap_err(), "unexpected \"and\" in: and");
        assert_eq!(parse_expr("a   ").unwrap(), Rc::new(Expr::Id { name: "a".into() }));
    }

    #[test]
    fn written_out_it_reads_back_the_same() {
        for src in [
            "a == 'x' and not (n in [1, 3]) or t",
            "n not in [2, 'b'] and \"it's\" != a",
            "not not t or (a and (b or c))",
            "-2.5 == n and null == a and false",
        ] {
            let e = parse_expr(src).unwrap();
            assert_eq!(parse_expr(&source(&e)).unwrap(), e, "{src}: {}", source(&e));
        }
    }

    #[test]
    fn surely_reads_the_unknown_names_as_deciding_nothing() {
        let place = |n: &str| n.starts_with("repeat.");
        let s = |src: &str, want: bool| source(&surely(&parse_expr(src).unwrap(), &place, want));
        // without an unknown name: the expression, or its negation
        assert_eq!(s("busy", true), "busy");
        assert_eq!(s("busy", false), "not busy");
        // a part that reads one decides nothing
        assert_eq!(s("repeat.first", true), "false");
        assert_eq!(s("repeat.index == 1", false), "false");
        assert_eq!(s("busy or repeat.first", true), "busy or false");
        assert_eq!(s("busy and repeat.last", true), "busy and false");
        assert_eq!(s("busy and repeat.last", false), "(not busy) or false");
        assert_eq!(s("not (busy or repeat.first)", true), "(not busy) and false");
        let holds = |src: &str, busy: bool| {
            let e = surely(&parse_expr(src).unwrap(), &place, true);
            evaluate(&e, &|n| if n == "busy" { Value::Bool(busy) } else { Value::Null }).truthy()
        };
        assert!(holds("busy or repeat.first", true));
        assert!(!holds("busy or repeat.first", false));
        assert!(!holds("busy and repeat.first", true));
        assert!(holds("not (not busy and repeat.first)", true));
        assert!(!holds("not (not busy and repeat.first)", false));
    }
}
