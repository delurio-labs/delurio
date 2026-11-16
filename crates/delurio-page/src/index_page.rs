//! The index of `page` on a folder of graphs: one row per graph, grouped by the folder it sits in, with what its check
//! found and a link to its page. The port of index-page.ts.

use indexmap::IndexMap;

use crate::html::CSS;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn count(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// one graph of the folder, as its page was written
#[derive(Debug, Clone, PartialEq)]
pub struct IndexEntry {
    pub path: String,       // the graph's folder relative to the indexed one, with forward slashes
    pub kind: &'static str, // flow or nav
    pub id: Option<String>, // None when the graph cannot be loaded
    pub title: String,
    pub status: Option<String>,
    pub holds: usize, // the flow's guarantees that hold
    pub total: usize, // the flow's guarantees; for a navigation graph, its problems
    pub warnings: Vec<String>,
    pub error: Option<String>, // why the graph cannot be loaded
}

const INDEX_CSS: &str = "
.groups h2 { font-size: 18px; margin: 32px 0 10px; } .groups table { table-layout: fixed; } .groups col.check { width: 9em; }
.groups a { color: var(--accent); } .groups td { overflow-wrap: anywhere; }
.meta { margin-top: 2px; } .meta .tag { margin-left: 6px; } .warn { margin-top: 6px; color: var(--muted); font-size: 14px; } .warn ul { margin: 4px 0 0; padding-left: 18px; }
";

fn state_cell(e: &IndexEntry) -> String {
    if e.error.is_some() {
        return "<span class=\"bad\">cannot be loaded</span>".to_string();
    }
    if e.kind == "nav" {
        let class = if e.total == 0 { "ok" } else { "bad" };
        return format!("<span class=\"{class}\">{}</span>", esc(&count(e.total, "problem")));
    }
    if e.total == 0 {
        return "<span class=\"muted\">no guarantees</span>".to_string();
    }
    if e.holds == e.total {
        format!("<span class=\"ok\">{}/{} hold</span>", e.holds, e.total)
    } else {
        format!("<span class=\"bad\">{} of {} broken</span>", e.total - e.holds, e.total)
    }
}

/// root: the indexed folder's name, which also heads the graphs that sit right in it
pub fn render_index_page(root: &str, entries: &[IndexEntry]) -> String {
    let mut out: Vec<String> = Vec::new();
    let failing = entries
        .iter()
        .filter(|e| e.error.is_some() || if e.kind == "flow" { e.holds < e.total } else { e.total > 0 })
        .count();
    let flows = entries.iter().filter(|e| e.kind == "flow").count();
    let navs = entries.len() - flows;
    out.push("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into());
    out.push(format!("<title>All graphs</title><style>{CSS}{INDEX_CSS}</style></head><body><main>"));
    out.push(format!(
        "<p class=\"eyebrow\"><span class=\"mono\">{}/</span> · delurio-graph/0</p><h1>All graphs</h1>",
        esc(root)
    ));
    let kinds = [
        if flows > 0 { count(flows, "flow") } else { String::new() },
        if navs > 0 { count(navs, "navigation graph") } else { String::new() },
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" and ");
    let state = if failing == 0 {
        "Every guarantee holds and nothing blocks anyone.".to_string()
    } else {
        format!(
            "{} need{} a look: marked in red below.",
            esc(&count(failing, "graph")),
            if failing == 1 { "s" } else { "" }
        )
    };
    out.push(format!("<p class=\"lede\">{}. {state}</p>", esc(&kinds)));
    let mut groups: IndexMap<String, Vec<&IndexEntry>> = IndexMap::new();
    for e in entries {
        let folder = match e.path.rfind('/') {
            None => root.to_string(),
            Some(slash) => e.path[..slash].to_string(),
        };
        groups.entry(folder).or_default().push(e);
    }
    out.push("<div class=\"groups\">".into());
    for (folder, rows) in &groups {
        out.push(format!("<h2>{}</h2><div class=\"scroll\"><table><colgroup><col><col class=\"check\"></colgroup><thead><tr><th>Graph</th><th>Check</th></tr></thead><tbody>", esc(folder)));
        for e in rows {
            let name = if e.error.is_some() {
                esc(&e.title)
            } else {
                format!("<a href=\"{}/index.html\">{}</a>", esc(&e.path), esc(&e.title))
            };
            let meta = match &e.id {
                None => String::new(),
                Some(id) => {
                    let status = e
                        .status
                        .as_ref()
                        .map_or_else(String::new, |s| format!("<span class=\"tag\">{}</span>", esc(s)));
                    format!("<div class=\"meta\"><span class=\"mono muted\">{}</span>{status}</div>", esc(id))
                }
            };
            let warn = match &e.error {
                Some(err) => format!("<div class=\"warn\">{}</div>", esc(err)),
                None if e.warnings.is_empty() => String::new(),
                None => format!(
                    "<div class=\"warn\">{}<ul>{}</ul></div>",
                    esc(&count(e.warnings.len(), "warning")),
                    e.warnings.iter().map(|w| format!("<li>{}</li>", esc(w))).collect::<String>()
                ),
            };
            out.push(format!("<tr><td>{name}{meta}{warn}</td><td class=\"state\">{}</td></tr>", state_cell(e)));
        }
        out.push("</tbody></table></div>".into());
    }
    out.push("</div></main></body></html>".into());
    out.join("\n")
}
