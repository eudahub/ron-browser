//! What C++ can call, and what Rust needs from Qt. The generated glue is the only unsafe code
//! in the browser's Rust.

use std::path::Path;

use ron_browser_core::images::{ImageStore, LocalFiles};
use ron_browser_core::layout::{self, ItemKind, TextMeasurer};
use ron_browser_core::page;
use ron_browser_core::selection::{Selection, Unit};
use ron_browser_core::{Color, charset};

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

        /// Parses a page and computes its styles.
        fn open_page(utf8: &str, directory: &str) -> Box<Page>;
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

fn open_page(utf8: &str, directory: &str) -> Box<Page> {
    let mut files = LocalFiles::new(Path::new(directory));
    Box::new(Page {
        page: page::Page::open_with(utf8, &mut files, ImageStore::default()),
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
