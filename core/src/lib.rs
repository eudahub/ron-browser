//! Everything in the browser that reads untrusted page content. No unsafe code and no C++,
//! so that all of it can be tested with `cargo test`.

pub use ron::css::{Color, Viewport};

pub mod charset;
pub mod history;
pub mod images;
pub mod layout;
pub mod links;
pub mod net;
pub mod page;
pub mod savepage;
pub mod selection;
pub mod sheets;
pub mod svg;
