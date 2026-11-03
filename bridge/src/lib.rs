//! What C++ can call, and what Rust needs from Qt. The generated glue is the only unsafe code
//! in the browser's Rust.

use std::path::Path;

use ron_browser_core::Viewport;
use ron_browser_core::images::{ImageStore, LocalFiles};
use ron_browser_core::layout::{self, ItemKind, TextMeasurer};
use ron_browser_core::links::{self, Target};
use ron_browser_core::selection::{Selection, Unit};
use ron_browser_core::{Color, charset};
use ron_browser_core::{history, net, page};

#[allow(unsafe_code)]
#[cxx::bridge(namespace = "ron_browser")]
mod ffi {
    /// A font as CSS asks for it.
    struct FontSpec {
        family: String,
        size: f32,
        weight: u16,
        italic: bool,
    }

    /// One thing to draw, in drawing order.
    #[derive(Clone, Copy)]
    struct DrawItem {
        /// 0 for a filled rectangle, 1 for text, 2 for an image and 3 for a repeated image.
        kind: u8,
        /// The number of the image.
        image: u32,
        /// Where one copy of a repeated image starts.
        origin_x: f32,
        origin_y: f32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        baseline: f32,
        /// As 0xAARRGGBB.
        color: u32,
        font: u32,
        decoration: u8,
    }

    /// Text decoded from a page.
    struct DecodedText {
        utf8: String,
        /// Whether something invalid had to be replaced.
        replaced: bool,
    }

    /// An encoding found by looking at a page.
    struct DetectedEncoding {
        encoding: String,
        certain: bool,
    }

    /// What kind of place a link leads to.
    enum LinkKind {
        SamePage,
        Page,
        NotAPage,
        Missing,
        Unsupported,
        Refused,
    }

    /// What has come of reading a page from the network.
    struct FetchedPage {
        /// False while it is still being read; the rest is empty until then.
        done: bool,
        /// What went wrong, or empty.
        error: String,
        /// Where it really came from, after any redirects.
        address: String,
        bytes: Vec<u8>,
        mime: String,
        charset: String,
    }

    /// Where a link or a typed address leads.
    struct LinkTarget {
        kind: LinkKind,
        /// The address of a page, or what else there is to tell about the place.
        place: String,
        fragment: String,
    }

    /// A rectangle to highlight.
    #[derive(Clone, Copy)]
    struct DrawRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }

    extern "Rust" {
        type Page;

        /// Parses a page and computes its styles for a view of this size.
        fn open_page(utf8: &str, directory: &str, width: f32, height: f32) -> Box<Page>;
        fn title(self: &Page) -> String;
        fn plain_text(self: &Page) -> String;

        fn lay_out(self: &mut Page, width: f32, height: f32);
        fn items(self: &Page) -> &[DrawItem];
        fn item_text(self: &Page, index: usize) -> &str;
        fn layout_width(self: &Page) -> f32;
        fn layout_height(self: &Page) -> f32;
        /// As 0xAARRGGBB; 0 leaves the default background.
        fn canvas_color(self: &Page) -> u32;
        /// The image repeated behind the whole page; `kind` is 0 when there is none.
        fn canvas_tile(self: &Page) -> DrawItem;

        fn image_width(self: &Page, image: u32) -> u32;
        fn image_height(self: &Page, image: u32) -> u32;
        /// Rows of red, green, blue and alpha bytes.
        fn image_pixels(self: &Page, image: u32) -> &[u8];

        /// Starts a selection, by characters, words or blocks after 1, 2 or 3 clicks.
        fn press(self: &mut Page, x: f32, y: f32, clicks: i32);
        /// Moves the end of the selection, as a drag or a click with Shift does.
        fn drag(self: &mut Page, x: f32, y: f32);
        fn extend(self: &mut Page, x: f32, y: f32);
        fn select_all(self: &mut Page);
        fn selected_text(self: &Page) -> String;
        fn highlights(self: &Page, top: f32, bottom: f32) -> Vec<DrawRect>;

        /// The address of the link at a point of the page, or an empty string.
        fn link_at(self: &Page, x: f32, y: f32) -> &str;
        /// How far down the page a place of this name is, or a negative number if it has none.
        fn fragment_y(self: &Page, fragment: &str) -> f32;
        /// Finds where a link in the page at this address leads.
        fn resolve_link(page_address: &str, href: &str) -> LinkTarget;
        /// Finds where an address typed by the user leads.
        fn resolve_address(text: &str) -> LinkTarget;
        /// The address of a local file, or an empty string if there is no such file.
        fn file_address(path: &str) -> String;
        /// The local file at an address, or an empty string if it is not one.
        fn local_path(address: &str) -> String;

        type PageFetch;

        /// Starts reading a page on a thread of its own. `asking_page` is the address of the page
        /// whose link it is, or empty when the user asks for it directly.
        fn start_page_fetch(address: &str, asking_page: &str) -> Box<PageFetch>;
        fn poll(self: &mut PageFetch) -> FetchedPage;

        /// Parses a page from this address on the network, without reading what it refers to.
        fn open_fetched_page(utf8: &str, address: &str, width: f32, height: f32) -> Box<Page>;

        type SheetFetch;

        /// Starts reading the style sheets of a page that came from this address.
        fn start_sheet_fetch(self: &mut Page, page_address: &str) -> Box<SheetFetch>;
        /// Takes the sheets that have arrived, and asks for what they import. Tells whether all
        /// have come.
        fn take_sheets(self: &mut Page, fetch: &mut SheetFetch, page_address: &str) -> bool;
        /// Computes styles again, with the sheets that have come.
        fn apply_sheets(self: &mut Page);

        type ImageFetch;

        /// Starts reading the images of a page that came from this address.
        fn start_image_fetch(self: &Page, page_address: &str) -> Box<ImageFetch>;
        /// Takes the images that have arrived, and tells whether there are any.
        fn take_images(self: &mut Page, fetch: &mut ImageFetch) -> bool;
        fn is_done(self: &mut ImageFetch) -> bool;

        type History;

        fn new_history() -> Box<History>;
        fn visit(self: &mut History, address: &str, fragment: &str);
        fn save_scroll(self: &mut History, scroll_y: i32);
        fn go_back(self: &mut History) -> bool;
        fn go_forward(self: &mut History) -> bool;
        fn can_go_back(self: &History) -> bool;
        fn can_go_forward(self: &History) -> bool;
        fn current_address(self: &History) -> &str;
        fn current_fragment(self: &History) -> &str;
        fn current_scroll(self: &History) -> i32;

        /// The charset a page declares in its first bytes, or an empty string.
        fn declared_charset(bytes: &[u8]) -> String;
        fn is_utf8(bytes: &[u8]) -> bool;
        fn is_known_encoding(encoding: &str) -> bool;
        /// Finds the encoding of a page that does not declare one.
        fn detect_encoding(bytes: &[u8]) -> DetectedEncoding;
        /// The bytes as UTF-8, with what is invalid replaced.
        fn decode_bytes(bytes: &[u8], encoding: &str) -> DecodedText;
    }

    unsafe extern "C++" {
        include!("text_measure.h");

        fn register_font(spec: &FontSpec) -> u32;
        fn text_width(font: u32, text: &str) -> f32;
        fn font_ascent(font: u32) -> f32;
        fn font_descent(font: u32) -> f32;
    }
}

/// Measures text with Qt, through the functions in `ui/textmeasure.cpp`.
struct QtMeasurer;

impl TextMeasurer for QtMeasurer {
    fn font(&mut self, spec: &layout::FontSpec) -> u32 {
        ffi::register_font(&ffi::FontSpec {
            family: spec.family.clone(),
            size: spec.size,
            weight: spec.weight,
            italic: spec.italic,
        })
    }

    fn width(&mut self, font: u32, text: &str) -> f32 {
        ffi::text_width(font, text)
    }

    fn ascent(&mut self, font: u32) -> f32 {
        ffi::font_ascent(font)
    }

    fn descent(&mut self, font: u32) -> f32 {
        ffi::font_descent(font)
    }
}

/// The engine's page, in a type of this crate as the bridge requires.
pub struct Page {
    page: page::Page,
    items: Vec<ffi::DrawItem>,
    selection: Option<Selection>,
}

fn packed(color: Color) -> u32 {
    u32::from(color.alpha) << 24
        | u32::from(color.red) << 16
        | u32::from(color.green) << 8
        | u32::from(color.blue)
}

impl Page {
    fn title(&self) -> String {
        self.page.title()
    }

    fn plain_text(&self) -> String {
        self.page.plain_text()
    }

    fn lay_out(&mut self, width: f32, height: f32) {
        self.page.lay_out(width, height, &mut QtMeasurer);
        let layout = self.page.layout();
        self.items = layout
            .items
            .iter()
            .map(|item| {
                let tile = (item.kind == ItemKind::Tile)
                    .then(|| layout.tiles.get(item.start as usize))
                    .flatten();
                ffi::DrawItem {
                    kind: match item.kind {
                        ItemKind::Rect => 0,
                        ItemKind::Text | ItemKind::Marker => 1,
                        ItemKind::Image => 2,
                        ItemKind::Tile => 3,
                    },
                    image: tile.map_or(item.start, |tile| tile.image),
                    origin_x: tile.map_or(0.0, |tile| tile.origin_x),
                    origin_y: tile.map_or(0.0, |tile| tile.origin_y),
                    x: item.x,
                    y: item.y,
                    width: item.width,
                    height: item.height,
                    baseline: item.baseline,
                    color: packed(item.color),
                    font: item.font,
                    decoration: item.decoration,
                }
            })
            .collect();
    }

    fn items(&self) -> &[ffi::DrawItem] {
        &self.items
    }

    fn item_text(&self, index: usize) -> &str {
        let layout = self.page.layout();
        let Some(item) = layout.items.get(index) else {
            return "";
        };
        let text = match item.kind {
            ItemKind::Marker => &layout.marker_text,
            ItemKind::Text => &layout.text,
            ItemKind::Rect | ItemKind::Image | ItemKind::Tile => return "",
        };
        text.get(item.start as usize..item.end as usize)
            .unwrap_or("")
    }

    fn press(&mut self, x: f32, y: f32, clicks: i32) {
        let unit = match clicks {
            1 => Unit::Character,
            2 => Unit::Word,
            _ => Unit::Block,
        };
        self.selection = Selection::start(self.page.layout(), x, y, unit, &mut QtMeasurer);
    }

    fn drag(&mut self, x: f32, y: f32) {
        if let Some(selection) = &mut self.selection {
            selection.drag_to(self.page.layout(), x, y, &mut QtMeasurer);
        }
    }

    fn extend(&mut self, x: f32, y: f32) {
        match &mut self.selection {
            Some(selection) => selection.extend_to(self.page.layout(), x, y, &mut QtMeasurer),
            None => self.press(x, y, 1),
        }
    }

    fn select_all(&mut self) {
        self.selection = Some(Selection::all(self.page.layout()));
    }

    fn selected_text(&self) -> String {
        self.selection
            .as_ref()
            .map_or_else(String::new, |selection| {
                selection.text(self.page.layout()).to_owned()
            })
    }

    fn highlights(&self, top: f32, bottom: f32) -> Vec<ffi::DrawRect> {
        let Some(selection) = &self.selection else {
            return Vec::new();
        };
        selection
            .highlights(self.page.layout(), top, bottom, &mut QtMeasurer)
            .iter()
            .map(|highlight| ffi::DrawRect {
                x: highlight.x,
                y: highlight.y,
                width: highlight.width,
                height: highlight.height,
            })
            .collect()
    }

    fn start_sheet_fetch(&mut self, page_address: &str) -> Box<SheetFetch> {
        let requests = self.page.sheet_requests();
        Box::new(SheetFetch(net::SheetFetch::start(page_address, requests)))
    }

    fn take_sheets(&mut self, fetch: &mut SheetFetch, page_address: &str) -> bool {
        for (slot, css) in fetch.0.poll() {
            self.page.add_sheet(slot, &css);
        }
        if !fetch.0.is_done() {
            return false;
        }
        let requests = self.page.sheet_requests();
        if requests.is_empty() {
            return true;
        }
        fetch.0 = net::SheetFetch::start(page_address, requests);
        false
    }

    fn apply_sheets(&mut self) {
        self.page.apply_sheets();
    }

    fn start_image_fetch(&self, page_address: &str) -> Box<ImageFetch> {
        let requests = net::image_requests(page_address, &self.page.image_sources());
        Box::new(ImageFetch(net::ImageFetch::start(page_address, requests)))
    }

    fn take_images(&mut self, fetch: &mut ImageFetch) -> bool {
        let images = fetch.0.poll();
        let any = !images.is_empty();
        for (written, image) in images {
            self.page.add_image(&written, image);
        }
        any
    }

    fn link_at(&self, x: f32, y: f32) -> &str {
        self.page.layout().link_at(x, y).unwrap_or("")
    }

    fn fragment_y(&self, fragment: &str) -> f32 {
        self.page.fragment_y(fragment).unwrap_or(-1.0)
    }

    fn layout_width(&self) -> f32 {
        self.page.layout().width
    }

    fn layout_height(&self) -> f32 {
        self.page.layout().height
    }

    fn canvas_color(&self) -> u32 {
        self.page.layout().canvas.map_or(0, packed)
    }

    fn canvas_tile(&self) -> ffi::DrawItem {
        let tile = self.page.layout().canvas_tile;
        ffi::DrawItem {
            kind: if tile.is_some() { 3 } else { 0 },
            image: tile.map_or(0, |tile| tile.image),
            origin_x: tile.map_or(0.0, |tile| tile.origin_x),
            origin_y: tile.map_or(0.0, |tile| tile.origin_y),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            baseline: 0.0,
            color: 0,
            font: 0,
            decoration: 0,
        }
    }

    fn image_width(&self, image: u32) -> u32 {
        self.page.images().get(image).map_or(0, |image| image.width)
    }

    fn image_height(&self, image: u32) -> u32 {
        self.page
            .images()
            .get(image)
            .map_or(0, |image| image.height)
    }

    fn image_pixels(&self, image: u32) -> &[u8] {
        self.page
            .images()
            .get(image)
            .map_or(&[], |image| &image.rgba)
    }
}

/// A page being read, in a type of this crate as the bridge requires.
pub struct PageFetch(net::PageFetch);

fn start_page_fetch(address: &str, asking_page: &str) -> Box<PageFetch> {
    let asking_page = (!asking_page.is_empty()).then_some(asking_page);
    Box::new(PageFetch(net::PageFetch::start(address, asking_page)))
}

impl PageFetch {
    fn poll(&mut self) -> ffi::FetchedPage {
        let mut result = ffi::FetchedPage {
            done: false,
            error: String::new(),
            address: String::new(),
            bytes: Vec::new(),
            mime: String::new(),
            charset: String::new(),
        };
        match self.0.poll() {
            None => {}
            Some(Ok(fetched)) => {
                result.done = true;
                result.address = fetched.address;
                result.bytes = fetched.bytes;
                result.mime = fetched.mime;
                result.charset = fetched.charset;
            }
            Some(Err(error)) => {
                result.done = true;
                result.error = error.to_string();
            }
        }
        result
    }
}

/// The style sheets of a page being read, in a type of this crate as the bridge requires.
pub struct SheetFetch(net::SheetFetch);

fn open_fetched_page(utf8: &str, address: &str, width: f32, height: f32) -> Box<Page> {
    Box::new(Page {
        page: page::Page::open_fetched(utf8, address, Viewport { width, height }),
        items: Vec::new(),
        selection: None,
    })
}

/// The images of a page being read, in a type of this crate as the bridge requires.
pub struct ImageFetch(net::ImageFetch);

impl ImageFetch {
    fn is_done(&mut self) -> bool {
        self.0.is_done()
    }
}

/// The history of a window, in a type of this crate as the bridge requires.
pub struct History(history::History);

fn new_history() -> Box<History> {
    Box::new(History(history::History::default()))
}

impl History {
    fn visit(&mut self, address: &str, fragment: &str) {
        self.0.visit(address, fragment);
    }

    fn save_scroll(&mut self, scroll_y: i32) {
        self.0.save_scroll(scroll_y);
    }

    fn go_back(&mut self) -> bool {
        self.0.go_back()
    }

    fn go_forward(&mut self) -> bool {
        self.0.go_forward()
    }

    fn can_go_back(&self) -> bool {
        self.0.can_go_back()
    }

    fn can_go_forward(&self) -> bool {
        self.0.can_go_forward()
    }

    fn current_address(&self) -> &str {
        self.0.current().map_or("", |entry| &entry.address)
    }

    fn current_fragment(&self) -> &str {
        self.0.current().map_or("", |entry| &entry.fragment)
    }

    fn current_scroll(&self) -> i32 {
        self.0.current().map_or(0, |entry| entry.scroll_y)
    }
}

fn link_target(target: Target) -> ffi::LinkTarget {
    let (kind, place, fragment) = match target {
        Target::SamePage { fragment } => (ffi::LinkKind::SamePage, String::new(), fragment),
        Target::Page { url, fragment } => (ffi::LinkKind::Page, url, fragment),
        Target::NotAPage(place) => (ffi::LinkKind::NotAPage, place, String::new()),
        Target::Missing(place) => (ffi::LinkKind::Missing, place, String::new()),
        Target::Unsupported(place) => (ffi::LinkKind::Unsupported, place, String::new()),
        Target::Refused(place) => (ffi::LinkKind::Refused, place, String::new()),
    };
    ffi::LinkTarget {
        kind,
        place,
        fragment,
    }
}

fn resolve_link(page_address: &str, href: &str) -> ffi::LinkTarget {
    link_target(links::resolve(page_address, href))
}

fn resolve_address(text: &str) -> ffi::LinkTarget {
    link_target(links::resolve_address(text))
}

fn file_address(path: &str) -> String {
    links::file_address(Path::new(path)).unwrap_or_default()
}

fn local_path(address: &str) -> String {
    links::local_path(address)
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_default()
}

fn open_page(utf8: &str, directory: &str, width: f32, height: f32) -> Box<Page> {
    let mut files = LocalFiles::new(Path::new(directory));
    Box::new(Page {
        page: page::Page::open_sized(
            utf8,
            &mut files,
            ImageStore::default(),
            Viewport { width, height },
        ),
        items: Vec::new(),
        selection: None,
    })
}

fn declared_charset(bytes: &[u8]) -> String {
    charset::declared(bytes).unwrap_or_default()
}

fn is_utf8(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

fn is_known_encoding(encoding: &str) -> bool {
    charset::is_known(encoding)
}

fn detect_encoding(bytes: &[u8]) -> ffi::DetectedEncoding {
    let detected = charset::detect(bytes);
    ffi::DetectedEncoding {
        encoding: detected.encoding.to_owned(),
        certain: detected.certain,
    }
}

fn decode_bytes(bytes: &[u8], encoding: &str) -> ffi::DecodedText {
    let decoded = charset::decode(bytes, encoding);
    ffi::DecodedText {
        utf8: decoded.utf8,
        replaced: decoded.replaced,
    }
}
