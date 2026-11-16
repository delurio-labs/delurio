//! How values cross between JavaScript and the checker. A value of a graph file, or any object a caller hands in,
//! comes as the JSON text `JSON.stringify` gave (js/internal.js), read with `js::json_parse`; a structure the checker
//! computes goes out as JSON text, which js/*.js reads with `JSON.parse` and gives the shape the TypeScript module gave
//! (Sets, Maps, contexts as their values). An error that keeps a graph from loading goes out as a JavaScript error
//! whose message is MARK and the error as JSON: js/internal.js throws it as a GraphError, ModelError, KitError or
//! YAMLParseError. A callback never throws into the addon: js/internal.js runs it in a trap, which keeps what it
//! threw and hands the addon a value that says so; the addon then stops as the TypeScript code stopped, and the trap
//! throws the caller's own error again.

use delurio_core::error::{Error as CoreError, ErrorKind, ErrorPlace};
use delurio_core::js::{self, Object, Value};
use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Serialize;

/// the start of the message of an error js/internal.js lifts into its class
pub const MARK: &str = "\u{0}delurio\u{0}";

/// the code of a core error that stands for what a caller's callback threw
const THROWN: &str = "\u{0}thrown";

/// the error that stands for a callback's throw, with the message its error had (`(err as Error).message`)
pub fn thrown(message: String) -> CoreError {
    let mut e = CoreError::graph(message, None);
    e.code = Some(THROWN.to_string());
    e
}

fn place_value(p: &ErrorPlace) -> Value {
    let mut o = Object::new();
    o.insert("file", Value::from(p.file.as_str()));
    if let Some(path) = &p.path {
        o.insert("path", Value::Array(path.iter().map(|s| Value::from(s.as_str())).collect()));
    }
    if let Some(line) = p.line {
        o.insert("line", Value::from(line));
    }
    if let Some(column) = p.column {
        o.insert("column", Value::from(column));
    }
    Value::Object(o)
}

/// a place as JSON, for the few calls that hand one back
pub fn place_json(p: &ErrorPlace) -> String {
    js::stringify(&place_value(p))
}

/// the JavaScript error for an error of the checker
pub fn fail(e: CoreError) -> Error {
    let mut o = Object::new();
    if e.code.as_deref() == Some(THROWN) {
        o.insert("thrown", Value::Bool(true));
    } else {
        let kind = match e.kind {
            ErrorKind::Graph => "graph",
            ErrorKind::Model => "model",
            ErrorKind::Kit => "kit",
            ErrorKind::Yaml => "yaml",
        };
        o.insert("kind", Value::from(kind));
        o.insert("message", Value::from(e.message.as_str()));
        if let Some(at) = e.line_pos {
            o.insert("line", Value::from(at.line));
            o.insert("col", Value::from(at.column));
        }
        if let Some(code) = &e.code {
            o.insert("code", Value::from(code.as_str()));
        }
    }
    if let Some(p) = &e.place {
        o.insert("place", place_value(p));
    }
    Error::new(Status::GenericFailure, format!("{MARK}{}", js::stringify(&Value::Object(o))))
}

/// a plain JavaScript `Error` with this message (what the TypeScript code threw as `new Error(...)`)
pub fn plain(message: impl Into<String>) -> Error {
    let mut o = Object::new();
    o.insert("kind", Value::from("error"));
    o.insert("message", Value::from(message.into()));
    Error::new(Status::GenericFailure, format!("{MARK}{}", js::stringify(&Value::Object(o))))
}

/// the result of the checker, its error made a JavaScript one
pub fn ok<T>(r: delurio_core::error::Result<T>) -> Result<T> {
    r.map_err(fail)
}

/// a value handed in as JSON text
pub fn parse(text: &str) -> Result<Value> {
    js::json_parse(text).map_err(|e| Error::new(Status::InvalidArg, format!("delurio: a value that is not JSON: {e}")))
}

/// a value handed in as JSON text, or undefined
pub fn parse_opt(text: Option<String>) -> Result<Option<Value>> {
    text.as_deref().map(parse).transpose()
}

/// a structure written as JSON text, for js/*.js to read
pub fn json<T: Serialize + ?Sized>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::new(Status::GenericFailure, format!("delurio: {e}")))
}

/// what a trapped reader gives: the text it read, or the message of what it threw
#[napi(object)]
pub struct ReadOut {
    pub ok: Option<String>,
    pub thrown: Option<String>,
}

/// `read(file)` of a caller, as the checker's readers call it
pub fn reader<'a>(
    read: &'a Function<'a, String, ReadOut>,
) -> impl Fn(&str) -> delurio_core::error::Result<String> + 'a {
    move |file: &str| match read.call(file.to_string()) {
        Ok(ReadOut { ok: Some(text), .. }) => Ok(text),
        Ok(ReadOut { thrown: Some(message), .. }) => Err(thrown(message)),
        Ok(_) => Err(thrown("undefined".to_string())),
        Err(e) => Err(thrown(e.reason.clone())),
    }
}

/// `exists(file)` of a caller: 1 when it is there, 0 when not, 2 when the callback threw (read as not there; the
/// trap throws its error once the call is done)
pub fn exister<'a>(exists: &'a Function<'a, String, u32>) -> impl Fn(&str) -> bool + 'a {
    move |file: &str| matches!(exists.call(file.to_string()), Ok(1))
}

/// `warn(message)` of a caller
pub fn warner<'a>(warn: &'a Function<'a, String, ()>) -> impl FnMut(&str) + 'a {
    move |message: &str| {
        let _ = warn.call(message.to_string());
    }
}
