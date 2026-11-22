//! The page of a flow: its story, its diagram, its screens, what it answers and calls, how it decides, every way it
//! ends, the flows it starts and its guarantees, as one HTML file. The port of html.ts; its styles, shared with the
//! index and the navigation pages, are kept in assets/ as the TypeScript module gave them, byte for byte.

use std::collections::HashSet;

use delurio_core::changes::Domain;
use delurio_core::edges::{EdgeKind, NodeKind, collect_edges, exit_line, kind_of, press_label, response_line};
use delurio_core::error::Result;
use delurio_core::expr::{identifiers, parse_expr};
use delurio_core::graph::{Graph, State, Transition, alternatives, given, humanize, title_of};
use delurio_core::js::{self, Object, Value};
use delurio_core::kit::{CssTarget, Kit, kit_css};
use delurio_core::model::{AssertionResult, Exploration, FinalPath, Model};
use delurio_core::starts::HandOver;
use delurio_core::story::{Story, narrate, steps_to};
use delurio_core::wireframe::{TONE_CSS, uses_tone};

use crate::pan::{PAN_CSS, PAN_SCRIPT};

/// the page's styles
pub const CSS: &str = include_str!("../assets/page.css");

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn code(s: &str) -> String {
    format!("<code>{}</code>", esc(s))
}

/// `String(x)` of a key's value as a template literal writes it; a value that is no string is written as JavaScript
/// writes it (the TypeScript checker gave such a value to `esc`, which threw)
fn text(v: &Value) -> String {
    js::to_js_string(v)
}

/// `(x ?? []).length > 0`: a list or a text that is not empty
fn has_items(v: Option<&Value>) -> bool {
    match v {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::String(s)) => !s.is_empty(),
        _ => false,
    }
}

/// the items of a list as texts: `(x ?? []).map(...)`
fn items(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().map(text).collect()).unwrap_or_default()
}

/// what `renderPage` is given
pub struct PageInput<'a> {
    pub g: &'a Graph,
    pub m: &'a Model,
    pub ex: &'a Exploration,
    pub assertions: &'a [AssertionResult],
    pub finals: &'a [FinalPath],
    pub main: Option<&'a Story>,
    pub spine_main: &'a str,
    pub spine_full: &'a str,
    pub screens: &'a str,
    pub kit: Option<&'a Kit>,
    pub hand_overs: &'a [HandOver],
    /// the domain the flow names, when it is found among the graphs beside it
    pub domain: Option<&'a Domain>,
}

// how an action of the domain is called (SPEC.md §5): an HTTP operation others call, a server function only the
// product's own pages call, or a step inside the service. `None`: the domain has no such action; `Some(null)`: it names
// the action and does not write it yet
pub fn call_line(a: Option<&Value>) -> String {
    let Some(a) = a else { return "not in its domain".to_string() };
    if a.is_null() {
        return "named, not written yet".to_string();
    }
    if let Some(route) = a.get("route") {
        return format!("an HTTP operation, {}", text(route));
    }
    if let Some(server_fn) = a.get("serverFn") {
        return format!("a server function, {}, called only by the product's own pages", text(server_fn));
    }
    "a step inside the service".to_string()
}

/// `a.guard`, any value (`a.guard === undefined` asks whether the key is there)
fn guard_of<'a>(a: &Transition<'a>) -> Option<&'a Value> {
    match a {
        Transition::Target(_) => None,
        Transition::Full(v) => v.get("guard"),
    }
}

/// `a.say ?? (a.guard === undefined ? 'nothing above applies' : a.guard)`
fn condition_of(a: &Transition<'_>) -> String {
    if let Transition::Full(v) = a
        && let Some(say) = given(v, "say")
    {
        return text(say);
    }
    guard_of(a).map_or_else(|| "nothing above applies".to_string(), text)
}

fn routing_section(g: &Graph, ex: &Exploration) -> String {
    let flow = g.flow();
    let mut out: Vec<String> = Vec::new();
    for (name, st) in flow.states() {
        // a rule list that is no list (the TS crashed on it) is read as none
        if st.0.get_array("always").is_none() {
            continue;
        }
        let counts = ex.branch_counts.get(name).map_or(&[][..], Vec::as_slice);
        let total = ex.at_state.get(name).copied().unwrap_or(0);
        let rules = st.always();
        out.push(format!("<h3 style=\"margin:18px 0 8px\">{}</h3>", esc(&title_of(flow, name))));
        out.push(
            "<div class=\"scroll\"><table><thead><tr><th>#</th><th>If</th><th>Then</th></tr></thead><tbody>".into(),
        );
        for (i, a) in rules.iter().enumerate() {
            out.push(format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                i + 1,
                esc(&condition_of(a)),
                esc(&title_of(flow, &a.target_text()))
            ));
        }
        out.push("</tbody></table></div>".into());
        out.push("<p class=\"muted\" style=\"margin:8px 0 0\">The first matching rule wins.</p>".into());
        out.push("<details><summary>Exact conditions</summary><div class=\"scroll\" style=\"margin-top:8px\"><table><thead><tr><th>#</th><th>Condition</th><th>Goes to</th><th>Situations</th></tr></thead><tbody>".into());
        for (i, a) in rules.iter().enumerate() {
            let cond =
                guard_of(a).map_or_else(|| "<span class=\"muted\">otherwise</span>".to_string(), |g| code(&text(g)));
            out.push(format!(
                "<tr><td>{}</td><td>{cond}</td><td>{}</td><td class=\"mono\">{} of {total}</td></tr>",
                i + 1,
                code(&a.target_text()),
                counts.get(i).copied().unwrap_or(0)
            ));
        }
        out.push("</tbody></table></div>".into());
        let mut used: HashSet<String> = HashSet::new();
        for a in &rules {
            // a guard that does not parse names nothing here (the TS threw; the exploration reports it first)
            if let Some(guard) = guard_of(a)
                && let Ok(e) = parse_expr(&text(guard))
            {
                used.extend(identifiers(&e));
            }
        }
        let named: Vec<(&str, &Value)> = flow.guards().filter(|(n, _)| used.contains(*n)).collect();
        if !named.is_empty() {
            out.push("<dl class=\"guards\">".into());
            for (n, expr) in named {
                out.push(format!("<dt>{}</dt><dd>{}</dd>", code(n), code(&text(expr))));
            }
            out.push("</dl>".into());
        }
        out.push("</details>".into());
    }
    out.join("\n")
}

fn transitions_table(g: &Graph, ex: &Exploration) -> String {
    let mut rows: Vec<String> = Vec::new();
    for (name, st) in g.flow().states() {
        let mut add = |event: &str, a: &Transition<'_>, taken: bool| {
            let guard = guard_of(a).map_or_else(String::new, |g| code(&text(g)));
            let effects = match a {
                Transition::Full(v) => items(v.get("effect")).iter().map(|e| code(e)).collect::<Vec<_>>().join(" "),
                Transition::Target(_) => String::new(),
            };
            rows.push(format!(
                "<tr><td>{}</td><td>{}</td><td>{guard}</td><td>{}</td><td>{effects}</td><td class=\"{}\">{}</td></tr>",
                code(name),
                code(event),
                code(&a.target_text()),
                if taken { "" } else { "bad" },
                if taken { "yes" } else { "never" }
            ));
        };
        for (i, a) in st.always().iter().enumerate() {
            add(&format!("rule {}", i + 1), a, ex.taken.contains(format!("{name}|always|{i}").as_str()));
        }
        for (event, spec) in st.on() {
            for (i, a) in alternatives(spec).iter().enumerate() {
                add(event, a, ex.taken.contains(format!("{name}|{event}|{i}").as_str()));
            }
        }
    }
    format!(
        "<div class=\"scroll\" style=\"margin-top:8px\"><table><thead><tr><th>From</th><th>Event</th><th>Guard</th><th>To</th><th>Effects</th><th>Taken</th></tr></thead><tbody>{}</tbody></table></div>",
        rows.join("\n")
    )
}

fn swatch_node(cls: &str, rx: u32, inner: bool) -> String {
    format!(
        "<svg width=\"34\" height=\"20\" viewBox=\"0 0 34 20\" aria-hidden=\"true\" class=\"sw-node sw-{cls}\"><rect x=\"1.5\" y=\"1.5\" width=\"31\" height=\"17\" rx=\"{rx}\"/>{}</svg>",
        if inner { "<rect class=\"inner\" x=\"4.5\" y=\"4.5\" width=\"25\" height=\"11\" rx=\"2\"/>" } else { "" }
    )
}

fn swatch_line(cls: &str) -> String {
    format!(
        "<svg width=\"34\" height=\"20\" viewBox=\"0 0 34 20\" aria-hidden=\"true\" class=\"sw-line sw-{cls}\"><path d=\"M2 10 L32 10\"/></svg>"
    )
}

fn legend(g: &Graph, ex: &Exploration, has_main: bool) -> String {
    let states: Vec<(&str, State<'_>)> = g.flow().states().collect();
    let edges = collect_edges(g, ex);
    let mut items: Vec<String> = Vec::new();
    let mut add = |sw: String, text: &str| {
        items.push(format!("<span class=\"sw\">{sw}<span>{}</span></span>", esc(text)));
    };
    if has_main {
        add(swatch_line("main"), "blue line: the main story");
    }
    if edges.iter().any(|e| e.kind == EdgeKind::Fail) {
        add(swatch_line("fail"), "red dashed line: a failure path (shown with “Show failure paths”)");
    }
    if edges.iter().any(|e| e.kind == EdgeKind::Route) {
        add(swatch_line("route"), "dotted line: decided by a rule, nobody acts");
    }
    let by = |w: &str| states.iter().any(|(_, s)| s.0.get("by").and_then(Value::as_str) == Some(w));
    if by("human") {
        add(swatch_node("human", 4, false), "a person does it (editor, review, approval)");
    }
    if by("ai") {
        add(swatch_node("ai", 9, false), "AI does it");
    }
    if by("agent") {
        add(swatch_node("agent", 4, true), "an agent does it, on the real app or for a user");
    }
    if by("ci") {
        add(swatch_node("ci", 0, false), "CI does it, automatically");
    }
    if states.iter().any(|(_, s)| s.goal()) {
        add(swatch_node("goal", 4, false), "green: the goal the flow keeps returning to");
    }
    let kinds: Vec<NodeKind> =
        states.iter().filter(|(_, s)| s.0.get("by").is_none() && !s.goal()).map(|(n, _)| kind_of(g, n)).collect();
    if kinds.contains(&NodeKind::Server) {
        add(swatch_node("server", 4, false), "dashed grey box: the server decides, nothing is shown");
    }
    if kinds.contains(&NodeKind::Screen) {
        add(swatch_node("screen", 4, false), "solid box: the user sees a screen (click it)");
    }
    if kinds.contains(&NodeKind::Subflow) {
        add(swatch_node("subflow", 4, true), "double border: another flow");
    }
    let exits = |how: &str| {
        states.iter().any(|(_, s)| s.is_final() && s.0.get("redirect").and_then(Value::as_str) == Some(how))
    };
    if exits("success") {
        add(swatch_node("goal", 9, false), "green exit: success");
    }
    if exits("never") {
        add(swatch_node("fail", 9, false), "red exit: no redirect");
    }
    if states.iter().any(|(_, s)| has_items(s.0.get("includes"))) {
        add("<b class=\"muted\">+n</b>".to_string(), "the step covers n more things (listed below)");
    }
    format!("<p class=\"legend\">{}</p>", items.join(""))
}

/// `st.invoke?.action`
fn action_of<'a>(st: &State<'a>) -> Option<&'a Value> {
    st.invoke().and_then(|i| i.get("action"))
}

/// `Object.values(g.flow.states)`: every transition, its `always` rules and the alternatives of its events
fn transition_count(g: &Graph) -> usize {
    g.flow()
        .states()
        .map(|(_, s)| {
            s.0.get_array("always").map_or(0, Vec::len)
                + s.on().iter().map(|(_, spec)| alternatives(spec).len()).sum::<usize>()
        })
        .sum()
}

/// the page of a flow: one HTML file; it fails only where telling a way it ends fails (the exploration reached it, so
/// it does not)
pub fn render_page(p: &PageInput<'_>) -> Result<String> {
    let (g, m, ex, assertions, finals, main) = (p.g, p.m, p.ex, p.assertions, p.finals, p.main);
    let flow = g.flow();
    let kit = p.kit;
    let holds = assertions.iter().filter(|a| a.violation.is_none()).count();
    let heading = given(&g.flow, "title").map_or_else(|| humanize(&flow.id_text()), text);
    let mut out: Vec<String> = Vec::new();
    out.push("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into());
    out.push(format!(
        "<title>{}</title><style>{CSS}{PAN_CSS}{}{}</style></head><body><main>",
        esc(&heading),
        kit.map_or_else(String::new, |k| kit_css(k, CssTarget::Page)),
        if uses_tone(g) { TONE_CSS } else { "" }
    ));
    let status =
        g.flow.get("status").map_or_else(String::new, |s| format!("<span class=\"tag\">{}</span> ", esc(&text(s))));
    out.push(format!("<p class=\"eyebrow\">{status}<span class=\"mono\">{}</span></p>", esc(&flow.id_text())));
    out.push(format!("<h1>{}</h1>", esc(&heading)));
    if let Some(summary) = flow.summary() {
        out.push(format!("<p class=\"lede\">{}</p>", esc(js::trim(summary))));
    }
    if let Some(sc) = flow.scope() {
        out.push(
            "<div class=\"story\" style=\"border-left-color:var(--muted)\"><p class=\"label\">What this view shows</p>"
                .into(),
        );
        if let Some(shows) = sc.get("shows") {
            out.push(format!("<p style=\"margin:0\">{}.</p>", esc(&text(shows))));
        }
        if sc.get("world").and_then(Value::as_str) == Some("open") {
            out.push("<p class=\"muted\" style=\"margin:6px 0 0\">Open world: what is not drawn here is not ruled out. Only the guarantees below are binding.</p>".into());
        }
        let left_out = items(given(sc, "outOfView"));
        if !left_out.is_empty() {
            out.push(format!(
                "<p style=\"margin:8px 0 0\"><b>Left out of this view:</b></p><ul style=\"margin:4px 0 0\">{}</ul>",
                left_out.iter().map(|o| format!("<li>{}</li>", esc(o))).collect::<String>()
            ));
        }
        out.push("</div>".into());
    }

    if let Some(main) = main {
        out.push("<section class=\"story\"><h2>The main story</h2>".into());
        out.push(format!("<p class=\"given\"><b>When:</b> {}.</p><ol>", esc(&main.given)));
        for s in &main.steps {
            out.push(format!("<li>{}</li>", esc(s)));
        }
        out.push(format!("</ol><p class=\"result\">Result: {}.</p></section>", esc(&main.result)));
    }

    out.push("<h2>The flow</h2>".into());
    out.push("<div class=\"views pz-host\"><input type=\"checkbox\" id=\"show-all\"><label for=\"show-all\">Show failure paths and “the user closes the page”</label>".into());
    out.push(format!(
        "<div class=\"scroll panel-main pz\">{}</div><div class=\"scroll panel-full pz\">{}</div></div>",
        p.spine_main, p.spine_full
    ));
    out.push(legend(g, ex, main.is_some()));
    let covers: Vec<(&str, State<'_>)> = flow.states().filter(|(_, s)| has_items(s.0.get("includes"))).collect();
    if !covers.is_empty() {
        out.push("<h2>What each step covers</h2><div class=\"covers\">".into());
        for (n, s) in covers {
            out.push(format!(
                "<div class=\"cv\"><b>{}</b><ul>{}</ul></div>",
                esc(&title_of(flow, n)),
                items(s.0.get("includes")).iter().map(|i| format!("<li>{}</li>", esc(i))).collect::<String>()
            ));
        }
        out.push("</div>".into());
    }

    if !js::trim(p.screens).is_empty() {
        out.push("<h2>What the user sees</h2>".into());
        match kit {
            None => out.push("<p class=\"muted\" style=\"margin-top:-4px\">Wireframes are generated from the UI graph with fixed components, so they show structure, wording and state, not the final look.</p>".into()),
            Some(kit) => out.push(format!(
                "<p class=\"muted\" style=\"margin-top:-4px\">Screens are drawn from the UI graph with the components of {} (kit <span class=\"mono\">{}</span>): the look is the kit's, the wording and state are the graph's. A component the kit does not cover is drawn with the fixed one.</p>",
                esc(&kit.title),
                esc(&kit.id)
            )),
        }
        out.push(format!("<div class=\"screens\">{}</div>", p.screens));
    }

    let answers: Vec<(&str, State<'_>)> = flow.states().filter(|(_, st)| st.0.get("response").is_some()).collect();
    if !answers.is_empty() {
        out.push("<h2>What each step answers</h2>".into());
        out.push(
            "<div class=\"scroll\"><table><thead><tr><th>Step</th><th>The outside world sees</th></tr></thead><tbody>"
                .into(),
        );
        let none = Object::new();
        for (name, st) in answers {
            out.push(format!(
                "<tr><td>{}<br><span class=\"mono muted\">{}</span></td><td>{}</td></tr>",
                esc(&title_of(flow, name)),
                esc(name),
                esc(&response_line(st.response().unwrap_or(&none)))
            ));
        }
        out.push("</tbody></table></div>".into());
    }

    // `p.domain?.actions ?? null`: a domain with actions, read as a map (a list or a text has none of its names)
    let actions = p.domain.and_then(|d| d.0.get("actions")).filter(|a| !a.is_null());
    let calls: Vec<(&str, State<'_>)> = flow.states().filter(|(_, st)| action_of(st).is_some()).collect();
    if let Some(actions) = actions
        && !calls.is_empty()
    {
        out.push("<h2>What each step calls</h2>".into());
        out.push(format!(
            "<p class=\"muted\" style=\"margin-top:-4px\">The actions of its domain, <span class=\"mono\">{}</span>, and how each is called.</p>",
            esc(&given(&g.flow, "domain").map_or_else(String::new, text))
        ));
        out.push("<div class=\"scroll\"><table><thead><tr><th>Step</th><th>Action</th><th>How it is called</th></tr></thead><tbody>".into());
        for (name, st) in calls {
            let action = action_of(&st).filter(|a| !a.is_null()).map_or_else(String::new, text);
            let a = actions.as_object().and_then(|o| o.get(&action));
            out.push(format!(
                "<tr><td>{}<br><span class=\"mono muted\">{}</span></td><td>{}</td><td>{}</td></tr>",
                esc(&title_of(flow, name)),
                esc(name),
                code(&action),
                esc(&call_line(a))
            ));
        }
        out.push("</tbody></table></div>".into());
    }

    let routing = routing_section(g, ex);
    if !routing.is_empty() {
        out.push("<h2>How it decides what happens next</h2>".into());
        out.push(routing);
    }

    if finals.is_empty() {
        out.push("<h2>Every way it can end</h2><p class=\"muted\">It never ends: every path returns to a state the flow keeps.</p>".into());
    } else {
        out.push("<h2>Every way it can end</h2>".into());
        out.push("<div class=\"scroll\"><table><thead><tr><th>It ends with</th><th>When</th><th>What happens</th></tr></thead><tbody>".into());
        for f in finals {
            let story = narrate(g, m, &steps_to(m, ex, f.index)?);
            let exit = flow.state(&f.state).map_or_else(String::new, exit_line);
            out.push(format!(
                "<tr><td><b>{}</b><br><span class=\"muted\">{}</span></td><td>{}</td><td><ol>{}</ol></td></tr>",
                esc(&story.result),
                esc(&exit),
                esc(&story.given),
                story.steps.iter().map(|s| format!("<li>{}</li>", esc(s))).collect::<String>()
            ));
        }
        out.push("</tbody></table></div>".into());
        out.push("<p class=\"muted\" style=\"margin:8px 0 0\">Each row is also a generated test: set the starting situation, send the events, expect the ending.</p>".into());
    }

    if !p.hand_overs.is_empty() {
        out.push("<h2>The flows it starts</h2>".into());
        out.push("<p class=\"muted\" style=\"margin-top:-4px\">This flow starts them and does not wait. Each begins with what this flow knows at that moment.</p>".into());
        out.push("<div class=\"scroll\"><table><thead><tr><th>From</th><th>It starts</th><th>Which begins with</th><th>Checked</th></tr></thead><tbody>".into());
        for h in p.hand_overs {
            let way = match &h.event {
                None => "a rule".to_string(),
                Some(event) => press_label(g, &h.from, event).unwrap_or_else(|| humanize(event).to_lowercase()),
            };
            let given_cells: Vec<String> = h
                .given
                .as_object()
                .into_iter()
                .flat_map(|o| o.iter())
                .map(|(k, v)| format!("{} = {}", code(k), code(&text(v))))
                .collect();
            let given_cell = if given_cells.is_empty() {
                "<span class=\"muted\">its own beginning</span>".to_string()
            } else {
                given_cells.join("<br>")
            };
            let checked = if !h.found {
                "<span class=\"muted\">not written yet</span>".to_string()
            } else if h.problems.is_empty() {
                "fits".to_string()
            } else {
                format!(
                    "<span class=\"bad\">{}</span>",
                    h.problems.iter().map(|p| esc(p)).collect::<Vec<_>>().join("<br>")
                )
            };
            out.push(format!(
                "<tr><td>{}<br><span class=\"muted\">{}</span></td><td class=\"mono\">{}</td><td>{given_cell}</td><td>{checked}</td></tr>",
                esc(&title_of(flow, &h.from)),
                esc(&way),
                esc(&h.flow)
            ));
        }
        out.push("</tbody></table></div>".into());
    }

    out.push("<h2>What is guaranteed</h2>".into());
    out.push(format!(
        "<p>These hold in all {} situations the model can reach, from {} different starting situations{}.</p>",
        ex.len(),
        ex.starts,
        if holds == assertions.len() {
            String::new()
        } else {
            format!(" — <span class=\"bad\">{} do not</span>", assertions.len() - holds)
        }
    ));
    out.push("<ul class=\"checks\">".into());
    for a in assertions {
        let said = esc(&a.say.clone().unwrap_or_else(|| humanize(&a.id)));
        out.push(match &a.violation {
            None => format!("<li>{said}</li>"),
            Some(v) => {
                let events = v.events.join(" > ");
                format!(
                    "<li class=\"fail\"><b>{said}</b> <span class=\"bad\">— violated</span> in <code>{}</code> after <span class=\"mono\">{}</span></li>",
                    esc(&v.state),
                    esc(if events.is_empty() { "(start)" } else { &events })
                )
            }
        });
    }
    out.push("</ul>".into());
    out.push("<details><summary>Exact rules</summary><div class=\"scroll\" style=\"margin-top:8px\"><table><thead><tr><th>Id</th><th>Never</th></tr></thead><tbody>".into());
    for a in assertions {
        out.push(format!("<tr><td>{}</td><td>{}</td></tr>", code(&a.id), code(&a.never)));
    }
    out.push("</tbody></table></div></details>".into());

    if let Some(decisions) = flow.decisions() {
        out.push("<h2>Still to decide</h2>".into());
        out.push("<div class=\"scroll\"><table><thead><tr><th>Id</th><th>Question</th><th>Recommendation</th><th>Affects</th></tr></thead><tbody>".into());
        for (id, d) in decisions.iter() {
            let field = |k: &str| d.get(k).map_or_else(|| "undefined".to_string(), text);
            out.push(format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(id),
                esc(&field("question")),
                esc(&field("recommend")),
                items(d.get("affects")).iter().map(|s| esc(&title_of(flow, s))).collect::<Vec<_>>().join(", ")
            ));
        }
        out.push("</tbody></table></div>".into());
    }

    out.push("<h2>Reference</h2>".into());
    out.push(format!(
        "<details><summary>All transitions ({})</summary>{}</details>",
        transition_count(g),
        transitions_table(g, ex)
    ));
    let from = if g.ui().absent() {
        "flow.yaml (it has no screens)".to_string()
    } else {
        format!(
            "flow.yaml, ui.yaml{}",
            kit.map_or_else(|| " and the catalog".to_string(), |k| format!(", the catalog and the kit {}", esc(&k.id)))
        )
    };
    out.push(format!(
        "<footer>delurio graph-viz · generated from {from} · no AI, no network: the same input gives the same bytes.</footer>"
    ));
    out.push(format!("</main>{PAN_SCRIPT}</body></html>"));
    Ok(out.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_an_action_is_called() {
        // callLine of html.ts on the same values
        assert_eq!(call_line(None), "not in its domain");
        assert_eq!(call_line(Some(&Value::Null)), "named, not written yet");
        let route = Value::Object(Object::from_iter([("route", Value::from("POST /x"))]));
        assert_eq!(call_line(Some(&route)), "an HTTP operation, POST /x");
        let server = Value::Object(Object::from_iter([("serverFn", Value::from("GET save"))]));
        assert_eq!(call_line(Some(&server)), "a server function, GET save, called only by the product's own pages");
        assert_eq!(call_line(Some(&Value::Object(Object::new()))), "a step inside the service");
    }
}
