//! The page of a navigation graph: who uses it, what each kind of user's menu shows, how to get to each page, the
//! goals, the pages, and every way to move; and its Mermaid diagram. The port of nav-page.ts.

use std::collections::HashSet;
use std::sync::LazyLock;

use delurio_core::js::{self, Value};
use delurio_core::nav::{
    Entry, Finding, Goalcheck, MenuRow, NavGraph, Page, PageKind, PersonaView, RepoCheck, Standing, path_to,
};
use regex::Regex;

use crate::html::CSS;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn code(s: &str) -> String {
    format!("<code>{}</code>", esc(s))
}

const NAV_CSS: &str = "
.persons { display: grid; gap: 12px; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); }
.person { border: 1px solid var(--line); border-radius: 10px; background: var(--surface); padding: 12px 14px; }
.person h3 { margin-bottom: 4px; } .person p { margin: 4px 0 0; } .person .stat { margin-top: 8px; font-size: 14px; }
.finding { margin: 12px 0; padding: 12px 16px; border: 1px solid var(--line); border-left: 5px solid var(--muted); border-radius: 10px; background: var(--surface); }
.finding.problem { border-left-color: var(--bad); } .finding p { margin: 0; } .finding ul { margin: 8px 0 0; padding-left: 20px; } .finding li { margin: 3px 0; }
.finding .tag { margin-right: 6px; }
table.matrix th, table.matrix td { padding: 6px 8px; } table.matrix th.persona { min-width: 92px; text-transform: none; letter-spacing: 0; font-size: 12px; }
table.matrix td.cell { text-align: center; white-space: nowrap; } td.cell .why { display: block; font-size: 11px; color: var(--muted); }
.c-ok { color: var(--ok); font-weight: 600; } .c-refused { color: var(--bad); font-weight: 600; } .c-placeholder { color: var(--muted); font-weight: 600; } .c-missing { color: var(--bad); font-weight: 600; } .c-hidden, .c-none { color: var(--line); }
tr.grp th { background: var(--accent-bg); color: var(--ink); text-transform: none; letter-spacing: 0; font-size: 13px; }
.path { display: block; font-size: 12px; color: var(--muted); max-width: 190px; white-space: normal; text-align: left; }
td.reach { vertical-align: top; } td.reach b { font-size: 15px; }
.legend-row { display: flex; flex-wrap: wrap; gap: 4px 18px; margin: 10px 0 0; font-size: 14px; color: var(--muted); }
ul.sitemap { list-style: none; margin: 0; padding: 0; } ul.sitemap li { padding: 6px 0; border-bottom: 1px solid var(--line); } ul.sitemap li:last-child { border-bottom: 0; }
ul.sitemap .route { margin-left: 8px; color: var(--muted); }
.checklist li.fail::before { content: \"✗\"; color: var(--bad); }
";

fn standing_word(s: Standing) -> &'static str {
    match s {
        Standing::Ok => "opens",
        Standing::Placeholder => "not built",
        Standing::Missing => "not found",
        Standing::Refused => "refuses",
        Standing::Unreachable => "no way",
    }
}

fn standing_mark(s: Standing) -> &'static str {
    match s {
        Standing::Ok => "✓",
        Standing::Placeholder => "◌",
        Standing::Missing => "✗",
        Standing::Refused => "⛔",
        Standing::Unreachable => "–",
    }
}

fn path_words(entries: &[&Entry]) -> String {
    entries.iter().filter(|e| !e.label.is_empty()).map(|e| e.label.as_str()).collect::<Vec<_>>().join(" → ")
}

/// a page's title, or nothing when the graph names a page it does not have (the TypeScript page failed there)
fn title_of(g: &NavGraph, id: &str) -> String {
    g.page(id).map(|p| p.title.clone()).unwrap_or_default()
}

// the way to a page in words: its labels from start, or from a page opened by its address (SPEC.md §6)
fn way_words(g: &NavGraph, v: &PersonaView<'_>, page: &str) -> String {
    let r = v.reach.get(page);
    let steps = if r.is_none() { String::new() } else { path_words(&path_to(&v.reach, page)) };
    let Some(by) = r.and_then(|r| r.by_address.as_deref()) else {
        return if steps.is_empty() { "automatic".to_string() } else { steps };
    };
    if by == page {
        return "by its address".to_string();
    }
    let then = if steps.is_empty() { String::new() } else { format!(", then {steps}") };
    format!("by the address of {}{then}", title_of(g, by))
}

fn menu_of(g: &NavGraph, frame: &str) -> String {
    g.frame(frame).menu().unwrap_or_else(|| frame.to_string())
}

fn landing_stats(g: &NavGraph, v: &PersonaView<'_>, rows: &[MenuRow<'_>], index: usize) -> String {
    let Some(landing) = &v.landing else { return String::new() };
    let Some(frame) = g.page(landing).map(|p| p.frame.clone()) else { return String::new() };
    if !g.menu_frames.contains(&frame) {
        return String::new();
    }
    let shown: Vec<&MenuRow<'_>> =
        rows.iter().filter(|r| r.frame == frame && r.cells.get(index).is_some_and(|c| c.shown)).collect();
    let menu = menu_of(g, &frame).to_lowercase();
    if shown.is_empty() {
        return format!("The {menu} has no entries.");
    }
    let by = |s: Standing| shown.iter().filter(|r| r.cells[index].standing == Some(s)).count();
    let mut parts = vec![format!("{} open", by(Standing::Ok))];
    if by(Standing::Refused) > 0 {
        parts.push(format!("{} refuse them", by(Standing::Refused)));
    }
    if by(Standing::Placeholder) > 0 {
        parts.push(format!("{} are not built", by(Standing::Placeholder)));
    }
    if by(Standing::Missing) > 0 {
        parts.push(format!("{} lead to a page that does not exist", by(Standing::Missing)));
    }
    let entries = if shown.len() == 1 { "entry" } else { "entries" };
    format!("The {menu} has {} {entries}: {}.", shown.len(), parts.join(", "))
}

fn finding_html(f: &Finding) -> String {
    let detail = if f.detail.is_empty() {
        String::new()
    } else {
        format!("<ul>{}</ul>", f.detail.iter().map(|d| format!("<li>{}</li>", esc(d))).collect::<String>())
    };
    let level = f.level.as_str();
    let tag = if level == "problem" { "bad" } else { "" };
    format!(
        "<section class=\"finding {level}\"><p><span class=\"tag {tag}\">{level}</span>{}</p>{detail}</section>",
        esc(&f.say)
    )
}

fn matrix(g: &NavGraph, views: &[PersonaView<'_>], rows: &[MenuRow<'_>]) -> String {
    let mut out: Vec<String> =
        vec!["<div class=\"scroll\"><table class=\"matrix\"><thead><tr><th>Menu entry</th>".into()];
    for v in views {
        out.push(format!("<th class=\"persona\">{}</th>", esc(&v.persona.title)));
    }
    out.push("</tr></thead><tbody>".into());
    for frame in &g.menu_frames {
        out.push(format!(
            "<tr class=\"grp\"><th colspan=\"{}\">{}</th></tr>",
            views.len() + 1,
            esc(&menu_of(g, frame))
        ));
        for r in rows.iter().filter(|x| &x.frame == frame) {
            let (title, route) = g.page(&r.entry.to).map(|t| (t.title.clone(), t.route.clone())).unwrap_or_default();
            let path = if title == r.entry.label { route } else { format!("{title} · {route}") };
            out.push(format!("<tr><td>{}<span class=\"path\">{}</span></td>", esc(&r.entry.label), esc(&path)));
            for (i, c) in r.cells.iter().enumerate() {
                if !r.frame_known.get(i).copied().unwrap_or(false) {
                    out.push("<td class=\"cell c-none\" title=\"this kind of user never has this menu\"></td>".into());
                } else if let (true, Some(s)) = (c.shown, c.standing) {
                    out.push(format!(
                        "<td class=\"cell c-{}\" title=\"{}\">{}<span class=\"why\">{}</span></td>",
                        s.as_str(),
                        standing_word(s),
                        standing_mark(s),
                        standing_word(s)
                    ));
                } else {
                    out.push("<td class=\"cell c-hidden\" title=\"not in this person’s menu\">–</td>".into());
                }
            }
            out.push("</tr>".into());
        }
    }
    out.push("</tbody></table></div>".into());
    let missing = if g.pages.values().any(|p| p.kind == PageKind::Missing) {
        "<span><b class=\"c-missing\">✗ not found</b> there is no such page</span>"
    } else {
        ""
    };
    out.push(format!("<p class=\"legend-row\"><span><b class=\"c-ok\">✓ opens</b> nothing in the application’s own code refuses it (the server can still say no)</span><span><b class=\"c-refused\">⛔ refuses</b> the entry is drawn, the page says no</span><span><b class=\"c-placeholder\">◌ not built</b> the page says it is not ready</span>{missing}<span>– not in this person’s menu</span><span>blank: this kind of user never has this menu</span></p>"));
    out.join("")
}

fn kind_tag(p: &Page) -> &'static str {
    match p.kind {
        PageKind::Placeholder => " <span class=\"tag\">not built</span>",
        PageKind::Missing => " <span class=\"tag\">does not exist</span>",
        _ => "",
    }
}

fn reach_table(g: &NavGraph, views: &[PersonaView<'_>]) -> String {
    let mut out: Vec<String> = vec!["<div class=\"scroll\"><table class=\"matrix\"><thead><tr><th>Page</th>".into()];
    for v in views {
        out.push(format!("<th class=\"persona\">{}</th>", esc(&v.persona.title)));
    }
    out.push("</tr></thead><tbody>".into());
    for frame in g.frames.keys() {
        let pages: Vec<&Page> = g.pages.values().filter(|p| &p.frame == frame && p.id != "start").collect();
        if pages.is_empty() {
            continue;
        }
        out.push(format!(
            "<tr class=\"grp\"><th colspan=\"{}\">{}</th></tr>",
            views.len() + 1,
            esc(&g.frame(frame).title_text())
        ));
        for p in pages {
            out.push(format!(
                "<tr><td>{}{}<span class=\"path\">{}</span></td>",
                esc(&p.title),
                kind_tag(p),
                esc(&p.route)
            ));
            for v in views {
                let st = v.standing.get(&p.id).copied();
                if st == Some(Standing::Unreachable) {
                    out.push("<td class=\"reach c-hidden\">–</td>".into());
                    continue;
                }
                let r = v.reach.get(&p.id);
                let mark = match st {
                    Some(Standing::Refused) => " <span class=\"c-refused\">⛔</span>",
                    Some(Standing::Placeholder) => " <span class=\"c-placeholder\">◌</span>",
                    Some(Standing::Missing) => " <span class=\"c-missing\">✗</span>",
                    _ => "",
                };
                let clicks = r.map_or(0.0, |r| r.clicks);
                let data = if r.is_some_and(|r| r.uses_data) { " †" } else { "" };
                out.push(format!(
                    "<td class=\"reach\"><b>{}</b>{mark}{data}<span class=\"path\">{}</span></td>",
                    js::number_to_string(clicks),
                    esc(&way_words(g, v, &p.id))
                ));
            }
            out.push("</tr>".into());
        }
    }
    out.push("</tbody></table></div>".into());
    let typed = if g.pages.values().any(|p| p.by_address) {
        "<span>by its address: opened by typing its address or by a link from outside the app, such as one in an e-mail, and counted from there</span>"
    } else {
        ""
    };
    out.push(format!("<p class=\"legend-row\"><span>number: fewest clicks from the moment the person signs in and “/” has sent them on</span><span>⛔ the page refuses them</span><span>◌ the page is not built</span><span>† needs an item in a list (a row to pick, a card to open)</span>{typed}<span>–: no way there</span></p>"));
    out.join("")
}

fn goals_table(g: &NavGraph, goals: &[Goalcheck<'_>]) -> String {
    if goals.is_empty() {
        return String::new();
    }
    let mut out: Vec<String> = vec!["<h2>Things people come to do</h2><div class=\"scroll\"><table><thead><tr><th>Goal</th><th>Who</th><th>Clicks</th><th>Result</th></tr></thead><tbody>".into()];
    for c in goals {
        let ok = c.outcome.as_str() == "ok";
        let route = g.page(&c.goal.page()).map(|p| p.route.clone()).unwrap_or_default();
        let clicks = c
            .clicks
            .map_or_else(|| "–".to_string(), |n| format!("{} of {}", js::number_to_string(n), c.goal.within_text()));
        let result = if ok { "ok".to_string() } else { esc(c.outcome.as_str()) };
        out.push(format!(
            "<tr><td>{}<span class=\"path\">{}</span></td><td>{}</td><td>{clicks}</td><td class=\"{}\">{result}</td></tr>",
            esc(&c.goal.say_text()),
            esc(&route),
            esc(&c.persona.title),
            if ok { "ok" } else { "bad" }
        ));
    }
    out.push("</tbody></table></div>".into());
    out.join("")
}

fn site_map(g: &NavGraph) -> String {
    let mut out: Vec<String> = Vec::new();
    for frame in g.frames.keys() {
        let pages: Vec<&Page> = g.pages.values().filter(|p| &p.frame == frame).collect();
        if pages.is_empty() {
            continue;
        }
        out.push(format!(
            "<h3 style=\"margin:16px 0 4px\">{}</h3><ul class=\"sitemap\">",
            esc(&g.frame(frame).title_text())
        ));
        for p in pages {
            let mut tags: Vec<String> = Vec::new();
            match p.kind {
                PageKind::Screen => {}
                PageKind::Placeholder => tags.push("not built".into()),
                PageKind::Missing => tags.push("does not exist".into()),
                k => tags.push(k.as_str().into()),
            }
            if let Some(o) = &p.opens_for {
                tags.push(format!("opens only when {o}"));
            }
            if p.by_address {
                tags.push("by its address".into());
            }
            let mut notes: Vec<String> = Vec::new();
            if let (Some(_), Some(refusal)) = (&p.opens_for, &p.refusal) {
                notes.push(format!("Otherwise {refusal}."));
            }
            if let Some(e) = &p.evidence {
                notes.push(e.clone());
            }
            out.push(format!(
                "<li>{}<span class=\"route\">{}</span> {}{}</li>",
                esc(&p.title),
                code(&p.route),
                tags.iter().map(|t| format!("<span class=\"tag\">{}</span>", esc(t))).collect::<Vec<_>>().join(" "),
                notes.iter().map(|n| format!("<br><span class=\"muted\">{}</span>", esc(n))).collect::<String>()
            ));
        }
        out.push("</ul>".into());
    }
    out.join("")
}

/// `(Array.isArray(v) ? v : [v]).join(', ')`: null and undefined join as nothing
fn joined(v: Option<&Value>) -> String {
    let one = |x: &Value| if x.is_null() { String::new() } else { js::to_js_string(x) };
    match v {
        None => String::new(),
        Some(Value::Array(items)) => items.iter().map(one).collect::<Vec<_>>().join(", "),
        Some(x) => one(x),
    }
}

fn reference_table(g: &NavGraph) -> String {
    let mut out: Vec<String> = vec!["<div class=\"scroll\" style=\"margin-top:8px\"><table><thead><tr><th>From</th><th>How</th><th>Label</th><th>To</th><th>Only when</th></tr></thead><tbody>".into()];
    for i in 0..g.specs.len() {
        let s = g.spec(i);
        let from = joined(s.from());
        let (title, route) =
            s.to().and_then(|t| g.page(t)).map(|t| (t.title.clone(), t.route.clone())).unwrap_or_default();
        let extra = [if s.data() { "needs an item in a list" } else { "" }, if s.back() { "goes back" } else { "" }]
            .into_iter()
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        let label = match s.get("label") {
            None => String::new(),
            Some(l) => {
                let key = js::to_js_string(l);
                match g.catalog.get(&key) {
                    Some(m) if !m.is_null() => js::to_js_string(m),
                    _ => key,
                }
            }
        };
        let when = s.get("when").map_or_else(String::new, |w| code(&js::to_js_string(w)));
        let extra =
            if extra.is_empty() { String::new() } else { format!(" <span class=\"muted\">{}</span>", esc(&extra)) };
        out.push(format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{} {}</td><td>{when}{extra}</td></tr>",
            code(&from),
            esc(&s.get("how").map_or_else(String::new, js::to_js_string)),
            esc(&label),
            esc(&title),
            code(&route)
        ));
    }
    out.push("</tbody></table></div>".into());
    out.join("")
}

fn guards_reference(g: &NavGraph) -> String {
    let mut out: Vec<String> = vec!["<dl class=\"guards\">".into()];
    for (n, expr) in g.guards.iter() {
        out.push(format!("<dt>{}</dt><dd>{}</dd>", code(n), code(&js::to_js_string(expr))));
    }
    out.push("</dl>".into());
    out.join("")
}

/// what the app's own files were checked against, when a checkout was given
pub struct RepoChecks<'a> {
    pub checks: &'a [RepoCheck],
    pub head: Option<&'a str>,
}

pub struct NavPageInput<'a, 'g> {
    pub g: &'g NavGraph,
    pub views: &'a [PersonaView<'g>],
    pub rows: &'a [MenuRow<'g>],
    pub goals: &'a [Goalcheck<'g>],
    pub found: &'a [Finding],
    pub repo: Option<RepoChecks<'a>>,
    pub warnings: &'a [String],
}

pub fn render_nav_page(p: &NavPageInput<'_, '_>) -> String {
    let g = p.g;
    let (views, rows, goals, found) = (p.views, p.rows, p.goals, p.found);
    let mut out: Vec<String> = Vec::new();
    let problems = found.iter().filter(|f| f.level.as_str() == "problem").count();
    out.push("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".into());
    out.push(format!("<title>{} navigation</title><style>{CSS}{NAV_CSS}</style></head><body><main>", esc(&g.title)));
    out.push(format!(
        "<p class=\"eyebrow\"><span class=\"tag\">{}</span> <span class=\"mono\">{}</span> · navigation graph</p>",
        esc(&g.status),
        esc(&g.id)
    ));
    out.push(format!("<h1>{}: where people go</h1>", esc(&g.title)));
    if !g.summary.is_empty() {
        out.push(format!("<p class=\"lede\">{}</p>", esc(&g.summary)));
    }
    let source = g.source();
    let read = match source.commit() {
        None => String::new(),
        Some(commit) => format!(" · read from {} at {}", esc(source.repo().unwrap_or("the app")), esc(commit)),
    };
    out.push(format!(
        "<p class=\"muted\">{} pages · {} ways to move · {} kinds of user{read} · wording: {}</p>",
        g.pages.len(),
        g.specs.len(),
        views.len(),
        esc(&g.locale)
    ));

    out.push("<h2>What stands out</h2>".into());
    out.push(if problems == 0 {
        "<p>Nothing blocks anyone.</p>".to_string()
    } else {
        let notes = found.len() - problems;
        format!(
            "<p>{problems} problem{} and {notes} note{}. Problems send a person somewhere that does not work; notes are about words.</p>",
            if problems == 1 { "" } else { "s" },
            if notes == 1 { "" } else { "s" }
        )
    });
    for f in found {
        out.push(finding_html(f));
    }

    out.push("<h2>Who uses it</h2><div class=\"persons\">".into());
    for (i, v) in views.iter().enumerate() {
        let landing = v.landing.as_deref().and_then(|l| g.page(l));
        out.push(format!(
            "<div class=\"person\"><h3>{}</h3><p class=\"muted\">{}</p>",
            esc(&v.persona.title),
            esc(&v.persona.say)
        ));
        let (title, route) = match landing {
            None => ("–".to_string(), String::new()),
            Some(l) => (l.title.clone(), format!(" {}", code(&l.route))),
        };
        let built = if landing.is_some_and(|l| l.kind == PageKind::Placeholder) {
            " <span class=\"tag\">not built</span>"
        } else {
            ""
        };
        out.push(format!("<p class=\"stat\">Lands on <b>{}</b>{route}{built}.</p>", esc(&title)));
        let line = landing_stats(g, v, rows, i);
        if !line.is_empty() {
            out.push(format!("<p class=\"stat\">{}</p>", esc(&line)));
        }
        out.push("</div>".into());
    }
    out.push("</div>".into());

    out.push("<h2>What the menu shows each kind of user</h2>".into());
    let menus: Vec<String> = g.menu_frames.iter().map(|f| menu_of(g, f).to_lowercase()).collect();
    let several =
        if menus.len() > 1 { format!(" The {} are different menus.", menus.join(" and the ")) } else { String::new() };
    out.push(format!("<p class=\"muted\" style=\"margin-top:-4px\">One row per menu entry.{several}</p>"));
    out.push(matrix(g, views, rows));

    out.push("<h2>How to get to each page</h2>".into());
    out.push(reach_table(g, views));

    out.push(goals_table(g, goals));

    out.push("<h2>The pages</h2>".into());
    out.push(site_map(g));

    if let Some(repo) = &p.repo {
        let bad: Vec<&RepoCheck> = repo.checks.iter().filter(|c| !c.ok).collect();
        out.push("<h2>Checked against the app</h2>".into());
        let at = repo.head.map_or_else(String::new, |h| format!(" in the checkout at {}", esc(h)));
        let stale = match (repo.head, source.commit()) {
            (Some(head), Some(commit)) if !head.starts_with(commit) && !commit.starts_with(head) => {
                format!(" <span class=\"bad\">(the graph was read at {})</span>", esc(commit))
            }
            _ => String::new(),
        };
        out.push(format!(
            "<p>{} of {} checks hold{at}{stale}. The checks read the route files, the menu registry, the permission table and the translation file; they do not run the app.</p>",
            repo.checks.len() - bad.len(),
            repo.checks.len()
        ));
        let line = |c: &RepoCheck| {
            let detail = if c.detail.is_empty() {
                String::new()
            } else {
                format!(" <span class=\"muted\">— {}</span>", esc(&c.detail))
            };
            format!("<li class=\"{}\">{}{detail}</li>", if c.ok { "" } else { "fail" }, esc(&c.what))
        };
        if !bad.is_empty() {
            out.push(format!(
                "<ul class=\"checks checklist\">{}</ul>",
                bad.iter().map(|c| line(c)).collect::<String>()
            ));
        }
        out.push(format!(
            "<details><summary>{} checks that hold</summary><ul class=\"checks checklist\" style=\"margin-top:8px\">{}</ul></details>",
            repo.checks.len() - bad.len(),
            repo.checks.iter().filter(|c| c.ok).map(line).collect::<String>()
        ));
    }

    out.push("<h2>Not modeled</h2><ul>".into());
    for n in &g.not_modeled {
        out.push(format!("<li>{}</li>", esc(n)));
    }
    out.push("</ul>".into());

    out.push("<h2>Reference</h2>".into());
    out.push(format!(
        "<details><summary>Every way to move ({})</summary>{}</details>",
        g.specs.len(),
        reference_table(g)
    ));
    out.push(format!("<details><summary>Named rules</summary>{}</details>", guards_reference(g)));
    out.push("<footer>delurio graph-viz · generated from nav.yaml and the catalog · no AI, no network: the same input gives the same bytes.</footer>".into());
    out.push("</main></body></html>".into());
    format!("{}\n", out.join("\n"))
}

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("the pattern compiles"));
static PARENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r" \([^\n\r\u{2028}\u{2029}]*$").expect("the pattern compiles"));

fn clean(s: &str) -> String {
    let s = s.replace('"', "'").replace(['<', '>', '|'], " ");
    js::trim(&SPACES.replace_all(&s, " ")).to_string()
}

/// Mermaid flowchart for PRs and READMEs: one menu hub per frame; links, buttons and landing rules as arrows.
pub fn render_nav_mermaid(g: &NavGraph) -> String {
    let mut lines: Vec<String> =
        vec!["---".into(), format!("title: {} navigation", clean(&g.title)), "---".into(), "flowchart LR".into()];
    let hub_of = |p: &Page| -> Option<String> {
        if g.menu_frames.contains(&p.frame) { Some(format!("hub_{}", p.frame)) } else { None }
    };
    for frame in g.frames.keys() {
        let pages: Vec<&Page> = g.pages.values().filter(|p| &p.frame == frame).collect();
        if pages.is_empty() {
            continue;
        }
        let title = g.frame(frame).title_text();
        lines.push(format!("  subgraph F_{frame}[\"{}\"]", clean(&PARENS.replace(&title, ""))));
        if g.menu_frames.contains(frame) {
            lines.push(format!("    hub_{frame}((\"menu\"))"));
        }
        for p in pages {
            lines.push(format!("    {}[\"{}<br/>{}\"]", p.id, clean(&p.title), clean(&p.route)));
        }
        lines.push("  end".into());
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut edge = |lines: &mut Vec<String>, from: &str, to: &str, label: &str, dotted: bool| {
        if !seen.insert(format!("{from}|{to}|{label}|{dotted}")) {
            return;
        }
        let label = if label.is_empty() { String::new() } else { format!("|\"{}\"|", clean(label)) };
        lines.push(format!("  {from} {}{label} {to}", if dotted { "-.->" } else { "-->" }));
    };
    for e in &g.entries {
        let from = g.page(&e.from);
        let hub = from.and_then(hub_of);
        match e.how.as_str() {
            "menu" => {
                if let Some(h) = &hub {
                    edge(&mut lines, h, &e.to, "", false);
                } else if from.is_some_and(|f| g.frame(&f.frame).shows().is_some()) {
                    continue; // drawn through the hubs
                }
            }
            "picker" | "user" => {
                if let Some(h) = &hub {
                    edge(&mut lines, h, &e.to, &e.label, true);
                }
            }
            how => {
                let auto = how == "auto";
                edge(&mut lines, &e.from, &e.to, if auto { "automatic" } else { &e.label }, auto);
            }
        }
    }
    lines.push("  classDef placeholder stroke-dasharray:4 3,fill:#f5f5f4,stroke:#78716c".into());
    lines.push("  classDef hub fill:#eef3ff,stroke:#1d4ed8".into());
    let placeholders: Vec<&str> =
        g.pages.values().filter(|p| p.kind == PageKind::Placeholder).map(|p| p.id.as_str()).collect();
    if !placeholders.is_empty() {
        lines.push(format!("  class {} placeholder", placeholders.join(",")));
    }
    let gone: Vec<&str> = g.pages.values().filter(|p| p.kind == PageKind::Missing).map(|p| p.id.as_str()).collect();
    if !gone.is_empty() {
        lines.push("  classDef missing stroke-dasharray:2 3,fill:#fff5f5,stroke:#b91c1c".into());
        lines.push(format!("  class {} missing", gone.join(",")));
    }
    if !g.menu_frames.is_empty() {
        lines.push(format!(
            "  class {} hub",
            g.menu_frames.iter().map(|f| format!("hub_{f}")).collect::<Vec<_>>().join(",")
        ));
    }
    format!("{}\n", lines.join("\n"))
}
