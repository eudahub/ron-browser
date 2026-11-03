//! An opened page: its tree, styles and layout.

use ron::css::{Styles, Viewport, compute_styles, document_stylesheets};
use ron::html::{Document, Tag, parse};

use crate::layout::{FontSpec, Layout, TextMeasurer, layout};

pub struct Page {
    document: Document,
    styles: Styles,
    viewport: Viewport,
    layout: Layout,
}

/// For reading the text alone: sizes do not matter to which text a page shows.
struct NoMeasurer;

impl TextMeasurer for NoMeasurer {
    fn font(&mut self, _spec: &FontSpec) -> u32 {
        0
    }
    fn width(&mut self, _font: u32, _text: &str) -> f32 {
        0.0
    }
    fn ascent(&mut self, _font: u32) -> f32 {
        0.0
    }
    fn descent(&mut self, _font: u32) -> f32 {
        0.0
    }
}

impl Page {
    /// Parses a page and computes its styles.
    #[must_use]
    pub fn open(utf8: &str) -> Self {
        let document = parse(utf8);
        let viewport = Viewport::default();
        let sheets = document_stylesheets(&document, viewport);
        let styles = compute_styles(&document, &sheets, viewport);
        Self {
            document,
            styles,
            viewport,
            layout: Layout::default(),
        }
    }

    /// Lays the page out for a view of this size.
    pub fn lay_out(&mut self, width: f32, height: f32, measurer: &mut dyn TextMeasurer) {
        let viewport = Viewport { width, height };
        if viewport != self.viewport {
            // Conditions and lengths can depend on the size of the view.
            let sheets = document_stylesheets(&self.document, viewport);
            self.styles = compute_styles(&self.document, &sheets, viewport);
            self.viewport = viewport;
        }
        self.layout = layout(&self.document, &self.styles, width, measurer);
    }

    /// The result of the last [`Self::lay_out`].
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The title, with its white space collapsed.
    #[must_use]
    pub fn title(&self) -> String {
        let document = &self.document;
        let title = document
            .descendants(document.root())
            .find(|&id| document.tag(id) == Tag::Title)
            .and_then(|title| document.first_child(title));
        title.map_or_else(String::new, |text| {
            document
                .decoded_text(text)
                .split_ascii_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
    }

    /// All visible text in document order, as it would be copied.
    #[must_use]
    pub fn plain_text(&self) -> String {
        layout(&self.document, &self.styles, 0.0, &mut NoMeasurer).text
    }
}
