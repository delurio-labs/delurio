//! The errors that keep a graph from loading (SPEC.md §10), one type for the four the TypeScript checker threw: a
//! graph error, a model error, a kit error and a YAML error. Each may carry its place: the file it is in, relative to
//! the graph's folder, and the keys that lead to it, or the line and column a parser gave.

use std::fmt;

/// a line and a column, both 1-based
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct At {
    pub line: usize,
    pub column: usize,
}

/// where in a graph's files a load error is: `ErrorPlace` of graph.ts
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ErrorPlace {
    pub file: String, // flow.yaml, ui.yaml, catalog/en.json
    pub path: Option<Vec<String>>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl ErrorPlace {
    pub fn at(file: &str, path: &[&str]) -> Self {
        ErrorPlace {
            file: file.to_string(),
            path: Some(path.iter().map(|s| s.to_string()).collect()),
            line: None,
            column: None,
        }
    }
    pub fn file(file: &str) -> Self {
        ErrorPlace { file: file.to_string(), ..Default::default() }
    }
}

/// which of the TypeScript errors it is: its class, or its `name`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Graph,
    Model,
    Kit,
    /// `YAMLParseError` of the yaml library, and the refusal of `readYaml` that is thrown as one
    Yaml,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub place: Option<ErrorPlace>,
    /// `linePos[0]` of a YAML error: where the parser found it
    pub line_pos: Option<At>,
    /// `code` of a YAML error (`BAD_ALIAS`, `TAG_RESOLVE_FAILED`, ...)
    pub code: Option<String>,
}

impl Error {
    pub fn graph(message: impl Into<String>, place: Option<ErrorPlace>) -> Self {
        Error { kind: ErrorKind::Graph, message: message.into(), place, line_pos: None, code: None }
    }
    pub fn model(message: impl Into<String>) -> Self {
        Error { kind: ErrorKind::Model, message: message.into(), place: None, line_pos: None, code: None }
    }
    pub fn kit(message: impl Into<String>) -> Self {
        Error { kind: ErrorKind::Kit, message: message.into(), place: None, line_pos: None, code: None }
    }
    /// `placed(err, place)`: the place, given to an error that has none yet
    pub fn placed(mut self, place: ErrorPlace) -> Self {
        if self.place.is_none() {
            self.place = Some(place);
        }
        self
    }
    /// the error's first line: what `check` and `page` print of it
    pub fn first_line(&self) -> &str {
        self.message.split('\n').next().unwrap_or("")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T, E = Error> = std::result::Result<T, E>;
