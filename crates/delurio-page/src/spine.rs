//! The flow's diagram as an SVG, laid out by elkjs. The port of spine.ts, split in two so that a caller with its own
//! elkjs (the Node bindings) can lay it out there: `spine_graph` is the graph ELK is given, `spine_svg` draws what ELK
//! gave back, and `render_spine` does both with the elkjs of `crate::elk`.

use std::collections::{HashMap, HashSet};

use delurio_core::edges::{EdgeKind, NodeKind, VizEdge, collect_edges, kind_of, node_line, wrap};
use delurio_core::error::Error;
use delurio_core::graph::{Graph, given, title_of};
use delurio_core::js::{self, Object, Value};
use delurio_core::model::Exploration;
use delurio_core::story::PathStep;

/// which of the two diagrams of a page: the main one leaves the failure paths and "the user closes the page" out
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Main,
    Full,
}

impl View {
    pub fn as_str(self) -> &'static str {
        match self {
            View::Main => "main",
            View::Full => "full",
        }
    }
    fn label_wrap(self) -> usize {
        match self {
            View::Main => 26,
            View::Full => 22,
        }
    }
}

const START: &str = "__start";
const CHAR_TITLE: f64 = 8.4; // 14px semibold sans
const CHAR_LINE: f64 = 6.9; // 11.5px monospace
const CHAR_LABEL: f64 = 7.1; // 13px sans

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `Math.round(n * 10) / 10`, written as JavaScript writes the number
fn r1(n: f64) -> String {
    js::number_to_string(js::round(n * 10.0) / 10.0)
}

/// the number `Math.round(n * 10) / 10`, for sums made after the rounding
fn r1n(n: f64) -> f64 {
    js::round(n * 10.0) / 10.0
}

fn num(n: f64) -> String {
    js::number_to_string(n)
}

fn label_lines(e: &VizEdge, view: View) -> Vec<String> {
    e.labels.iter().flat_map(|l| wrap(l, view.label_wrap())).collect()
}

/// the edges a view draws: the main view leaves out the failure branches and "the user closes the page"
fn edges_of(g: &Graph, ex: &Exploration, view: View) -> Vec<VizEdge> {
    let all = collect_edges(g, ex);
    if view == View::Full { all } else { all.into_iter().filter(|e| e.kind != EdgeKind::Fail && !e.leave).collect() }
}

/// the initial state as `g.flow.initial` names it (`load` holds it to be a state)
fn initial_of(g: &Graph) -> String {
    g.flow.get("initial").map_or_else(|| "undefined".to_string(), js::to_js_string)
}

fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Object(Object::from_iter(pairs))
}

fn strs(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|s| Value::from(*s)).collect())
}

/// The graph `renderSpine` hands to `new ELK().layout`, exactly as spine.ts builds it: the start, every state the view
/// draws, and one edge per pair of states, its label holding every transition between them.
pub fn spine_graph(g: &Graph, ex: &Exploration, view: View, _story: Option<&[PathStep]>) -> Value {
    let edges = edges_of(g, ex, view);
    let initial = initial_of(g);
    let mut visible: HashSet<&str> = HashSet::from([initial.as_str()]);
    for e in &edges {
        visible.insert(&e.from);
        visible.insert(&e.to);
    }
    let names: Vec<&str> = g.flow().state_names().into_iter().filter(|n| visible.contains(n)).collect();

    let mut children = vec![obj([("id", START.into()), ("width", 14.0.into()), ("height", 14.0.into())])];
    for name in names {
        let title = js::len16(&title_of(g.flow(), name)) as f64 * CHAR_TITLE;
        let line = js::len16(&node_line(g, name)) as f64 * CHAR_LINE;
        let width = (150f64.max(title).max(line) + 36.0).ceil();
        let mut node = Object::from_iter([("id", Value::from(name)), ("width", width.into()), ("height", 58.0.into())]);
        if kind_of(g, name) == NodeKind::Exit {
            node.insert("layoutOptions", obj([("elk.layered.layering.layerConstraint", "LAST".into())]));
        }
        children.push(Value::Object(node));
    }
    let mut elk_edges =
        vec![obj([("id", "e_start".into()), ("sources", strs(&[START])), ("targets", strs(&[initial.as_str()]))])];
    for (i, e) in edges.iter().enumerate() {
        let lines = label_lines(e, view);
        let labels = if lines.is_empty() {
            Vec::new()
        } else {
            let longest = lines.iter().map(|l| js::len16(l)).max().unwrap_or(0) as f64;
            vec![obj([
                ("text", lines.join("\n").into()),
                ("width", (longest * CHAR_LABEL + 14.0).ceil().into()),
                ("height", ((lines.len() * 17 + 6) as f64).into()),
            ])]
        };
        elk_edges.push(obj([
            ("id", format!("e{i}").into()),
            ("sources", strs(&[e.from.as_str()])),
            ("targets", strs(&[e.to.as_str()])),
            ("labels", Value::Array(labels)),
        ]));
    }
    obj([
        ("id", "root".into()),
        (
            "layoutOptions",
            obj([
                ("elk.algorithm", "layered".into()),
                ("elk.direction", "DOWN".into()),
                ("elk.edgeRouting", "ORTHOGONAL".into()),
                ("elk.padding", "[top=24,left=24,bottom=24,right=24]".into()),
                ("elk.spacing.nodeNode", "36".into()),
                ("elk.spacing.edgeNode", "22".into()),
                ("elk.spacing.edgeEdge", "16".into()),
                ("elk.spacing.edgeLabel", "6".into()),
                ("elk.layered.spacing.nodeNodeBetweenLayers", "56".into()),
                ("elk.layered.spacing.edgeNodeBetweenLayers", "22".into()),
                ("elk.layered.considerModelOrder.strategy", "NODES_AND_EDGES".into()),
            ]),
        ),
        ("children", Value::Array(children)),
        ("edges", Value::Array(elk_edges)),
    ])
}

/// `x ?? d` of a number ELK gave
fn number(v: &Value, key: &str, d: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(d)
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get_array(key).map_or(&[][..], Vec::as_slice)
}

/// the path of an edge's first section: its start, its bends and its end
fn path(e: &Value) -> String {
    let Some(section) = list(e, "sections").first() else { return String::new() };
    let mut pts: Vec<&Value> = Vec::new();
    pts.extend(section.get("startPoint"));
    pts.extend(list(section, "bendPoints"));
    pts.extend(section.get("endPoint"));
    pts.iter()
        .enumerate()
        .map(|(i, p)| {
            format!(
                "{}{} {}",
                if i == 0 { 'M' } else { 'L' },
                r1(number(p, "x", f64::NAN)),
                r1(number(p, "y", f64::NAN))
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The flow's diagram as an SVG, drawn from the graph ELK laid out (`laid_out`: what `layout(spine_graph(..))` gave).
/// Every state's group carries `data-state`, and every edge and its label `data-from` and `data-to`: one edge is drawn
/// per pair of states, its label holding every transition between them. `story`: the states to draw as the story
/// (`on-main`), in order.
pub fn spine_svg(g: &Graph, ex: &Exploration, view: View, story: Option<&[PathStep]>, laid_out: &Value) -> String {
    let edges = edges_of(g, ex, view);
    let v = view.as_str();

    let mut main_edges: HashSet<String> = HashSet::new();
    let mut main_nodes: HashSet<&str> = HashSet::new();
    let story = story.unwrap_or(&[]);
    for (k, s) in story.iter().enumerate() {
        main_nodes.insert(&s.state);
        if k > 0 {
            main_edges.insert(format!("{}>{}", story[k - 1].state, s.state));
        }
    }

    let laid = laid_out;
    let width = number(laid, "width", 800.0).ceil();
    let height = number(laid, "height", 400.0).ceil();
    let (width, height) = (num(width), num(height));
    let title = given(&g.flow, "title").map_or_else(|| g.flow().id_text(), js::to_js_string);

    let mut out: Vec<String> = Vec::new();
    out.push(format!(
        "<svg class=\"spine\" xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" style=\"{}\" role=\"img\" aria-label=\"Flowchart of {}{}. The text sections of this page describe every step.\">",
        if view == View::Full { format!("width:{width}px;max-width:none") } else { format!("max-width:{width}px") },
        esc(&title),
        if view == View::Main { " without failure paths" } else { " with failure paths" },
    ));
    out.push("<defs>".into());
    for kind in ["user", "system", "route", "fail", "main"] {
        out.push(format!(
            "<marker id=\"arrow-{v}-{kind}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"8\" markerHeight=\"8\" orient=\"auto-start-reverse\"><path class=\"arrow arrow-{kind}\" d=\"M0 0 L10 5 L0 10 z\"/></marker>"
        ));
    }
    out.push("</defs>".into());

    let by_id: HashMap<String, &VizEdge> = edges.iter().enumerate().map(|(i, e)| (format!("e{i}"), e)).collect();
    let by_id = |id: Option<&Value>| -> Option<&VizEdge> { by_id.get(id?.as_str()?).copied() };
    let drawn: Vec<&Value> = list(laid, "edges").iter().filter(|e| !list(e, "sections").is_empty()).collect();
    let is_main = |e: Option<&VizEdge>| e.is_some_and(|e| main_edges.contains(&format!("{}>{}", e.from, e.to)));
    // which states an edge joins, so a tool can find the drawing of a transition: one edge per pair of states
    let ends = |e: &VizEdge| format!("data-from=\"{}\" data-to=\"{}\"", esc(&e.from), esc(&e.to));
    for e in &drawn {
        if e.get("id").and_then(Value::as_str) == Some("e_start") {
            out.push(format!(
                "<path class=\"edge edge-system\" d=\"{}\" marker-end=\"url(#arrow-{v}-system)\"/>",
                path(e)
            ));
        }
    }
    for pass in [false, true] {
        for e in &drawn {
            let viz = by_id(e.get("id"));
            let Some(edge) = viz else { continue };
            if is_main(viz) != pass {
                continue;
            }
            let kind = if pass { "main" } else { kind_name(edge) };
            out.push(format!(
                "<path class=\"edge edge-{kind}{}\" {} d=\"{}\" marker-end=\"url(#arrow-{v}-{kind})\"/>",
                if edge.unused { " edge-unused" } else { "" },
                ends(edge),
                path(e)
            ));
        }
    }
    for e in &drawn {
        let viz = by_id(e.get("id"));
        let label = list(e, "labels").first();
        let (Some(edge), Some(label)) = (viz, label) else { continue };
        let lines = label_lines(edge, view);
        let x = number(label, "x", 0.0);
        let y = number(label, "y", 0.0);
        out.push(format!(
            "<g class=\"elabel{}{}\" {}><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\"/>",
            if is_main(viz) { " elabel-main" } else { "" },
            if edge.kind == EdgeKind::Fail { " elabel-fail" } else { "" },
            ends(edge),
            r1(x),
            r1(y),
            num(number(label, "width", 0.0)),
            num(number(label, "height", 0.0)),
        ));
        for (i, l) in lines.iter().enumerate() {
            out.push(format!("<text x=\"{}\" y=\"{}\">{}</text>", r1(x + 7.0), r1(y + 17.0 + i as f64 * 17.0), esc(l)));
        }
        out.push("</g>".into());
    }
    for n in list(laid, "children") {
        let x = r1n(number(n, "x", 0.0));
        let y = r1n(number(n, "y", 0.0));
        let w = number(n, "width", 0.0);
        let h = number(n, "height", 0.0);
        let id = n.get("id").map_or_else(|| "undefined".to_string(), js::to_js_string);
        if id == START {
            out.push(format!(
                "<circle class=\"start\" cx=\"{}\" cy=\"{}\" r=\"7\"><title>start</title></circle>",
                r1(x + 7.0),
                r1(y + 7.0)
            ));
            continue;
        }
        let kind = kind_of(g, &id);
        let Some(st) = g.flow().state(&id) else { continue };
        let s = st.0;
        let exit_class = if kind == NodeKind::Exit {
            let how = if st.leaves_page() {
                "left".to_string()
            } else {
                given(s, "redirect").map_or_else(|| "error".to_string(), js::to_js_string)
            };
            format!(" exit-{how}")
        } else {
            String::new()
        };
        let unreachable = !ex.at_state.contains_key(id.as_str());
        let screen = kind == NodeKind::Screen;
        let open = if screen { format!("<a href=\"#screen-{}\">", esc(&id)) } else { String::new() };
        let close = if screen { "</a>" } else { "" };
        let by_value = s.get("by");
        let by = by_value.map_or_else(String::new, |b| format!(" by-{}", js::to_js_string(b)));
        let by_is = |w: &str| by_value.and_then(Value::as_str) == Some(w);
        let includes = st.includes();
        let mut tip = vec![given(s, "description").map_or_else(|| id.clone(), js::to_js_string)];
        tip.extend(includes.iter().map(|i| format!("+ {i}")));
        out.push(format!(
            "{open}<g class=\"node node-{}{by}{}{exit_class}{}{}\" data-state=\"{}\"><title>{}</title>",
            node_kind_name(kind),
            if st.goal() { " goal" } else { "" },
            if main_nodes.contains(id.as_str()) { " on-main" } else { "" },
            if unreachable { " node-unreachable" } else { "" },
            esc(&id),
            esc(&tip.join("\n")),
        ));
        let rx = if kind == NodeKind::Exit || by_is("ai") {
            28
        } else if by_is("ci") {
            0
        } else {
            8
        };
        out.push(format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{rx}\"/>",
            num(x),
            num(y),
            num(w),
            num(h)
        ));
        if by_is("agent") {
            out.push(format!(
                "<rect class=\"inner\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"5\"/>",
                r1(x + 4.0),
                r1(y + 4.0),
                num(w - 8.0),
                num(h - 8.0)
            ));
        }
        let more = includes_length(s);
        if more > 0 {
            out.push(format!(
                "<text class=\"nmore\" x=\"{}\" y=\"{}\" text-anchor=\"end\">+{more}</text>",
                r1(x + w - 8.0),
                r1(y + 14.0)
            ));
        }
        if kind == NodeKind::Subflow {
            out.push(format!(
                "<rect class=\"inner\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"5\"/>",
                r1(x + 4.0),
                r1(y + 4.0),
                num(w - 8.0),
                num(h - 8.0)
            ));
        }
        out.push(format!(
            "<text class=\"nname\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text>",
            r1(x + w / 2.0),
            r1(y + 25.0),
            esc(&title_of(g.flow(), &id))
        ));
        out.push(format!(
            "<text class=\"nline\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}{}</text>",
            r1(x + w / 2.0),
            r1(y + 44.0),
            esc(&node_line(g, &id)),
            if unreachable { " · unreachable" } else { "" }
        ));
        out.push(format!("</g>{close}"));
    }
    out.push("</svg>".into());
    out.join("\n")
}

/// `(st.includes ?? []).length`: the items of the list
fn includes_length(st: &Value) -> usize {
    st.get_array("includes").map_or(0, Vec::len)
}

fn kind_name(e: &VizEdge) -> &'static str {
    match e.kind {
        EdgeKind::User => "user",
        EdgeKind::System => "system",
        EdgeKind::Route => "route",
        EdgeKind::Fail => "fail",
    }
}

fn node_kind_name(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Server => "server",
        NodeKind::Screen => "screen",
        NodeKind::Subflow => "subflow",
        NodeKind::Exit => "exit",
    }
}

/// `await renderSpine(g, ex, view, story)`: the diagram laid out by the elkjs of this crate
pub fn render_spine(g: &Graph, ex: &Exploration, view: View, story: Option<&[PathStep]>) -> Result<String, Error> {
    let laid = crate::elk::layout(&spine_graph(g, ex, view, story))
        .map_err(|e| Error::graph(format!("the diagram cannot be laid out: {e}"), None))?;
    Ok(spine_svg(g, ex, view, story, &laid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use delurio_core::graph::load;
    use delurio_core::model::{Model, explore};
    use delurio_core::story::{main_story_index, steps_to};

    // the fixtures are the TypeScript checker's: the graph renderSpine handed to `ELK.layout` (caught by wrapping
    // elkjs's `layout` in node), and renderSpine's SVG, for the example flows
    fn spine_of(dir: &str, view: View) -> (Value, String) {
        let g = load(&format!("{}/../../examples/{dir}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let m = Model::new(g.flow.clone());
        let ex = explore(&m, None, None).unwrap();
        let story = main_story_index(&g, &ex).map(|i| steps_to(&m, &ex, i).unwrap());
        (spine_graph(&g, &ex, view, story.as_deref()), render_spine(&g, &ex, view, story.as_deref()).unwrap())
    }

    #[test]
    fn the_graph_elk_is_given() {
        let (main, _) = spine_of("sign-out", View::Main);
        assert_eq!(js::stringify(&main), include_str!("../tests/fixtures/sign-out-elk-main.json"));
        let (full, svg) = spine_of("sign-out", View::Full);
        assert_eq!(js::stringify(&full), include_str!("../tests/fixtures/sign-out-elk-full.json"));
        assert_eq!(svg, include_str!("../tests/fixtures/sign-out-full.svg"));
    }

    #[test]
    fn the_story_drawn_on_a_screen_flow() {
        let (_, svg) = spine_of("notes/edit", View::Main);
        assert_eq!(svg, include_str!("../tests/fixtures/notes-edit-main.svg"));
    }

    #[test]
    fn numbers_as_javascript_rounds_and_writes_them() {
        // node -e "for (const n of [12.25, -0.04, 3.05, 1e21]) console.log(String(Math.round(n * 10) / 10))"
        assert_eq!(r1(12.25), "12.3");
        assert_eq!(r1(-0.04), "0");
        assert_eq!(r1(3.05), "3.1");
        assert_eq!(r1(1e21), "1e+21");
    }
}
