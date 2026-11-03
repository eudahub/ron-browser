//! An opened page: its tree, styles and layout.

use ron::css::{Styles, Viewport, compute_styles, document_stylesheets};
use ron::html::{Document, NodeId, NodeKind, Tag, parse};

use crate::images::{ImageStore, NoResources, ResourceLoader};
use crate::layout::{FontSpec, Layout, TextMeasurer, layout};

pub struct Page {
    document: Document,
    styles: Styles,
    viewport: Viewport,
    layout: Layout,
    images: ImageStore,
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
    /// Parses a page and computes its styles, without loading anything it refers to.
    #[must_use]
    pub fn open(utf8: &str) -> Self {
        Self::open_with(utf8, &mut NoResources, ImageStore::default())
    }

    /// Parses a page, computes its styles, and loads its images through `loader`, on top of
    /// those already in `images`.
    pub fn open_with(utf8: &str, loader: &mut dyn ResourceLoader, images: ImageStore) -> Self {
        let document = parse(utf8);
        let viewport = Viewport::default();
        let sheets = document_stylesheets(&document, viewport);
        let styles = compute_styles(&document, &sheets, viewport);
        let mut page = Self {
            document,
            styles,
            viewport,
            layout: Layout::default(),
            images,
        };
        page.load_images(loader);
        page
    }

    fn load_images(&mut self, loader: &mut dyn ResourceLoader) {
        let document = &self.document;
        for id in document.descendants(document.root()) {
            if document.kind(id) != NodeKind::Element {
                continue;
            }
            if document.tag(id) == Tag::Img
                && let Some(source) = document
                    .attributes(id)
                    .find(|attribute| attribute.name.eq_ignore_ascii_case("src"))
            {
                self.images.load(&source.decoded_value(), loader);
            }
            if let Some(url) = &self.styles.get(id).background_image {
                self.images.load(url, loader);
            }
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
        self.layout = layout(&self.document, &self.styles, &self.images, width, measurer);
    }

    /// How far down the page a link to `#fragment` leads, if there is such a place: an element
    /// with that `id`, or else an `<a>` with that `name`. An empty fragment and `top` lead to the
    /// top, unless the page has a place of that name.
    #[must_use]
    pub fn fragment_y(&self, fragment: &str) -> Option<f32> {
        let document = &self.document;
        let mut by_name = None;
        let has = |id: NodeId, name: &str| {
            document.attributes(id).any(|attribute| {
                attribute.name.eq_ignore_ascii_case(name) && attribute.decoded_value() == fragment
            })
        };
        let mut found = None;
        if !fragment.is_empty() {
            for id in document.descendants(document.root()) {
                if document.kind(id) != NodeKind::Element {
                    continue;
                }
                if has(id, "id") {
                    found = Some(id);
                    break;
                }
                if by_name.is_none() && document.tag(id) == Tag::A && has(id, "name") {
                    by_name = Some(id);
                }
            }
        }
        match found.or(by_name) {
            Some(id) => self.layout.anchor_y(id),
            None => (fragment.is_empty() || fragment.eq_ignore_ascii_case("top")).then_some(0.0),
        }
    }

    /// The images of the page.
    #[must_use]
    pub fn images(&self) -> &ImageStore {
        &self.images
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
        layout(
            &self.document,
            &self.styles,
            &self.images,
            0.0,
            &mut NoMeasurer,
        )
        .text
    }
}
