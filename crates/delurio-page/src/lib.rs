//! The page of a graph (`delurio page`), ported from the TypeScript checker: the flow's diagram laid out by elkjs, its
//! screens, its story and its checks as one HTML file, its Mermaid diagram, and the pages of a navigation graph.

// an error that keeps a graph from loading is rare and carries its place: it is returned as it is, not boxed
#![allow(clippy::result_large_err)]

pub mod command;
pub mod elk;
pub mod html;
pub mod index_page;
pub mod mermaid;
pub mod nav_page;
pub mod pan;
pub mod spine;
pub mod watch;
