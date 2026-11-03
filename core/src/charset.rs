//! Finds the charset a page declares, and decodes it.

pub use cpg::LossyText;

/// How far into a page a declaration counts, as in the HTML standard.
const DECLARATION_BYTES: usize = 1024;

/// Detection looks at this much of a page; the rest would only take time.
const DETECTION_BYTES: usize = 256 * 1024;
/// How likely the only remaining encoding must be to count as certain.
const CERTAIN: f64 = 0.95;

/// An encoding found by looking at a page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Detected {
    pub encoding: &'static str,
    pub certain: bool,
}

/// Finds the encoding of a page that does not declare one. It looks across all languages, as the
/// language of the system says little about a page's. The language models are large, so they are
/// only unpacked for a page that needs them, and let go of afterwards.
#[must_use]
pub fn detect(bytes: &[u8]) -> Detected {
    let detector = cpg::Detector::new();
    let results = detector.detect(&bytes[..bytes.len().min(DETECTION_BYTES)]);
    let options = cpg::Detector::plausible(&results, CERTAIN);
    match options.as_slice() {
        [] => Detected {
            encoding: "cp1252",
            certain: false,
        },
        [only] => Detected {
            encoding: only.codepage,
            certain: only.confidence >= CERTAIN,
        },
        [first, ..] => Detected {
            encoding: first.codepage,
            certain: false,
        },
    }
}

/// Whether an encoding of this name is known.
#[must_use]
pub fn is_known(encoding: &str) -> bool {
    cpg::by_name(encoding).is_some()
}

/// Decodes a page, replacing what is invalid: a page with a few bad bytes is still worth showing.
/// An unknown encoding counts as UTF-8.
#[must_use]
pub fn decode(bytes: &[u8], encoding: &str) -> LossyText {
    match cpg::by_name(encoding) {
        Some(codepage) => codepage.to_utf8_lossy(bytes),
        None => {
            let utf8 = String::from_utf8_lossy(bytes);
            LossyText {
                replaced: std::str::from_utf8(bytes).is_err(),
                utf8: utf8.into_owned(),
            }
        }
    }
}

/// Like [`decode`], for bytes that are not needed elsewhere. Text that is already right takes no
/// copy, which on a large page saves as much memory as the page takes.
#[must_use]
pub fn decode_owned(bytes: Vec<u8>, encoding: &str) -> LossyText {
    if encoding.eq_ignore_ascii_case("utf8") {
        return match String::from_utf8(bytes) {
            Ok(utf8) => LossyText {
                utf8,
                replaced: false,
            },
            Err(error) => decode(error.as_bytes(), encoding),
        };
    }
    decode(&bytes, encoding)
}

/// Finds `charset=...` in the first bytes of a page, as in
/// `<meta charset="utf-8">` or `<meta http-equiv=content-type content="...; charset=utf-8">`.
/// The declaration must be in a `<meta>` tag.
#[must_use]
pub fn declared(bytes: &[u8]) -> Option<String> {
    let start = &bytes[..bytes.len().min(DECLARATION_BYTES)];
    let lower = start.to_ascii_lowercase();
    let mut position = 0;
    while let Some(found) = find(&lower[position..], b"<meta") {
        let tag_start = position + found;
        let tag_end = find(&lower[tag_start..], b">").map_or(lower.len(), |end| tag_start + end);
        let tag = &lower[tag_start..tag_end];
        if let Some(name) = charset_in_tag(tag) {
            return Some(name);
        }
        position = tag_end;
    }
    None
}

fn charset_in_tag(tag: &[u8]) -> Option<String> {
    let after = &tag[find(tag, b"charset")? + b"charset".len()..];
    let after = after
        .trim_ascii_start()
        .strip_prefix(b"=")?
        .trim_ascii_start();
    let is_end =
        |byte: &u8| matches!(byte, b'"' | b'\'' | b';' | b'/') || byte.is_ascii_whitespace();
    let name = match after.first() {
        Some(b'"' | b'\'') => &after[1..],
        _ => after,
    };
    let end = name.iter().position(is_end).unwrap_or(name.len());
    let name = &name[..end];
    (!name.is_empty() && name.is_ascii()).then(|| String::from_utf8_lossy(name).into_owned())
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
