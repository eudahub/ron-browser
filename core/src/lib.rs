//! Everything in the browser that reads untrusted page content. No unsafe code and no C++,
//! so that all of it can be tested with `cargo test`.

pub mod charset;
pub mod page;
