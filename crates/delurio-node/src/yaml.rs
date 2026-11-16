//! yaml.js: YAML as delurio reads it (yaml.ts).

use delurio_core::js::{Object, Value};
use delurio_core::yaml::{
    key_at, line_and_column, parse_document, parsing_once, read_yaml, take_warnings, unsupported,
};
use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::conv::{MARK, fail, json};

fn at(line: usize, column: usize) -> Value {
    let mut o = Object::new();
    o.insert("line", Value::from(line));
    o.insert("column", Value::from(column));
    Value::Object(o)
}

/// `unsupported(text)`, as JSON (null when nothing is)
#[napi(js_name = "unsupported")]
pub fn unsupported_in(text: String) -> Result<String> {
    let Some(u) = unsupported(&text) else { return Ok("null".into()) };
    let mut o = Object::new();
    o.insert("offset", Value::from(u.offset));
    o.insert("line", Value::from(u.line));
    o.insert("column", Value::from(u.column));
    o.insert("code", Value::from(u.code));
    o.insert("message", Value::from(u.message));
    json(&Value::Object(o))
}

/// `readYaml(text)`, as JSON. Its error is the library's YAMLParseError: the place the parser gives it (`pos`, as
/// offsets, and `linePos`) is added to the error, as the library's error has it
#[napi(js_name = "readYaml")]
pub fn read_yaml_text(text: String) -> Result<String> {
    match read_yaml(&text) {
        Ok(v) => json(&v),
        Err(e) => {
            let mut err = fail(e);
            let (pos, line_pos) = match unsupported(&text) {
                Some(u) => ((u.offset, u.offset + 1), vec![(u.line, u.column)]),
                None => {
                    let d = parse_document(&text);
                    match d.doc.errors.first() {
                        Some(first) => {
                            let a = d.line_pos(first.pos.0);
                            let b = d.line_pos(first.pos.1);
                            (first.pos, vec![(a.line, a.column), (b.line, b.column)])
                        }
                        None => return Err(err),
                    }
                }
            };
            // the error's JSON, with the place added
            if let Some(body) = err.reason.strip_prefix(MARK)
                && let Ok(Value::Object(mut o)) = delurio_core::js::json_parse(body)
            {
                o.insert("pos", Value::Array(vec![Value::from(pos.0), Value::from(pos.1)]));
                o.insert(
                    "linePos",
                    Value::Array(
                        line_pos
                            .into_iter()
                            .map(|(line, col)| {
                                let mut p = Object::new();
                                p.insert("line", Value::from(line));
                                p.insert("col", Value::from(col));
                                Value::Object(p)
                            })
                            .collect(),
                    ),
                );
                err = Error::new(
                    Status::GenericFailure,
                    format!("{MARK}{}", delurio_core::js::stringify(&Value::Object(o))),
                );
            }
            Err(err)
        }
    }
}

/// `keyAt(text, path)`, as JSON
#[napi(js_name = "keyAt")]
pub fn key_at_path(text: String, path: Vec<String>) -> Result<String> {
    let p = key_at(&text, &path);
    json(&at(p.line, p.column))
}

/// `lineAndColumn(text, offset)`, as JSON
#[napi(js_name = "lineAndColumn")]
pub fn line_and_column_at(text: String, offset: f64) -> Result<String> {
    let offset = if offset.is_nan() || offset < 0.0 { 0 } else { offset.min(usize::MAX as f64) as usize };
    let p = line_and_column(&text, offset);
    json(&at(p.line, p.column))
}

/// `parsingOnce(run)`: `run` is a trapped callback that keeps its own result
#[napi(js_name = "parsingOnce")]
pub fn parsing_once_run(run: Function<(), ()>) {
    parsing_once(|| {
        let _ = run.call(());
    });
}

/// the warnings the YAML reader met since the last call (the library hands each to `process.emitWarning`), as JSON
#[napi]
pub fn take_yaml_warnings() -> Result<String> {
    let all: Vec<Value> = take_warnings()
        .into_iter()
        .map(|w| {
            let mut o = Object::new();
            o.insert("code", Value::from(w.code));
            o.insert("message", Value::from(w.message));
            Value::Object(o)
        })
        .collect();
    json(&all)
}
