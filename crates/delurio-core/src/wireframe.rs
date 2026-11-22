//! The screens of a flow drawn as wireframes, with the accessibility tree each is expected to give: for the page, for
//! the tutorial stage and for `kit check`. The port of wireframe.ts; the HTML is the TypeScript checker's, byte for
//! byte, and the warnings come in the same order.

use std::sync::LazyLock;

use std::collections::HashMap;

use regex::{Captures, Regex};
use serde::Serialize;

use crate::expr::{evaluate, parse_expr};
use crate::graph::{self, Graph, Label, UiNode, row_field, shown_in, title_of};
use crate::js::{self, Object, Value};
use crate::kit::{Fill, Kit, draw_with, esc, mark};

/// where a drawing reports what it could not draw as the graph says (a missing catalog key, a component no kit draws)
type Warn<'w> = &'w mut dyn FnMut(&str);

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{((?-u:\w)+)\}").expect("the pattern compiles"));

/// `String(g.ui.samples[p])` of a name the samples do not give but every JavaScript object has from
/// `Object.prototype`: the TypeScript checker found it there and wrote it in
fn inherited_sample(p: &str) -> Option<String> {
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

/// The value a placeholder `{p}` is filled with: for a node drawn for a row of its repeat, the row's field that the
/// words' param reads (`params: { p: row.<field> }`, SPEC.md §4); else, and where the row has no such field, the sample
/// of the placeholder's name.
pub(crate) fn placeholder_value(g: &Graph, p: &str, params: Option<&Object>, row: Option<&Object>) -> Option<String> {
    if let Some(row) = row
        && let Some(field) = params.and_then(|o| o.get(p)).and_then(Value::as_str).and_then(row_field)
        && let Some(v) = row.get(field)
    {
        return Some(js::to_js_string(v));
    }
    g.ui().sample(p).map(js::to_js_string).or_else(|| inherited_sample(p))
}

/// where the words of a node take their values: the params of the words being filled, and the row the node is drawn
/// for (None when it is drawn once)
#[derive(Clone, Copy, Default)]
struct Src<'a> {
    params: Option<&'a Object>,
    row: Option<&'a Object>,
}

fn fill(g: &Graph, key: &str, src: Src<'_>, warn: Warn) -> String {
    // a message that is no string is read as missing (the TypeScript checker could not fill it)
    let Some(raw) = g.message(key) else {
        warn(&format!("catalog key {key} is missing"));
        return format!("⟨missing {key}⟩");
    };
    PLACEHOLDER
        .replace_all(raw, |m: &Captures| {
            let p = &m[1];
            match placeholder_value(g, p, src.params, src.row) {
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

fn texts(g: &Graph, n: UiNode<'_>, row: Option<&Object>, warn: Warn) -> Vec<String> {
    let Some(t) = n.text() else { return Vec::new() };
    let src = Src { params: t.params(), row };
    if let Some(key) = key_of(t, "key") {
        return vec![fill(g, &key, src, warn)];
    }
    if let Some(keys) = one_of(t) {
        return keys.iter().map(|k| fill(g, k, src, warn)).collect();
    }
    if let Some(literal) = t.literal() {
        return vec![literal.to_string()];
    }
    Vec::new()
}

fn accessible_name(g: &Graph, n: UiNode<'_>, row: Option<&Object>, warn: Warn) -> Option<String> {
    let nm = n.name();
    if let Some(key) = nm.and_then(|l| key_of(l, "key")) {
        return Some(fill(g, &key, Src { params: nm.and_then(|l| l.params()), row }, warn));
    }
    if let Some(literal) = nm.and_then(|l| l.literal()) {
        return Some(literal.to_string());
    }
    if let Some(by) = nm.and_then(|l| key_of(l, "labelledBy")) {
        // a node of the same repeat labels it with the words of the same row
        return g.ui().node(&by).map(|r| {
            let same = r.repeat().is_some() && r.repeat() == n.repeat();
            texts(g, r, if same { row } else { None }, warn).join(" ")
        });
    }
    if matches!(n.role(), Some("button" | "heading" | "status" | "alert")) {
        return texts(g, n, row, warn).into_iter().next();
    }
    // a checkbox, a radio and a select are named by their label, the words beside them (SPEC.md §4)
    if LABELLED.contains(&n.component().unwrap_or("")) {
        return texts(g, n, row, warn).into_iter().next();
    }
    None
}

const LABELLED: [&str; 3] = ["Checkbox", "Radio", "Select"];

/// the words a field holds when it is shown (`value`), for a select the option it shows chosen
fn value_words(g: &Graph, n: UiNode<'_>, row: Option<&Object>, warn: Warn) -> String {
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
        Some(k) => fill(g, &js::to_js_string(&k), Src { params: v.params(), row }, warn),
    }
}

/// an expression's value over a situation's context and the row a node is drawn for: `state`, `row.<field>`, a
/// variable, a guard (read in turn, as deep as the checker reads one); a name that is none of these is null
fn value_over(g: &Graph, src: &str, state: &str, ctx: &Object, row: Option<&Object>, depth: usize) -> Value {
    let Ok(e) = parse_expr(src) else { return Value::Null };
    evaluate(&e, &|name: &str| {
        if name == "state" {
            return Value::from(state);
        }
        if let Some(field) = row_field(name) {
            return row.and_then(|r| r.get(field)).cloned().unwrap_or(Value::Null);
        }
        if let Some(v) = ctx.get(name) {
            return v.clone();
        }
        match g.flow().guard(name).and_then(Value::as_str) {
            Some(guard) if depth < 8 => value_over(g, guard, state, ctx, row, depth + 1),
            _ => Value::Null,
        }
    })
}

/// whether a node is drawn checked, for the row it is drawn for: with the context of a situation, its `checked` read
/// over it and the row (SPEC.md §4); without one, only `checked: true`, as a screen that stands for every situation
/// can say no more
fn drawn_checked(g: &Graph, n: UiNode<'_>, state: &str, ctx: Option<&Object>, row: Option<&Object>) -> bool {
    match (n.checked(), ctx) {
        (Some(Value::Bool(b)), _) => *b,
        (Some(Value::String(src)), Some(ctx)) => value_over(g, src, state, ctx, row, 0).truthy(),
        _ => false,
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

// ---- the layout of a screen: which nodes are drawn alone, which in rows, and which lists hold them

/// a list a screen draws: a plain one, or a table or a grid, drawn row by row
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListKind {
    Plain,
    Table,
}

/// what holds a row of a repeat's nodes, side by side
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowKind {
    /// a row of a table or a grid
    Table,
    /// an item of a list
    Item,
    /// a row of its own, outside any list
    Cells,
}

/// one piece of a screen, in the order it is drawn
enum Piece<'a> {
    /// a node drawn alone (a repeat node that is drawn alone, once per row)
    Node { at: usize, row: Option<&'a Object> },
    /// the header row of a table: its header cells, before its rows
    Header { cells: Vec<usize> },
    /// one item of the nodes that share a repeat, drawn side by side; a ListItem first is the item itself
    Row { members: Vec<usize>, row: Option<&'a Object>, kind: RowKind },
}

/// the repeat of a node that is drawn in rows with the nodes beside it that share it: any but a page and a list
fn row_repeat<'a>(n: UiNode<'a>) -> Option<&'a str> {
    n.repeat().filter(|_| !matches!(n.component(), Some("Page" | "List")))
}

/// the rows a repeat node is drawn for: the samples' rows of its data, or once (None) when they give none
fn rows_of<'a>(g: &'a Graph, n: UiNode<'a>) -> Vec<Option<&'a Object>> {
    match n.repeat().and_then(|r| g.ui().rows(r)) {
        Some(rows) => rows.iter().map(Value::as_object).collect(),
        None => vec![None],
    }
}

fn list_kind(n: UiNode<'_>) -> ListKind {
    if matches!(n.role(), Some("table" | "grid")) { ListKind::Table } else { ListKind::Plain }
}

/// The pieces a screen is drawn as (SPEC.md §4). A List holds the ListItems after it, as before, and the nodes after
/// it that share a repeat; a table's or a grid's also holds its header cells (role columnheader) before them, which
/// form its header row. Adjacent nodes that share a repeat (a ListItem only as the first of them) are drawn as one row
/// per item, side by side: a row of the table, an item of the list, or a row of their own. A lone repeat node outside
/// a list, and a lone ListItem, are drawn as before, once per item. Any other node closes the list.
fn layout<'a>(g: &'a Graph, items: &[Item<'a>]) -> Vec<Piece<'a>> {
    let mut out: Vec<Piece<'a>> = Vec::new();
    let mut list: Option<ListKind> = None;
    // a table's header cells, while it may still take them (before its first row)
    let mut header: Option<Vec<usize>> = None;
    let flush = |out: &mut Vec<Piece<'a>>, header: &mut Option<Vec<usize>>| {
        if let Some(cells) = header.take().filter(|c| !c.is_empty()) {
            out.push(Piece::Header { cells });
        }
    };
    let mut i = 0;
    while i < items.len() {
        let n = items[i].node;
        let component = n.component();
        if let Some(repeat) = row_repeat(n) {
            let mut j = i + 1;
            while j < items.len()
                && row_repeat(items[j].node) == Some(repeat)
                && items[j].node.component() != Some("ListItem")
            {
                j += 1;
            }
            flush(&mut out, &mut header);
            let rows = rows_of(g, n);
            if j == i + 1 && (component == Some("ListItem") || list.is_none()) {
                out.extend(rows.into_iter().map(|row| Piece::Node { at: i, row }));
            } else {
                let kind = match list {
                    Some(ListKind::Table) => RowKind::Table,
                    Some(ListKind::Plain) => RowKind::Item,
                    None => RowKind::Cells,
                };
                out.extend(rows.into_iter().map(|row| Piece::Row { members: (i..j).collect(), row, kind }));
            }
            i = j;
            continue;
        }
        if let Some(cells) = header.as_mut()
            && n.role() == Some("columnheader")
        {
            cells.push(i);
            i += 1;
            continue;
        }
        if component != Some("ListItem") {
            flush(&mut out, &mut header);
            list = None;
        }
        out.push(Piece::Node { at: i, row: None });
        if component == Some("List") {
            list = Some(list_kind(n));
            header = (list == Some(ListKind::Table)).then(Vec::new);
        }
        i += 1;
    }
    flush(&mut out, &mut header);
    out
}

// a row's nodes side by side, in any wireframe: inline, so that the stage and kit check draw them as the page does
const CELLS: &str = "display:flex;flex-wrap:wrap;align-items:baseline;gap:4px 12px";
// a row of a table: a column for each of its nodes, so that the rows' columns line up
const TABLE_ROW: &str =
    "display:grid;grid-auto-flow:column;grid-auto-columns:minmax(0,1fr);align-items:baseline;gap:4px 12px";
// beside a ListItem's own words, in the item
const BESIDE: &str = "display:inline-flex;flex-wrap:wrap;align-items:baseline;gap:4px 12px;margin-left:12px";

/// a List drawn with the kit, waiting for its items
struct KitList<'a> {
    it: Item<'a>,
    items: Vec<String>,
}

/// What a wireframe is drawn with, and what it has drawn so far.
struct Wireframe<'a, 'k> {
    g: &'a Graph,
    state: &'a str,
    /// the situation the screen is drawn for, when there is one: `checked` is read over it
    ctx: Option<&'a Object>,
    clean: bool,
    kit: Option<&'k Kit>,
    out: Vec<String>,
    in_list: bool,
    /// what closes the list the fixed wireframe draws: a list, or a table drawn row by row
    list_end: &'static str,
    in_row: bool,
    kit_list: Option<KitList<'a>>,
}

impl<'a, 'k> Wireframe<'a, 'k> {
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

    fn chips(&self, it: &Item<'_>) -> String {
        if self.clean { String::new() } else { chips(it) }
    }

    fn close_list(&mut self, warn: Warn) {
        if let Some(list) = self.kit_list.take() {
            let drawn = self.draw_kit_list(&list.it, &list.items, warn);
            self.out.push(drawn);
        } else if self.in_list {
            self.out.push(self.list_end.into());
        }
        self.in_list = false;
    }

    fn fill(&self, it: &Item<'_>, text: &str, row: Option<&Object>, warn: Warn) -> Fill {
        let n = it.node;
        let name = accessible_name(self.g, n, row, warn);
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
            checked: drawn_checked(self.g, n, self.state, self.ctx, row),
        }
    }

    fn draw_kit_list(&self, it: &Item<'_>, drawn: &[String], warn: Warn) -> String {
        let first = texts(self.g, it.node, None, warn).into_iter().next().unwrap_or_default();
        let mut f = self.fill(it, &first, None, warn);
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

    /// the kit the node is drawn with: the kit, when it has a template for the node's component (any value but the
    /// word Page, looked up as `kit.components[n.component]` looks it up); else the fixed components, with a warning
    /// when there is a kit
    fn kit_for(&self, it: &Item<'_>, warn: Warn) -> Option<&'k Kit> {
        let kit = self.kit?;
        let c = it.node.get("component").filter(|v| v.as_str() != Some("Page")).map(js::to_js_string)?;
        if kit.components.contains_key(&c) {
            return Some(kit);
        }
        warn(&format!("kit {} has no template for {c} (node {}); drawn with the fixed component", kit.id, it.id));
        None
    }

    /// one ListItem drawn for one of its texts
    fn list_item(
        &self,
        it: &Item<'_>,
        kit: Option<&Kit>,
        line: &str,
        row: Option<&Object>,
        beside: &str,
        warn: Warn,
    ) -> String {
        let chips = self.chips(it);
        match kit {
            Some(kit) => {
                let f = self.fill(it, line, row, warn);
                let drawn = draw_with(kit, "ListItem", &f, warn).unwrap_or_default();
                mark(&drawn, it.id, &format!("{chips}{beside}"))
            }
            None => format!("<li data-node=\"{}\">{}{chips}{beside}</li>", esc(it.id), esc(line)),
        }
    }

    /// a node that is neither a list nor a list item, drawn alone: with the kit's template or the fixed component
    fn node_html(&self, it: &Item<'_>, kit: Option<&Kit>, first: &str, row: Option<&Object>, warn: Warn) -> String {
        let n = it.node;
        let component = n.component();
        if let Some(kit) = kit {
            let c = n.get("component").map(js::to_js_string).unwrap_or_default();
            let f = self.fill(it, first, row, warn);
            let drawn = draw_with(kit, &c, &f, warn).unwrap_or_default();
            return self.item(it, &drawn);
        }
        let text = esc(first);
        let disabled = if it.disabled { " disabled" } else { "" };
        match component {
            Some("Heading") => self.item(it, &format!("<h4 class=\"wf-h\">{text}</h4>")),
            Some("Text") => self.item(it, &format!("<p class=\"wf-t\">{text}</p>")),
            Some("Button") => {
                let intent = esc(n.intent().unwrap_or("secondary"));
                self.item(
                    it,
                    &format!("<button type=\"button\" class=\"wf-btn wf-{intent}\"{disabled}>{text}</button>"),
                )
            }
            Some("LinkButton") => {
                self.item(it, &format!("<button type=\"button\" class=\"wf-link\"{disabled}>{text}</button>"))
            }
            // the visible text is the label; the input is named by it, as a <label> around it does
            Some("TextField") => self.item(
                it,
                &format!(
                    "<label class=\"wf-field\"><span>{text}</span><input type=\"text\" class=\"wf-input\" tabindex=\"-1\"{disabled}></label>"
                ),
            ),
            Some(c @ ("Checkbox" | "Radio")) => {
                // the box beside its label, as a <label> around an <input> draws it
                let kind = if c == "Radio" { "radio" } else { "checkbox" };
                let checked = if drawn_checked(self.g, n, self.state, self.ctx, row) { " checked" } else { "" };
                self.item(
                    it,
                    &format!(
                        "<label class=\"wf-check\"><input type=\"{kind}\" tabindex=\"-1\"{checked}{disabled}><span>{text}</span></label>"
                    ),
                )
            }
            Some("Select") => {
                let value = esc(&value_words(self.g, n, row, warn));
                self.item(
                    it,
                    &format!(
                        "<label class=\"wf-field\"><span>{text}</span><select class=\"wf-input\" tabindex=\"-1\"{disabled}><option>{value}</option></select></label>"
                    ),
                )
            }
            Some("Status") => self.item(it, &format!("<div class=\"wf-status\" role=\"status\">{text}</div>")),
            Some("Alert") => self.item(it, &format!("<div class=\"wf-alert\" role=\"alert\">{text}</div>")),
            _ => {
                warn(&format!("{}: component {} has no wireframe renderer", it.id, component.unwrap_or("(none)")));
                let what = esc(component.or(n.kind()).unwrap_or("undefined"));
                self.item(it, &format!("<div class=\"wf-unmapped\">{what} {}</div>", esc(it.id)))
            }
        }
    }

    /// a node drawn alone, as the wireframe has always drawn it: a list opens, a list item goes into it, and buttons
    /// side by side share a row
    fn node(&mut self, it: &Item<'a>, row: Option<&Object>, warn: Warn) {
        let n = it.node;
        let t = texts(self.g, n, row, warn);
        let component = n.component();
        if component != Some("ListItem") {
            self.close_list(warn);
        }
        if component != Some("Button") {
            self.close_row();
        }
        let kit = self.kit_for(it, warn);
        match component {
            Some("Page") if kit.is_none() => {}
            Some("List") => {
                if kit.is_some() {
                    self.kit_list = Some(KitList { it: *it, items: Vec::new() });
                } else if list_kind(n) == ListKind::Table {
                    self.out.push(format!(
                        "<div class=\"wf-table\" data-node=\"{}\" style=\"display:grid;gap:4px\">",
                        esc(it.id)
                    ));
                    self.list_end = "</div>";
                } else {
                    self.out.push(format!("<ul class=\"wf-list\" data-node=\"{}\">", esc(it.id)));
                    self.list_end = "</ul>";
                }
                self.in_list = true;
            }
            Some("ListItem") => {
                if !self.in_list {
                    warn(&format!("{}: ListItem outside a List in {}", it.id, self.state));
                }
                for line in &t {
                    let html = self.list_item(it, kit, line, row, "", warn);
                    self.push_item(html);
                }
            }
            _ => {
                if component == Some("Button") && !self.in_row {
                    self.out.push("<div class=\"wf-row\">".into());
                }
                if component == Some("Button") {
                    self.in_row = true;
                }
                let html = self.node_html(it, kit, t.first().map_or("", String::as_str), row, warn);
                self.out.push(html);
            }
        }
    }

    /// a node drawn in a row beside others: as alone, a ListItem for its first text only
    fn cell(&self, it: &Item<'_>, row: Option<&Object>, warn: Warn) -> String {
        let t = texts(self.g, it.node, row, warn);
        let first = t.first().map_or("", String::as_str);
        let kit = self.kit_for(it, warn);
        if it.node.component() == Some("ListItem") {
            return self.list_item(it, kit, first, row, "", warn);
        }
        self.node_html(it, kit, first, row, warn)
    }

    /// one row of the nodes that share a repeat: the table's row, the list's item, or a row of its own; a ListItem
    /// first is the item, the others beside its words
    fn row(&mut self, items: &[Item<'a>], members: &[usize], row: Option<&Object>, kind: RowKind, warn: Warn) {
        self.close_row();
        let lead = items[members[0]];
        let html = if kind != RowKind::Table && lead.node.component() == Some("ListItem") {
            if !self.in_list {
                warn(&format!("{}: ListItem outside a List in {}", lead.id, self.state));
            }
            let others: String = members[1..].iter().map(|&m| self.cell(&items[m], row, warn)).collect();
            let beside = format!("<div class=\"wf-cells\" style=\"{BESIDE}\">{others}</div>");
            let t = texts(self.g, lead.node, row, warn);
            let kit = self.kit_for(&lead, warn);
            self.list_item(&lead, kit, t.first().map_or("", String::as_str), row, &beside, warn)
        } else {
            let cells: String = members.iter().map(|&m| self.cell(&items[m], row, warn)).collect();
            self.wrap(kind, false, &cells)
        };
        self.push_item(html);
    }

    /// a table's header row: its header cells side by side
    fn header(&mut self, items: &[Item<'a>], cells: &[usize], warn: Warn) {
        self.close_row();
        let drawn: String = cells.iter().map(|&m| self.cell(&items[m], None, warn)).collect();
        let html = self.wrap(RowKind::Table, true, &drawn);
        self.push_item(html);
    }

    /// what holds a row's nodes. A kit's table gets each row as an element of role row, which the kit's cells are in;
    /// the fixed wireframe's rows give no role, as its cells give none
    fn wrap(&self, kind: RowKind, header: bool, cells: &str) -> String {
        match kind {
            RowKind::Table => {
                let role = if self.kit_list.is_some() { " role=\"row\"" } else { "" };
                let weight = if header { ";font-weight:600" } else { "" };
                format!("<div class=\"wf-tr\"{role} style=\"{TABLE_ROW}{weight}\">{cells}</div>")
            }
            RowKind::Item => format!("<li class=\"wf-li\" style=\"{CELLS}\">{cells}</li>"),
            RowKind::Cells => format!("<div class=\"wf-cells\" style=\"{CELLS}\">{cells}</div>"),
        }
    }
}

fn render_wireframe<'a>(
    g: &'a Graph,
    state: &'a str,
    items: &[Item<'a>],
    warn: Warn,
    clean: bool,
    kit: Option<&Kit>,
    ctx: Option<&'a Object>,
) -> String {
    let mut w = Wireframe {
        g,
        state,
        ctx,
        clean,
        kit,
        out: Vec::new(),
        in_list: false,
        list_end: "</ul>",
        in_row: false,
        kit_list: None,
    };
    for piece in layout(g, items) {
        match piece {
            Piece::Node { at, row } => w.node(&items[at], row, warn),
            Piece::Header { cells } => w.header(items, &cells, warn),
            Piece::Row { members, row, kind } => w.row(items, &members, row, kind, warn),
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

/// the expected tree's line, or lines, of one node at a depth: a ListItem's for each of its texts (its first only, in
/// a row), any other node's role and name
fn tree_lines(g: &Graph, it: &Item<'_>, row: Option<&Object>, depth: usize, first: bool, warn: Warn) -> Vec<String> {
    let n = it.node;
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
        let mut t = texts(g, n, row, warn);
        if first {
            t.truncate(1);
        }
        return t.into_iter().map(|line| format!("{pad}listitem \"{line}\"{suffix}")).collect();
    }
    let (role, is_text) = tree_role(n);
    let name = if is_text { texts(g, n, row, warn).into_iter().next() } else { accessible_name(g, n, row, warn) };
    let named = name.map_or_else(String::new, |nm| format!(" \"{nm}\""));
    vec![format!("{pad}{}{named}{suffix}", role.as_deref().unwrap_or("undefined"))]
}

fn render_tree(g: &Graph, items: &[Item<'_>], warn: Warn) -> String {
    let mut lines: Vec<String> = Vec::new();
    for piece in layout(g, items) {
        match piece {
            Piece::Node { at, row } => {
                let it = &items[at];
                let depth = match it.node.component() {
                    Some("Page") => 0,
                    Some("ListItem") => 2,
                    _ => 1,
                };
                lines.extend(tree_lines(g, it, row, depth, false, warn));
            }
            Piece::Header { cells } => {
                lines.push("    row".to_string());
                for m in cells {
                    lines.extend(tree_lines(g, &items[m], None, 3, true, warn));
                }
            }
            Piece::Row { members, row, kind } => {
                let lead = &items[members[0]];
                let (rest, depth) = if kind != RowKind::Table && lead.node.component() == Some("ListItem") {
                    lines.extend(tree_lines(g, lead, row, 2, true, warn));
                    (&members[1..], 3)
                } else {
                    match kind {
                        RowKind::Table => lines.push("    row".to_string()),
                        RowKind::Item => lines.push("    listitem".to_string()),
                        RowKind::Cells => {}
                    }
                    (&members[..], if kind == RowKind::Cells { 1 } else { 3 })
                };
                for &m in rest {
                    lines.extend(tree_lines(g, &items[m], row, depth, true, warn));
                }
            }
        }
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
        out.push(render_wireframe(g, state, &items, warn, false, kit, None));
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
/// `ctx`, the context of the situation the stage shows (the flow's variables and their values), draws each node's
/// `checked` as it reads there, for each row it is drawn for; without it only `checked: true` is drawn.
pub fn render_stage_screen(
    g: &Graph,
    state: &str,
    reveal: &[&str],
    warn: &mut dyn FnMut(&str),
    kit: Option<&Kit>,
    ctx: Option<&Object>,
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
        render_wireframe(g, state, &items, warn, true, kit, ctx)
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

/// what one drawn element of a node must expose: a ListItem's for one of its texts, any other node's
fn expectation(
    g: &Graph,
    state: &str,
    it: &Item<'_>,
    index: usize,
    line: Option<String>,
    row: Option<&Object>,
    warn: Warn,
) -> Expectation {
    let n = it.node;
    if let Some(line) = line {
        return Expectation {
            state: state.to_string(),
            id: it.id.to_string(),
            index,
            role: Some(graph::given(n.value(), "role").map_or_else(|| "listitem".into(), js::to_js_string)),
            role_given: true,
            native: None,
            name: Some(line),
            disabled: it.disabled,
        };
    }
    let (role, is_text) = tree_role(n);
    // a node's type that is no WAI-ARIA role: a field or an action with no `role` says nothing a screen reader can find
    let role_given = n.get("role").is_some() || !matches!(n.kind(), Some("field" | "action"));
    let name = if is_text { texts(g, n, row, warn).into_iter().next() } else { accessible_name(g, n, row, warn) };
    Expectation {
        state: state.to_string(),
        id: it.id.to_string(),
        index,
        role,
        role_given,
        native: n.get("native").and_then(Value::as_str).map(str::to_string),
        name,
        disabled: it.disabled,
    }
}

/// For `kit check`: every screen with all its nodes (flags revealed, no annotations), and what each node must expose:
/// one expectation for each element drawn for it, in the order they are drawn (a repeat node's for each row).
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
            render_wireframe(g, state, &items, warn, true, kit, None)
        ));
        // how many elements each node has drawn so far
        let mut drawn: HashMap<String, usize> = HashMap::new();
        let mut expect = |it: &Item<'_>, row: Option<&Object>, first: bool, warn: Warn| {
            let n = it.node;
            if n.component() == Some("Page") {
                return; // the screen itself: nothing is drawn for it
            }
            let at = drawn.entry(it.id.to_string()).or_default();
            if n.component() == Some("ListItem") {
                let mut t = texts(g, n, row, warn);
                if first {
                    t.truncate(1);
                }
                for line in t {
                    expected.push(expectation(g, state, it, *at, Some(line), row, warn));
                    *at += 1;
                }
                return;
            }
            expected.push(expectation(g, state, it, *at, None, row, warn));
            *at += 1;
        };
        for piece in layout(g, &items) {
            match piece {
                Piece::Node { at, row } => expect(&items[at], row, false, warn),
                Piece::Header { cells } => cells.iter().for_each(|&m| expect(&items[m], None, true, warn)),
                Piece::Row { members, row, .. } => members.iter().for_each(|&m| expect(&items[m], row, true, warn)),
            }
        }
    }
    CheckScreens { html: html.join("\n"), expected }
}

/// the visible words of a node, for narration: a node with several texts gives several entries, and a repeat node
/// those of each of its sample rows in turn
pub fn node_texts(g: &Graph, id: &str, warn: &mut dyn FnMut(&str)) -> Vec<String> {
    match g.ui().node(id) {
        None => Vec::new(),
        Some(n) => rows_of(g, n).into_iter().flat_map(|row| texts(g, n, row, warn)).collect(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::kit::resolve_kit;
    use crate::kit::tests::{NotesCopy, notes};

    // a small exam: one question's options, a radio group whose chosen one is the context's `answer`, a table of
    // results, and a list of questions whose number and state share a row (SPEC.md §4, Rows of a repeat)
    pub(crate) const EXAM_FLOW: &str = "schema: delurio-graph/0
kind: flow
id: exam.take
status: proposed
title: Take an exam
context:
  answer: { values: [none, a, b], initial: none }
guards:
  picked: \"answer != 'none'\"
effects:
  choose: { set: { answer: row.id } }
initial: answering
states:
  answering: { title: Answering, on: { CHOOSE: { target: answering, effect: [choose] }, FINISH: done } }
  done: { type: final, title: Done }
";

    pub(crate) const EXAM_UI: &str = "schema: delurio-graph/0
kind: ui
id: exam.take
flow: exam.take
status: proposed
catalog: catalog/en.json
locales: [en]
samples:
  question: How many legs has a spider?
  question.options: [ { id: a, text: Eight }, { id: b, text: Six } ]
  results.rows: [ { name: Ada, score: '90' }, { name: Bo, score: '75' } ]
  attempt.questions: [ { number: '1', state: Answered }, { number: '2', state: Skipped } ]
nodes:
  page: { type: page, component: Page, role: main, in: [answering] }
  title: { type: text, component: Heading, role: heading, text: { key: Exam.question, params: { question: exam.question } }, in: [answering] }
  options: { type: list, component: List, role: radiogroup, name: { labelledBy: title }, in: [answering] }
  option: { type: field, component: OPTION, role: radio, repeat: question.options, text: { key: Exam.option, params: { option: row.text } }, checked: \"row.id == answer\", in: [answering], emits: CHOOSE }
  results: { type: list, component: List, role: table, name: { literal: Results }, in: [answering] }
  colName: { type: text, component: Text, role: columnheader, text: { literal: Name }, in: [answering] }
  colScore: { type: text, component: Text, role: columnheader, text: { literal: Score }, in: [answering] }
  rowName: { type: text, component: Text, role: rowheader, repeat: results.rows, text: { key: Exam.name, params: { name: row.name } }, in: [answering] }
  rowScore: { type: text, component: Text, role: cell, repeat: results.rows, text: { key: Exam.score, params: { score: row.score } }, in: [answering] }
  finish: { type: action, component: Button, role: button, intent: confirm, text: { literal: Finish }, in: [answering], emits: FINISH }
  number: { type: action, component: Button, role: button, repeat: attempt.questions, text: { key: Exam.number, params: { number: row.number } }, in: [answering], emits: CHOOSE }
  state: { type: text, component: Text, repeat: attempt.questions, text: { key: Exam.state, params: { state: row.state } }, in: [answering] }
";

    const EXAM_CATALOG: &str = r#"{ "Exam.question": "{question}", "Exam.option": "{option}", "Exam.name": "{name}", "Exam.score": "{score}", "Exam.number": "{number}", "Exam.state": "{state}" }"#;

    /// the exam, its options drawn with the component given, and its ui.yaml edited
    pub(crate) fn exam(option: &str, edit: &dyn Fn(String) -> String) -> Graph {
        let ui = edit(EXAM_UI.replace("OPTION", option));
        let read = move |f: &str| -> crate::error::Result<String> {
            Ok(match f {
                "flow.yaml" => EXAM_FLOW.to_string(),
                "ui.yaml" => ui.clone(),
                _ => EXAM_CATALOG.to_string(),
            })
        };
        graph::load_with("exam", &read, &|f| f != "tutorial.yaml").expect("the exam loads")
    }

    fn ctx(answer: &str) -> Object {
        Object::from_iter([("answer", Value::from(answer))])
    }

    #[test]
    fn a_repeat_is_drawn_once_per_sample_row_with_its_fields() {
        let g = exam("ListItem", &|u| u);
        let mut warnings = Vec::new();
        let page = render_screens(&g, &mut |m| warnings.push(m.to_string()), None);
        assert_eq!(warnings, Vec::<String>::new());
        assert!(page.contains("<li data-node=\"option\">Eight<span class=\"chip\">"), "{page}");
        assert!(page.contains("<li data-node=\"option\">Six<span class=\"chip\">"));
        assert_eq!(node_texts(&g, "option", &mut |_| {}), ["Eight", "Six"]);
        // no rows: drawn once, its placeholder from the samples by its name, which give none
        let once = exam("ListItem", &|u| {
            u.replace("  question.options: [ { id: a, text: Eight }, { id: b, text: Six } ]\n", "")
        });
        let mut warnings = Vec::new();
        let page = render_screens(&once, &mut |m| warnings.push(m.to_string()), None);
        assert!(page.contains("<li data-node=\"option\">⟨option⟩"));
        warnings.dedup();
        assert_eq!(warnings, ["no sample value for {option} in Exam.option"]);
        // a node drawn once reads no row, whatever its params name
        let flat = exam("ListItem", &|u| {
            u.replace("  question: How", "  option: Any\n  question: How")
                .replace(" repeat: question.options,", "")
                .replace("checked: \"row.id == answer\", ", "")
        });
        assert_eq!(node_texts(&flat, "option", &mut |_| {}), ["Any"]);
    }

    #[test]
    fn a_table_is_drawn_row_by_row_and_nodes_that_share_a_repeat_side_by_side() {
        let g = exam("ListItem", &|u| u);
        let page = render_screens(&g, &mut |_| {}, None);
        let table = &page[page.find("<div class=\"wf-table\" data-node=\"results\"").expect("the table")..];
        let header = format!(
            "<div class=\"wf-tr\" style=\"{TABLE_ROW};font-weight:600\"><div class=\"wf-item\" data-node=\"colName\"><p class=\"wf-t\">Name</p>"
        );
        assert!(
            table.starts_with(&format!(
                "<div class=\"wf-table\" data-node=\"results\" style=\"display:grid;gap:4px\">\n{header}"
            )),
            "{table}"
        );
        assert!(table.contains(&format!("<div class=\"wf-tr\" style=\"{TABLE_ROW}\"><div class=\"wf-item\" data-node=\"rowName\"><p class=\"wf-t\">Ada</p>")));
        assert!(table.contains("<div class=\"wf-item\" data-node=\"rowScore\"><p class=\"wf-t\">75</p>"));
        // the number and the state of a question share a row, outside any list, which the Finish button closed
        let row = format!(
            "<div class=\"wf-cells\" style=\"{CELLS}\"><div class=\"wf-item\" data-node=\"number\"><button type=\"button\" class=\"wf-btn wf-secondary\">2</button>"
        );
        assert!(page.contains(&row), "{page}");
        assert!(page.contains("<div class=\"wf-item\" data-node=\"state\"><p class=\"wf-t\">Skipped</p>"));
        let tree = &page[page.find("<pre class=\"ax\">").expect("the tree")..];
        for line in [
            "\n  radiogroup &quot;How many legs has a spider?&quot;\n    listitem &quot;Eight&quot;  [→ CHOOSE]\n    listitem &quot;Six&quot;  [→ CHOOSE]",
            "\n  table &quot;Results&quot;\n    row\n      columnheader\n      columnheader\n    row\n      rowheader\n      cell\n    row\n      rowheader\n      cell\n",
            "\n  button &quot;1&quot;  [→ CHOOSE]\n  text &quot;Answered&quot;\n  button &quot;2&quot;",
        ] {
            assert!(tree.contains(line), "{line} in {tree}");
        }
        let check = render_check_screens(&g, None, &mut |_| {});
        let of = |id: &str| -> Vec<(usize, String)> {
            check
                .expected
                .iter()
                .filter(|e| e.id == id)
                .map(|e| (e.index, e.name.clone().unwrap_or_default()))
                .collect()
        };
        assert_eq!(of("option"), [(0, "Eight".to_string()), (1, "Six".to_string())]);
        assert_eq!(of("rowScore"), [(0, String::new()), (1, String::new())]);
        assert_eq!(of("colName"), [(0, String::new())]);
        let order: Vec<&str> =
            check.expected.iter().map(|e| e.id.as_str()).skip_while(|id| *id != "number").take(4).collect();
        assert_eq!(order, ["number", "state", "number", "state"]);
    }

    #[test]
    fn a_list_takes_the_rows_of_the_repeat_after_it_and_a_list_item_holds_its_row() {
        // a radio that is no ListItem: each is one item of the radio group
        let g = exam("Radio", &|u| u);
        let page = render_screens(&g, &mut |_| {}, None);
        assert!(page.contains(&format!("<ul class=\"wf-list\" data-node=\"options\">\n<li class=\"wf-li\" style=\"{CELLS}\"><div class=\"wf-item\" data-node=\"option\"><label class=\"wf-check\"><input type=\"radio\" tabindex=\"-1\"><span>Eight</span>")), "{page}");
        // a ListItem that begins a row holds the nodes beside it
        let led = exam("ListItem", &|u| {
            u.replace("  results: {", "  hint: { type: text, component: Text, repeat: question.options, text: { key: Exam.option, params: { option: row.id } }, in: [answering] }\n  results: {")
        });
        let page = render_screens(&led, &mut |_| {}, None);
        assert!(page.contains(&format!("<li data-node=\"option\">Eight<span class=\"chip\">radio · emits CHOOSE</span><div class=\"wf-cells\" style=\"{BESIDE}\"><div class=\"wf-item\" data-node=\"hint\"><p class=\"wf-t\">a</p>")), "{page}");
        let tree = &page[page.find("<pre class=\"ax\">").expect("the tree")..];
        assert!(
            tree.contains(
                "\n    listitem &quot;Eight&quot;  [→ CHOOSE]\n      text &quot;a&quot;\n    listitem &quot;Six&quot;"
            ),
            "{tree}"
        );
    }

    #[test]
    fn checked_reads_the_situation_and_the_row_on_the_stage() {
        let g = exam("Radio", &|u| u);
        let none = render_stage_screen(&g, "answering", &[], &mut |_| {}, None, None);
        assert!(!none.contains(" checked"));
        let b = render_stage_screen(&g, "answering", &[], &mut |_| {}, None, Some(&ctx("b")));
        assert!(b.contains("<input type=\"radio\" tabindex=\"-1\"><span>Eight</span>"), "{b}");
        assert!(b.contains("<input type=\"radio\" tabindex=\"-1\" checked><span>Six</span>"));
        // the page draws every situation at once: only `checked: true`
        let page = render_screens(&g, &mut |_| {}, None);
        assert!(!page.contains(" checked"));
        let option = g.ui().node("option").expect("the option");
        let row = Object::from_iter([("id", Value::from("a"))]);
        assert!(drawn_checked(&g, option, "answering", Some(&ctx("a")), Some(&row)));
        assert!(!drawn_checked(&g, option, "answering", Some(&ctx("b")), Some(&row)));
        assert!(!drawn_checked(&g, option, "answering", None, Some(&row)));
        // a guard is read too, and a name that is none is null
        let guarded = exam("Radio", &|u| u.replace("\"row.id == answer\"", "\"picked and row.id == answer\""));
        let option = guarded.ui().node("option").expect("the option");
        assert!(drawn_checked(&guarded, option, "answering", Some(&ctx("a")), Some(&row)));
    }

    #[test]
    fn a_kit_keeps_its_templates_inside_the_rows() {
        let t = NotesCopy::new("rows");
        let kit = resolve_kit(&format!("{}/edit", t.0), Some("../kit"), None).expect("the kit loads").expect("a kit");
        let g = exam("ListItem", &|u| u);
        let mut warnings = Vec::new();
        let page = render_screens(&g, &mut |m| warnings.push(m.to_string()), Some(&kit));
        assert!(
            page.contains("<li class=\"k-item\" data-node=\"option\"><label class=\"k-check\"><input type=\"radio\""),
            "{page}"
        );
        // the kit has no table template: its List is drawn, holding rows of role row
        assert!(page.contains(&format!("<ul class=\"k-list\" data-node=\"results\">\n<div class=\"wf-tr\" role=\"row\" style=\"{TABLE_ROW};font-weight:600\"><div class=\"wf-item\" data-node=\"colName\"><p class=\"k-text\">Name</p>")), "{page}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn a_row_in_checked_needs_a_repeat() {
        let read = |f: &str| -> crate::error::Result<String> {
            Ok(match f {
                "flow.yaml" => EXAM_FLOW.to_string(),
                "ui.yaml" => EXAM_UI.replace("OPTION", "ListItem").replace(" repeat: question.options,", ""),
                _ => EXAM_CATALOG.to_string(),
            })
        };
        let e = graph::load_with("exam", &read, &|_| true).expect_err("refused");
        assert_eq!(
            e.message,
            "ui node option: checked: row.id == answer names row.id, which is not a variable or a guard of the flow: a node reads the row it is drawn for (row.<field>) only when it has repeat"
        );
    }

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
        let stage = render_stage_screen(&g, "saved", &[], &mut |_| {}, None, None);
        assert!(stage.starts_with("<div class=\"wf\" role=\"group\" aria-label=\"Screen: Saved\">"));
    }
}
