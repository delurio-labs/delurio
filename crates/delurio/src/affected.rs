//! `delurio affected`: which graphs a change touches, read from a checkout (SPEC.md §9, What a change touches). The
//! change is the one from the merge base of `--base` and `--head` to `--head`, or to the working tree with the files
//! git does not ignore: its files from `git diff --name-status`, their lines from `git diff -U0`, each side's files
//! read with `git ls-tree` and `git cat-file`, nothing checked out. The selection is `delurio_core::affected`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::process::{Command, Stdio};

use delurio_core::affected::{
    AFFECTED_USAGE, Change, ChangeStatus, ChangedFile, Hunk, Label, Tree, affected, inside, parse_hunks,
};
use delurio_core::js;
use delurio_core::nodepath::{dirname, join2, normalize, relative, resolve, resolve1};

/// `git -C <repo> -c core.quotePath=false <args>`: its output, None when it fails
fn git(repo: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn git_text(repo: &str, args: &[&str]) -> Option<String> {
    git(repo, args).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// the paths of a `-z` list
fn paths_of(list: &str) -> Vec<String> {
    list.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect()
}

/// a file's bytes as text: None when they hold a NUL, as a binary file does
fn text_of(bytes: &[u8]) -> Option<String> {
    (!bytes.contains(&0)).then(|| String::from_utf8_lossy(bytes).into_owned())
}

/// the texts of these files at `rev`, read with one `git cat-file --batch`
fn cat_files(repo: &str, rev: &str, paths: &[String]) -> Vec<Option<String>> {
    let none = || vec![None; paths.len()];
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return none();
    };
    let input: String = paths.iter().map(|p| format!("{rev}:{p}\n")).collect();
    let stdin = child.stdin.take();
    // written from a thread of its own, so that a long answer never waits on a full pipe
    let writer = std::thread::spawn(move || {
        if let Some(mut stdin) = stdin {
            let _ = stdin.write_all(input.as_bytes());
        }
    });
    let out = child.wait_with_output();
    let _ = writer.join();
    let Ok(out) = out else { return none() };
    let bytes = out.stdout;
    let mut at = 0;
    let mut texts = Vec::with_capacity(paths.len());
    for _ in paths {
        let Some(eol) = bytes[at.min(bytes.len())..].iter().position(|&b| b == b'\n').map(|i| at + i) else {
            texts.push(None);
            continue;
        };
        let header = String::from_utf8_lossy(&bytes[at..eol]).into_owned();
        at = eol + 1;
        // `<object> <type> <size>`, or `<name> missing`
        let words: Vec<&str> = header.split(' ').collect();
        let size = match words.as_slice() {
            [_, _, size] => size.parse::<usize>().ok(),
            _ => None,
        };
        let Some(size) = size else {
            texts.push(None);
            continue;
        };
        let body = bytes.get(at..at + size).unwrap_or_default();
        at += size + 1;
        texts.push(if words[1] == "blob" { text_of(body) } else { None });
    }
    texts
}

/// The files of a commit, read through `git cat-file`, never checked out.
pub struct GitTree {
    repo: String,
    rev: String,
    all: Vec<String>,
    known: HashSet<String>,
    by_dir: HashMap<String, Vec<String>>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl GitTree {
    pub fn new(repo: &str, rev: &str) -> Self {
        let mut all =
            git_text(repo, &["ls-tree", "-r", "-z", "--name-only", rev]).map_or_else(Vec::new, |l| paths_of(&l));
        js::sort_utf16(&mut all);
        let mut by_dir: HashMap<String, Vec<String>> = HashMap::new();
        for p in &all {
            by_dir.entry(dirname(p)).or_default().push(p.clone());
        }
        GitTree {
            repo: repo.to_string(),
            rev: rev.to_string(),
            known: all.iter().cloned().collect(),
            all,
            by_dir,
            cache: RefCell::new(HashMap::new()),
        }
    }

    fn batch(&self, paths: &[String]) {
        let wanted: Vec<String> = {
            let cache = self.cache.borrow();
            let mut seen = HashSet::new();
            paths.iter().filter(|p| !cache.contains_key(*p) && seen.insert(p.as_str())).cloned().collect()
        };
        if wanted.is_empty() {
            return;
        }
        let texts = cat_files(&self.repo, &self.rev, &wanted);
        self.cache.borrow_mut().extend(wanted.into_iter().zip(texts));
    }
}

impl Tree for GitTree {
    fn files(&self, dir: &str) -> Vec<String> {
        self.all.iter().filter(|p| inside(dir, p)).cloned().collect()
    }

    fn preload(&self, paths: &[String]) {
        let known: Vec<String> = paths.iter().filter(|p| self.known.contains(*p)).cloned().collect();
        self.batch(&known);
    }

    fn read(&self, path: &str) -> Option<String> {
        if !self.cache.borrow().contains_key(path) {
            // a small folder's files at once: the graphs folder is read whole, one folder at a time
            let near = self.by_dir.get(&dirname(path)).map_or(&[][..], Vec::as_slice);
            if near.len() <= 200 {
                self.batch(near);
            } else if self.known.contains(path) {
                self.batch(&[path.to_string()]);
            }
            self.cache.borrow_mut().entry(path.to_string()).or_insert(None);
        }
        self.cache.borrow().get(path).cloned().flatten()
    }
}

/// a file larger than this is not read as text
const LARGEST: u64 = 8 * 1024 * 1024;

/// The files on disk: the tracked ones and those git does not ignore.
pub struct DiskTree {
    repo: String,
    all: Vec<String>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl DiskTree {
    pub fn new(repo: &str) -> Self {
        let listed = git_text(repo, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]);
        let mut seen = HashSet::new();
        let mut all: Vec<String> = listed
            .map_or_else(Vec::new, |l| paths_of(&l))
            .into_iter()
            .filter(|p| seen.insert(p.clone()))
            .filter(|p| std::fs::metadata(join2(repo, p)).is_ok_and(|m| m.is_file()))
            .collect();
        js::sort_utf16(&mut all);
        DiskTree { repo: repo.to_string(), all, cache: RefCell::new(HashMap::new()) }
    }
}

impl Tree for DiskTree {
    fn files(&self, dir: &str) -> Vec<String> {
        self.all.iter().filter(|p| inside(dir, p)).cloned().collect()
    }

    fn read(&self, path: &str) -> Option<String> {
        if let Some(kept) = self.cache.borrow().get(path) {
            return kept.clone();
        }
        let within = !path.starts_with('/') && !normalize(path).starts_with("..");
        let abs = join2(&self.repo, path);
        let text = std::fs::metadata(&abs)
            .ok()
            .filter(|m| within && m.is_file() && m.len() <= LARGEST)
            .and_then(|_| std::fs::read(&abs).ok())
            .and_then(|bytes| text_of(&bytes));
        self.cache.borrow_mut().insert(path.to_string(), text.clone());
        text
    }
}

/// `git diff --name-status -z -M`: each changed file, its old path when it was renamed; a copy is an added file
fn changed_files(list: &str) -> Vec<ChangedFile> {
    let parts: Vec<&str> = list.split('\0').collect();
    let mut files = Vec::new();
    let mut i = 0;
    let file = |path: &str, from: Option<&str>, status| ChangedFile {
        path: path.to_string(),
        from: from.map(str::to_string),
        status,
        hunks: None,
    };
    while i + 1 < parts.len() {
        let code = parts[i];
        i += 1;
        let Some(letter) = code.chars().next() else { continue };
        if letter == 'R' || letter == 'C' {
            let (from, path) =
                (parts.get(i).copied().unwrap_or_default(), parts.get(i + 1).copied().unwrap_or_default());
            i += 2;
            files.push(if letter == 'R' {
                file(path, Some(from), ChangeStatus::Renamed)
            } else {
                file(path, None, ChangeStatus::Added)
            });
        } else {
            let path = parts.get(i).copied().unwrap_or_default();
            i += 1;
            let status = match letter {
                'A' => ChangeStatus::Added,
                'D' => ChangeStatus::Deleted,
                _ => ChangeStatus::Modified,
            };
            files.push(file(path, None, status));
        }
    }
    files
}

/// the change as read from a checkout: its trees, its files, what was compared
struct Read {
    head: Box<dyn Tree>,
    base: Option<GitTree>,
    files: Option<Vec<ChangedFile>>,
    why: Option<String>,
    label: Label,
}

/// a revision's commit, None when it names none
fn commit_of(repo: &str, rev: &str) -> Option<String> {
    let sha = git_text(repo, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")])?;
    let sha = js::trim(&sha);
    (!sha.is_empty()).then(|| sha.to_string())
}

/// The change from the merge base of `base` and `head` to `head` (or to the working tree), read from a checkout.
fn read_change(repo: &str, base: &str, head: Option<&str>) -> Read {
    let head_label = head.unwrap_or("the working tree").to_string();
    let failed = |why: String| Read {
        head: match head {
            None => Box::new(DiskTree::new(repo)),
            Some(h) => Box::new(GitTree::new(repo, &commit_of(repo, h).unwrap_or_else(|| "HEAD".to_string()))),
        },
        base: None,
        files: None,
        why: Some(why),
        label: Label { base: None, head: head_label.clone() },
    };
    let Some(base_sha) = commit_of(repo, base) else { return failed(format!("{base} is not a commit of {repo}")) };
    let head_rev = head.unwrap_or("HEAD");
    let Some(head_sha) = commit_of(repo, head_rev) else {
        return failed(format!("{head_rev} is not a commit of {repo}"));
    };
    let Some(merge_base) = git_text(repo, &["merge-base", &base_sha, &head_sha]).map(|m| js::trim(&m).to_string())
    else {
        return failed(format!("{base} and {head_rev} have no merge base"));
    };
    let mut range = vec![merge_base.as_str()];
    if head.is_some() {
        range.push(head_sha.as_str());
    }
    let with = |args: &[&str]| -> Vec<String> {
        args.iter().chain(&range).map(|s| s.to_string()).chain(std::iter::once("--".to_string())).collect()
    };
    let run = |args: Vec<String>| git_text(repo, &args.iter().map(String::as_str).collect::<Vec<_>>());
    let (Some(statuses), Some(patch)) = (
        run(with(&["diff", "--name-status", "-z", "-M", "--no-color"])),
        run(with(&["diff", "-U0", "--no-color", "--no-ext-diff", "-M"])),
    ) else {
        return failed(format!("git cannot compare {merge_base} with {head_label}"));
    };
    let mut files = changed_files(&statuses);
    let hunks = parse_hunks(&patch);
    for f in &mut files {
        f.hunks = hunks.get(&f.path).cloned(); // none for a binary file, or a change of mode alone
    }
    let head_tree: Box<dyn Tree> = match head {
        None => Box::new(DiskTree::new(repo)),
        Some(_) => Box::new(GitTree::new(repo, &head_sha)),
    };
    if head.is_none() {
        let known: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
        let others = git_text(repo, &["ls-files", "-z", "--others", "--exclude-standard"]).unwrap_or_default();
        for path in paths_of(&others).into_iter().filter(|p| !known.contains(p)) {
            let lines = head_tree.read(&path).map_or(0, |t| t.split('\n').count()) as u64;
            files.push(ChangedFile {
                path,
                from: None,
                status: ChangeStatus::Added,
                hunks: Some(vec![Hunk { old_start: 0, old_lines: 0, new_start: 1, new_lines: lines }]),
            });
        }
    }
    Read {
        head: head_tree,
        base: Some(GitTree::new(repo, &merge_base)),
        files: Some(files),
        why: None,
        label: Label { base: Some(merge_base.clone()), head: head.map_or(head_label, |_| head_sha.clone()) },
    }
}

/// `relative(top, dir)`, read again through the links on the way when it climbs out of `top` (a temporary folder
/// that is a link to another, as on macOS)
fn under(top: &str, dir: &str) -> String {
    let rel = relative(top, dir);
    if !rel.starts_with("..") {
        return rel;
    }
    let real = |p: &str| std::fs::canonicalize(p).ok().map(|p| p.to_string_lossy().into_owned());
    match (real(top), real(dir)) {
        (Some(t), Some(d)) => relative(&t, &d),
        _ => rel,
    }
}

/// `delurio affected <args>`: 0 once it has answered, 1 when the arguments or the checkout keep it from answering
pub fn affected_main(args: &[String]) -> i32 {
    let flag = |name: &str| -> Option<&str> {
        let i = args.iter().position(|a| a == name)?;
        args.get(i + 1).map(String::as_str)
    };
    let has = |name: &str| args.iter().any(|a| a == name);
    let Some(base) = flag("--base").filter(|b| !b.starts_with("--")) else {
        eprintln!("usage:\n{AFFECTED_USAGE}");
        return 1;
    };
    for name in ["--head", "--graphs", "--repo"] {
        if has(name) && flag(name).is_none_or(|v| v.starts_with("--")) {
            eprintln!("error: {name} needs a value");
            return 1;
        }
    }
    let repo = resolve1(flag("--repo").unwrap_or("."));
    let Some(top) = git_text(&repo, &["rev-parse", "--show-toplevel"]).map(|t| js::trim(&t).to_string()) else {
        eprintln!("error: {repo} is not a git checkout");
        return 1;
    };
    let graphs_arg = flag("--graphs").unwrap_or("graphs");
    let graphs = under(&top, &resolve(&[&repo, graphs_arg]));
    if graphs.starts_with("..") {
        eprintln!("error: {graphs_arg} is outside the repository");
        return 1;
    }
    let read = read_change(&top, base, flag("--head"));
    let change = Change {
        graphs,
        head: read.head.as_ref(),
        base: read.base.as_ref().map(|b| b as &dyn Tree),
        files: read.files,
        complete: true,
        why: read.why,
        everything: has("--all").then(|| "every graph was asked for (--all)".to_string()),
        label: Some(read.label),
    };
    let answer = affected(&change);
    let text = if has("--json") { answer.to_json() } else { answer.to_text() };
    let mut stdout = std::io::stdout().lock();
    // all of it, before the process exits; a closed pipe is the reader's choice
    let _ = stdout.write_all(text.as_bytes()).and_then(|()| stdout.flush());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_changed_files_of_name_status() {
        let list = "M\0src/a.ts\0A\0new.ts\0D\0gone.ts\0R087\0old.ts\0moved.ts\0C100\0from.ts\0copy.ts\0T\0link\0";
        let said: Vec<(String, Option<String>, ChangeStatus)> =
            changed_files(list).into_iter().map(|f| (f.path, f.from, f.status)).collect();
        let f = |p: &str, from: Option<&str>, s| (p.to_string(), from.map(str::to_string), s);
        assert_eq!(
            said,
            [
                f("src/a.ts", None, ChangeStatus::Modified),
                f("new.ts", None, ChangeStatus::Added),
                f("gone.ts", None, ChangeStatus::Deleted),
                f("moved.ts", Some("old.ts"), ChangeStatus::Renamed),
                f("copy.ts", None, ChangeStatus::Added),
                f("link", None, ChangeStatus::Modified),
            ]
        );
        assert!(changed_files("").is_empty());
    }

    #[test]
    fn a_binary_file_is_no_text() {
        assert_eq!(text_of(b"a\nb\n").as_deref(), Some("a\nb\n"));
        assert_eq!(text_of(b"PNG\0\x01"), None);
    }
}
