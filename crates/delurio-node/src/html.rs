//! html.js: the page of a flow (html.ts): its styles, and the page itself.
//!
//! `CSS` is the page's styles, byte for byte (crates/delurio-page/assets/page.css), which delurio's engine reads.
//! `renderPage` and `callLine` are not in the npm package since 0.11.0: nothing imports them, and `delurio page`
//! writes the page.

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// `CSS`
#[napi]
pub fn page_css() -> &'static str {
    delurio_page::html::CSS
}

/// `callLine(a)` and `renderPage(p)`: not in the npm package
#[napi]
pub fn page_not_ported(name: String) -> Result<()> {
    Err(crate::conv::plain(format!(
        "{name} is not in @delurio/cli's library since 0.11.0: `delurio page` writes a flow's page"
    )))
}
