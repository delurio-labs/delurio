//! The schema rule against the TypeScript checker it ports: tests/schemas/cases.jsonl holds small graph folders (a
//! file of every kind, valid and not: typos, missing keys, wrong types and enums, patterns, if/then rules and their
//! reasons, names not allowed, a comma inside `{ }`, YAML errors before and after a refusal, catalogs that are not
//! JSON, another format, the graph's id from the file or beside it, a BOM, CRLF and non-ASCII columns), each with the
//! file it checks, Ajv 8.20.0's raw error list for it (keyword, instancePath, schemaPath, params, propertyName) and the
//! findings of validateFile, from `node gen-cases.mjs` on Node 26 (the differential harness of the port, running
//! dist/schemas.js and Ajv as schemas.ts builds it). The full harness also ran on every graph file of four graph sets
//! and examples/ (1798 files) and on 13500 mutated copies of them (values, documents and texts mutated): raw lists
//! and findings identical.

use std::path::Path;

use delurio_core::js::{self, Object, Value};
use delurio_core::schemas::{self, SchemaFinding, SchemaKind};
use delurio_core::yaml;

// a finding as the TypeScript object is written by JSON.stringify: a refusal's offset and code where they are
fn finding(f: &SchemaFinding) -> Value {
    let mut o = Object::new();
    if let Some(x) = f.offset {
        o.insert("offset", Value::from(x));
    }
    o.insert("line", Value::from(f.line));
    o.insert("column", Value::from(f.column));
    if let Some(c) = &f.code {
        o.insert("code", Value::from(c.as_str()));
    }
    o.insert("message", Value::from(f.message.as_str()));
    o.insert("graph", f.graph.as_deref().map_or(Value::Null, Value::from));
    o.insert("element", Value::from(f.element.as_str()));
    Value::Object(o)
}

// Ajv's errors for the value validateFile validates, or None where it returns before validating
fn raw(file: &str) -> Option<Vec<String>> {
    let kind = schemas::schema_kind_of(file)?;
    let text = String::from_utf8_lossy(&std::fs::read(file).ok()?).into_owned();
    let refused = kind != SchemaKind::Catalog && yaml::unsupported(&text).is_some();
    let parsed = yaml::parse_document(&text);
    if refused || !parsed.doc.errors.is_empty() {
        return None;
    }
    let data = if kind == SchemaKind::Catalog { js::json_parse(&text).ok()? } else { parsed.doc.to_js() };
    if kind != SchemaKind::Catalog && data.get_str("schema").is_some_and(|s| s != schemas::FORMAT) {
        return None;
    }
    let errors = schemas::validator(kind).validate(&data);
    Some(
        errors
            .iter()
            .map(|e| {
                let mut line = format!(
                    "{} {} {} {}",
                    e.keyword,
                    e.instance_path,
                    e.schema_path,
                    js::stringify(&e.params.to_value())
                );
                if let Some(p) = e.property_name {
                    line.push_str(&format!(" {p}"));
                }
                line
            })
            .collect(),
    )
}

#[test]
fn every_case_as_the_typescript_checker() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("schema-cases");
    let _ = std::fs::remove_dir_all(&dir);
    let cases = include_str!("schemas/cases.jsonl");
    let mut failed = Vec::new();
    for (i, line) in cases.lines().filter(|l| !l.is_empty()).enumerate() {
        let case = js::json_parse(line).expect("a case is JSON");
        let name = case.get_str("name").unwrap_or_default();
        let base = dir.join(i.to_string());
        for (path, text) in case.get_object("files").expect("a case has files").iter() {
            let file = base.join(path);
            std::fs::create_dir_all(file.parent().expect("a file is in a folder")).expect("the folder is made");
            std::fs::write(&file, text.as_str().unwrap_or_default()).expect("the file is written");
        }
        let file = base.join(case.get_str("check").unwrap_or_default());
        let file = file.to_str().expect("the path is UTF-8");
        let expected_raw = case.get_array("raw").map(|xs| xs.iter().map(js::to_js_string).collect::<Vec<_>>());
        let got_raw = raw(file);
        if got_raw != expected_raw {
            failed.push(format!("{name}: raw\n  expected {expected_raw:?}\n  got      {got_raw:?}"));
        }
        let expected = js::stringify(case.get("findings").unwrap_or(&Value::Null));
        let got = js::stringify(&Value::Array(schemas::validate_file(file).iter().map(finding).collect()));
        if got != expected {
            failed.push(format!("{name}: findings\n  expected {expected}\n  got      {got}"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
