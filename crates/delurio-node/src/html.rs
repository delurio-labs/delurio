//! html.js: the page of a flow (html.ts): its styles, and the page itself.
//!
//! PLACEHOLDER for `renderPage` and `callLine` until the page porter's port of html.ts is on rust/next: both answer
//! with an error that says so. `CSS` is the page's styles, byte for byte (crates/delurio-page/assets/page.css).

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// `CSS`
#[napi]
pub fn page_css() -> &'static str {
    delurio_page::html::CSS
}

/// `callLine(a)` and `renderPage(p)`: not on this build yet
#[napi]
pub fn page_not_ported(name: String) -> Result<()> {
    Err(crate::conv::plain(format!(
        "{name}: the page is not ported to Rust on this build yet (the port of html.ts in crates/delurio-page)"
    )))
}
