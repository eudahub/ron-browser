//! What C++ can call. The generated glue is the only unsafe code in the browser's Rust.

use ron_browser_core::charset;
use ron_browser_core::page;

/// The engine's page, in a type of this crate as the bridge requires.
pub struct Page(page::Page);

impl Page {
    fn title(&self) -> String {
        self.0.title()
    }

    fn plain_text(&self) -> String {
        self.0.plain_text()
    }
}

#[allow(unsafe_code)]
#[cxx::bridge(namespace = "ron_browser")]
mod ffi {
    extern "Rust" {
        type Page;

        /// Parses a page and computes its styles.
        fn open_page(utf8: &str) -> Box<Page>;
        fn title(self: &Page) -> String;
        fn plain_text(self: &Page) -> String;

        /// The charset a page declares in its first bytes, or an empty string.
        fn declared_charset(bytes: &[u8]) -> String;
        fn is_utf8(bytes: &[u8]) -> bool;
        /// The bytes as UTF-8, with what is invalid replaced.
        fn lossy_utf8(bytes: &[u8]) -> String;
    }
}

fn open_page(utf8: &str) -> Box<Page> {
    Box::new(Page(page::Page::open(utf8)))
}

fn declared_charset(bytes: &[u8]) -> String {
    charset::declared(bytes).unwrap_or_default()
}

fn is_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

fn lossy_utf8(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
