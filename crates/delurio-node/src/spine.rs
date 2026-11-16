//! spine.js: the flow's diagram (spine.ts). js/spine.js lays it out with Node's own elkjs, the one delurio's dashboard
//! lays its diagrams out with: the addon gives the graph elkjs is handed (`spineGraph`) and draws what elkjs gave back
//! (`spineSvg`).
//!
//! PLACEHOLDER until the page porter's crates/delurio-page/src/spine.rs is on rust/next: its two functions,
//! `spine_graph` and `spine_svg`, are called behind the `spine` feature, which is off; without it both answer with an
//! error that says so. Once spine.rs lands, the feature (or this cfg) goes and the calls below are checked against its
//! signatures.

use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::graph::NativeGraph;
use crate::model::NativeExploration;

#[cfg(feature = "spine")]
mod real {
    use std::rc::Rc;

    use delurio_core::story::PathStep;

    /// the story's states as the steps spine.ts reads (only `state`)
    pub fn story_of(states: &Option<Vec<String>>) -> Option<Vec<PathStep>> {
        states.as_ref().map(|s| {
            s.iter()
                .map(|state| PathStep { state: Rc::from(state.as_str()), ctx: Box::new([]), event: None, taken: None })
                .collect()
        })
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
    #[cfg(feature = "spine")]
    {
        let story = real::story_of(&story);
        let root = delurio_page::spine::spine_graph(&g.g, &ex.ex, &view, story.as_deref());
        return crate::conv::json(&root);
    }
    #[cfg(not(feature = "spine"))]
    {
        let _ = (g, ex, view, story);
        Err(crate::conv::plain(
            "renderSpine: the diagram is not ported to Rust on this build yet (crates/delurio-page/src/spine.rs)",
        ))
    }
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
    #[cfg(feature = "spine")]
    {
        let story = real::story_of(&story);
        let laid = crate::conv::parse(&laid)?;
        return Ok(delurio_page::spine::spine_svg(&g.g, &ex.ex, &view, story.as_deref(), &laid));
    }
    #[cfg(not(feature = "spine"))]
    {
        let _ = (g, ex, view, story, laid);
        Err(crate::conv::plain(
            "renderSpine: the diagram is not ported to Rust on this build yet (crates/delurio-page/src/spine.rs)",
        ))
    }
}
