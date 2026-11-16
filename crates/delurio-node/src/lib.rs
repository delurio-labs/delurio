//! The Node bindings of @delurio/cli (napi-rs): what the package's JavaScript modules (js/*.js) call. Each function
//! takes and gives plain JavaScript values, as the TypeScript modules did; the JavaScript side keeps the classes
//! (GraphError, ModelError, KitError, Model) and turns an error the checker returns into one of them.

use napi_derive::napi;

/// the version of the checker the addon was built from
#[napi]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
