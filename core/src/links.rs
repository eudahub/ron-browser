//! Where a link leads, and whether to go there. Nothing here depends on what a page wants beyond
//! the address it gives: going to a page always takes a click by the user.

use std::path::{Path, PathBuf};

use crate::images::percent_decode;

/// What opens as a page. Anything else on the disk stays where it is, whatever a page links to.
const PAGE_EXTENSIONS: [&str; 4] = ["htm", "html", "shtml", "xhtml"];
/// What a directory shows, if it has one of these.
const INDEX_FILES: [&str; 4] = ["index.html", "index.htm", "default.htm", "default.html"];

/// Where a link leads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// A place in the same page; empty for its top.
    SamePage { fragment: String },
    /// A local page.
    Page { path: PathBuf, fragment: String },
    /// Something on a network. Nothing goes there yet.
    External(String),
    /// A local file that is not a page.
    NotAPage(PathBuf),
    /// No such file.
    Missing,
    /// A scheme that is not followed at all, such as `javascript:` or `data:`.
    Refused,
}

/// Finds where a link in the local page `page` leads. A later version that reads from a network
/// must not let what came from there reach local files.
#[must_use]
pub fn resolve(page: &Path, href: &str) -> Target {
    // As browsers do: spaces around it and line ends inside it do not count, and old pages
    // written on Windows use backslashes.
    let cleaned: String = href
        .trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_control())
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    let (address, fragment) = match cleaned.split_once('#') {
        Some((address, fragment)) => (address, percent_decode(fragment).unwrap_or_default()),
        None => (cleaned.as_str(), String::new()),
    };
    if address.is_empty() {
        return Target::SamePage { fragment };
    }
    let local_path = match scheme_of(address) {
        Some(scheme) if scheme.eq_ignore_ascii_case("file") => {
            let Some(path) = file_url_path(address) else {
                return Target::Refused;
            };
            path
        }
        Some(scheme) if is_network_scheme(scheme) => return Target::External(cleaned),
        Some(_) => return Target::Refused,
        None if address.starts_with("//") => return Target::External(cleaned),
        None => address,
    };
    let path = local_path.split('?').next().unwrap_or_default();
    if path.is_empty() {
        return Target::SamePage { fragment };
    }
    let Some(decoded) = percent_decode(path) else {
        return Target::Missing;
    };
    let base = page.parent().unwrap_or_else(|| Path::new(""));
    // The real place, which is what shows and what the images of the page then load from.
    let Ok(file) = base.join(decoded).canonicalize() else {
        return Target::Missing;
    };
    if file.is_dir() {
        return match INDEX_FILES
            .iter()
            .map(|name| file.join(name))
            .find(|index| index.is_file())
        {
            Some(index) => Target::Page {
                path: index,
                fragment,
            },
            None => Target::NotAPage(file),
        };
    }
    if is_page_file(&file) {
        Target::Page {
            path: file,
            fragment,
        }
    } else {
        Target::NotAPage(file)
    }
}

/// The scheme of an address, if it has one.
fn scheme_of(address: &str) -> Option<&str> {
    let (scheme, _) = address.split_once(':')?;
    let mut characters = scheme.chars();
    let starts_well = characters.next()?.is_ascii_alphabetic();
    let goes_on_well =
        characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    (starts_well && goes_on_well).then_some(scheme)
}

fn is_network_scheme(scheme: &str) -> bool {
    ["http", "https", "ftp", "mailto"]
        .iter()
        .any(|known| scheme.eq_ignore_ascii_case(known))
}

/// The path of a `file:` address; `None` when it names another computer.
fn file_url_path(address: &str) -> Option<&str> {
    let rest = &address["file:".len()..];
    let Some(after_slashes) = rest.strip_prefix("//") else {
        return Some(rest);
    };
    let path_start = after_slashes.find('/').unwrap_or(after_slashes.len());
    let host = &after_slashes[..path_start];
    (host.is_empty() || host.eq_ignore_ascii_case("localhost"))
        .then_some(&after_slashes[path_start..])
}

fn is_page_file(file: &Path) -> bool {
    file.is_file()
        && file
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                PAGE_EXTENSIONS
                    .iter()
                    .any(|known| extension.eq_ignore_ascii_case(known))
            })
}
