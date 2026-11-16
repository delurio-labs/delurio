//! The Node bindings of @delurio/cli (napi-rs): what the package's JavaScript modules (js/*.js) call. Each module of
//! the package has its file here, named as the TypeScript module it stands for; js/internal.js holds how values,
//! callbacks and errors cross (conv.rs is this side of it). The JavaScript side keeps the classes (GraphError,
//! ModelError, KitError, Model) and the shapes the TypeScript modules gave (Sets, Maps, contexts of values).

// an error that keeps a graph from loading is rare and carries its place: it is returned as it is, not boxed
#![allow(clippy::result_large_err)]

mod conv;
mod edges;
mod expr;
mod graph;
mod html;
mod kit;
mod model;
mod nav;
mod needs;
mod spine;
mod story;
mod wireframe;
mod yaml;

use std::io::Write;

use napi_derive::napi;

/// the version of the checker the addon was built from
#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// `delurio <args>`, the whole command line, in this process: its exit code. What it writes goes to the process's
/// own stdout and stderr, flushed before it returns (nothing of Rust's runs after, to flush them at exit).
#[napi]
pub fn run_cli(args: Vec<String>) -> i32 {
    let code = delurio::run(&args);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    code
}
