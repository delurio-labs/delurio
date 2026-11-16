//! spine.js: the flow's diagram (spine.ts). js/spine.js lays it out with Node's own elkjs, the one delurio's dashboard
//! lays its diagrams out with: the addon gives the graph elkjs is handed (`spineGraph`) and draws what elkjs gave back
//! (`spineSvg`).

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::graph::NativeGraph;
use crate::model::NativeExploration;

use std::rc::Rc;

use delurio_core::story::PathStep;
use delurio_page::spine::View;

/// the story's states as the steps spine.ts reads (only `state`)
fn story_of(states: &Option<Vec<String>>) -> Option<Vec<PathStep>> {
    states.as_ref().map(|s| {
        s.iter()
            .map(|state| PathStep { state: Rc::from(state.as_str()), ctx: Box::new([]), event: None, taken: None })
            .collect()
    })
}

/// `'main'` or `'full'`
fn view_of(view: &str) -> Result<View> {
    match view {
        "main" => Ok(View::Main),
        "full" => Ok(View::Full),
        other => Err(crate::conv::plain(format!("renderSpine: the view is main or full, not {other}"))),
    }
}

/// the graph `renderSpine` hands elkjs, as JSON
#[napi]
pub fn spine_graph(
    g: &NativeGraph,
    ex: &NativeExploration,
    view: String,
    story: Option<Vec<String>>,
) -> Result<String> {
    let story = story_of(&story);
    let root = delurio_page::spine::spine_graph(&g.g, &ex.ex, view_of(&view)?, story.as_deref());
    crate::conv::json(&root)
}

/// the SVG `renderSpine` gives, from the layout elkjs gave back (as JSON)
#[napi]
pub fn spine_svg(
    g: &NativeGraph,
    ex: &NativeExploration,
    view: String,
    story: Option<Vec<String>>,
    laid: String,
) -> Result<String> {
    let story = story_of(&story);
    let laid = crate::conv::parse(&laid)?;
    Ok(delurio_page::spine::spine_svg(&g.g, &ex.ex, view_of(&view)?, story.as_deref(), &laid))
}
