//! Finds the charset a page declares, and decodes it.

pub use cpg::LossyText;

/// How far into a page a declaration counts, as in the HTML standard.
const DECLARATION_BYTES: usize = 1024;

/// Detection looks at this much of a page; the rest would only take time.
const DETECTION_BYTES: usize = 256 * 1024;
/// How likely the only remaining encoding must be to count as certain.
const CERTAIN: f64 = 0.95;
const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";
/// How much of a page that says nothing of its encoding is looked at before showing any of it.
const STREAM_DETECTION_BYTES: usize = 64 * 1024;

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

/// How the encoding of a page was found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodingSource {
    ByteOrderMark,
    Server,
    Declared,
    /// The bytes are valid UTF-8 as such. Text in another encoding very rarely is, by chance.
    ValidUtf8,
    Detected,
    /// Detected, but other encodings are possible too.
    Uncertain,
}

/// The encoding found for a page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChosenEncoding {
    pub encoding: String,
    pub source: EncodingSource,
}

/// Maps a web charset label to the name of an encoding, unless it is not known.
#[must_use]
pub fn encoding_for_label(label: &str) -> Option<String> {
    let label = label.trim().to_ascii_lowercase();
    if label == "utf-8" || label == "utf8" {
        return Some("utf8".to_owned());
    }
    // Web pages that say Latin-1 or ASCII really mean Windows-1252.
    if matches!(
        label.as_str(),
        "iso-8859-1" | "latin1" | "us-ascii" | "ascii"
    ) {
        return Some("cp1252".to_owned());
    }
    let name = match label.as_str() {
        "utf-16" | "utf-16le" => "utf16".to_owned(),
        "utf-16be" => "utf16be".to_owned(),
        "utf-32" | "utf-32le" => "utf32".to_owned(),
        "utf-32be" => "utf32be".to_owned(),
        _ => match label.strip_prefix("windows-") {
            Some(rest) => format!("cp{rest}"),
            None => label,
        },
    };
    is_known(&name).then_some(name)
}

/// Finds the encoding of a page from what is known of it. `complete` tells that these are all its
/// bytes; otherwise a character cut off at the end does not count against them being UTF-8.
#[must_use]
pub fn choose_encoding(bytes: &[u8], server_charset: &str, complete: bool) -> ChosenEncoding {
    let chosen = |encoding: &str, source| ChosenEncoding {
        encoding: encoding.to_owned(),
        source,
    };
    if bytes.starts_with(BYTE_ORDER_MARK) {
        return chosen("utf8", EncodingSource::ByteOrderMark);
    }
    if let Some(encoding) = encoding_for_label(server_charset) {
        return chosen(&encoding, EncodingSource::Server);
    }
    if let Some(encoding) = declared(bytes).and_then(|label| encoding_for_label(&label)) {
        return chosen(&encoding, EncodingSource::Declared);
    }
    let valid = match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => !complete && error.error_len().is_none(),
    };
    if valid {
        return chosen("utf8", EncodingSource::ValidUtf8);
    }
    let detected = detect(bytes);
    let source = if detected.certain {
        EncodingSource::Detected
    } else {
        EncodingSource::Uncertain
    };
    chosen(detected.encoding, source)
}

/// Turns the pieces of a page into UTF-8 as they come. It takes the first bytes to know the
/// encoding, so nothing comes out of the first pieces until then.
pub struct StreamDecoder {
    server_charset: String,
    chosen: Option<ChosenEncoding>,
    /// What came before the encoding was known, or the start of a character cut off by a piece.
    waiting: Vec<u8>,
    replaced: bool,
}

impl StreamDecoder {
    #[must_use]
    pub fn new(server_charset: &str) -> Self {
        Self {
            server_charset: server_charset.to_owned(),
            chosen: None,
            waiting: Vec::new(),
            replaced: false,
        }
    }

    /// Takes the next piece, and gives the text that can be told from it.
    pub fn push(&mut self, piece: &[u8]) -> String {
        self.waiting.extend_from_slice(piece);
        if self.chosen.is_none() {
            // What is sure needs no more to go on, while looking at the bytes does.
            let is_sure = self.waiting.starts_with(BYTE_ORDER_MARK)
                || encoding_for_label(&self.server_charset).is_some()
                || self.waiting.len() >= DECLARATION_BYTES && declared(&self.waiting).is_some();
            if !is_sure && self.waiting.len() < STREAM_DETECTION_BYTES {
                return String::new();
            }
            self.choose(false);
        }
        self.decode_waiting(false)
    }

    /// Ends the page, and gives the text that was left.
    pub fn finish(&mut self) -> String {
        if self.chosen.is_none() {
            self.choose(true);
        }
        self.decode_waiting(true)
    }

    fn choose(&mut self, complete: bool) {
        let chosen = choose_encoding(&self.waiting, &self.server_charset, complete);
        if chosen.source == EncodingSource::ByteOrderMark {
            self.waiting.drain(..BYTE_ORDER_MARK.len());
        }
        self.chosen = Some(chosen);
    }

    /// The encoding, once it is known.
    #[must_use]
    pub fn chosen(&self) -> Option<&ChosenEncoding> {
        self.chosen.as_ref()
    }

    /// Whether something invalid had to be replaced.
    #[must_use]
    pub fn replaced(&self) -> bool {
        self.replaced
    }

    fn decode_waiting(&mut self, at_end: bool) -> String {
        let encoding = self
            .chosen
            .as_ref()
            .map_or("utf8", |chosen| chosen.encoding.as_str())
            .to_owned();
        let whole = if at_end {
            self.waiting.len()
        } else {
            whole_characters(&self.waiting, &encoding)
        };
        let text = decode(&self.waiting[..whole], &encoding);
        self.waiting.drain(..whole);
        self.replaced |= text.replaced;
        text.utf8
    }
}

/// How many of these bytes make whole characters, in an encoding where one can take several.
fn whole_characters(bytes: &[u8], encoding: &str) -> usize {
    let is_big_endian = encoding.ends_with("be");
    match encoding {
        "utf8" => match std::str::from_utf8(bytes) {
            Ok(_) => bytes.len(),
            // Only a character that is not complete yet waits. What is invalid goes on to be
            // replaced, so the check can start over after it.
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => {
                let tail_start = bytes.len().saturating_sub(3);
                (tail_start..bytes.len())
                    .find(|&start| {
                        bytes[start] >= 0xC0
                            && matches!(
                                std::str::from_utf8(&bytes[start..]),
                                Err(error) if error.error_len().is_none()
                            )
                    })
                    .unwrap_or(bytes.len())
            }
        },
        "utf16" | "utf16be" => {
            let even = bytes.len() & !1;
            // A surrogate that starts a pair waits for the rest of it.
            if even >= 2 {
                let last = if is_big_endian {
                    bytes[even - 2]
                } else {
                    bytes[even - 1]
                };
                if (0xD8..0xDC).contains(&last) {
                    return even - 2;
                }
            }
            even
        }
        "utf32" | "utf32be" => bytes.len() & !3,
        _ => bytes.len(),
    }
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
