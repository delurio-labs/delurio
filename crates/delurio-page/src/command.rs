//! `delurio page`: the page of a graph, or of every graph under a folder and their index, and `--watch` / `--serve`.
//! The port of the page half of cli.ts (navPage, flowPage, treePages, writePages and the page arguments).
//!
//! The pages of a folder are written on several threads, each graph whole on one (its model, its two layouts, its
//! files), and what each says is printed in the folder's order once the graphs before it are done: the lines and the
//! files are those of one graph after the other. One thread per core, at most 8; `DELURIO_PAGE_THREADS=<n>` sets
//! another number (1: one graph after the other on one thread), for a machine with little memory to spare.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use delurio_core::changes::{check_changes, flow_domain};
use delurio_core::check::{CHECK_USAGE, count, graph_dirs};
use delurio_core::diff::DIFF_USAGE;
use delurio_core::error::Error;
use delurio_core::graph::{given, humanize, load, read_error};
use delurio_core::js;
use delurio_core::kit::resolve_kit;
use delurio_core::model::{Model, check_assertions, explore, final_paths};
use delurio_core::nav::{Level, check_goals, findings, load_nav, menu_rows, view_of};
use delurio_core::nodepath::{basename, join2, relative, resolve1};
use delurio_core::starts::check_starts;
use delurio_core::story::{main_story_index, narrate, steps_by_events, steps_to};
use delurio_core::wireframe::render_screens;

use crate::html::{PageInput, render_page};
use crate::index_page::{IndexEntry, render_index_page};
use crate::mermaid::render_mermaid;
use crate::nav_page::{NavPageInput, render_nav_mermaid, render_nav_page};
use crate::spine::{View, render_spine};
use crate::watch::{Rendered, watch_pages};

/// the usage lines of `page`
pub const PAGE_USAGE: &str = "  delurio page <graph dir | graphs folder> [--out <dir>] [--kit <kit>] [--locale <code>] [--strict] [--watch] [--serve [--port <n>]]
    writes the graph's page: <out>/index.html and its diagram (default out: <graph dir>/viz); on a folder of graphs,
    the page of each at <out>/<its path> and an index of them all at <out>/index.html (default out: <folder>/viz);
    --watch writes them again on every change under the folder; --serve also shows them at http://127.0.0.1:<port>/
    (default 4600) and reloads the open page after each write";

/// the usage of the whole command line, as cli.ts writes it
pub fn usage() -> String {
    ["usage:", CHECK_USAGE, PAGE_USAGE, &DIFF_USAGE.replace("node src/cli.ts", "delurio")]
        .join("\n")
        .replace("node src/cli.ts", "delurio")
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == name)?;
    args.get(i + 1).map(String::as_str)
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// what a page says, in order: `console.log` lines (stdout) and `console.error` lines (stderr), printed when it is
/// its turn
#[derive(Default)]
struct Said(Vec<(bool, String)>);

impl Said {
    fn out(&mut self, line: String) {
        self.0.push((false, line));
    }
    fn err(&mut self, line: String) {
        self.0.push((true, line));
    }
    fn print(self) {
        use std::io::Write;
        let (stdout, stderr) = (std::io::stdout(), std::io::stderr());
        for (to_err, line) in self.0 {
            // a closed pipe drops the rest, as Node's console does
            let _ = if to_err { writeln!(stderr.lock(), "{line}") } else { writeln!(stdout.lock(), "{line}") };
        }
    }
}

struct Paged {
    code: i32,
    entry: IndexEntry,
}

/// `mkdirSync(out, { recursive: true })`: a folder that cannot be made fails the write after it
fn make_dir(out: &str) {
    let _ = std::fs::create_dir_all(out);
}

/// `writeFileSync(path, text)`; a file that cannot be written (the TS crashed on it) is a load error of the graph
fn write(path: &str, text: &str) -> Result<(), Error> {
    std::fs::write(path, text).map_err(|e| Error::graph(read_error(&e, path), None))
}

fn nav_page(dir: &str, out: &str, args: &[String], said: &mut Said) -> Result<Paged, Error> {
    let loaded = load_nav(dir, flag(args, "--locale"))?;
    let (g, warnings) = (&loaded.g, &loaded.warnings);
    let views = g.personas.iter().map(|p| view_of(g, p)).collect::<Result<Vec<_>, _>>()?;
    let rows = menu_rows(g, &views)?;
    let goals = check_goals(g, &views);
    let found = findings(g, &views, &goals)?;
    make_dir(out);
    let page = render_nav_page(&NavPageInput {
        g,
        views: &views,
        rows: &rows,
        goals: &goals,
        found: &found,
        repo: None,
        warnings,
    });
    write(&join2(out, "index.html"), &page)?;
    write(&join2(out, "nav.mmd"), &render_nav_mermaid(g))?;
    let problems = found.iter().filter(|f| f.level == Level::Problem).count();
    said.out(format!(
        "{}: {}, {} to move, {} {} of user; {}, {}",
        g.id,
        count(g.pages.len(), "page"),
        count(g.specs.len(), "way"),
        g.personas.len(),
        if g.personas.len() == 1 { "kind" } else { "kinds" },
        count(problems, "problem"),
        count(found.len() - problems, "note")
    ));
    for w in warnings {
        said.err(format!("warning: {w}"));
    }
    Ok(Paged {
        code: if has(args, "--strict") && (!warnings.is_empty() || problems > 0) { 2 } else { 0 },
        entry: IndexEntry {
            path: String::new(),
            kind: "nav",
            id: Some(g.id.clone()),
            title: format!("{}: where people go", g.title),
            status: Some(g.status.clone()),
            holds: 0,
            total: problems,
            warnings: warnings.clone(),
            error: None,
        },
    })
}

fn flow_page(dir: &str, out: &str, args: &[String], said: &mut Said) -> Result<Paged, Error> {
    let warnings: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let warn = |m: &str| {
        let mut w = warnings.borrow_mut();
        if !w.iter().any(|x| x == m) {
            w.push(m.to_string());
        }
    };
    let g = load(dir)?;
    let kit = resolve_kit(dir, g.ui().kit(), flag(args, "--kit"))?;
    let model = Model::new(g.flow.clone());
    let ex = explore(&model, None, None)?;
    let assertions = check_assertions(&model, &ex)?;
    let finals = final_paths(&model, &ex);
    let main_index = main_story_index(&g, &ex);
    let declared = match g.flow().main() {
        None => None,
        Some(m) => steps_by_events(&model, m)?,
    };
    if g.flow().main().is_some() && declared.is_none() {
        warn("the declared main story cannot be walked: an event is not possible where it is sent");
    }
    let main_steps = match declared {
        Some(steps) => Some(steps),
        None => main_index.map(|i| steps_to(&model, &ex, i)).transpose()?,
    };
    let main = main_steps.as_ref().map(|s| narrate(&g, &model, s));
    let spine_main = render_spine(&g, &ex, View::Main, main_steps.as_deref())?;
    let spine_full = render_spine(&g, &ex, View::Full, main_steps.as_deref())?;
    let screens = render_screens(&g, &mut |m| warn(m), kit.as_ref());
    let hand_overs = check_starts(&g, &model, &ex)?;
    for h in &hand_overs {
        for p in &h.problems {
            warn(&format!("{} starts {}: {p}", h.from, h.flow));
        }
    }
    // the domain is looked up once: `checkChanges(g)` and the page each looked it up, and found the same
    let domain = flow_domain(&g, None);
    for p in check_changes(&g, || domain.clone()).map(|c| c.problems).unwrap_or_default() {
        warn(&p);
    }
    make_dir(out);
    let page = render_page(&PageInput {
        g: &g,
        m: &model,
        ex: &ex,
        assertions: &assertions,
        finals: &finals,
        main: main.as_ref(),
        spine_main: &spine_main,
        spine_full: &spine_full,
        screens: &screens,
        kit: kit.as_ref(),
        hand_overs: &hand_overs,
        domain: domain.as_ref(),
    })?;
    write(&join2(out, "index.html"), &page)?;
    write(&join2(out, "flow.mmd"), &render_mermaid(&g, &ex))?;
    let violated: Vec<&str> = assertions.iter().filter(|a| a.violation.is_some()).map(|a| a.id.as_str()).collect();
    let id = g.flow().id_text();
    said.out(format!(
        "{id}: {}, {} from {}, {}/{} assertions hold",
        count(g.flow().state_names().len(), "state"),
        count(ex.configs.len(), "configuration"),
        count(ex.starts, "start"),
        assertions.len() - violated.len(),
        assertions.len()
    ));
    for a in &violated {
        said.out(format!("  violated: {a}"));
    }
    let warnings = warnings.into_inner();
    for w in &warnings {
        said.err(format!("warning: {w}"));
    }
    Ok(Paged {
        code: if has(args, "--strict") && (!violated.is_empty() || !warnings.is_empty()) { 2 } else { 0 },
        entry: IndexEntry {
            path: String::new(),
            kind: "flow",
            id: Some(id),
            title: given(&g.flow, "title").map_or_else(|| humanize(&g.flow().id_text()), js::to_js_string),
            status: given(&g.flow, "status").map(js::to_js_string),
            holds: assertions.len() - violated.len(),
            total: assertions.len(),
            warnings,
            error: None,
        },
    })
}

fn exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
}

fn is_graph(dir: &str) -> bool {
    exists(&join2(dir, "flow.yaml")) || exists(&join2(dir, "nav.yaml"))
}

struct Written {
    code: i32,
    message: String, // why a graph could not be loaded, when one could not
}

/// `err.message.split('\n')[0].replace(/:$/, '')`: the first line of a load error, without a colon at its end
fn said_of(err: &Error) -> String {
    let first = err.first_line();
    first.strip_suffix(':').unwrap_or(first).to_string()
}

/// how many graphs are written at once: one per core, at most 8 (each holds its model and its own elkjs), or
/// `DELURIO_PAGE_THREADS`
fn jobs(graphs: usize) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let cores = std::env::var("DELURIO_PAGE_THREADS").ok().and_then(|n| n.parse().ok()).unwrap_or(cores);
    cores.clamp(1, 8).min(graphs.max(1))
}

// the stack a page is written on: elkjs may recurse deeply in QuickJS, whose own limit is 64 MB (elk.rs)
const STACK: usize = 256 * 1024 * 1024;

/// one graph of a folder: its page written, and what it says
fn one_page(dir: &str, root: &str, out: &str, args: &[String]) -> (String, bool, Said, Result<Paged, Error>) {
    let path = relative(root, dir);
    let nav = !exists(&join2(dir, "flow.yaml"));
    let target = join2(out, &path);
    let mut said = Said::default();
    let paged = if nav { nav_page(dir, &target, args, &mut said) } else { flow_page(dir, &target, args, &mut said) };
    (path, nav, said, paged)
}

/// every graph of the folder written, each on a thread of its own, `each` given them in the folder's order
fn each_page(
    dirs: &[String],
    root: &str,
    out: &str,
    args: &[String],
    mut each: impl FnMut(String, bool, Said, Result<Paged, Error>),
) {
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|s| {
        for _ in 0..jobs(dirs.len()) {
            let tx = tx.clone();
            let next = &next;
            let spawned = std::thread::Builder::new().stack_size(STACK).spawn_scoped(s, move || {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(dir) = dirs.get(i) else { break };
                    if tx.send((i, one_page(dir, root, out, args))).is_err() {
                        break;
                    }
                }
            });
            if spawned.is_err() {
                break;
            }
        }
        drop(tx);
        let mut done = BTreeMap::new();
        let mut want = 0;
        for (i, page) in rx {
            done.insert(i, page);
            while let Some((path, nav, said, paged)) = done.remove(&want) {
                each(path, nav, said, paged);
                want += 1;
            }
        }
        // a thread that could not be started leaves its graphs to this one
        while want < dirs.len() {
            let (path, nav, said, paged) = one_page(&dirs[want], root, out, args);
            each(path, nav, said, paged);
            want += 1;
        }
    });
}

// every graph under the folder, each into <out>/<its path>, and the index of them all. A graph that cannot be loaded
// is listed as such and the rest are still written: exit 1 when one cannot be loaded, else 2 under --strict when one
// would fail on its own, else 0.
fn tree_pages(root: &str, out: &str, args: &[String]) -> Written {
    let dirs = graph_dirs(root);
    if dirs.is_empty() {
        let message =
            format!("{root} is not a graph and holds none: there is no flow.yaml or nav.yaml in it or under it");
        eprintln!("error: {message}");
        return Written { code: 1, message };
    }
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut broken = false;
    let mut failing = false;
    each_page(&dirs, root, out, args, |path, nav, mut said, paged| {
        match paged {
            Ok(Paged { code, mut entry }) => {
                entry.path = path;
                entries.push(entry);
                failing |= code != 0;
            }
            Err(err) => {
                broken = true;
                let message = said_of(&err);
                said.err(format!("error: {path}: {message}"));
                entries.push(IndexEntry {
                    path: path.clone(),
                    kind: if nav { "nav" } else { "flow" },
                    id: None,
                    title: path,
                    status: None,
                    holds: 0,
                    total: 0,
                    warnings: Vec::new(),
                    error: Some(message),
                });
            }
        }
        said.print();
    });
    make_dir(out);
    let index = join2(out, "index.html");
    if let Err(err) = write(&index, &render_index_page(&basename(&resolve1(root)), &entries)) {
        eprintln!("error: {}", err.message);
        return Written { code: 1, message: err.message };
    }
    let written = entries.iter().filter(|e| e.error.is_none()).count();
    println!("wrote {index} and the pages of {} under {out}", count(written, "graph"));
    let first_error = entries.iter().find(|e| e.error.is_some());
    Written {
        code: if broken {
            1
        } else if failing {
            2
        } else {
            0
        },
        message: first_error.map_or_else(String::new, |e| format!("{}: {}", e.path, e.error.as_deref().unwrap_or(""))),
    }
}

// one graph's page, or the pages of every graph under the folder
fn write_pages(dir: &str, out: &str, args: &[String]) -> Written {
    if !is_graph(dir) {
        return tree_pages(dir, out, args);
    }
    let flow = exists(&join2(dir, "flow.yaml"));
    let mut said = Said::default();
    let paged =
        on_big_stack(|| if flow { flow_page(dir, out, args, &mut said) } else { nav_page(dir, out, args, &mut said) });
    said.print();
    match paged {
        Ok(Paged { code, .. }) => {
            println!(
                "wrote {} and {}",
                join2(out, "index.html"),
                join2(out, if flow { "flow.mmd" } else { "nav.mmd" })
            );
            Written { code, message: String::new() }
        }
        Err(err) => {
            let message = said_of(&err);
            eprintln!("error: {message}");
            Written { code: 1, message }
        }
    }
}

/// `f` run on a thread with the stack a layout may need
fn on_big_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| match std::thread::Builder::new().stack_size(STACK).spawn_scoped(s, f) {
        Ok(h) => h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)),
        Err(e) => {
            eprintln!("error: no thread can be started to write the page: {e}");
            std::process::exit(1)
        }
    })
}

/// `Number(s)` of what `--port` is given: blank is 0, a decimal, `Infinity` or a 0x / 0o / 0b integer, else NaN
fn number_of(s: &str) -> f64 {
    let t = js::trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |digits: &str, radix: u32| {
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return f64::NAN;
        }
        digits.chars().fold(0.0, |n, c| n * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)))
    };
    match t.get(..2) {
        Some("0x" | "0X") => return radix(&t[2..], 16),
        Some("0o" | "0O") => return radix(&t[2..], 8),
        Some("0b" | "0B") => return radix(&t[2..], 2),
        _ => {}
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if !t.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `delurio page <args>`
pub fn page_main(rest: &[String]) -> i32 {
    let with_value = ["--out", "--kit", "--locale", "--port"];
    let dir = rest
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && !(*i > 0 && with_value.contains(&rest[i - 1].as_str())))
        .map(|(_, a)| a.as_str());
    let Some(dir) = dir else {
        eprintln!("{}", usage());
        return 1;
    };
    if !std::path::Path::new(dir).is_dir() {
        eprintln!("error: {dir} is not a folder");
        return 1;
    }
    let out = resolve1(&flag(rest, "--out").map_or_else(|| join2(dir, "viz"), str::to_string));
    let first = write_pages(dir, &out, rest);
    let serve = has(rest, "--serve");
    if !serve && !has(rest, "--watch") {
        return first.code;
    }
    let port = flag(rest, "--port").map_or(4600.0, number_of);
    if port.fract() != 0.0 || !(1.0..=65535.0).contains(&port) {
        eprintln!("error: --port is a port number, not {}", flag(rest, "--port").unwrap_or("undefined"));
        return 1;
    }
    watch_pages(
        dir,
        &out,
        || {
            let again = write_pages(dir, &out, rest);
            Rendered { ok: again.code != 1, message: again.message }
        },
        if serve { Some(port as u16) } else { None },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_as_number_reads_it() {
        // node -e "for (const s of ['4600', ' 80 ', '0x10', '1e3', '', 'abc', '12.5', '-1']) console.log(Number(s))"
        assert_eq!(number_of("4600"), 4600.0);
        assert_eq!(number_of(" 80 "), 80.0);
        assert_eq!(number_of("0x10"), 16.0);
        assert_eq!(number_of("1e3"), 1000.0);
        assert_eq!(number_of(""), 0.0);
        assert!(number_of("abc").is_nan());
        assert_eq!(number_of("12.5"), 12.5);
        assert_eq!(number_of("-1"), -1.0);
    }

    #[test]
    fn the_first_line_of_an_error() {
        let e = Error::graph("flow.yaml cannot be read:\nmore", None);
        assert_eq!(said_of(&e), "flow.yaml cannot be read");
    }
}
