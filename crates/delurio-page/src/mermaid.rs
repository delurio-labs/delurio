//! A flow as Mermaid stateDiagram-v2 text, for PRs and READMEs. The port of mermaid.ts.

use std::collections::{HashMap, HashSet};

use delurio_core::edges::{NodeKind, collect_edges, exit_line, kind_of};
use delurio_core::graph::{Graph, given, title_of};
use delurio_core::js::{self};
use delurio_core::model::Exploration;

// Mermaid ends a statement at `;` even inside a transition's label: the rest of the label became stray states,
// or, with a `-` or `{` after it, a parse error. A label keeps its words with a comma in its place.
fn clean(s: &str) -> String {
    s.replace('"', "'").replace(':', " -").replace(';', ",")
}

// Mermaid's ids, which take no dot: an exit named `<error>.<cause>` is written `<error>__<cause>`
fn mermaid_ids(names: &[&str]) -> HashMap<String, String> {
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = names.iter().filter(|n| !n.contains('.')).map(|n| n.to_string()).collect();
    for n in names {
        if !n.contains('.') {
            ids.insert(n.to_string(), n.to_string());
            continue;
        }
        let base = n.replace('.', "__");
        let mut id = base.clone();
        let mut i = 2;
        while taken.contains(&id) {
            id = format!("{base}_{i}");
            i += 1;
        }
        taken.insert(id.clone());
        ids.insert(n.to_string(), id);
    }
    ids
}

// Mermaid stateDiagram-v2 text, for PRs and READMEs. The full page is html.rs.
pub fn render_mermaid(g: &Graph, ex: &Exploration) -> String {
    let flow = g.flow();
    let title = given(&g.flow, "title").map_or_else(|| flow.id_text(), js::to_js_string);
    let mut lines: Vec<String> = vec![
        "---".into(),
        format!("title: {}", clean(&title)),
        "---".into(),
        "stateDiagram-v2".into(),
        "  direction TB".into(),
    ];
    let names = flow.state_names();
    let ids = mermaid_ids(&names);
    let id = |name: &str| ids.get(name).cloned().unwrap_or_else(|| name.to_string());
    for (name, st) in flow.states() {
        let detail = if st.is_final() && !st.leaves_page() { format!(" ({})", exit_line(st)) } else { String::new() };
        lines.push(format!("  state \"{}{}\" as {}", clean(&title_of(flow, name)), clean(&detail), id(name)));
    }
    let initial = g.flow.get("initial").map_or_else(|| "undefined".to_string(), js::to_js_string);
    lines.push(format!("  [*] --> {}", id(&initial)));
    for e in collect_edges(g, ex) {
        // "never taken" belongs in the label: after an arrow with no `:` Mermaid read its words as two more states
        let said = e.labels.iter().map(|l| clean(l)).collect::<Vec<_>>().join(", ");
        let label = if e.unused { js::trim(&format!("{said} (never taken)")).to_string() } else { said };
        let tail = if label.is_empty() { String::new() } else { format!(": {label}") };
        lines.push(format!("  {} --> {}{tail}", id(&e.from), id(&e.to)));
    }
    lines.push("  classDef server fill:#eeeeec,stroke:#6b6560,stroke-dasharray:4 3".into());
    lines.push("  classDef screen fill:#ffffff,stroke:#1c1917,stroke-width:2px".into());
    lines.push("  classDef subflow fill:#ffffff,stroke:#1d4ed8,stroke-width:2px".into());
    lines.push("  classDef exit fill:#ffffff,stroke:#6b6560,stroke-width:2px".into());
    for (kind, word) in [
        (NodeKind::Server, "server"),
        (NodeKind::Screen, "screen"),
        (NodeKind::Subflow, "subflow"),
        (NodeKind::Exit, "exit"),
    ] {
        let members: Vec<String> = names.iter().filter(|n| kind_of(g, n) == kind).map(|n| id(n)).collect();
        if !members.is_empty() {
            lines.push(format!("  class {} {word}", members.join(",")));
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_without_dots() {
        // as mermaidIds of mermaid.ts gives them: a dotted name whose plain form is taken gets the next free suffix
        let ids = mermaid_ids(&["a.b", "a__b", "a__b_2", "c.d", "e"]);
        assert_eq!(ids["a.b"], "a__b_3");
        assert_eq!(ids["c.d"], "c__d");
        assert_eq!(ids["e"], "e");
    }

    #[test]
    fn labels_lose_what_ends_a_statement() {
        assert_eq!(clean("say \"x\": a; b"), "say 'x' - a, b");
    }
}
