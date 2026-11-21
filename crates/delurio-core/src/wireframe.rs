//! The screens of a flow drawn as wireframes, with the accessibility tree each is expected to give: for the page, for
//! the tutorial stage and for `kit check`. The port of wireframe.ts; the HTML is the TypeScript checker's, byte for
//! byte, and the warnings come in the same order.

use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde::Serialize;

use crate::graph::{self, Graph, Label, UiNode, shown_in, title_of};
use crate::js::{self, Value};
use crate::kit::{Fill, Kit, draw_with, esc, mark};

/// where a drawing reports what it could not draw as the graph says (a missing catalog key, a component no kit draws)
type Warn<'w> = &'w mut dyn FnMut(&str);

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{((?-u:\w)+)\}").expect("the pattern compiles"));

/// `String(g.ui.samples[p])` of a name the samples do not give but every JavaScript object has from
/// `Object.prototype`: the TypeScript checker found it there and wrote it in
pub(crate) fn inherited_sample(p: &str) -> Option<String> {
    const FUNCTIONS: [&str; 11] = [
        "constructor",
        "__defineGetter__",
        "__defineSetter__",
        "hasOwnProperty",
        "__lookupGetter__",
        "__lookupSetter__",
        "isPrototypeOf",
        "propertyIsEnumerable",
        "toString",
        "valueOf",
        "toLocaleString",
    ];
    match p {
        "__proto__" => Some("[object Object]".to_string()),
        "constructor" => Some("function Object() { [native code] }".to_string()),
        _ if FUNCTIONS.contains(&p) => Some(format!("function {p}() {{ [native code] }}")),
        _ => None,
    }
}

fn fill(g: &Graph, key: &str, warn: Warn) -> String {
    // a message that is no string is read as missing (the TypeScript checker could not fill it)
    let Some(raw) = g.message(key) else {
        warn(&format!("catalog key {key} is missing"));
        return format!("⟨missing {key}⟩");
    };
    PLACEHOLDER
        .replace_all(raw, |m: &Captures| {
            let p = &m[1];
            match g.ui().sample(p).map(js::to_js_string).or_else(|| inherited_sample(p)) {
                None => {
                    warn(&format!("no sample value for {{{p}}} in {key}"));
                    format!("⟨{p}⟩")
                }
                Some(sample) => sample,
            }
        })
        .into_owned()
}

/// a key of a node's words as `g.catalog[k]` and `g.ui.nodes[k]` look it up, when it is there (`t.key !== undefined`):
/// any value, written as a string
fn key_of(l: Label<'_>, key: &str) -> Option<String> {
    l.value().get(key).map(js::to_js_string)
}

/// `t.oneOf`, each key written as a string; a value that is no list (which the TypeScript checker could not map) is
/// passed over
fn one_of(l: Label<'_>) -> Option<Vec<String>> {
    l.value().get_array("oneOf").map(|items| items.iter().map(js::to_js_string).collect())
}

fn texts(g: &Graph, n: UiNode<'_>, warn: Warn) -> Vec<String> {
    let Some(t) = n.text() else { return Vec::new() };
    if let Some(key) = key_of(t, "key") {
        return vec![fill(g, &key, warn)];
    }
    if let Some(keys) = one_of(t) {
        return keys.iter().map(|k| fill(g, k, warn)).collect();
    }
    if let Some(literal) = t.literal() {
        return vec![literal.to_string()];
    }
    Vec::new()
}

fn accessible_name(g: &Graph, n: UiNode<'_>, warn: Warn) -> Option<String> {
    let nm = n.name();
    if let Some(key) = nm.and_then(|l| key_of(l, "key")) {
        return Some(fill(g, &key, warn));
    }
    if let Some(literal) = nm.and_then(|l| l.literal()) {
        return Some(literal.to_string());
    }
    if let Some(by) = nm.and_then(|l| key_of(l, "labelledBy")) {
        return g.ui().node(&by).map(|r| texts(g, r, warn).join(" "));
    }
    if matches!(n.role(), Some("button" | "heading" | "status" | "alert")) {
        return texts(g, n, warn).into_iter().next();
    }
    // a checkbox, a radio and a select are named by their label, the words beside them (SPEC.md §4)
    if LABELLED.contains(&n.component().unwrap_or("")) {
        return texts(g, n, warn).into_iter().next();
    }
    None
}

const LABELLED: [&str; 3] = ["Checkbox", "Radio", "Select"];

/// the words a field holds when it is shown (`value`), for a select the option it shows chosen
fn value_words(g: &Graph, n: UiNode<'_>, warn: Warn) -> String {
    let Some(v) = n.value_label() else { return String::new() };
    if let Some(literal) = v.literal() {
        return literal.to_string();
    }
    // `v.key ?? v.oneOf?.[0]`: a list's first item, a string's first character, an object's key 0
    let first = |o: &Value| match o {
        Value::Array(items) => items.first().cloned(),
        Value::String(s) => s.chars().next().map(|c| Value::String(c.to_string())),
        Value::Object(o) => o.get("0").cloned(),
        _ => None,
    };
    let key = graph::given(v.value(), "key").cloned().or_else(|| v.value().get("oneOf").and_then(first));
    match key {
        None => String::new(),
        Some(k) => fill(g, &js::to_js_string(&k), warn),
    }
}

/// one node drawn in one state
#[derive(Clone, Copy)]
struct Item<'a> {
    id: &'a str,
    node: UiNode<'a>,
    disabled: bool,
    focus: bool,
}

/// a key of a node as a template literal writes it, when the key is there (`n.k !== undefined`)
fn shown_key(n: UiNode<'_>, key: &str) -> Option<String> {
    n.get(key).map(js::to_js_string)
}

fn chips(it: &Item<'_>) -> String {
    let n = it.node;
    // `[n.role ?? n.type]`, which `join` writes as a string (null and undefined as nothing)
    let v = n.value();
    let mut parts: Vec<String> =
        vec![graph::given(v, "role").or(graph::given(v, "type")).map_or_else(String::new, js::to_js_string)];
    if let Some(e) = shown_key(n, "emits") {
        parts.push(format!("emits {e}"));
    }
    if it.disabled {
        parts.push("disabled".into());
    } else if let Some(d) = shown_key(n, "disabledWhen") {
        parts.push(format!("disabled when {d}"));
    }
    if let Some(w) = shown_key(n, "when") {
        parts.push(format!("when {w}"));
    }
    if it.focus {
        parts.push("first focus".into());
    }
    format!("<span class=\"chip\">{}</span>", esc(&parts.join(" · ")))
}

/// a List drawn with the kit, waiting for its items
struct KitList<'a> {
    it: Item<'a>,
    items: Vec<String>,
}

/// What a wireframe is drawn with, and what it has drawn so far.
struct Wireframe<'a, 'k> {
    g: &'a Graph,
    clean: bool,
    kit: Option<&'k Kit>,
    out: Vec<String>,
    in_list: bool,
    in_row: bool,
    kit_list: Option<KitList<'a>>,
}

impl<'a> Wireframe<'a, '_> {
    // clean: no annotation chips and no dashed or focus outlines, for the tutorial stage
    fn item(&self, it: &Item<'_>, inner: &str) -> String {
        let cls = if self.clean {
            "wf-item".to_string()
        } else {
            let mut c = vec!["wf-item"];
            if it.node.get("when").is_some() {
                c.push("wf-cond");
            }
            if it.focus {
                c.push("wf-focus");
            }
            c.join(" ")
        };
        let chips = if self.clean { String::new() } else { chips(it) };
        format!("<div class=\"{cls}\" data-node=\"{}\">{inner}{chips}</div>", esc(it.id))
    }

    fn close_list(&mut self, warn: Warn) {
        if let Some(list) = self.kit_list.take() {
            let drawn = self.draw_kit_list(&list.it, &list.items, warn);
            self.out.push(drawn);
        } else if self.in_list {
            self.out.push("</ul>".into());
        }
        self.in_list = false;
    }

    fn fill(&self, it: &Item<'_>, text: &str, warn: Warn) -> Fill {
        let n = it.node;
        let name = accessible_name(self.g, n, warn);
        // the role the expected tree gives the node, which picks a template from the component's `roles`:
        // `n.role ?? (n.type === 'text' ? undefined : n.type)`, as `c.roles[f.role]` looks it up
        let v = n.value();
        let role = graph::given(v, "role").or_else(|| v.get("type").filter(|t| t.as_str() != Some("text")));
        Fill {
            id: it.id.to_string(),
            text: text.to_string(),
            named: name.as_deref().is_some_and(|nm| nm != text),
            name: name.unwrap_or_else(|| text.to_string()),
            intent: n.intent().map(str::to_string),
            role: role.map(js::to_js_string),
            children: None,
            disabled: it.disabled,
            conditional: n.get("when").is_some(),
            checked: n.checked() == Some(&Value::Bool(true)),
        }
    }

    fn draw_kit_list(&self, it: &Item<'_>, drawn: &[String], warn: Warn) -> String {
        let first = texts(self.g, it.node, warn).into_iter().next().unwrap_or_default();
        let mut f = self.fill(it, &first, warn);
        f.children = Some(drawn.iter().map(|d| format!("\n{d}")).collect::<String>() + "\n");
        let kit = self.kit.map(|k| draw_with(k, "List", &f, warn));
        mark(&kit.flatten().unwrap_or_default(), it.id, "")
    }

    fn close_row(&mut self) {
        if self.in_row {
            self.out.push("</div>".into());
        }
        self.in_row = false;
    }

    /// the list a list item goes into: the kit's List that waits for its items, or the wireframe itself
    fn push_item(&mut self, html: String) {
        match &mut self.kit_list {
            Some(list) => list.items.push(html),
            None => self.out.push(html),
        }
    }
}

fn render_wireframe<'a>(
    g: &'a Graph,
    state: &str,
    items: &[Item<'a>],
    warn: Warn,
    clean: bool,
    kit: Option<&Kit>,
) -> String {
    let mut w = Wireframe { g, clean, kit, out: Vec::new(), in_list: false, in_row: false, kit_list: None };
    for it in items {
        let n = it.node;
        let t = texts(g, n, warn);
        let text = esc(t.first().map_or("", String::as_str));
        let component = n.component();
        if component != Some("ListItem") {
            w.close_list(warn);
        }
        if component != Some("Button") {
            w.close_row();
        }
        // `n.component !== undefined && n.component !== 'Page'`: any value but the word Page (a `component:` left empty
        // is null), looked up as `kit.components[n.component]` looks it up
        let kit_key = n.get("component").filter(|v| v.as_str() != Some("Page")).map(js::to_js_string);
        if let Some(kit) = kit
            && let Some(c) = kit_key.as_deref()
        {
            if kit.components.contains_key(c) {
                if component == Some("List") {
                    w.kit_list = Some(KitList { it: *it, items: Vec::new() });
                    w.in_list = true;
                } else if component == Some("ListItem") {
                    if !w.in_list {
                        warn(&format!("{}: ListItem outside a List in {state}", it.id));
                    }
                    for line in &t {
                        let f = w.fill(it, line, warn);
                        let drawn = draw_with(kit, "ListItem", &f, warn).unwrap_or_default();
                        let chips = if clean { String::new() } else { chips(it) };
                        w.push_item(mark(&drawn, it.id, &chips));
                    }
                } else {
                    if component == Some("Button") && !w.in_row {
                        w.out.push("<div class=\"wf-row\">".into());
                    }
                    if component == Some("Button") {
                        w.in_row = true;
                    }
                    let f = w.fill(it, t.first().map_or("", String::as_str), warn);
                    let drawn = draw_with(kit, c, &f, warn).unwrap_or_default();
                    let html = w.item(it, &drawn);
                    w.out.push(html);
                }
                continue;
            }
            warn(&format!("kit {} has no template for {c} (node {}); drawn with the fixed component", kit.id, it.id));
        }
        let disabled = if it.disabled { " disabled" } else { "" };
        match component {
            Some("Page") => {}
            Some("Heading") => {
                let html = w.item(it, &format!("<h4 class=\"wf-h\">{text}</h4>"));
                w.out.push(html);
            }
            Some("Text") => {
                let html = w.item(it, &format!("<p class=\"wf-t\">{text}</p>"));
                w.out.push(html);
            }
            Some("Button") => {
                if !w.in_row {
                    w.out.push("<div class=\"wf-row\">".into());
                }
                w.in_row = true;
                let intent = esc(n.intent().unwrap_or("secondary"));
                let html = w.item(
                    it,
                    &format!("<button type=\"button\" class=\"wf-btn wf-{intent}\"{disabled}>{text}</button>"),
                );
                w.out.push(html);
            }
            Some("LinkButton") => {
                let html = w.item(it, &format!("<button type=\"button\" class=\"wf-link\"{disabled}>{text}</button>"));
                w.out.push(html);
            }
            Some("TextField") => {
                // the visible text is the label; the input is named by it, as a <label> around it does
                let html = w.item(
                    it,
                    &format!(
                        "<label class=\"wf-field\"><span>{text}</span><input type=\"text\" class=\"wf-input\" tabindex=\"-1\"{disabled}></label>"
                    ),
                );
                w.out.push(html);
            }
            Some(c @ ("Checkbox" | "Radio")) => {
                // the box beside its label, as a <label> around an <input> draws it
                let kind = if c == "Radio" { "radio" } else { "checkbox" };
                let checked = if n.checked() == Some(&Value::Bool(true)) { " checked" } else { "" };
                let html = w.item(
                    it,
                    &format!(
                        "<label class=\"wf-check\"><input type=\"{kind}\" tabindex=\"-1\"{checked}{disabled}><span>{text}</span></label>"
                    ),
                );
                w.out.push(html);
            }
            Some("Select") => {
                let value = esc(&value_words(g, n, warn));
                let html = w.item(
                    it,
                    &format!(
                        "<label class=\"wf-field\"><span>{text}</span><select class=\"wf-input\" tabindex=\"-1\"{disabled}><option>{value}</option></select></label>"
                    ),
                );
                w.out.push(html);
            }
            Some("List") => {
                w.out.push(format!("<ul class=\"wf-list\" data-node=\"{}\">", esc(it.id)));
                w.in_list = true;
            }
            Some("ListItem") => {
                if !w.in_list {
                    warn(&format!("{}: ListItem outside a List in {state}", it.id));
                }
                for line in &t {
                    let chips = if clean { String::new() } else { chips(it) };
                    w.push_item(format!("<li data-node=\"{}\">{}{chips}</li>", esc(it.id), esc(line)));
                }
            }
            Some("Status") => {
                let html = w.item(it, &format!("<div class=\"wf-status\" role=\"status\">{text}</div>"));
                w.out.push(html);
            }
            Some("Alert") => {
                let html = w.item(it, &format!("<div class=\"wf-alert\" role=\"alert\">{text}</div>"));
                w.out.push(html);
            }
            _ => {
                warn(&format!("{}: component {} has no wireframe renderer", it.id, component.unwrap_or("(none)")));
                let what = esc(component.or(n.kind()).unwrap_or("undefined"));
                let html = w.item(it, &format!("<div class=\"wf-unmapped\">{what} {}</div>", esc(it.id)));
                w.out.push(html);
            }
        }
    }
    w.close_list(warn);
    w.close_row();
    w.out.join("\n")
}

/// the role the expected tree gives a node: its role, else its type (`text` for a text node), written as a string
/// (`n.role ?? (n.type === 'text' ? 'text' : n.type)`; None when neither is there), and whether it is the role text
fn tree_role(n: UiNode<'_>) -> (Option<String>, bool) {
    let v = n.value();
    let role = graph::given(v, "role").or_else(|| v.get("type"));
    (role.map(js::to_js_string), role.and_then(Value::as_str) == Some("text"))
}

fn render_tree(g: &Graph, items: &[Item<'_>], warn: Warn) -> String {
    let mut lines: Vec<String> = Vec::new();
    for it in items {
        let n = it.node;
        let depth = match n.component() {
            Some("Page") => 0,
            Some("ListItem") => 2,
            _ => 1,
        };
        let disabled = if it.disabled {
            "disabled".to_string()
        } else {
            shown_key(n, "disabledWhen").map_or_else(String::new, |d| format!("disabled when {d}"))
        };
        let flags = [
            disabled,
            shown_key(n, "when").map_or_else(String::new, |w| format!("when {w}")),
            if it.focus { "first focus".to_string() } else { String::new() },
            shown_key(n, "emits").map_or_else(String::new, |e| format!("→ {e}")),
        ]
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
        let suffix = if flags.is_empty() { String::new() } else { format!("  [{flags}]") };
        let pad = "  ".repeat(depth);
        if n.component() == Some("ListItem") {
            for line in texts(g, n, warn) {
                lines.push(format!("{pad}listitem \"{line}\"{suffix}"));
            }
            continue;
        }
        let (role, is_text) = tree_role(n);
        let name = if is_text { texts(g, n, warn).into_iter().next() } else { accessible_name(g, n, warn) };
        let named = name.map_or_else(String::new, |nm| format!(" \"{nm}\""));
        lines.push(format!("{pad}{}{named}{suffix}", role.as_deref().unwrap_or("undefined")));
    }
    lines.join("\n")
}

/// the nodes a state shows, in the order of ui.yaml
fn shown<'a>(g: &'a Graph, state: &str) -> Vec<(&'a str, UiNode<'a>)> {
    g.ui().nodes().filter(|(_, n)| shown_in(*n, state)).collect()
}

/// `(node.disabledIn ?? []).includes(state)`
fn disabled_in(n: UiNode<'_>, state: &str) -> bool {
    n.disabled_in().contains(&state)
}

/// The screens of a flow for its page: each state that shows a node, with its wireframe and its expected accessibility
/// tree.
pub fn render_screens(g: &Graph, warn: &mut dyn FnMut(&str), kit: Option<&Kit>) -> String {
    let mut out: Vec<String> = Vec::new();
    for (state, st) in g.flow().states() {
        let shown = shown(g, state);
        if shown.is_empty() {
            continue;
        }
        let focus_targets: Vec<&str> = shown.iter().filter_map(|(_, n)| n.initial_focus()).collect();
        let items: Vec<Item<'_>> = shown
            .iter()
            .map(|&(id, node)| Item {
                id,
                node,
                disabled: disabled_in(node, state),
                focus: focus_targets.contains(&id),
            })
            .collect();
        let title = esc(&title_of(g.flow(), state));
        let tag = if st.is_final() { "ends here, page shown" } else { "screen" };
        out.push(format!(
            "<article class=\"screen\" id=\"screen-{}\"><header><h3>{title}</h3><span class=\"tag\">{tag}</span><span class=\"mono muted\">{}</span></header>",
            esc(state),
            esc(state)
        ));
        if let Some(d) = st.description() {
            out.push(format!("<p class=\"muted\">{}</p>", esc(d)));
        }
        let kit_class = if kit.is_none() { "" } else { " kit" };
        out.push(format!("<div class=\"wf{kit_class}\" role=\"group\" aria-label=\"Wireframe of {title}\">"));
        out.push(render_wireframe(g, state, &items, warn, false, kit));
        out.push("</div>".into());
        out.push(format!(
            "<details class=\"ax-wrap\"><summary>Expected accessibility tree</summary><pre class=\"ax\">{}</pre></details>",
            esc(&render_tree(g, &items, warn))
        ));
        out.push("</article>".into());
    }
    out.join("\n")
}

/// One state's screen for the tutorial stage. Nodes that depend on a flag (`when`) appear only when named in `reveal`.
pub fn render_stage_screen(
    g: &Graph,
    state: &str,
    reveal: &[&str],
    warn: &mut dyn FnMut(&str),
    kit: Option<&Kit>,
) -> String {
    let items: Vec<Item<'_>> = g
        .ui()
        .nodes()
        .filter(|(id, n)| shown_in(*n, state) && (n.get("when").is_none() || reveal.contains(id)))
        .map(|(id, node)| Item { id, node, disabled: disabled_in(node, state), focus: false })
        .collect();
    let kit_class = if kit.is_none() { "" } else { " kit" };
    format!(
        "<div class=\"wf{kit_class}\" role=\"group\" aria-label=\"Screen: {}\">{}</div>",
        esc(&title_of(g.flow(), state)),
        render_wireframe(g, state, &items, warn, true, kit)
    )
}

/// What the graph expects a screen reader to find for one drawn node: `index` counts the elements drawn for the node
/// in its screen (a ListItem draws one per text).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Expectation {
    pub state: String,
    pub id: String,
    pub index: usize,
    /// None only for a node that has neither a role nor a type; a role that is no string is written as one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// false when ui.yaml gives a control (a field or an action) no role: `role` is then its type, which no element is
    #[serde(rename = "roleGiven")]
    pub role_given: bool,
    /// the native control a field or an action with no role says it is (`native: date`), found by its name among
    /// controls of that kind
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub disabled: bool,
}

/// the screens `kit check` draws, and what each node must expose
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckScreens {
    pub html: String,
    pub expected: Vec<Expectation>,
}

/// For `kit check`: every screen with all its nodes (flags revealed, no annotations), and what each node must expose.
pub fn render_check_screens(g: &Graph, kit: Option<&Kit>, warn: &mut dyn FnMut(&str)) -> CheckScreens {
    let mut html: Vec<String> = Vec::new();
    let mut expected: Vec<Expectation> = Vec::new();
    for state in g.flow().state_names() {
        let shown = shown(g, state);
        if shown.is_empty() {
            continue;
        }
        let items: Vec<Item<'_>> = shown
            .iter()
            .map(|&(id, node)| Item { id, node, disabled: disabled_in(node, state), focus: false })
            .collect();
        let kit_class = if kit.is_none() { "" } else { " kit" };
        html.push(format!(
            "<section class=\"screen\" data-state=\"{}\"><div class=\"wf{kit_class}\">{}</div></section>",
            esc(state),
            render_wireframe(g, state, &items, warn, true, kit)
        ));
        for it in &items {
            let n = it.node;
            if n.component() == Some("Page") {
                continue; // the screen itself: nothing is drawn for it
            }
            if n.component() == Some("ListItem") {
                for (index, line) in texts(g, n, warn).into_iter().enumerate() {
                    expected.push(Expectation {
                        state: state.to_string(),
                        id: it.id.to_string(),
                        index,
                        role: Some(graph::given(n.value(), "role").map_or_else(|| "listitem".into(), js::to_js_string)),
                        role_given: true,
                        native: None,
                        name: Some(line),
                        disabled: it.disabled,
                    });
                }
                continue;
            }
            let (role, is_text) = tree_role(n);
            // a node's type that is no WAI-ARIA role: a field or an action with no `role` says nothing a screen reader
            // can find
            let role_given = n.get("role").is_some() || !matches!(n.kind(), Some("field" | "action"));
            let name = if is_text { texts(g, n, warn).into_iter().next() } else { accessible_name(g, n, warn) };
            expected.push(Expectation {
                state: state.to_string(),
                id: it.id.to_string(),
                index: 0,
                role,
                role_given,
                native: n.get("native").and_then(Value::as_str).map(str::to_string),
                name,
                disabled: it.disabled,
            });
        }
    }
    CheckScreens { html: html.join("\n"), expected }
}

/// the visible words of a node, for narration: a node with several texts gives several entries
pub fn node_texts(g: &Graph, id: &str, warn: &mut dyn FnMut(&str)) -> Vec<String> {
    match g.ui().node(id) {
        None => Vec::new(),
        Some(n) => texts(g, n, warn),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::resolve_kit;
    use crate::kit::tests::{NotesCopy, notes};

    // the CI step "The example kit draws the notes screens, and a kit that is not data is refused"
    #[test]
    fn the_example_kit_draws_the_notes_screens() {
        let g = graph::load(&format!("{}/edit", notes())).expect("the notes edit graph loads");
        let kit = resolve_kit(&g.dir, g.ui().kit(), None).expect("its kit loads");
        let mut warnings = Vec::new();
        let page = render_screens(&g, &mut |m| warnings.push(m.to_string()), kit.as_ref());
        assert!(page.contains("<div class=\"wf kit\""));
        assert!(page.contains("<button type=\"button\" class=\"k-button k-primary\">Save</button>"));
        assert!(
            page.contains(
                "<li class=\"k-item\" data-node=\"pinBox\"><label class=\"k-check\"><input type=\"checkbox\""
            )
        );
        assert_eq!(warnings, Vec::<String>::new());
        let fixed = render_screens(&g, &mut |_| {}, None);
        assert!(fixed.contains("<button type=\"button\" class=\"wf-btn wf-confirm\">Save</button>"));
    }

    // as the TypeScript checker warned (work-kit/unit/expect.mjs)
    #[test]
    fn a_component_the_kit_lacks_is_drawn_fixed() {
        let t = NotesCopy::new("status");
        t.edit("kit/kit.yaml", "  Status:\n    template: '<p class=\"k-status\" role=\"status\">{text}</p>'\n", "");
        let g = graph::load(&format!("{}/edit", t.0)).expect("the graph loads");
        let kit = resolve_kit(&g.dir, g.ui().kit(), None).expect("its kit loads");
        let mut warnings = Vec::new();
        let page = render_screens(&g, &mut |m| warnings.push(m.to_string()), kit.as_ref());
        assert_eq!(
            warnings,
            [
                "kit example has no template for Status (node working); drawn with the fixed component",
                "kit example has no template for Status (node savedStatus); drawn with the fixed component"
            ]
        );
        assert!(page.contains("<div class=\"wf-status\" role=\"status\">The note is saved</div>"));
    }

    #[test]
    fn texts_and_expectations() {
        let g = graph::load(&format!("{}/edit", notes())).expect("the notes edit graph loads");
        assert_eq!(node_texts(&g, "save", &mut |_| {}), ["Save"]);
        assert_eq!(node_texts(&g, "nope", &mut |_| {}), Vec::<String>::new());
        let check = render_check_screens(&g, None, &mut |_| {});
        let pin = check.expected.iter().find(|e| e.id == "pinBox" && e.state == "saving").expect("pinBox in saving");
        assert_eq!(
            (pin.index, pin.role.as_deref(), pin.name.as_deref(), pin.disabled),
            (0, Some("checkbox"), Some("Pin it to the top of my notes"), true)
        );
        let stage = render_stage_screen(&g, "saved", &[], &mut |_| {}, None);
        assert!(stage.starts_with("<div class=\"wf\" role=\"group\" aria-label=\"Screen: Saved\">"));
    }
}
