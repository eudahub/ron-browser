//! Where a link or a typed address leads, and whether to go there. Nothing here depends on what a
//! page wants beyond the address it gives: going to a page always takes an act of the user.

use std::path::{Path, PathBuf};

use url::Url;

use crate::images::percent_decode;

/// What a link can open as a page. Anything else on the disk stays where it is.
const PAGE_EXTENSIONS: [&str; 4] = ["htm", "html", "shtml", "xhtml"];
/// What a directory shows, if it has one of these.
const INDEX_FILES: [&str; 4] = ["index.html", "index.htm", "default.htm", "default.html"];

/// Where a link or an address leads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Target {
    /// A place in the same page; empty for its top.
    SamePage { fragment: String },
    /// A page at this address, which has no fragment.
    Page { url: String, fragment: String },
    /// A local file that is not a page.
    NotAPage(String),
    /// No such file.
    Missing(String),
    /// Something this browser does not handle, such as `mailto:`.
    Unsupported(String),
    /// What is never followed: schemes such as `javascript:` or `data:`, and local files from a
    /// page on a network.
    Refused(String),
}

/// Finds where a link in the page at `page` leads.
#[must_use]
pub fn resolve(page: &str, href: &str) -> Target {
    let Ok(base) = Url::parse(page) else {
        return Target::Refused(href.to_owned());
    };
    // As browsers do, this takes no notice of spaces around it, or line ends and tabs inside it,
    // and reads backslashes as slashes.
    let Ok(url) = base.join(href) else {
        return Target::Refused(href.to_owned());
    };
    if is_same_page(&base, &url) {
        return Target::SamePage {
            fragment: fragment_of(&url),
        };
    }
    match url.scheme() {
        "file" if base.scheme() == "file" => local_page(&url, true),
        "http" | "https" => network_page(url),
        "ftp" | "mailto" | "tel" | "news" => Target::Unsupported(url.into()),
        _ => Target::Refused(url.into()),
    }
}

/// Finds where an address typed by the user leads. As it is the user's own choice, it can open any
/// local file. An address with no scheme means HTTPS.
#[must_use]
pub fn resolve_address(text: &str) -> Target {
    let text = text.trim();
    if text.is_empty() {
        return Target::Missing(String::new());
    }
    if let Some(url) = local_address(text) {
        return local_page(&url, false);
    }
    let parsed = if has_scheme(text) {
        Url::parse(text)
    } else {
        let scheme = if is_this_computer(text) {
            "http"
        } else {
            "https"
        };
        Url::parse(&format!("{scheme}://{text}"))
    };
    let Ok(url) = parsed else {
        return Target::Missing(text.to_owned());
    };
    match url.scheme() {
        "file" => local_page(&url, false),
        "http" | "https" => network_page(url),
        "ftp" | "mailto" | "tel" | "news" => Target::Unsupported(url.into()),
        _ => Target::Refused(url.into()),
    }
}

/// The address of a local file.
#[must_use]
pub fn file_address(path: &Path) -> Option<String> {
    let absolute = path.canonicalize().ok()?;
    Url::from_file_path(absolute).ok().map(Into::into)
}

/// The local file at an address, if it is one.
#[must_use]
pub fn local_path(address: &str) -> Option<PathBuf> {
    let url = Url::parse(address).ok()?;
    if url.scheme() == "file" {
        url.to_file_path().ok()
    } else {
        None
    }
}

fn is_same_page(base: &Url, url: &Url) -> bool {
    if base.scheme() == "file" && url.scheme() == "file" {
        // Local files have no queries.
        return base.path() == url.path() && base.host() == url.host();
    }
    let without_fragment = |url: &Url| {
        let mut url = url.clone();
        url.set_fragment(None);
        url
    };
    without_fragment(base) == without_fragment(url) && url.fragment().is_some()
}

fn fragment_of(url: &Url) -> String {
    url.fragment().and_then(percent_decode).unwrap_or_default()
}

fn network_page(mut url: Url) -> Target {
    let fragment = fragment_of(&url);
    url.set_fragment(None);
    Target::Page {
        url: url.into(),
        fragment,
    }
}

/// A file on the disk given by its path: absolute, from the home directory, or an existing file
/// here.
fn local_address(text: &str) -> Option<Url> {
    let path = if text.starts_with('/') {
        PathBuf::from(text)
    } else if let Some(rest) = text.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME")?).join(rest)
    } else if !has_scheme(text) && Path::new(text).exists() {
        std::path::absolute(text).ok()?
    } else {
        return None;
    };
    Url::from_file_path(path).ok()
}

/// Whether it starts with a scheme. `localhost:8080` and `example.com:8080/a` do not.
fn has_scheme(text: &str) -> bool {
    let Some((scheme, rest)) = text.split_once(':') else {
        return false;
    };
    let mut characters = scheme.chars();
    let well_formed = characters.next().is_some_and(|c| c.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let is_port = rest
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|port| !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()));
    well_formed && !is_port
}

/// Servers on this computer usually have no certificates.
fn is_this_computer(text: &str) -> bool {
    let host = text.split(['/', ':', '?', '#']).next().unwrap_or_default();
    host.eq_ignore_ascii_case("localhost") || host.starts_with("127.")
}

/// A file at an address: with `only_pages`, what is not a page does not open.
fn local_page(url: &Url, only_pages: bool) -> Target {
    let fragment = fragment_of(url);
    let Ok(path) = url.to_file_path() else {
        return Target::Refused(url.to_string());
    };
    // The real place, which is what shows and what the images of the page then load from.
    let Ok(file) = path.canonicalize() else {
        return Target::Missing(url.to_string());
    };
    let page = |file: &Path, fragment: String| match Url::from_file_path(file) {
        Ok(url) => Target::Page {
            url: url.into(),
            fragment,
        },
        Err(()) => Target::Missing(file.display().to_string()),
    };
    if file.is_dir() {
        return match INDEX_FILES
            .iter()
            .map(|name| file.join(name))
            .find(|index| index.is_file())
        {
            Some(index) => page(&index, fragment),
            None => Target::NotAPage(file.display().to_string()),
        };
    }
    if is_page_file(&file) || (!only_pages && file.is_file()) {
        page(&file, fragment)
    } else {
        Target::NotAPage(file.display().to_string())
    }
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
