//! `page --watch` writes the pages again whenever a graph file under the folder changes; with `--serve` it also shows
//! them at http://127.0.0.1:<port>/ and reloads the open page after each write, or shows why the write failed. Local
//! only: the server listens on the loopback address and serves the written pages and nothing else. The port of
//! watch.ts, on `notify` for the folder's changes and a small HTTP server of its own.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use delurio_core::nodepath;
use notify::{RecursiveMode, Watcher};

/// whether a write of every page succeeded, and the first error when it did not
pub struct Rendered {
    pub ok: bool,
    pub message: String,
}

const WATCHED: [&str; 3] = [".yaml", ".yml", ".json"];

fn content_type(ext: &str) -> &'static str {
    match ext {
        ".html" => "text/html; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".js" => "text/javascript; charset=utf-8",
        ".svg" => "image/svg+xml",
        ".json" => "application/json",
        ".mmd" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

// added to every page the server sends, never to the files on disk
const RELOAD: &str = r#"<script>(() => {
  const events = new EventSource('/__delurio/events');
  events.addEventListener('reload', () => location.reload());
  events.addEventListener('failed', (e) => {
    let box = document.getElementById('__delurio');
    if (!box) {
      box = document.createElement('pre');
      box.id = '__delurio';
      box.setAttribute('role', 'alert');
      box.style.cssText = 'position:fixed;inset:auto 0 0 0;margin:0;padding:12px 16px;background:#7f1d1d;color:#fff;font:13px/1.5 ui-monospace,Menlo,Consolas,monospace;white-space:pre-wrap;z-index:2147483647';
      document.body.append(box);
    }
    box.textContent = 'delurio page: ' + e.data;
  });
})();</script>"#;

// whether a changed file can change a page: a graph file, not one of the pages being written
fn counts(root: &str, out: &str, name: &str) -> bool {
    let abs = nodepath::resolve(&[root, name]);
    if abs == out || abs.starts_with(&format!("{out}/")) {
        return false;
    }
    if abs.split('/').any(|p| p == "viz") {
        return false;
    }
    WATCHED.contains(&nodepath::extname(&abs).as_str())
}

type Clients = Arc<Mutex<Vec<TcpStream>>>;

/// `decodeURIComponent`: None when a `%` is not followed by two hex digits or the bytes are not UTF-8
fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn respond(mut stream: TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn handle(stream: TcpStream, out: &str, clients: &Clients) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // the headers are read and left: the server answers GET of a path, whatever else the request says
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) | Err(_) => break,
            Ok(_) if h == "\r\n" || h == "\n" => break,
            Ok(_) => {}
        }
    }
    let target = line.split(' ').nth(1).unwrap_or("/");
    let pathname = target.split(['?', '#']).next().unwrap_or("/");
    let pathname = if pathname.starts_with('/') { pathname.to_string() } else { format!("/{pathname}") };
    if pathname == "/__delurio/events" {
        let mut s = stream;
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n: delurio\n\n";
        if s.write_all(head.as_bytes()).is_ok()
            && let Ok(mut list) = clients.lock()
        {
            list.push(s);
        }
        return;
    }
    let Some(wanted) = decode_uri_component(&pathname) else {
        respond(stream, "400 Bad Request", &[], b"");
        return;
    };
    let mut path = nodepath::join(&[out, &nodepath::normalize(&wanted)]);
    if path != out && !path.starts_with(&format!("{out}/")) {
        respond(stream, "403 Forbidden", &[], b"");
        return;
    }
    if Path::new(&path).is_dir() {
        path = nodepath::join2(&path, "index.html");
    }
    let Ok(body) = std::fs::read(&path) else {
        respond(stream, "404 Not Found", &[("Content-Type", "text/plain; charset=utf-8")], b"not found");
        return;
    };
    let kind = content_type(&nodepath::extname(&path));
    let body = if kind.starts_with("text/html") {
        let html = String::from_utf8_lossy(&body).into_owned();
        let page = if html.contains("</body>") {
            html.replacen("</body>", &format!("{RELOAD}</body>"), 1)
        } else {
            format!("{html}{RELOAD}")
        };
        page.into_bytes()
    } else {
        body
    };
    respond(stream, "200 OK", &[("Content-Type", kind), ("Cache-Control", "no-store")], &body);
}

fn serve(out: String, port: u16, clients: Clients) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let out = out.clone();
            let clients = Arc::clone(&clients);
            std::thread::spawn(move || handle(stream, &out, &clients));
        }
    });
    Ok(())
}

fn tell(clients: &Clients, event: &str, data: &str) {
    let message = format!("event: {event}\ndata: {}\n\n", data.replace('\n', " "));
    if let Ok(mut list) = clients.lock() {
        list.retain_mut(|c| c.write_all(message.as_bytes()).is_ok());
    }
}

/// Runs until stopped. `render` writes every page once and says whether it could. Returns only when the folder cannot
/// be watched or the port cannot be listened on, with the exit code.
pub fn watch_pages(root: &str, out: &str, mut render: impl FnMut() -> Rendered, port: Option<u16>) -> i32 {
    let clients: Clients = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |e: notify::Result<notify::Event>| {
        if let Ok(e) = e {
            for p in e.paths {
                let _ = tx.send(p.to_string_lossy().into_owned());
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("error: {root} cannot be watched: {e}");
            return 1;
        }
    };
    if let Err(e) = watcher.watch(Path::new(root), RecursiveMode::Recursive) {
        eprintln!("error: {root} cannot be watched: {e}");
        return 1;
    }
    println!("watching {root} for changes");
    if let Some(port) = port {
        if let Err(e) = serve(out.to_string(), port, Arc::clone(&clients)) {
            eprintln!("error: listen {}: 127.0.0.1:{port}", e);
            return 1;
        }
        println!("serving http://127.0.0.1:{port}/ (Ctrl-C stops)");
    }
    let root_abs = nodepath::resolve1(root);
    loop {
        // a change that counts, then quiet for 150 ms, then every page once; changes meanwhile write them again
        let Ok(first) = rx.recv() else { return 0 };
        if !counts(&root_abs, out, &first) {
            continue;
        }
        let mut deadline = Instant::now() + Duration::from_millis(150);
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            match rx.recv_timeout(deadline - now) {
                Ok(name) if counts(&root_abs, out, &name) => deadline = Instant::now() + Duration::from_millis(150),
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return 0,
            }
        }
        let done = render();
        tell(&clients, if done.ok { "reload" } else { "failed" }, if done.ok { "" } else { &done.message });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_counts_as_a_change() {
        assert!(counts("/g", "/g/viz", "flow.yaml"));
        assert!(counts("/g", "/out", "a/ui.yaml"));
        assert!(!counts("/g", "/g/viz", "viz/index.html"));
        assert!(!counts("/g", "/out", "a/viz/x.json"));
        assert!(!counts("/g", "/out", "a/notes.js"));
        assert!(!counts("/g", "/g", "flow.yaml"));
    }

    #[test]
    fn uri_decoding() {
        assert_eq!(decode_uri_component("/a%20b").as_deref(), Some("/a b"));
        assert_eq!(decode_uri_component("/%E2%9C%93").as_deref(), Some("/✓"));
        assert_eq!(decode_uri_component("/%zz"), None);
        assert_eq!(decode_uri_component("/%ff"), None);
    }
}
