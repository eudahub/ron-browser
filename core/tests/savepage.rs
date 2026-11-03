//! Saving a page, through a small server on this computer.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use ron_browser_core::savepage::{LIST_NAME, save_page};

/// Bytes that are not valid UTF-8, and must come through as they are.
const PAGE: &[u8] = b"<!doctype html><title>caf\xE9</title>\
    <link rel=stylesheet href=/a.css>\
    <link rel=preload as=script href=/preloaded.js>\
    <link rel=preload as=image href=/no.png>\
    <link rel=icon href=/no.ico>\
    <style>@import '/inline.css';</style>\
    <script src=/one.js></script><script src=/one.js></script>\
    <script src=/x/..%2f..%2fevil.desktop?v=1></script>\
    <script src=/.hidden></script>\
    <script src=/missing.js></script>\
    <script>inside()</script>\
    <script src=http://localhost:9/other.js></script>\
    <script src=data:text/javascript,1></script>\
    <img src=/no.png>";

fn answer(path: &str) -> Option<&'static [u8]> {
    match path {
        "/" => Some(PAGE),
        "/a.css" => Some(b"@import '/b.css';p{color:red}"),
        "/b.css" => Some(b"@import '/a.css';b{color:blue}"),
        "/inline.css" => Some(b"i{color:green}"),
        "/one.js" => Some(b"one()\r\n"),
        "/preloaded.js" => Some(b"preloaded()"),
        "/x/..%2f..%2fevil.desktop" => Some(b"evil()"),
        "/.hidden" => Some(b"hidden()"),
        _ => None,
    }
}

/// Answers from the above, and notes what was asked for.
fn serve() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    let asked = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&asked);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut buffer = [0; 4096];
            let count = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..count]).into_owned();
            let target = request.split(' ').nth(1).unwrap_or("/").to_owned();
            noted.lock().unwrap().push(target.clone());
            let path = target.split('?').next().unwrap_or("/");
            let (status, body) =
                answer(path).map_or(("404 Not Found", &b""[..]), |body| ("200 OK", body));
            let kind = if path == "/" {
                "text/html"
            } else {
                "text/plain"
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
    });
    (address, asked)
}

/// A new empty directory, gone when this ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "ron-savepage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn all_files(directory: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for folder in [
        directory.to_owned(),
        directory.join("scripts"),
        directory.join("styles"),
    ] {
        for entry in fs::read_dir(folder).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                found.push(
                    entry
                        .path()
                        .strip_prefix(directory)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                );
            }
        }
    }
    found.sort();
    found
}

fn save(address: &str, parent: &Path) -> ron_browser_core::savepage::SaveSummary {
    save_page(address, parent, &mut |_, _| true).unwrap()
}

#[test]
fn the_page_and_what_it_runs_come_through_unchanged() {
    let (address, _) = serve();
    let scratch = Scratch::new();
    let summary = save(&address, &scratch.0);
    assert_eq!(summary.directory, scratch.0.join("127.0.0.1"));
    assert_eq!(fs::read(summary.directory.join("page.html")).unwrap(), PAGE);
    let files = all_files(&summary.directory);
    let script = files
        .iter()
        .find(|name| name.ends_with("-one.js"))
        .expect("the script");
    assert_eq!(
        fs::read(summary.directory.join(script)).unwrap(),
        b"one()\r\n"
    );
    assert!(files.iter().any(|name| name.ends_with("-preloaded.js")));
}

#[test]
fn imports_of_sheets_are_followed_and_each_file_comes_once() {
    let (address, asked) = serve();
    let scratch = Scratch::new();
    let summary = save(&address, &scratch.0);
    let files = all_files(&summary.directory);
    for name in ["-a.css", "-b.css", "-inline.css"] {
        assert_eq!(
            files.iter().filter(|file| file.ends_with(name)).count(),
            1,
            "{name} in {files:?}"
        );
    }
    let asked = asked.lock().unwrap();
    assert_eq!(asked.iter().filter(|path| *path == "/one.js").count(), 1);
    assert_eq!(asked.iter().filter(|path| *path == "/a.css").count(), 1);
}

#[test]
fn images_are_not_asked_for() {
    let (address, asked) = serve();
    let scratch = Scratch::new();
    save(&address, &scratch.0);
    let asked = asked.lock().unwrap();
    assert!(
        !asked.iter().any(|path| path.starts_with("/no.")),
        "{asked:?}"
    );
}

#[test]
fn names_from_the_server_stay_harmless() {
    let (address, _) = serve();
    let scratch = Scratch::new();
    let summary = save(&address, &scratch.0);
    for file in all_files(&summary.directory) {
        let name = Path::new(&file).file_name().unwrap().to_str().unwrap();
        assert!(!name.starts_with('.'), "{name}");
        assert!(
            name.chars()
                .all(|character| character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '_' | '-')),
            "{name}"
        );
        assert!(
            [".js", ".css", ".html", ".txt"]
                .iter()
                .any(|ending| name.ends_with(ending)),
            "{name}"
        );
    }
    // All stayed within the directory, and what would run when clicked no longer would.
    let files = all_files(&summary.directory);
    assert!(
        files
            .iter()
            .any(|name| name.contains("evil.desktop") && name.ends_with(".js")),
        "{files:?}"
    );
}

#[test]
fn nothing_there_already_is_touched() {
    let (address, _) = serve();
    let scratch = Scratch::new();
    let there = scratch.0.join("127.0.0.1");
    fs::create_dir(&there).unwrap();
    fs::write(there.join("page.html"), "mine").unwrap();
    let summary = save(&address, &scratch.0);
    assert_eq!(summary.directory, scratch.0.join("127.0.0.1-2"));
    assert_eq!(fs::read(there.join("page.html")).unwrap(), b"mine");
}

#[test]
fn what_cannot_be_read_is_listed() {
    let (address, _) = serve();
    let scratch = Scratch::new();
    let summary = save(&address, &scratch.0);
    let list = fs::read_to_string(summary.directory.join(LIST_NAME)).unwrap();
    let line = |part: &str| {
        list.lines()
            .find(|line| line.contains(part))
            .unwrap_or_else(|| panic!("{part} in {list}"))
            .to_owned()
    };
    assert!(line("/missing.js").starts_with("failed\t-\tthe server answered 404"));
    // A page reaches no local address but its own.
    assert!(line("localhost").starts_with("failed\t-\tnot allowed: a page"));
    assert!(line("/one.js").starts_with("saved\tscripts/"));
    assert_eq!(summary.failed, 2);
    assert!(!list.contains("data:"));
}

#[test]
fn a_page_that_cannot_be_read_leaves_nothing() {
    let (address, _) = serve();
    let scratch = Scratch::new();
    let failed = save_page(&format!("{address}nothing"), &scratch.0, &mut |_, _| true);
    assert!(failed.unwrap_err().contains("404"));
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
}

#[test]
fn it_ends_when_nobody_wants_more() {
    let (address, asked) = serve();
    let scratch = Scratch::new();
    let summary = save_page(&address, &scratch.0, &mut |_, _| false).unwrap();
    assert_eq!(summary.saved, 1);
    assert_eq!(asked.lock().unwrap().len(), 1);
    let list = fs::read_to_string(summary.directory.join(LIST_NAME)).unwrap();
    assert!(list.ends_with("# stopped before the end\n"));
}
