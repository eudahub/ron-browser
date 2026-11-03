//! Saves a page from the network as the server gives it, with its scripts and style sheets, for
//! looking at them. Nothing in them is changed or run, so links still lead to the network and the
//! result is not for viewing from the disk; images are left out for the same reason.
//!
//! The page may be hostile, and nothing it says decides what happens on the disk: all goes into a
//! new directory, nothing is overwritten or made executable, and of a name only harmless characters
//! stay, with an ending that fits what the file is meant to be. What was really where is in
//! [`LIST_NAME`], with what could not be read.

use std::collections::{HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use ron::css::{SheetSource, Stylesheet, document_sheet_sources};
use ron::html::{Document, NodeId, NodeKind, Tag, parse_owned};
use url::Url;

use crate::charset;
use crate::net::{self, Asker, MAX_SHEET_BYTES, may_redirect};
use crate::sheets::decode_css;

/// The list of all files, with their addresses.
pub const LIST_NAME: &str = "files.txt";
const MAX_SCRIPT_BYTES: u64 = 32 * 1024 * 1024;
/// More are not read, as a page could otherwise name files without end.
const MAX_FILES: usize = 512;
const MAX_ALL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_IMPORT_DEPTH: u8 = 5;
/// Of a name from a server, this much is kept.
const MAX_NAME_CHARS: usize = 80;
const MAX_DIRECTORY_TRIES: u32 = 1000;
const MAX_LISTED_ADDRESS: usize = 2000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Script,
    Sheet,
}

impl Kind {
    const fn directory(self) -> &'static str {
        match self {
            Self::Script => "scripts",
            Self::Sheet => "styles",
        }
    }

    const fn max_bytes(self) -> u64 {
        match self {
            Self::Script => MAX_SCRIPT_BYTES,
            Self::Sheet => MAX_SHEET_BYTES,
        }
    }
}

/// A file that the page needs, as far as is known. It may turn out not to be a valid address.
struct Wanted {
    written: String,
    base: Url,
    kind: Kind,
    depth: u8,
}

/// What came of saving a page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveSummary {
    pub directory: PathBuf,
    /// The files saved, with the page itself.
    pub saved: usize,
    /// The files that could not be read.
    pub failed: usize,
}

/// Where the page says that its addresses count from.
fn base_of(document: &Document, address: &Url) -> Url {
    document
        .descendants(document.root())
        .find(|&id| document.kind(id) == NodeKind::Element && document.tag(id) == Tag::Base)
        .and_then(|id| attribute(document, id, "href"))
        .and_then(|href| address.join(&href).ok())
        .filter(|base| matches!(base.scheme(), "http" | "https"))
        .unwrap_or_else(|| address.clone())
}

fn attribute(document: &Document, id: NodeId, name: &str) -> Option<String> {
    document
        .attributes(id)
        .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
        .map(|attribute| attribute.decoded_value().into_owned())
}

/// What a `<link>` brings in, if it is a script or a style sheet. A sheet that is only an
/// alternative counts too, as it can be switched to.
fn link_kind(document: &Document, id: NodeId) -> Option<Kind> {
    let rel = attribute(document, id, "rel")?.to_ascii_lowercase();
    let has = |word: &str| rel.split_ascii_whitespace().any(|found| found == word);
    if has("stylesheet") {
        return Some(Kind::Sheet);
    }
    if has("modulepreload") {
        return Some(Kind::Script);
    }
    if has("preload") || has("prefetch") {
        let goes_as = attribute(document, id, "as")?.to_ascii_lowercase();
        return match goes_as.as_str() {
            "script" => Some(Kind::Script),
            "style" => Some(Kind::Sheet),
            _ => None,
        };
    }
    None
}

/// The scripts and style sheets that a page names, in the order that it names them.
fn files_of_page(bytes: &[u8], server_charset: &str, address: &Url) -> Vec<Wanted> {
    let chosen = charset::choose_encoding(bytes, server_charset, true);
    let document = parse_owned(charset::decode(bytes, &chosen.encoding).utf8);
    let base = base_of(&document, address);
    let mut wanted = Vec::new();
    let mut want = |written: String, kind: Kind| {
        wanted.push(Wanted {
            written,
            base: base.clone(),
            kind,
            depth: 0,
        });
    };
    for id in document.descendants(document.root()) {
        if document.kind(id) != NodeKind::Element {
            continue;
        }
        let found = match document.tag(id) {
            Tag::Script => attribute(&document, id, "src").map(|src| (src, Kind::Script)),
            Tag::Link => link_kind(&document, id)
                .and_then(|kind| attribute(&document, id, "href").map(|href| (href, kind))),
            _ => None,
        };
        if let Some((written, kind)) = found {
            want(written, kind);
        }
    }
    // What the sheets within the page import, which is known without reading anything more.
    for source in document_sheet_sources(&document) {
        if let SheetSource::Inline { text, .. } = source {
            for import in Stylesheet::parse(&text).imports {
                want(import.into_string(), Kind::Sheet);
            }
        }
    }
    wanted
}

/// Keeps of a name only what is harmless in any file system and shell, and never a leading dot.
fn harmless_name(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(MAX_NAME_CHARS)
        .collect();
    kept.trim_start_matches(['.', '-']).to_owned()
}

/// A name for a file from this address. The number keeps names apart, whatever servers call them,
/// and the ending stays what the file was asked for as, so that a file cannot end up looking like
/// something that runs when clicked.
fn file_name(url: &Url, kind: Kind, number: usize) -> String {
    let last = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .unwrap_or_default();
    let mut name = harmless_name(last);
    if name.is_empty() {
        name.push_str("file");
    }
    let lower = name.to_ascii_lowercase();
    let fits = match kind {
        Kind::Script => lower.ends_with(".js") || lower.ends_with(".mjs"),
        Kind::Sheet => lower.ends_with(".css"),
    };
    if !fits {
        name.push_str(match kind {
            Kind::Script => ".js",
            Kind::Sheet => ".css",
        });
    }
    format!("{number:03}-{name}")
}

/// Makes a directory of its own for what is saved. One that is there already stays as it is.
fn new_directory(parent: &Path, url: &Url) -> io::Result<PathBuf> {
    let mut name = harmless_name(url.host_str().unwrap_or_default());
    if name.is_empty() {
        name.push_str("page");
    }
    for attempt in 1..=MAX_DIRECTORY_TRIES {
        let path = if attempt == 1 {
            parent.join(&name)
        } else {
            parent.join(format!("{name}-{attempt}"))
        };
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("too many directories of this name"))
}

/// Writes a new file. This cannot go through a link or replace anything.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    File::create_new(path)?.write_all(bytes)
}

/// One line of the list. Tabs and line ends in what a server says cannot make it look like more.
fn list_line(state: &str, file: &str, note: &str, address: &str) -> String {
    let flat = |text: &str| -> String {
        text.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(MAX_LISTED_ADDRESS)
            .collect()
    };
    format!("{state}\t{file}\t{}\t{}\n", flat(note), flat(address))
}

/// Reads and writes all that the page names, until it is done or nobody wants more.
fn save_files(
    directory: &Path,
    page: &Url,
    mut queue: VecDeque<Wanted>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
    summary: &mut SaveSummary,
    list: &mut String,
) -> io::Result<()> {
    let mut seen = HashSet::new();
    let mut number = 0;
    let mut done = 0;
    let mut all_bytes = 0_u64;
    while let Some(wanted) = queue.pop_front() {
        if !progress(done, done + queue.len() + 1) {
            list.push_str("# stopped before the end\n");
            return Ok(());
        }
        let Ok(mut url) = wanted.base.join(&wanted.written) else {
            summary.failed += 1;
            list.push_str(&list_line("failed", "-", "not an address", &wanted.written));
            continue;
        };
        url.set_fragment(None);
        // What is written out within the page is there already.
        if url.scheme() == "data" || !seen.insert(url.to_string()) {
            continue;
        }
        if number >= MAX_FILES || all_bytes >= MAX_ALL_BYTES {
            summary.failed += 1;
            list.push_str(&list_line("failed", "-", "too many files", url.as_str()));
            continue;
        }
        if !may_redirect(page, &url) {
            summary.failed += 1;
            list.push_str(&list_line("failed", "-", "not allowed", url.as_str()));
            continue;
        }
        done += 1;
        let fetched = net::fetch_limited(
            url.as_str(),
            Asker::Page(page.as_str()),
            "*/*",
            wanted.kind.max_bytes(),
        );
        let fetched = match fetched {
            Ok(fetched) => fetched,
            Err(error) => {
                summary.failed += 1;
                list.push_str(&list_line("failed", "-", &error.to_string(), url.as_str()));
                continue;
            }
        };
        number += 1;
        all_bytes += fetched.bytes.len() as u64;
        let folder = directory.join(wanted.kind.directory());
        if !folder.is_dir() {
            fs::create_dir(&folder)?;
        }
        let final_url = Url::parse(&fetched.address).unwrap_or(url);
        let name = file_name(&final_url, wanted.kind, number);
        write_new(&folder.join(&name), &fetched.bytes)?;
        summary.saved += 1;
        list.push_str(&list_line(
            "saved",
            &format!("{}/{name}", wanted.kind.directory()),
            &fetched.mime,
            final_url.as_str(),
        ));
        if wanted.kind == Kind::Sheet && wanted.depth < MAX_IMPORT_DEPTH {
            let css = decode_css(&fetched.bytes, &fetched.charset);
            for import in Stylesheet::parse(&css).imports {
                queue.push_back(Wanted {
                    written: import.into_string(),
                    base: final_url.clone(),
                    kind: Kind::Sheet,
                    depth: wanted.depth + 1,
                });
            }
        }
    }
    Ok(())
}

/// Saves the page at this address into a new directory within `parent`. This blocks, so it is for
/// threads other than that of the window. `progress` hears how many files are done of how many
/// known so far, and ends it by returning false.
///
/// # Errors
/// Tells why the page could not be read or written. Other files that cannot be read do not end it,
/// but are noted in the list.
pub fn save_page(
    address: &str,
    parent: &Path,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<SaveSummary, String> {
    let fetched = net::fetch_page(address, Asker::User).map_err(|error| error.to_string())?;
    let page = Url::parse(&fetched.address).map_err(|error| error.to_string())?;
    let directory = new_directory(parent, &page).map_err(|error| error.to_string())?;
    let is_html = matches!(
        fetched.mime.as_str(),
        "" | "text/html" | "application/xhtml+xml"
    );
    // Something else stays out of the reach of what opens web pages.
    let page_name = if is_html { "page.html" } else { "page.data" };
    write_new(&directory.join(page_name), &fetched.bytes).map_err(|error| error.to_string())?;
    let mut summary = SaveSummary {
        directory: directory.clone(),
        saved: 1,
        failed: 0,
    };
    let mut list = list_line("saved", page_name, &fetched.mime, page.as_str());
    let queue: VecDeque<Wanted> = if is_html {
        files_of_page(&fetched.bytes, &fetched.charset, &page).into()
    } else {
        VecDeque::new()
    };
    let result = save_files(&directory, &page, queue, progress, &mut summary, &mut list);
    // The list says how far it came, also when something went wrong on the way.
    write_new(&directory.join(LIST_NAME), list.as_bytes()).map_err(|error| error.to_string())?;
    result.map_err(|error| error.to_string())?;
    Ok(summary)
}

/// What has happened with a page being saved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SaveEvent {
    Progress { done: usize, known: usize },
    Finished(SaveSummary),
    Failed(String),
}

/// A page being saved on a thread of its own. Dropping this ends it after the file on the way.
pub struct PageSave {
    receiver: Receiver<SaveEvent>,
    ended: bool,
}

impl PageSave {
    /// Starts saving. See [`save_page`].
    #[must_use]
    pub fn start(address: &str, parent: &Path) -> Self {
        let (sender, receiver) = mpsc::channel();
        let address = address.to_owned();
        let parent = parent.to_owned();
        thread::spawn(move || {
            let result = save_page(&address, &parent, &mut |done, known| {
                sender.send(SaveEvent::Progress { done, known }).is_ok()
            });
            let _ = sender.send(result.map_or_else(SaveEvent::Failed, SaveEvent::Finished));
        });
        Self {
            receiver,
            ended: false,
        }
    }

    /// The next thing that has happened, if any.
    pub fn poll(&mut self) -> Option<SaveEvent> {
        if self.ended {
            return None;
        }
        let event = match self.receiver.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => SaveEvent::Failed("saving stopped".to_owned()),
        };
        self.ended = !matches!(event, SaveEvent::Progress { .. });
        Some(event)
    }
}
