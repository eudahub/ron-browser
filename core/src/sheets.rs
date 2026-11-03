//! The style sheets of a page: those in the page, and those it names, which come in later and in
//! any order, but count in the order the page gives them.

use ron::css::{
    CssTokenKind, SheetSource, Stylesheet, Viewport, document_sheet_sources, matches_media,
    tokenize,
};
use ron::html::Document;
use url::Url;

/// More are not read. This also ends sheets that import each other without end.
const MAX_SHEETS: usize = 128;
const MAX_IMPORT_DEPTH: u8 = 5;

/// A sheet to read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SheetRequest {
    /// What to give back with what comes.
    pub slot: u32,
    /// Where it is: a whole address for a page on the network, and otherwise a path from the page.
    pub location: String,
}

/// A place for a sheet, in the order that they count.
struct Slot {
    id: u32,
    media: Option<String>,
    sheet: Option<Stylesheet>,
    /// Where it is, once known to be needed; nothing for what is in the page itself.
    location: Option<String>,
    depth: u8,
    requested: bool,
}

/// All sheets of a page.
#[derive(Default)]
pub struct Sheets {
    slots: Vec<Slot>,
    next_id: u32,
    /// How many sheets of the document have been taken in.
    sources_seen: usize,
}

impl Sheets {
    /// Finds the sheets of a document at this address, or `None` for a local one, whose paths
    /// count from its directory.
    #[must_use]
    pub fn of_document(document: &Document, page_address: Option<&str>) -> Self {
        let mut sheets = Self::default();
        sheets.add_new_sources(document, page_address);
        sheets
    }

    /// Takes in the sheets of a document that have come since the last time. Returns whether there
    /// were any.
    pub fn add_new_sources(&mut self, document: &Document, page_address: Option<&str>) -> bool {
        let sources = document_sheet_sources(document);
        let new = sources.len() > self.sources_seen;
        for source in sources.into_iter().skip(self.sources_seen) {
            self.sources_seen += 1;
            match source {
                SheetSource::Inline { text, media } => {
                    let sheet = Stylesheet::parse(&text);
                    let imports = sheet.imports.clone();
                    let position = self.push(Some(sheet), media, None, 0);
                    self.add_imports(position, &imports, page_address.unwrap_or(""), 0);
                }
                SheetSource::Link { href, media } => {
                    let location = join_location(page_address.unwrap_or(""), &href);
                    self.push(None, media, Some(location), 0);
                }
            }
        }
        new
    }

    fn push(
        &mut self,
        sheet: Option<Stylesheet>,
        media: Option<String>,
        location: Option<String>,
        depth: u8,
    ) -> usize {
        self.slots
            .push(self.new_slot(sheet, media, location, depth));
        self.next_id += 1;
        self.slots.len() - 1
    }

    fn new_slot(
        &self,
        sheet: Option<Stylesheet>,
        media: Option<String>,
        location: Option<String>,
        depth: u8,
    ) -> Slot {
        Slot {
            id: self.next_id,
            media,
            sheet,
            location,
            depth,
            requested: false,
        }
    }

    /// What an `@import` names counts just before the sheet that names it.
    fn add_imports(&mut self, position: usize, imports: &[Box<str>], base: &str, depth: u8) {
        if depth >= MAX_IMPORT_DEPTH {
            return;
        }
        let media = self.slots[position].media.clone();
        for (offset, target) in imports.iter().enumerate() {
            if self.slots.len() >= MAX_SHEETS {
                return;
            }
            let location = join_location(base, target);
            let slot = self.new_slot(None, media.clone(), location.into(), depth + 1);
            self.next_id += 1;
            self.slots.insert(position + offset, slot);
        }
    }

    /// The sheets to read that have not been asked for yet.
    pub fn requests(&mut self) -> Vec<SheetRequest> {
        let mut requests = Vec::new();
        for slot in &mut self.slots {
            if !slot.requested
                && slot.sheet.is_none()
                && let Some(location) = &slot.location
            {
                slot.requested = true;
                requests.push(SheetRequest {
                    slot: slot.id,
                    location: location.clone(),
                });
            }
        }
        requests
    }

    /// Takes in what came for a slot. Addresses in it count from where it came from.
    pub fn add(&mut self, slot: u32, css: &str) {
        let Some(position) = self.slots.iter().position(|found| found.id == slot) else {
            return;
        };
        let location = self.slots[position].location.clone().unwrap_or_default();
        let sheet = Stylesheet::parse(&with_whole_addresses(css, &location));
        let imports = sheet.imports.clone();
        let depth = self.slots[position].depth;
        self.slots[position].sheet = Some(sheet);
        self.add_imports(position, &imports, &location, depth);
    }

    /// The sheets that hold for a view of this size, in order.
    #[must_use]
    pub fn active(&self, viewport: Viewport) -> Vec<&Stylesheet> {
        self.slots
            .iter()
            .filter(|slot| {
                slot.media
                    .as_deref()
                    .is_none_or(|media| matches_media(media, viewport))
            })
            .filter_map(|slot| slot.sheet.as_ref())
            .collect()
    }

    /// Whether the same sheets hold for views of these two sizes.
    #[must_use]
    pub fn hold_alike(&self, one: Viewport, other: Viewport) -> bool {
        let alike = |media: &str| matches_media(media, one) == matches_media(media, other);
        self.slots
            .iter()
            .all(|slot| slot.media.as_deref().is_none_or(alike))
    }
}

/// Whether a location is an address over HTTPS.
#[must_use]
pub fn is_https(location: &str) -> bool {
    Url::parse(location).is_ok_and(|url| url.scheme() == "https")
}

/// Turns the bytes of a sheet into text. `charset` is what the server says, or empty.
#[must_use]
pub fn decode_css(bytes: &[u8], charset: &str) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let label = charset.to_ascii_lowercase();
    let label = label
        .strip_prefix("windows-")
        .map_or(label.clone(), |rest| format!("cp{rest}"));
    match cpg::by_name(&label) {
        Some(codepage) => codepage.to_utf8_lossy(bytes).utf8,
        None => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Where `target` is, as seen from something at `base`.
fn join_location(base: &str, target: &str) -> String {
    if let Ok(base) = Url::parse(base) {
        return base
            .join(target)
            .map_or_else(|_| target.to_owned(), Into::into);
    }
    let is_relative = !target.starts_with('/')
        && !target
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|first| first.contains(':'));
    match base.rfind('/') {
        Some(end) if is_relative => format!("{}{target}", &base[..=end]),
        _ => target.to_owned(),
    }
}

/// The source of a sheet, with what its `url()` values name counted from where the sheet is, as
/// they count from the page once all sheets of a page are together. What `@import` names stays, as
/// that goes through [`join_location`].
fn with_whole_addresses(css: &str, location: &str) -> String {
    if location.is_empty() || !location.contains('/') {
        return css.to_owned();
    }
    let tokens = tokenize(css);
    let mut out = String::with_capacity(css.len());
    let mut copied = 0;
    let mut in_import = false;
    let mut after_url_function = false;
    let end_of = |index: usize| {
        tokens
            .get(index + 1)
            .map_or(css.len(), |next| next.start as usize)
    };
    let mut replace = |out: &mut String, start: usize, end: usize, value: &str, whole: bool| {
        out.push_str(&css[copied..start]);
        let target = join_location(location, value);
        let quoted = target.replace('\\', "\\\\").replace('"', "\\\"");
        if whole {
            out.push_str("url(\"");
            out.push_str(&quoted);
            out.push_str("\")");
        } else {
            out.push('"');
            out.push_str(&quoted);
            out.push('"');
        }
        copied = end;
    };
    for (index, token) in tokens.iter().enumerate() {
        let start = token.start as usize;
        let end = end_of(index);
        let text = &css[start..end];
        match token.kind {
            CssTokenKind::Whitespace | CssTokenKind::Comment => continue,
            CssTokenKind::AtKeyword => in_import = text.eq_ignore_ascii_case("@import"),
            CssTokenKind::Semicolon => in_import = false,
            CssTokenKind::Url if !in_import => {
                let inner = text.get(4..).unwrap_or_default();
                let inner = inner.strip_suffix(')').unwrap_or(inner).trim();
                if !inner.starts_with("data:") && !inner.contains('\\') {
                    replace(&mut out, start, end, inner, true);
                }
            }
            CssTokenKind::String if after_url_function && !in_import => {
                let mut characters = text.chars();
                let quote = characters.next().unwrap_or('"');
                let inner = characters.as_str();
                let inner = inner.strip_suffix(quote).unwrap_or(inner);
                if !inner.starts_with("data:") && !inner.contains('\\') {
                    replace(&mut out, start, end, inner, false);
                }
            }
            _ => {}
        }
        after_url_function =
            token.kind == CssTokenKind::Function && text.eq_ignore_ascii_case("url(");
    }
    out.push_str(&css[copied..]);
    out
}
