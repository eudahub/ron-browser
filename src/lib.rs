//! The Rust part of the browser. Everything that reads untrusted page content lives here;
//! the C++ part only draws and handles the window.

mod bridge;
pub mod charset;
pub mod page;
