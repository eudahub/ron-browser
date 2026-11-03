//! What C++ can call, and what Rust needs from Qt. The generated glue is the only unsafe code
//! in the browser's Rust.

use ron_browser_core::layout::{self, ItemKind, TextMeasurer};
use ron_browser_core::page;
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
        /// 0 for a filled rectangle, otherwise text.
        is_text: bool,
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

    extern "Rust" {
        type Page;

        /// Parses a page and computes its styles.
        fn open_page(utf8: &str) -> Box<Page>;
        fn title(self: &Page) -> String;
        fn plain_text(self: &Page) -> String;

        fn lay_out(self: &mut Page, width: f32, height: f32);
        fn items(self: &Page) -> &[DrawItem];
        fn item_text(self: &Page, index: usize) -> &str;
        fn layout_width(self: &Page) -> f32;
        fn layout_height(self: &Page) -> f32;
        /// As 0xAARRGGBB; 0 leaves the default background.
        fn canvas_color(self: &Page) -> u32;

        /// The charset a page declares in its first bytes, or an empty string.
        fn declared_charset(bytes: &[u8]) -> String;
        fn is_utf8(bytes: &[u8]) -> bool;
        /// The bytes as UTF-8, with what is invalid replaced.
        fn lossy_utf8(bytes: &[u8]) -> String;
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
        self.items = self
            .page
            .layout()
            .items
            .iter()
            .map(|item| ffi::DrawItem {
                is_text: item.kind != ItemKind::Rect,
                x: item.x,
                y: item.y,
                width: item.width,
                height: item.height,
                baseline: item.baseline,
                color: packed(item.color),
                font: item.font,
                decoration: item.decoration,
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
        let text = if item.kind == ItemKind::Marker {
            &layout.marker_text
        } else {
            &layout.text
        };
        text.get(item.start as usize..item.end as usize)
            .unwrap_or("")
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
}

fn open_page(utf8: &str) -> Box<Page> {
    Box::new(Page {
        page: page::Page::open(utf8),
        items: Vec::new(),
    })
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
