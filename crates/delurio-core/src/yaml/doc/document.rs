//! The port of the yaml library's doc/Document.ts, as a parsed document holds it: its contents, errors, warnings and
//! range, and `toJS`.

use super::directives::Directives;
use crate::js::Value;
use crate::yaml::errors::YamlIssue;
use crate::yaml::nodes::{Node, ToJs};

/// `Document`: `contents` is None for a stream with no document in it
#[derive(Debug, Clone)]
pub struct Document {
    pub contents: Option<Node>,
    pub errors: Vec<YamlIssue>,
    pub warnings: Vec<YamlIssue>,
    pub range: [usize; 3],
    pub(crate) directives: Directives,
}

impl Document {
    pub(crate) fn new(directives: Directives) -> Self {
        Document { contents: None, errors: Vec::new(), warnings: Vec::new(), range: [0; 3], directives }
    }

    /// `doc.toJS()`: the document's value as JavaScript holds it
    pub fn to_js(&self) -> Value {
        match &self.contents {
            Some(node) => ToJs::new().js_of(node),
            None => Value::Null,
        }
    }
}
