//! Node's `path` (POSIX) as the TypeScript checker used it, so that every file name in the output is written alike:
//! `join`, `resolve`, `relative`, `dirname`, `basename`, `extname`. Paths are strings, `/` separated.

/// `path.normalize`'s core: `.` and `..` resolved, separators collapsed
fn normalize_parts(path: &str, allow_above_root: bool) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|p| *p != "..") {
                    out.pop();
                } else if allow_above_root {
                    out.push("..");
                }
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

/// `path.normalize(p)`
pub fn normalize(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let absolute = p.starts_with('/');
    let trailing = p.ends_with('/');
    let mut body = normalize_parts(p, !absolute);
    if body.is_empty() && !absolute {
        body = ".".to_string();
    }
    if !body.is_empty() && trailing {
        body.push('/');
    }
    if absolute { format!("/{body}") } else { body }
}

/// `path.join(...parts)`
pub fn join(parts: &[&str]) -> String {
    let joined = parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/");
    if joined.is_empty() { ".".to_string() } else { normalize(&joined) }
}

/// `path.join(a, b)`
pub fn join2(a: &str, b: &str) -> String {
    join(&[a, b])
}

/// the working directory, as `process.cwd()` gives it
pub fn cwd() -> String {
    std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "/".to_string())
}

/// `path.resolve(...parts)` from the working directory
pub fn resolve(parts: &[&str]) -> String {
    let mut resolved = String::new();
    let mut absolute = false;
    for p in parts.iter().rev() {
        if p.is_empty() {
            continue;
        }
        resolved = if resolved.is_empty() { (*p).to_string() } else { format!("{p}/{resolved}") };
        if p.starts_with('/') {
            absolute = true;
            break;
        }
    }
    if !absolute {
        let cwd = cwd();
        resolved = if resolved.is_empty() { cwd } else { format!("{cwd}/{resolved}") };
    }
    let body = normalize_parts(&resolved, false);
    format!("/{body}")
}

/// `path.resolve(p)`
pub fn resolve1(p: &str) -> String {
    resolve(&[p])
}

/// `path.relative(from, to)`
pub fn relative(from: &str, to: &str) -> String {
    let from = resolve1(from);
    let to = resolve1(to);
    if from == to {
        return String::new();
    }
    let f: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = std::iter::repeat_n("..", f.len() - common).collect();
    out.extend(&t[common..]);
    out.join("/")
}

/// `path.dirname(p)`
pub fn dirname(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        Some(i) => {
            let d = trimmed[..i].trim_end_matches('/');
            if d.is_empty() { "/".to_string() } else { d.to_string() }
        }
    }
}

/// `path.basename(p)`
pub fn basename(p: &str) -> String {
    let trimmed = p.trim_end_matches('/');
    match trimmed.rfind('/') {
        None => trimmed.to_string(),
        Some(i) => trimmed[i + 1..].to_string(),
    }
}

/// `path.extname(p)`
pub fn extname(p: &str) -> String {
    let base = basename(p);
    match base.rfind('.') {
        None | Some(0) => String::new(),
        Some(i) => base[i..].to_string(),
    }
}

/// `path.sep`
pub const SEP: &str = "/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_node_path_posix() {
        assert_eq!(join(&["a", "b/../c", "d/"]), "a/c/d/");
        assert_eq!(join(&["", ""]), ".");
        assert_eq!(join(&["/a", "../.."]), "/");
        assert_eq!(join(&["..", "x"]), "../x");
        assert_eq!(normalize("./a//b/."), "a/b");
        assert_eq!(resolve(&["/a/b", "../c"]), "/a/c");
        assert_eq!(resolve(&["/a/b/", "/x", "y"]), "/x/y");
        assert_eq!(relative("/a/b/c", "/a/d"), "../../d");
        assert_eq!(relative("/a", "/a"), "");
        assert_eq!(relative("/a", "/a/b/c"), "b/c");
        assert_eq!(dirname("/a/b/"), "/a");
        assert_eq!(dirname("a"), ".");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(basename("/a/b.yaml"), "b.yaml");
        assert_eq!(extname("x/flow.yaml"), ".yaml");
        assert_eq!(extname(".env"), "");
    }
}
