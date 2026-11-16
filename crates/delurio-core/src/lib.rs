//! delurio-graph/0 read and checked as `@delurio/cli` does (SPEC.md): every module here is the Rust port of the
//! TypeScript module of the same name, and gives the same output for the same graphs.

// an error that keeps a graph from loading is rare and carries its place: it is returned as it is, not boxed
#![allow(clippy::result_large_err)]

pub mod anchors;
pub mod changes;
pub mod check;
pub mod compare;
pub mod diff;
pub mod diff_flow;
pub mod diff_nav;
pub mod domain;
pub mod domain_names;
pub mod edges;
pub mod error;
pub mod expr;
pub mod graph;
pub mod invokes;
pub mod js;
pub mod kit;
pub mod model;
pub mod nav;
pub mod needs;
pub mod nodepath;
pub mod schemas;
pub mod starts;
pub mod story;
pub mod tutorial_check;
pub mod ui_check;
pub mod wireframe;
pub mod yaml;
