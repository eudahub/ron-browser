//! An opened page: its tree, styles and layout.

use std::collections::HashSet;

use ron::css::{StyleProgress, Styles, Viewport, compute_styles, extend_styles};
use ron::html::{Document, NodeId, NodeKind, Parser, Tag, parse_owned};

use crate::images::{DecodedImage, ImageStore, NoResources, ResourceLoader};
use crate::layout::{FontSpec, Layout, LayoutProgress, TextMeasurer, layout, layout_more};
use crate::sheets::{SheetRequest, Sheets, decode_css, is_https};

pub struct Page {
    /// Empty while the page is arriving, when the parser holds what has come of it.
    document: Document,
    arriving: Option<Arriving>,
    sheets: Sheets,
    waiting_sheets: Vec<SheetRequest>,
    styles: Styles,
    viewport: Viewport,
    layout: Layout,
    images: ImageStore,
}

/// What a page has while it is still coming in.
struct Arriving {
    parser: Parser,
    styles: StyleProgress,
    layout: LayoutProgress,
    /// How many nodes have been looked at for style sheets and for images.
    sheets_looked_at: usize,
    images_looked_at: usize,
    /// The last style sheet found, until it is complete.
    sheet_to_read: Option<NodeId>,
}

impl Arriving {
    fn new() -> Self {
        Self {
            parser: Parser::new(),
            styles: StyleProgress::new(),
            layout: LayoutProgress::default(),
            sheets_looked_at: 0,
            images_looked_at: 0,
            sheet_to_read: None,
        }
    }

    /// Goes over all that has come again, as style sheets have changed.
    fn start_over(&mut self) {
        self.styles = StyleProgress::new();
        self.layout = LayoutProgress::default();
        self.images_looked_at = 0;
    }
}

/// Reads what a local page can get by itself. Each round brings what the sheets of the last one
/// import, which has a limit. What can only come over HTTPS is left for later.
fn read_local_sheets(
    sheets: &mut Sheets,
    waiting: &mut Vec<SheetRequest>,
    loader: &mut dyn ResourceLoader,
) {
    loop {
        let requests = sheets.requests();
        if requests.is_empty() {
            return;
        }
        for request in requests {
            if is_https(&request.location) {
                waiting.push(request);
            } else if let Some(bytes) = loader.load(&request.location) {
                sheets.add(request.slot, &decode_css(&bytes, ""));
            }
        }
    }
}

/// Loads what an element shows.
fn load_images_of(
    document: &Document,
    styles: &Styles,
    id: NodeId,
    images: &mut ImageStore,
    loader: &mut dyn ResourceLoader,
) {
    if document.kind(id) != NodeKind::Element {
        return;
    }
    if document.tag(id) == Tag::Img
        && let Some(source) = document
            .attributes(id)
            .find(|attribute| attribute.name.eq_ignore_ascii_case("src"))
    {
        images.load(&source.decoded_value(), loader);
    }
    if let Some(url) = &styles.get(id).background_image {
        images.load(url, loader);
    }
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
        Self::open_sized(utf8, loader, images, Viewport::default())
    }

    /// Like [`Self::open_with`], for a view of this size. Styles depend on it, so a later layout at
    /// this size need not compute them again, which takes long on a large page.
    pub fn open_sized(
        utf8: impl Into<String>,
        loader: &mut dyn ResourceLoader,
        images: ImageStore,
        viewport: Viewport,
    ) -> Self {
        let document = parse_owned(utf8.into());
        let mut waiting_sheets = Vec::new();
        let mut sheets = Sheets::of_document(&document, None);
        read_local_sheets(&mut sheets, &mut waiting_sheets, loader);
        let styles = compute_styles(&document, &sheets.active(viewport), viewport);
        let mut page = Self {
            document,
            arriving: None,
            sheets,
            waiting_sheets,
            styles,
            viewport,
            layout: Layout::default(),
            images,
        };
        page.load_images(loader);
        page
    }

    /// Starts a local page that arrives in pieces, for a view of this size. It can be shown and
    /// scrolled after each [`Self::feed`], and only grows downwards.
    #[must_use]
    pub fn open_arriving(viewport: Viewport) -> Self {
        let empty = Parser::new().into_document();
        let styles = Styles::unstyled(&empty);
        Self {
            document: empty,
            arriving: Some(Arriving::new()),
            sheets: Sheets::default(),
            waiting_sheets: Vec::new(),
            styles,
            viewport,
            layout: Layout::default(),
            images: ImageStore::default(),
        }
    }

    /// Whether more of the page is to come.
    #[must_use]
    pub fn is_arriving(&self) -> bool {
        self.arriving.is_some()
    }

    /// Adds the next piece of a page, and lays out what can be known of it.
    pub fn feed(
        &mut self,
        piece: &str,
        loader: &mut dyn ResourceLoader,
        measurer: &mut dyn TextMeasurer,
    ) {
        let Some(arriving) = &mut self.arriving else {
            return;
        };
        arriving.parser.feed(piece);
        self.take_new_sheets(loader);
        self.go_on(Some(loader), measurer);
    }

    /// Ends a page that arrived in pieces, and goes over all of it. Some things can only be known
    /// now, such as which element is the last among its siblings.
    pub fn finish_arriving(
        &mut self,
        loader: &mut dyn ResourceLoader,
        measurer: &mut dyn TextMeasurer,
    ) {
        let Some(mut arriving) = self.arriving.take() else {
            return;
        };
        arriving.parser.finish();
        self.document = arriving.parser.into_document();
        self.sheets.add_new_sources(&self.document, None);
        read_local_sheets(&mut self.sheets, &mut self.waiting_sheets, loader);
        self.styles = compute_styles(
            &self.document,
            &self.sheets.active(self.viewport),
            self.viewport,
        );
        self.load_images(loader);
        self.layout = layout(
            &self.document,
            &self.styles,
            &self.images,
            self.viewport.width,
            measurer,
        );
    }

    /// Notes style sheets that have come in a page that is arriving. All styles then count again.
    fn take_new_sheets(&mut self, loader: &mut dyn ResourceLoader) {
        let Some(arriving) = &mut self.arriving else {
            return;
        };
        let document = arriving.parser.document();
        for id in document.nodes_from(arriving.sheets_looked_at) {
            if matches!(document.tag(id), Tag::Style | Tag::Link) {
                arriving.sheet_to_read = Some(id);
            }
        }
        arriving.sheets_looked_at = document.node_count();
        // A style sheet still being written would count as half of it.
        let Some(last) = arriving.sheet_to_read else {
            return;
        };
        if document.is_open(last) {
            return;
        }
        arriving.sheet_to_read = None;
        if self.sheets.add_new_sources(document, None) {
            read_local_sheets(&mut self.sheets, &mut self.waiting_sheets, loader);
            arriving.start_over();
            self.layout = Layout::default();
        }
    }

    /// Computes styles and layout for all that has come since the last time. Without a loader,
    /// images wait for the next time.
    fn go_on(&mut self, loader: Option<&mut dyn ResourceLoader>, measurer: &mut dyn TextMeasurer) {
        let Some(arriving) = &mut self.arriving else {
            return;
        };
        let document = arriving.parser.document();
        extend_styles(
            document,
            &self.sheets.active(self.viewport),
            self.viewport,
            &mut arriving.styles,
        );
        let styles = arriving.styles.styles();
        if let Some(loader) = loader {
            for id in document.nodes_from(arriving.images_looked_at) {
                load_images_of(document, styles, id, &mut self.images, loader);
            }
            arriving.images_looked_at = document.node_count();
        }
        layout_more(
            document,
            styles,
            &self.images,
            self.viewport.width,
            measurer,
            false,
            &mut arriving.layout,
            &mut self.layout,
        );
    }

    /// Parses a page from this address on the network. What it refers to comes in later, through
    /// [`Self::sheet_requests`] and [`Self::image_sources`], so styles wait for
    /// [`Self::apply_sheets`].
    #[must_use]
    pub fn open_fetched(utf8: impl Into<String>, address: &str, viewport: Viewport) -> Self {
        let document = parse_owned(utf8.into());
        let sheets = Sheets::of_document(&document, Some(address));
        let styles = Styles::unstyled(&document);
        Self {
            document,
            arriving: None,
            sheets,
            waiting_sheets: Vec::new(),
            styles,
            viewport,
            layout: Layout::default(),
            images: ImageStore::default(),
        }
    }

    /// Whether a local page names style sheets that can only come over HTTPS.
    #[must_use]
    pub fn has_waiting_sheets(&self) -> bool {
        !self.waiting_sheets.is_empty()
    }

    /// The style sheets to read that have not been asked for yet.
    pub fn sheet_requests(&mut self) -> Vec<SheetRequest> {
        let mut requests = std::mem::take(&mut self.waiting_sheets);
        requests.extend(self.sheets.requests());
        requests
    }

    /// Adds a style sheet that came. It takes [`Self::apply_sheets`] to count.
    pub fn add_sheet(&mut self, slot: u32, css: &str) {
        self.sheets.add(slot, css);
    }

    /// Computes styles again, with the sheets that have come.
    pub fn apply_sheets(&mut self) {
        if let Some(arriving) = &mut self.arriving {
            // The next layout starts over too, with these styles.
            arriving.start_over();
            self.layout = Layout::default();
            return;
        }
        self.styles = compute_styles(
            &self.document,
            &self.sheets.active(self.viewport),
            self.viewport,
        );
    }

    fn load_images(&mut self, loader: &mut dyn ResourceLoader) {
        let document = &self.document;
        for id in document.descendants(document.root()) {
            load_images_of(document, &self.styles, id, &mut self.images, loader);
        }
    }

    /// The tree as far as it has come.
    fn tree(&self) -> &Document {
        self.arriving
            .as_ref()
            .map_or(&self.document, |arriving| arriving.parser.document())
    }

    /// The styles as far as they have come.
    fn current_styles(&self) -> &Styles {
        self.arriving
            .as_ref()
            .map_or(&self.styles, |arriving| arriving.styles.styles())
    }

    /// What the page names as its images and backgrounds, each once, for reading them elsewhere.
    #[must_use]
    pub fn image_sources(&self) -> Vec<String> {
        let document = self.tree();
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for id in document.descendants(document.root()) {
            if document.kind(id) != NodeKind::Element {
                continue;
            }
            if document.tag(id) == Tag::Img
                && let Some(source) = document
                    .attributes(id)
                    .find(|attribute| attribute.name.eq_ignore_ascii_case("src"))
            {
                let written = source.decoded_value().into_owned();
                if seen.insert(written.clone()) {
                    sources.push(written);
                }
            }
            if let Some(url) = &self.current_styles().get(id).background_image
                && seen.insert(url.to_string())
            {
                sources.push(url.to_string());
            }
        }
        sources
    }

    /// Adds an image read from elsewhere. It takes a new layout to show.
    pub fn add_image(&mut self, written: &str, image: DecodedImage) {
        self.images.insert(written, image);
    }

    /// Lays the page out for a view of this size.
    pub fn lay_out(&mut self, width: f32, height: f32, measurer: &mut dyn TextMeasurer) {
        let viewport = Viewport { width, height };
        if viewport != self.viewport {
            // Conditions and lengths can depend on the size of the view.
            self.viewport = viewport;
            self.apply_sheets();
        }
        if let Some(arriving) = &mut self.arriving {
            arriving.layout = LayoutProgress::default();
            self.layout = Layout::default();
            self.go_on(None, measurer);
            return;
        }
        self.layout = layout(&self.document, &self.styles, &self.images, width, measurer);
    }

    /// How far down the page a link to `#fragment` leads, if there is such a place: an element
    /// with that `id`, or else an `<a>` with that `name`. An empty fragment and `top` lead to the
    /// top, unless the page has a place of that name.
    #[must_use]
    pub fn fragment_y(&self, fragment: &str) -> Option<f32> {
        let document = self.tree();
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
        let document = self.tree();
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
            self.tree(),
            self.current_styles(),
            &self.images,
            0.0,
            &mut NoMeasurer,
        )
        .text
    }
}
