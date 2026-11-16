//! delurio-graph/0 read and checked as `@delurio/cli` does (SPEC.md): every module here is the Rust port of the
//! TypeScript module of the same name, and gives the same output for the same graphs.

// an error that keeps a graph from loading is rare and carries its place: it is returned as it is, not boxed
#![allow(clippy::result_large_err)]

pub mod error;
pub mod expr;
pub mod graph;
pub mod js;
pub mod nodepath;
pub mod yaml;
