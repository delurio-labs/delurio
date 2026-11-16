//! YAML as delurio reads it (SPEC.md §1): the port of src/yaml.ts.
//!
//! TEMPORARY: until the port of the yaml library lands, every call here asks Node to run the TypeScript checker's own
//! dist/yaml.js (the folder named by DELURIO_YAML_SHIM), so that the modules built on it can be run and compared
//! meanwhile. The signatures are the ones the port keeps.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{At, Error, ErrorKind, Result};
use crate::js::{self, Object, Value};

/// the first place in a text that delurio does not read: `Unsupported` of yaml.ts
#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported {
    pub offset: usize,
    pub line: usize,
    pub column: usize,
    pub code: String,
    pub message: String,
}

fn shim(request: Value) -> Value {
    let dist = std::env::var("DELURIO_YAML_SHIM").expect("DELURIO_YAML_SHIM names the TypeScript checker's dist/");
    let script = format!(
        r#"import {{ readYaml, keyAt, unsupported, lineAndColumn }} from '{dist}/yaml.js';
let input = ''; process.stdin.setEncoding('utf8');
for await (const c of process.stdin) input += c;
const r = JSON.parse(input); let out;
try {{
  if (r.op === 'read') out = {{ value: readYaml(r.text) }};
  else if (r.op === 'keyAt') out = keyAt(r.text, r.path);
  else if (r.op === 'unsupported') out = {{ u: unsupported(r.text) }};
}} catch (e) {{ out = {{ error: {{ name: e.name, message: e.message, linePos: e.linePos?.[0] ?? null, code: e.code ?? null }} }}; }}
process.stdout.write(JSON.stringify(out));"#
    );
    let mut child = Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("node runs");
    child.stdin.take().expect("stdin").write_all(js::stringify(&request).as_bytes()).expect("write");
    let out = child.wait_with_output().expect("node ends");
    js::json_parse(&String::from_utf8_lossy(&out.stdout)).expect("the shim answers JSON")
}

fn request(op: &str, text: &str) -> Object {
    let mut r = Object::new();
    r.insert("op", op.into());
    r.insert("text", text.into());
    r
}

/// the 1-based line and column of a UTF-16 offset into the text
pub fn line_and_column(text: &str, offset: usize) -> At {
    let mut line = 1;
    let mut from = 0;
    let mut at = 0;
    for c in text.chars() {
        if at >= offset {
            break;
        }
        at += c.len_utf16();
        if c == '\n' && at <= offset {
            line += 1;
            from = at;
        }
    }
    At { line, column: offset - from + 1 }
}

/// the first place in the text, by offset, that delurio does not read; None when there is none
pub fn unsupported(text: &str) -> Option<Unsupported> {
    let out = shim(Value::Object(request("unsupported", text)));
    let u = out.get("u")?;
    if u.is_null() {
        return None;
    }
    let n = |k: &str| u.get(k).and_then(Value::as_f64).unwrap_or(0.0) as usize;
    Some(Unsupported {
        offset: n("offset"),
        line: n("line"),
        column: n("column"),
        code: u.get_str("code").unwrap_or("").to_string(),
        message: u.get_str("message").unwrap_or("").to_string(),
    })
}

/// `run`, with every text it reads through `read_yaml` or `key_at` parsed once
pub fn parsing_once<T>(run: impl FnOnce() -> T) -> T {
    run()
}

/// the value of a graph file's text, once nothing unsupported is found in it; a text that is not YAML, or holds what
/// delurio does not read, is an error of kind Yaml at the place the parser gave
pub fn read_yaml(text: &str) -> Result<Value> {
    let out = shim(Value::Object(request("read", text)));
    if let Some(e) = out.get("error") {
        let line_pos = e.get("linePos").filter(|p| !p.is_null()).map(|p| At {
            line: p.get("line").and_then(Value::as_f64).unwrap_or(1.0) as usize,
            column: p.get("col").and_then(Value::as_f64).unwrap_or(1.0) as usize,
        });
        let kind = if e.get_str("name") == Some("YAMLParseError") { ErrorKind::Yaml } else { ErrorKind::Graph };
        return Err(Error {
            kind,
            message: e.get_str("message").unwrap_or("").to_string(),
            place: None,
            line_pos,
            code: e.get_str("code").map(str::to_string),
        });
    }
    Ok(out.get("value").cloned().unwrap_or(Value::Null))
}

/// the 1-based line and column of the key at `path` in a YAML text (of the item, for a list index), else the file's
/// start; for a text `read_yaml` has read
pub fn key_at<S: AsRef<str>>(text: &str, path: &[S]) -> At {
    let mut r = request("keyAt", text);
    r.insert("path", Value::Array(path.iter().map(|s| Value::from(s.as_ref())).collect()));
    let out = shim(Value::Object(r));
    At {
        line: out.get("line").and_then(Value::as_f64).unwrap_or(1.0) as usize,
        column: out.get("column").and_then(Value::as_f64).unwrap_or(1.0) as usize,
    }
}
