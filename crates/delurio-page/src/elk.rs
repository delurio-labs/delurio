//! The layout of a flow's diagram: elkjs 0.12.1 itself (vendor/elkjs), run in an embedded JavaScript engine
//! (QuickJS-NG, through rquickjs), so the page lays a graph out exactly as the TypeScript checker did and as delurio's
//! dashboard does with the same elkjs: the same positions to the last digit. ELK is Java compiled to JavaScript; no
//! Rust layout gives its results.
//!
//! Each thread has its own engine, made on its first layout (the bundle is read once per thread).

use std::cell::RefCell;

use delurio_core::js::{self, Value};
use rquickjs::{Context, Function, Persistent, Runtime};

const BUNDLE: &str = include_str!("../vendor/elkjs/elk.bundled.js");

// what elkjs looks for in its host: a CommonJS module, a console, and setTimeout, which its in-process worker posts
// its messages with; the timers are run until the layout settles
const PRELUDE: &str = r#"
var module = { exports: {} }; var exports = module.exports; var self = globalThis; var window = undefined;
var console = { log: function () {}, warn: function () {}, error: function () {} };
var __timers = [];
function setTimeout(f) { __timers.push(f); return __timers.length; }
function __drain() { var n = 0; while (__timers.length > 0) { __timers.shift()(); n++; } return n; }
"#;

const RUNNER: &str = r#"
(function () {
  var elk = new (module.exports)();
  return function (text) {
    var state = { done: false, out: null, err: null };
    elk.layout(JSON.parse(text)).then(
      function (r) { state.done = true; state.out = JSON.stringify(r); },
      function (e) { state.done = true; state.err = String(e && e.message ? e.message : e); }
    );
    return function () {
      if (!state.done) return null;
      if (state.err !== null) throw new Error(state.err);
      return state.out;
    };
  };
})()
"#;

struct Engine {
    // dropped in this order: the function before its context before the runtime
    run: Persistent<Function<'static>>,
    drain: Persistent<Function<'static>>,
    context: Context,
    _runtime: Runtime,
}

fn engine() -> Result<Engine, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_max_stack_size(64 * 1024 * 1024);
    // ELK allocates many small objects: a high threshold spares most cycle collections
    runtime.set_gc_threshold(512 * 1024 * 1024);
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    let (run, drain) = context.with(|ctx| -> Result<_, String> {
        let fail = |e: rquickjs::Error| {
            let caught = ctx.catch();
            let message = caught.as_exception().and_then(|x| x.message()).unwrap_or_default();
            format!("elkjs cannot be started: {e} {message}")
        };
        ctx.eval::<(), _>(PRELUDE).map_err(fail)?;
        ctx.eval::<(), _>(BUNDLE).map_err(fail)?;
        let run: Function = ctx.eval(RUNNER).map_err(fail)?;
        let drain: Function = ctx.globals().get("__drain").map_err(fail)?;
        Ok((Persistent::save(&ctx, run), Persistent::save(&ctx, drain)))
    })?;
    Ok(Engine { run, drain, context, _runtime: runtime })
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// `await new ELK().layout(graph)`: the graph with every node's and edge's place
pub fn layout(graph: &Value) -> Result<Value, String> {
    ENGINE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(engine()?);
        }
        let e = slot.as_ref().ok_or("no engine")?;
        let text = js::stringify(graph);
        let out = e.context.with(|ctx| -> Result<String, String> {
            let fail = |err: rquickjs::Error| {
                let caught = ctx.catch();
                caught.as_exception().and_then(|x| x.message()).unwrap_or_else(|| err.to_string())
            };
            let run = e.run.clone().restore(&ctx).map_err(fail)?;
            let drain = e.drain.clone().restore(&ctx).map_err(fail)?;
            let result: Function = run.call((text,)).map_err(fail)?;
            loop {
                let mut jobs = 0;
                while ctx.execute_pending_job() {
                    jobs += 1;
                }
                let timers: i32 = drain.call(()).map_err(fail)?;
                if jobs == 0 && timers == 0 {
                    break;
                }
            }
            let out: Option<String> = result.call(()).map_err(fail)?;
            out.ok_or_else(|| "the layout did not settle".to_string())
        })?;
        js::json_parse(&out).map_err(|e| e.message)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_as_elkjs_does() {
        // node -e "new (require('elkjs/lib/elk.bundled.js'))().layout(g).then(r => console.log(JSON.stringify(r)))"
        let graph = js::json_parse(
            r#"{"id":"root","layoutOptions":{"elk.algorithm":"layered","elk.direction":"DOWN"},"children":[{"id":"a","width":30,"height":20},{"id":"b","width":30,"height":20}],"edges":[{"id":"e","sources":["a"],"targets":["b"]}]}"#,
        )
        .unwrap();
        let out = layout(&graph).unwrap();
        assert_eq!(
            js::stringify(&out),
            r#"{"id":"root","layoutOptions":{"elk.algorithm":"layered","elk.direction":"DOWN"},"children":[{"id":"a","width":30,"height":20,"$H":277,"x":12,"y":12},{"id":"b","width":30,"height":20,"$H":279,"x":12,"y":52}],"edges":[{"id":"e","sources":["a"],"targets":["b"],"sections":[{"id":"e_s0","startPoint":{"x":27,"y":32},"endPoint":{"x":27,"y":52},"incomingShape":"a","outgoingShape":"b"}],"container":"root"}],"$H":13,"x":0,"y":0,"width":54,"height":84}"#
        );
    }
}
