//! Turns a styled tree into positioned rectangles and text. Blocks stack, text wraps into
//! lines. Walks the tree without recursion, as elements can nest very deeply.
//!
//! All visible text goes into one string in document order, and everything on screen refers
//! to ranges of it. Selection and copying work on that string, so they follow the text and
//! not the geometry. List markers live apart from it, so they cannot be selected.
//!
//! Floats take their place at the side, and lines of text go around them. Tables are in
//! [`table`], flexible boxes and grids in [`flex`] and positioned boxes in [`positioned`].

mod flex;
mod positioned;
mod table;

pub use positioned::{ClipArea, PaintLayer, VisibleItem};

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::images::ImageStore;

use ron::css::{
    BackgroundRepeat, BorderStyle, Clear, Color, ComputedStyle, Display, Float, LineHeight,
    ListStyleType, Overflow, Size, Styles, TextAlign, TextTransform, VerticalAlign,
    VerticalPosition, Viewport, Visibility, WhiteSpace,
};
use ron::html::{Document, NodeId, NodeKind, Tag};

/// A font as CSS asks for it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    /// The `font-family` value as written.
    pub family: String,
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
}

/// Measures text, which only the toolkit that draws it can do exactly.
pub trait TextMeasurer {
    /// Returns a number for a font, to use with the other methods.
    fn font(&mut self, spec: &FontSpec) -> u32;
    fn width(&mut self, font: u32, text: &str) -> f32;
    fn ascent(&mut self, font: u32) -> f32;
    fn descent(&mut self, font: u32) -> f32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Rect,
    /// Text from [`Layout::text`].
    Text,
    /// Text from [`Layout::marker_text`], which cannot be selected.
    Marker,
    /// The image whose number is `start`, filling the item's box.
    Image,
    /// The image of [`Layout::tiles`] at `start`, repeated to fill the item's box.
    Tile,
}

/// How a repeated image lines up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub image: u32,
    /// Where one copy of the image starts, from which all others follow.
    pub origin_x: f32,
    pub origin_y: f32,
}

/// One thing to draw, in drawing order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Where text sits, for text items.
    pub baseline: f32,
    pub color: Color,
    pub font: u32,
    /// `UNDERLINE`, `OVERLINE` and `LINE_THROUGH` bits.
    pub decoration: u8,
    /// The range of the item's text.
    pub start: u32,
    pub end: u32,
}

/// A line of text, for finding what is under a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    /// The text items of this line, from left to right.
    pub first_item: u32,
    pub item_count: u32,
    /// The range of the text on this line, without the space hidden at a wrap.
    pub start: u32,
    pub end: u32,
}

/// A part of the page that a link covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The number of the link in [`Layout::links`]. A number beyond them means a part of an image
    /// map that leads nowhere, and keeps what is below it from being clicked.
    pub link: u32,
    /// For a part of an image map that is not a rectangle, its number in [`Layout::area_shapes`].
    pub shape: Option<u32>,
}

/// The shape of a part of an image map, in positions of the page.
#[derive(Clone, Debug, PartialEq)]
pub enum AreaShape {
    Circle { x: f32, y: f32, radius: f32 },
    Polygon(Box<[(f32, f32)]>),
}

impl AreaShape {
    fn contains(&self, x: f32, y: f32) -> bool {
        match self {
            Self::Circle {
                x: center_x,
                y: center_y,
                radius,
            } => (x - center_x).powi(2) + (y - center_y).powi(2) <= radius * radius,
            Self::Polygon(corners) => {
                // A ray to the right crosses the edge of a shape an odd number of times from inside.
                let mut inside = false;
                let mut previous = corners.last().copied().unwrap_or_default();
                for &corner in corners {
                    if (corner.1 > y) != (previous.1 > y)
                        && x < (previous.0 - corner.0) * (y - corner.1) / (previous.1 - corner.1)
                            + corner.0
                    {
                        inside = !inside;
                    }
                    previous = corner;
                }
                inside
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub items: Vec<Item>,
    pub lines: Vec<Line>,
    /// All visible text in document order, with line ends between blocks.
    pub text: String,
    pub marker_text: String,
    pub width: f32,
    pub height: f32,
    /// What the area behind the page is filled with.
    pub canvas: Option<Color>,
    pub canvas_tile: Option<Tile>,
    pub tiles: Vec<Tile>,
    /// Whether lines go down the page in order, which floats with text in them can upset.
    pub lines_in_order: bool,
    /// The addresses of the links, as written.
    pub links: Vec<Box<str>>,
    /// In drawing order, so the last under a point is the one on top.
    pub link_areas: Vec<LinkArea>,
    pub area_shapes: Vec<AreaShape>,
    /// Elements that a link can lead to, with where their text starts.
    pub anchors: Vec<(NodeId, u32)>,
    /// While a page is arriving, the first item that does not have its place yet. It and those
    /// after it are not to be drawn.
    pub unplaced_items: Option<u32>,
    /// The same for [`Self::link_areas`].
    pub unplaced_link_areas: Option<u32>,
    /// Ranges of items that draw above others or are cut off, ordered by where they start.
    pub paint_layers: Vec<PaintLayer>,
}

impl Layout {
    /// The address of the link at a point of the page.
    #[must_use]
    pub fn link_at(&self, x: f32, y: f32) -> Option<&str> {
        let placed = self
            .unplaced_link_areas
            .map_or(self.link_areas.len(), |first| first as usize);
        let area = self.link_areas[..placed].iter().rev().find(|area| {
            x >= area.x
                && x < area.x + area.width
                && y >= area.y
                && y < area.y + area.height
                && area.shape.is_none_or(|shape| {
                    self.area_shapes
                        .get(shape as usize)
                        .is_some_and(|shape| shape.contains(x, y))
                })
        })?;
        self.links.get(area.link as usize).map(AsRef::as_ref)
    }

    /// How far down the page an element is, if it is one that a link can lead to.
    #[must_use]
    pub fn anchor_y(&self, node: NodeId) -> Option<f32> {
        let &(_, position) = self.anchors.iter().find(|(anchor, _)| *anchor == node)?;
        let line = self.lines.iter().find(|line| {
            // Text before it may end right there, but a line with no text is really at that place.
            line.end > position || (line.start == position && line.end == position)
        });
        Some(line.map_or(self.height, |line| line.top))
    }
}

/// Collapsed margins that wait for what comes next.
#[derive(Clone, Copy, Default)]
struct Margin {
    positive: f32,
    negative: f32,
}

impl Margin {
    fn add(&mut self, value: f32) {
        self.positive = self.positive.max(value);
        self.negative = self.negative.min(value);
    }

    fn value(self) -> f32 {
        self.positive + self.negative
    }
}

/// Where the normal flow was when a float started.
#[derive(Clone, Copy)]
struct FloatReturn {
    cursor: f32,
    margin: Margin,
    is_left: bool,
    margin_right: f32,
    margin_left: f32,
    margin_top: f32,
    margin_bottom: f32,
}

/// The room a float takes up, with its margins.
#[derive(Clone, Copy)]
struct FloatBox {
    is_left: bool,
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

/// An image or an inline block among the text of a line.
#[derive(Clone, Copy)]
struct Atom {
    node: NodeId,
    image: Option<u32>,
    /// Which of [`Layouter::inline_boxes`] it is, when it is an inline block.
    inline_box: Option<u32>,
    /// The size of its content.
    width: f32,
    height: f32,
}

/// An inline block that has been laid out on its own and waits for a line to take it. All it has
/// put in the layout is at a place of its own, and moves once the line has decided.
struct InlineBox {
    x: f32,
    y: f32,
    width: f32,
    items: Range<usize>,
    lines: Range<usize>,
    link_areas: Range<usize>,
    area_shapes: Range<usize>,
    /// How far down from its top its text sits, if it has a baseline.
    baseline: Option<f32>,
}

/// What goes on around an inline block, while it is being laid out.
struct InlineReturn {
    cursor: f32,
    margin: Margin,
    runs: Vec<Run>,
    marker: Option<Marker>,
    floats: Vec<FloatBox>,
    lowest_float: f32,
    last_line_bottom: Option<f32>,
    last_baseline: Option<f32>,
    items_start: usize,
    lines_start: usize,
    link_areas_start: usize,
    area_shapes_start: usize,
}

struct Frame {
    node: NodeId,
    /// The border box.
    x: f32,
    width: f32,
    /// Unknown until something in this block takes up room, as margins collapse through.
    top: Option<f32>,
    content_x: f32,
    content_width: f32,
    bottom_inset: f32,
    border_top: f32,
    /// Keeps its floats inside, unlike an ordinary block, which lets them hang out.
    contains_floats: bool,
    lowest_float: f32,
    /// What to go back to when this block is a float.
    float_return: Option<FloatReturn>,
    /// What to go back to when this block is an inline block.
    inline_return: Option<Box<InlineReturn>>,
    /// What to go back to when this block is a cell of a table.
    cell_return: Option<Box<InlineReturn>>,
    /// What to go back to when this block is an item of a flexible box.
    flex_return: Option<Box<InlineReturn>>,
    /// What to go back to when this block is out of the flow.
    out_return: Option<Box<positioned::PositionedReturn>>,
    /// Where it started, when it is positioned or hides what goes beyond it.
    span: Option<Box<positioned::FrameSpan>>,
    background_tile: Option<usize>,
    padding_top: f32,
    margin_bottom: f32,
    height: Option<f32>,
    min_height: f32,
    max_height: Option<f32>,
    background_item: Option<usize>,
    /// The number of the last list item inside.
    list_number: i64,
}

#[derive(Clone, Copy)]
struct Run {
    start: usize,
    end: usize,
    font: u32,
    color: Color,
    decoration: u8,
    /// White space that wraps: everything except `pre` and `nowrap`.
    wraps: bool,
    /// Pre-formatted spaces count as text; they do not disappear at a wrap.
    keeps_spaces: bool,
    ascent: f32,
    descent: f32,
    line_height: f32,
    shift: f32,
    background: Color,
    /// An image in place of text. Its text range is empty.
    atom: Option<Atom>,
    /// Text right in the block, not in an inline element.
    direct: bool,
    /// The number of the link it is in.
    link: Option<u32>,
}

impl Run {
    fn same_look(&self, other: &Self) -> bool {
        self.font == other.font
            && self.color == other.color
            && self.decoration == other.decoration
            && self.wraps == other.wraps
            && self.keeps_spaces == other.keeps_spaces
            && self.line_height.to_bits() == other.line_height.to_bits()
            && self.shift.to_bits() == other.shift.to_bits()
            && self.background == other.background
            && self.direct == other.direct
            && self.link == other.link
            && self.atom.is_none()
            && other.atom.is_none()
    }
}

/// A piece of a run placed on the line being built.
#[derive(Clone, Copy)]
struct Piece {
    run: usize,
    start: usize,
    end: usize,
    width: f32,
}

struct Marker {
    text_start: usize,
    text_end: usize,
    font: u32,
    color: Color,
    width: f32,
    right: f32,
    gap: f32,
}

struct Layouter<'a> {
    document: &'a Document,
    styles: &'a Styles,
    images: &'a ImageStore,
    /// The element whose background is that of the whole page, and not its own.
    canvas_node: Option<NodeId>,
    floats: Vec<FloatBox>,
    lowest_float: f32,
    in_float: usize,
    /// Whether the block being collected has anything on its line yet.
    has_inline_content: bool,
    /// Whether what separates it from the text before has been added.
    text_begun: bool,
    /// Whether all of the document is there.
    complete: bool,
    measurer: &'a mut dyn TextMeasurer,
    fonts: HashMap<usize, u32>,
    /// Asking the toolkit for each word is what would take the most time on a long page,
    /// and most words repeat.
    word_widths: HashMap<u32, HashMap<Box<str>, f32>>,
    out: Layout,
    frames: Vec<Frame>,
    cursor: f32,
    margin: Margin,
    runs: Vec<Run>,
    /// A collapsed space, with how it looks where it was written. It is only added once
    /// more text follows, as it never ends a block.
    pending_space: Option<Run>,
    marker: Option<Marker>,
    /// Decorations in force, with the elements that added to them.
    decorations: Vec<(NodeId, u8)>,
    /// Links in force, with their elements.
    open_links: Vec<(NodeId, u32)>,
    maps: Option<HashMap<String, NodeId>>,
    /// How many more parts of image maps a page gets, however often it uses a map.
    map_areas_left: usize,
    inline_boxes: Vec<InlineBox>,
    /// How many inline blocks are open around the current position.
    inline_box_depth: usize,
    last_line_bottom: Option<f32>,
    last_baseline: Option<f32>,
    /// The tables open around the current position. A table is all there before it starts, so
    /// none is open when a layout stops to wait for more.
    tables: Vec<table::TableState>,
    /// What has been worked out about tables, as those around them need it too. Nothing when an
    /// element is not made up as a table.
    table_grids: HashMap<usize, Option<Rc<table::TableGrid>>>,
    /// How many tables and flexible boxes are being measured around the current one.
    grid_depth: usize,
    /// The flexible boxes open around the current position, of which the same holds.
    flexes: Vec<flex::FlexState>,
    flex_contents: HashMap<usize, Option<Rc<flex::FlexContent>>>,
    /// How high the view is, which boxes going by the page count from.
    view_height: f32,
    /// Absolute boxes that wait for the box they go by, innermost last.
    pending_boxes: Vec<positioned::PendingBox>,
    /// How far down positioned boxes reach, as the page can be scrolled that far.
    lowest_positioned: f32,
}

/// Lays out a document in a view of this size.
pub fn layout(
    document: &Document,
    styles: &Styles,
    images: &ImageStore,
    viewport: Viewport,
    measurer: &mut dyn TextMeasurer,
) -> Layout {
    let mut result = Layout::default();
    let mut progress = LayoutProgress::default();
    layout_more(
        document,
        styles,
        images,
        viewport,
        measurer,
        true,
        &mut progress,
        &mut result,
    );
    result.text.shrink_to_fit();
    result.items.shrink_to_fit();
    result.lines.shrink_to_fit();
    result
}

/// Lays out what has come of a document since the last time, adding to `out`. With `complete`
/// false, more is to come: it stops where it cannot know what comes next, which is before the last
/// text and at elements that are still open, and what is waiting for the end of its block does not
/// show until then. What is laid out before stays where it is, so a page only grows downwards.
#[allow(clippy::too_many_arguments)]
pub fn layout_more(
    document: &Document,
    styles: &Styles,
    images: &ImageStore,
    viewport: Viewport,
    measurer: &mut dyn TextMeasurer,
    complete: bool,
    progress: &mut LayoutProgress,
    out: &mut Layout,
) {
    let width = viewport.width;
    let state = progress
        .0
        .take()
        .unwrap_or_else(|| LayoutState::new(document, width));
    let canvas_node = canvas_node(document, styles);
    let mut layouter = Layouter {
        document,
        styles,
        images,
        canvas_node,
        complete,
        measurer,
        out: std::mem::take(out),
        floats: state.floats,
        lowest_float: state.lowest_float,
        in_float: state.in_float,
        has_inline_content: state.has_inline_content,
        text_begun: state.text_begun,
        // These go by where a style is in memory, which can change as more styles come.
        fonts: HashMap::new(),
        word_widths: state.word_widths,
        frames: state.frames,
        cursor: state.cursor,
        margin: state.margin,
        runs: state.runs,
        pending_space: state.pending_space,
        marker: state.marker,
        decorations: state.decorations,
        open_links: state.open_links,
        // The maps of a page that is still arriving can grow.
        maps: None,
        map_areas_left: state.map_areas_left,
        inline_boxes: state.inline_boxes,
        inline_box_depth: state.inline_box_depth,
        last_line_bottom: state.last_line_bottom,
        last_baseline: state.last_baseline,
        tables: Vec::new(),
        table_grids: HashMap::new(),
        grid_depth: 0,
        flexes: Vec::new(),
        flex_contents: HashMap::new(),
        view_height: viewport.height,
        pending_boxes: state.pending_boxes,
        lowest_positioned: state.lowest_positioned,
    };
    let position = layouter.run(state.position);
    if complete {
        layouter.out.unplaced_items = None;
        layouter.out.unplaced_link_areas = None;
    } else {
        layouter.show_so_far();
    }
    *out = layouter.out;
    progress.0 = Some(LayoutState {
        position,
        floats: layouter.floats,
        lowest_float: layouter.lowest_float,
        in_float: layouter.in_float,
        has_inline_content: layouter.has_inline_content,
        text_begun: layouter.text_begun,
        word_widths: layouter.word_widths,
        frames: layouter.frames,
        cursor: layouter.cursor,
        margin: layouter.margin,
        runs: layouter.runs,
        pending_space: layouter.pending_space,
        marker: layouter.marker,
        decorations: layouter.decorations,
        open_links: layouter.open_links,
        map_areas_left: layouter.map_areas_left,
        inline_boxes: layouter.inline_boxes,
        inline_box_depth: layouter.inline_box_depth,
        last_line_bottom: layouter.last_line_bottom,
        last_baseline: layouter.last_baseline,
        pending_boxes: layouter.pending_boxes,
        lowest_positioned: layouter.lowest_positioned,
    });
    positioned::order_layers(&mut out.paint_layers);
    out.width = out.width.max(width);
    if let Some(node) = canvas_node {
        let style = styles.get(node);
        let color = style.background_color;
        out.canvas = (color.alpha != 0).then_some(color);
        out.canvas_tile = background_tile(style, images, 0.0, 0.0, out.width, out.height)
            .map(|(tile, _, _, _, _)| tile);
    }
    out.lines_in_order = out
        .lines
        .windows(2)
        .all(|pair| pair[0].bottom <= pair[1].top);
}

/// How far the layout of a document has come, for going on when more of it has arrived.
#[derive(Default)]
pub struct LayoutProgress(Option<LayoutState>);

/// Where the walk through a document stands.
#[derive(Clone, Copy)]
enum Position {
    /// Nothing is done yet.
    Start,
    /// This node is next.
    At(NodeId),
    /// This node is done. When `entered`, it was entered and has no children yet.
    After {
        node: NodeId,
        entered: bool,
    },
    End,
}

/// What comes after a node that is done.
enum Next {
    Node(NodeId),
    Wait(Position),
    End,
}

/// All that a layout keeps between two parts of a document.
struct LayoutState {
    position: Position,
    floats: Vec<FloatBox>,
    lowest_float: f32,
    in_float: usize,
    has_inline_content: bool,
    text_begun: bool,
    word_widths: HashMap<u32, HashMap<Box<str>, f32>>,
    frames: Vec<Frame>,
    cursor: f32,
    margin: Margin,
    runs: Vec<Run>,
    pending_space: Option<Run>,
    marker: Option<Marker>,
    decorations: Vec<(NodeId, u8)>,
    open_links: Vec<(NodeId, u32)>,
    map_areas_left: usize,
    inline_boxes: Vec<InlineBox>,
    inline_box_depth: usize,
    last_line_bottom: Option<f32>,
    last_baseline: Option<f32>,
    pending_boxes: Vec<positioned::PendingBox>,
    lowest_positioned: f32,
}

impl LayoutState {
    fn new(document: &Document, width: f32) -> Self {
        Self {
            position: Position::Start,
            floats: Vec::new(),
            lowest_float: 0.0,
            in_float: 0,
            has_inline_content: false,
            text_begun: false,
            word_widths: HashMap::new(),
            frames: vec![Frame {
                node: document.root(),
                x: 0.0,
                width,
                top: Some(0.0),
                content_x: 0.0,
                content_width: width,
                bottom_inset: 0.0,
                border_top: 0.0,
                contains_floats: false,
                lowest_float: 0.0,
                float_return: None,
                inline_return: None,
                cell_return: None,
                flex_return: None,
                out_return: None,
                span: None,
                background_tile: None,
                padding_top: 0.0,
                margin_bottom: 0.0,
                height: None,
                min_height: 0.0,
                max_height: None,
                background_item: None,
                list_number: 0,
            }],
            cursor: 0.0,
            margin: Margin::default(),
            runs: Vec::new(),
            pending_space: None,
            marker: None,
            decorations: Vec::new(),
            open_links: Vec::new(),
            map_areas_left: MAX_MAP_AREAS,
            inline_boxes: Vec::new(),
            inline_box_depth: 0,
            last_line_bottom: None,
            last_baseline: None,
            pending_boxes: Vec::new(),
            lowest_positioned: 0.0,
        }
    }
}

/// The element that gives the whole page its background: the root element, or `<body>` when
/// the root has none.
fn canvas_node(document: &Document, styles: &Styles) -> Option<NodeId> {
    let has_background = |id: NodeId| {
        let style = styles.get(id);
        style.background_color.alpha != 0 || style.background_image.is_some()
    };
    let html = document
        .children(document.root())
        .find(|&id| document.kind(id) == NodeKind::Element)?;
    if has_background(html) {
        return Some(html);
    }
    document
        .children(html)
        .find(|&id| document.tag(id) == Tag::Body)
        .filter(|&body| has_background(body))
}

/// Works out how a background image fills a box: how it lines up, and the part of the box it
/// covers, as x, y, width and height.
fn background_tile(
    style: &ComputedStyle,
    images: &ImageStore,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> Option<(Tile, f32, f32, f32, f32)> {
    let number = images.number(style.background_image.as_deref()?)?;
    let image = images.get(number)?;
    #[allow(clippy::cast_precision_loss)]
    let (image_width, image_height) = (image.width as f32, image.height as f32);
    let place = |size: Size, room: f32, own: f32| match size {
        Size::Auto => 0.0,
        Size::Px(px) => px,
        Size::Percent(percent) => (room - own) * percent / 100.0,
        Size::Calc(calc) => calc.of(room - own),
    };
    let origin_x = x + place(style.background_position_x, width, image_width);
    let origin_y = y + place(style.background_position_y, height, image_height);
    let (repeats_x, repeats_y) = match style.background_repeat {
        BackgroundRepeat::Repeat => (true, true),
        BackgroundRepeat::RepeatX => (true, false),
        BackgroundRepeat::RepeatY => (false, true),
        BackgroundRepeat::NoRepeat => (false, false),
    };
    let span = |repeats: bool, start: f32, room: f32, origin: f32, own: f32| {
        if repeats {
            (start, room)
        } else {
            let from = origin.max(start);
            let to = (origin + own).min(start + room);
            (from, (to - from).max(0.0))
        }
    };
    let (cover_x, cover_width) = span(repeats_x, x, width, origin_x, image_width);
    let (cover_y, cover_height) = span(repeats_y, y, height, origin_y, image_height);
    Some((
        Tile {
            image: number,
            origin_x,
            origin_y,
        },
        cover_x,
        cover_y,
        cover_width,
        cover_height,
    ))
}

/// A sunken or raised border is darker on one side: at two thirds of its color, as in Firefox.
fn shaded(color: Color, line: BorderStyle, top_or_left: bool) -> Color {
    let dark = match line {
        BorderStyle::Inset => top_or_left,
        BorderStyle::Outset => !top_or_left,
        _ => false,
    };
    if !dark {
        return color;
    }
    let darker = |channel: u8| u8::try_from(u16::from(channel) * 2 / 3).unwrap_or(channel);
    Color {
        red: darker(color.red),
        green: darker(color.green),
        blue: darker(color.blue),
        alpha: color.alpha,
    }
}

fn resolve(size: Size, whole: f32) -> Option<f32> {
    match size {
        Size::Auto => None,
        Size::Px(px) => Some(px),
        Size::Percent(percent) => Some(whole * percent / 100.0),
        Size::Calc(calc) => Some(calc.of(whole)),
    }
}

/// Elements drawn from something else, apart from images, which there is no support for yet. Their content is
/// a fallback for browsers that lack them, or not for showing at all.
fn is_replaced(document: &Document, id: NodeId) -> bool {
    matches!(
        document.tag(id),
        Tag::Embed | Tag::Iframe | Tag::Applet | Tag::Object
    ) || (document.tag(id) == Tag::Other
        && matches!(document.name(id), "video" | "audio" | "canvas"))
}

/// Whether what shows through an element is its own text, unlike a box that paints itself.
fn is_inline_level_text(display: Display) -> bool {
    matches!(display, Display::Inline | Display::Contents)
}

fn is_block(display: Display) -> bool {
    !matches!(
        display,
        Display::Inline
            | Display::InlineBlock
            | Display::InlineTable
            | Display::Contents
            | Display::InlineFlex
            | Display::InlineGrid
    )
}

impl Layouter<'_> {
    /// Goes on from a position as far as what has come allows, and tells where it got to.
    fn run(&mut self, position: Position) -> Position {
        let document = self.document;
        let mut current = match position {
            Position::Start => match document.first_child(document.root()) {
                Some(first) => Some(first),
                None if self.complete => None,
                None => return Position::Start,
            },
            Position::At(id) => Some(id),
            Position::After { node, entered } => match self.next_after(node, entered) {
                Next::Node(next) => Some(next),
                Next::Wait(position) => return position,
                Next::End => None,
            },
            Position::End => return Position::End,
        };
        while let Some(id) = current {
            // The last text can still grow.
            if document.kind(id) == NodeKind::Text
                && document.next_sibling(id).is_none()
                && document
                    .parent(id)
                    .is_some_and(|parent| self.is_open(parent))
            {
                return Position::At(id);
            }
            // The columns of a table depend on all of it, and so do the items of a flexible box.
            if document.kind(id) == NodeKind::Element
                && self.is_open(id)
                && (table::is_table(self.styles.get(id).display)
                    || flex::is_flex(self.styles.get(id).display)
                    || positioned::is_out_of_flow(self.styles.get(id)))
            {
                if is_block(self.styles.get(id).display) {
                    self.flush_inline();
                }
                return Position::At(id);
            }
            let descend = match document.kind(id) {
                NodeKind::Element => self.enter_element(id),
                NodeKind::Text => {
                    if self.in_flex_structure() {
                        self.add_flex_text(id);
                    } else if !self.in_table_structure() {
                        self.add_text(id);
                    }
                    false
                }
                _ => false,
            };
            let child = if descend {
                document.first_child(id)
            } else {
                None
            };
            current = match child {
                Some(child) => Some(child),
                None => match self.next_after(id, descend) {
                    Next::Node(next) => Some(next),
                    Next::Wait(position) => return position,
                    Next::End => None,
                },
            };
        }
        self.flush_inline();
        self.out.height = (self.cursor + self.margin.value().max(0.0))
            .max(self.lowest_float)
            .max(self.lowest_positioned);
        Position::End
    }

    fn is_open(&self, id: NodeId) -> bool {
        !self.complete && (id == self.document.root() || self.document.is_open(id))
    }

    /// Makes what has been laid out so far ready to show, with more to come. This leaves all as it
    /// was for going on, apart from what gets its final size later anyway.
    fn show_so_far(&mut self) {
        // Inside an inline block, positions are its own until a line takes it.
        let open_box = self
            .frames
            .iter()
            .position(|frame| frame.inline_return.is_some());
        let around = open_box.and_then(|index| self.frames[index].inline_return.as_deref());
        let (cursor, margin, lowest_float) = around
            .map_or((self.cursor, self.margin, self.lowest_float), |saved| {
                (saved.cursor, saved.margin, saved.lowest_float)
            });
        self.out.height = (cursor + margin.value().max(0.0))
            .max(lowest_float)
            .max(self.lowest_positioned);
        // Blocks that are still open reach as far as has come.
        for frame in &self.frames[..open_box.unwrap_or(self.frames.len())] {
            if let (Some(index), Some(top)) = (frame.background_item, frame.top) {
                self.out.items[index].height = (cursor - top).max(0.0);
            }
        }
        // What waits for a line to place it is not at its place yet.
        let waiting = around.map_or(self.runs.as_slice(), |saved| saved.runs.as_slice());
        let first_box = waiting
            .iter()
            .find_map(|run| run.atom.and_then(|atom| atom.inline_box))
            .and_then(|number| self.inline_boxes.get(number as usize));
        self.out.unplaced_items = around
            .map(|saved| saved.items_start)
            .into_iter()
            .chain(first_box.map(|inline_box| inline_box.items.start))
            .min()
            .map(offset);
        self.out.unplaced_link_areas = around
            .map(|saved| saved.link_areas_start)
            .into_iter()
            .chain(first_box.map(|inline_box| inline_box.link_areas.start))
            .min()
            .map(offset);
    }

    /// Finds the next node once `id` is done, leaving the elements that end on the way.
    /// `entered` tells whether `id` itself was entered.
    fn next_after(&mut self, id: NodeId, entered: bool) -> Next {
        let document = self.document;
        if entered {
            // It had no children when it was entered, but they may have come since.
            if let Some(child) = document.first_child(id) {
                return Next::Node(child);
            }
            if self.is_open(id) {
                return Next::Wait(Position::After { node: id, entered });
            }
            self.leave_element(id);
        }
        let mut node = id;
        loop {
            if let Some(sibling) = document.next_sibling(node) {
                return Next::Node(sibling);
            }
            let Some(parent) = document.parent(node) else {
                return Next::End;
            };
            if self.is_open(parent) {
                return Next::Wait(Position::After {
                    node,
                    entered: false,
                });
            }
            if parent == document.root() {
                return Next::End;
            }
            self.leave_element(parent);
            node = parent;
        }
    }

    /// Returns whether to go on to the element's children.
    fn enter_element(&mut self, id: NodeId) -> bool {
        let style = self.styles.get(id);
        if style.display == Display::None || is_replaced(self.document, id) {
            return false;
        }
        let out_of_flow = positioned::is_out_of_flow(style);
        let in_table_structure = !out_of_flow && self.in_table_structure();
        if in_table_structure && !Self::is_table_part(style.display) {
            return false;
        }
        self.note_link_and_anchor(id);
        if out_of_flow {
            if style.text_decoration != 0 && self.document.tag(id) != Tag::Img {
                let all = self.current_decoration() | style.text_decoration;
                self.decorations.push((id, all));
            }
            let goes_on = self.enter_out_of_flow(id, style);
            if goes_on {
                self.begin_inner_layout(id, style);
            }
            return goes_on;
        }
        let in_flex_structure = self.in_flex_structure();
        if self.document.tag(id) == Tag::Img && !in_flex_structure {
            self.add_image(id, style);
            return false;
        }
        if style.text_decoration != 0 && self.document.tag(id) != Tag::Img {
            let all = self.current_decoration() | style.text_decoration;
            self.decorations.push((id, all));
        }
        if in_table_structure {
            self.enter_table_part(id, style);
            return true;
        }
        if in_flex_structure {
            let goes_on = self.enter_flex_item(id, style);
            if goes_on {
                self.begin_inner_layout(id, style);
            }
            return goes_on;
        }
        if style.float != Float::None {
            self.flush_inline();
            self.open_float(id, style);
        } else if self.becomes_inline_box(id, style) {
            self.open_inline_box(id, style);
        } else if is_block(style.display) {
            self.flush_inline();
            let table_width = self.table_content_width(id, style);
            self.open_frame_at(id, style, table_width, None);
        } else if self.document.tag(id) == Tag::Br {
            self.add_line_break(style);
            if style.clear != Clear::None {
                self.flush_inline();
                self.clear_floats(style.clear);
            }
        }
        self.begin_inner_layout(id, style);
        true
    }

    /// Starts on what an element lays out in a way of its own, once its box has been opened.
    fn begin_inner_layout(&mut self, id: NodeId, style: &ComputedStyle) {
        if table::is_table(style.display) {
            self.begin_table(id);
        } else if flex::is_flex(style.display) {
            self.begin_flex(id);
        }
    }

    /// Notes what the element is for links: one itself, or a place that they can lead to.
    fn note_link_and_anchor(&mut self, id: NodeId) {
        let is_a = self.document.tag(id) == Tag::A;
        let mut is_anchor = false;
        for attribute in self.document.attributes(id) {
            if attribute.name.eq_ignore_ascii_case("id")
                || (is_a && attribute.name.eq_ignore_ascii_case("name"))
            {
                is_anchor = true;
            } else if is_a && attribute.name.eq_ignore_ascii_case("href") {
                let number = offset(self.out.links.len());
                self.out.links.push(attribute.decoded_value().into());
                self.open_links.push((id, number));
            }
        }
        if is_anchor {
            let position = offset(self.out.text.len());
            self.out.anchors.push((id, position));
        }
    }

    /// Makes a part of the page lead where the link goes.
    fn push_link_area(&mut self, link: Option<u32>, x: f32, y: f32, width: f32, height: f32) {
        if let Some(link) = link {
            self.out.link_areas.push(LinkArea {
                x,
                y,
                width,
                height,
                link,
                shape: None,
            });
        }
    }

    /// Finds the `<map>` of each name, the first time an image asks for one.
    fn map_named(&mut self, name: &str) -> Option<NodeId> {
        let document = self.document;
        let maps = self.maps.get_or_insert_with(|| {
            let mut maps = HashMap::new();
            for id in document.descendants(document.root()) {
                if document.kind(id) == NodeKind::Element
                    && document.tag(id) == Tag::Other
                    && document.name(id).eq_ignore_ascii_case("map")
                    && let Some(name) = document
                        .attributes(id)
                        .find(|attribute| attribute.name.eq_ignore_ascii_case("name"))
                {
                    maps.entry(name.decoded_value().into_owned()).or_insert(id);
                }
            }
            maps
        });
        maps.get(name).copied()
    }

    /// Makes the parts of an image's map lead where they go, over any link around the image. The
    /// image's content starts at this point, as the positions in a map do.
    fn add_image_map(&mut self, atom: Atom, left: f32, top: f32) {
        let document = self.document;
        let Some(name) = document
            .attributes(atom.node)
            .find(|attribute| attribute.name.eq_ignore_ascii_case("usemap"))
            .map(|attribute| attribute.decoded_value().into_owned())
        else {
            return;
        };
        let Some(map) = self.map_named(name.strip_prefix('#').unwrap_or(&name)) else {
            return;
        };
        let areas: Vec<NodeId> = document
            .descendants(map)
            .filter(|&id| document.kind(id) == NodeKind::Element && document.tag(id) == Tag::Area)
            .collect();
        // The first that covers a point counts, and the last added is looked at first.
        for &area in areas.iter().rev() {
            if self.map_areas_left == 0 {
                return;
            }
            self.map_areas_left -= 1;
            self.add_map_area(area, atom, left, top);
        }
    }

    fn add_map_area(&mut self, area: NodeId, atom: Atom, left: f32, top: f32) {
        let attribute = |name: &str| {
            self.document
                .attributes(area)
                .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
                .map(|attribute| attribute.decoded_value().into_owned())
        };
        let shape =
            attribute("shape").map_or_else(|| "rect".to_owned(), |s| s.to_ascii_lowercase());
        let numbers: Vec<f32> = attribute("coords")
            .unwrap_or_default()
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().unwrap_or(0.0))
            .collect();
        let href = attribute("href");
        let (mut from_x, mut from_y) = (left, top);
        let (mut to_x, mut to_y) = (left + atom.width, top + atom.height);
        let mut shape_number = None;
        match shape.as_str() {
            "default" => {}
            "circle" | "circ" => {
                let &[x, y, radius, ..] = numbers.as_slice() else {
                    return;
                };
                let (x, y) = (left + x, top + y);
                from_x = from_x.max(x - radius);
                from_y = from_y.max(y - radius);
                to_x = to_x.min(x + radius);
                to_y = to_y.min(y + radius);
                shape_number = Some(offset(self.out.area_shapes.len()));
                self.out
                    .area_shapes
                    .push(AreaShape::Circle { x, y, radius });
            }
            "poly" | "polygon" => {
                if numbers.len() < 6 {
                    return;
                }
                let corners: Box<[(f32, f32)]> = numbers
                    .chunks_exact(2)
                    .map(|pair| (left + pair[0], top + pair[1]))
                    .collect();
                let bound =
                    |values: fn(&(f32, f32)) -> f32, start: f32, pick: fn(f32, f32) -> f32| {
                        corners.iter().map(values).fold(start, pick)
                    };
                let (low_x, low_y) = (
                    bound(|c| c.0, f32::INFINITY, f32::min),
                    bound(|c| c.1, f32::INFINITY, f32::min),
                );
                let (high_x, high_y) = (
                    bound(|c| c.0, f32::NEG_INFINITY, f32::max),
                    bound(|c| c.1, f32::NEG_INFINITY, f32::max),
                );
                from_x = from_x.max(low_x);
                from_y = from_y.max(low_y);
                to_x = to_x.min(high_x);
                to_y = to_y.min(high_y);
                shape_number = Some(offset(self.out.area_shapes.len()));
                self.out.area_shapes.push(AreaShape::Polygon(corners));
            }
            "rect" | "rectangle" => {
                let &[x1, y1, x2, y2, ..] = numbers.as_slice() else {
                    return;
                };
                from_x = from_x.max(left + x1.min(x2));
                from_y = from_y.max(top + y1.min(y2));
                to_x = to_x.min(left + x1.max(x2));
                to_y = to_y.min(top + y1.max(y2));
            }
            _ => return,
        }
        let link = match href {
            Some(address) => {
                let number = offset(self.out.links.len());
                self.out.links.push(address.into());
                number
            }
            None => u32::MAX,
        };
        self.out.link_areas.push(LinkArea {
            x: from_x,
            y: from_y,
            width: (to_x - from_x).max(0.0),
            height: (to_y - from_y).max(0.0),
            link,
            shape: shape_number,
        });
    }

    fn current_link(&self) -> Option<u32> {
        self.open_links.last().map(|&(_, link)| link)
    }

    fn leave_element(&mut self, id: NodeId) {
        if self.decorations.last().is_some_and(|&(node, _)| node == id) {
            self.decorations.pop();
        }
        if self
            .tables
            .last()
            .is_some_and(|_| self.in_table_structure())
            && self.frames.last().is_some_and(|frame| frame.node == id)
        {
            self.finish_table();
        }
        if self.in_flex_structure() && self.frames.last().is_some_and(|frame| frame.node == id) {
            self.finish_flex(id);
        }
        if self.frames.last().is_some_and(|frame| frame.node == id) {
            self.flush_inline();
            self.close_frame();
        }
        // After the block, as an inline block goes in its line as part of the link around it.
        if self.open_links.last().is_some_and(|&(node, _)| node == id) {
            self.open_links.pop();
        }
    }

    /// Whether an element makes a box of its own within a line. Form controls do not yet, as
    /// they will need drawing of their own.
    fn becomes_inline_box(&self, id: NodeId, style: &ComputedStyle) -> bool {
        matches!(
            style.display,
            Display::InlineBlock | Display::InlineTable | Display::InlineFlex | Display::InlineGrid
        ) && !matches!(
            self.document.tag(id),
            Tag::Input | Tag::Select | Tag::Textarea | Tag::Button
        ) && self.inline_box_depth < MAX_INLINE_BOX_DEPTH
    }

    /// Starts laying out an inline block by itself. What goes on around it waits until it closes.
    fn open_inline_box(&mut self, id: NodeId, style: &ComputedStyle) {
        self.begin_text();
        self.flush_pending_space();
        let available = self.frames.last().map_or(0.0, |frame| frame.content_width);
        let sides = BoxSides::of(style, available);
        // As wide as what it holds, as far as there is room.
        let wanted = resolve(style.width, available).map(|width| {
            if style.box_sizing == ron::css::BoxSizing::BorderBox {
                (width - sides.insets()).max(0.0)
            } else {
                width
            }
        });
        let content_width = wanted.unwrap_or_else(|| {
            let preferred = self.preferred_width(id, self.inline_box_depth);
            preferred.min((available - sides.horizontal()).max(0.0))
        });
        let saved = Box::new(InlineReturn {
            cursor: self.cursor,
            margin: self.margin,
            runs: std::mem::take(&mut self.runs),
            marker: self.marker.take(),
            floats: std::mem::take(&mut self.floats),
            lowest_float: self.lowest_float,
            last_line_bottom: self.last_line_bottom.take(),
            last_baseline: self.last_baseline.take(),
            items_start: self.out.items.len(),
            lines_start: self.out.lines.len(),
            link_areas_start: self.out.link_areas.len(),
            area_shapes_start: self.out.area_shapes.len(),
        });
        self.margin = Margin::default();
        self.has_inline_content = false;
        // Its text follows that of the line it is in, with nothing between them.
        self.text_begun = true;
        self.lowest_float = 0.0;
        self.inline_box_depth += 1;
        // Decorations of what is around it do not reach inside.
        if self.decorations.last().is_some_and(|&(node, _)| node == id) {
            self.decorations.pop();
        }
        self.decorations.push((id, style.text_decoration));
        self.open_frame_at(id, style, Some(content_width), Some((0.0, 0.0)));
        if let Some(frame) = self.frames.last_mut() {
            frame.inline_return = Some(saved);
        }
    }

    /// Ends an inline block, and hands it to the line it is in.
    fn close_inline_box(
        &mut self,
        node: NodeId,
        saved: InlineReturn,
        (x, width): (f32, f32),
        (top, bottom): (f32, f32),
    ) {
        let style = self.styles.get(node);
        let baseline = if style.overflow == Overflow::Visible {
            self.last_baseline.map(|baseline| baseline - top)
        } else {
            None
        };
        let number = offset(self.inline_boxes.len());
        self.inline_boxes.push(InlineBox {
            x,
            y: top,
            width,
            items: saved.items_start..self.out.items.len(),
            lines: saved.lines_start..self.out.lines.len(),
            link_areas: saved.link_areas_start..self.out.link_areas.len(),
            area_shapes: saved.area_shapes_start..self.out.area_shapes.len(),
            baseline,
        });
        self.inline_box_depth -= 1;
        self.cursor = saved.cursor;
        self.margin = saved.margin;
        self.runs = saved.runs;
        self.pending_space = None;
        self.text_begun = true;
        self.marker = saved.marker;
        self.floats = saved.floats;
        self.lowest_float = saved.lowest_float;
        self.last_line_bottom = saved.last_line_bottom;
        self.last_baseline = saved.last_baseline;
        let available = self.frames.last().map_or(0.0, |frame| frame.content_width);
        let sides = BoxSides::of(style, available);
        let atom = Atom {
            node,
            image: None,
            inline_box: Some(number),
            width: (width - sides.insets()).max(0.0),
            height: (bottom - top - sides.vertical_insets()).max(0.0),
        };
        let start = self.out.text.len();
        let mut run = self.make_run(style, start, start);
        run.atom = Some(atom);
        // What it holds goes by its own white space, but whether a line can break around it is up
        // to what it stands in.
        if let Some(parent) = self.document.parent(node) {
            run.wraps = !matches!(
                self.styles.get(parent).white_space,
                WhiteSpace::Pre | WhiteSpace::Nowrap
            );
        }
        self.runs.push(run);
        self.has_inline_content = true;
    }

    /// Moves all that an inline block has put in the layout.
    fn move_inline_box(&mut self, number: u32, x: f32, y: f32) {
        let Some(inline_box) = self.inline_boxes.get(number as usize) else {
            return;
        };
        let (dx, dy) = (x - inline_box.x, y - inline_box.y);
        let width = inline_box.width;
        let ranges = [
            inline_box.items.clone(),
            inline_box.lines.clone(),
            inline_box.link_areas.clone(),
            inline_box.area_shapes.clone(),
        ];
        self.move_ranges(ranges, dx, dy);
        self.out.width = self.out.width.max(x + width);
    }

    /// Moves what lies in these ranges of the items, lines, link areas and shapes of areas.
    fn move_ranges(&mut self, ranges: [Range<usize>; 4], dx: f32, dy: f32) {
        let [items, lines, link_areas, area_shapes] = ranges;
        let out = &mut self.out;
        for item in &mut out.items[items] {
            item.x += dx;
            item.y += dy;
            match item.kind {
                ItemKind::Text | ItemKind::Marker => item.baseline += dy,
                ItemKind::Tile => {
                    if let Some(tile) = out.tiles.get_mut(item.start as usize) {
                        tile.origin_x += dx;
                        tile.origin_y += dy;
                    }
                }
                ItemKind::Rect | ItemKind::Image => {}
            }
        }
        for line in &mut out.lines[lines] {
            line.top += dy;
            line.bottom += dy;
            line.left += dx;
            line.right += dx;
        }
        for area in &mut out.link_areas[link_areas] {
            area.x += dx;
            area.y += dy;
        }
        for shape in &mut out.area_shapes[area_shapes] {
            match shape {
                AreaShape::Circle { x, y, .. } => {
                    *x += dx;
                    *y += dy;
                }
                AreaShape::Polygon(corners) => {
                    for corner in corners {
                        corner.0 += dx;
                        corner.1 += dy;
                    }
                }
            }
        }
    }

    fn current_decoration(&self) -> u8 {
        self.decorations.last().map_or(0, |&(_, lines)| lines)
    }

    fn open_frame(&mut self, id: NodeId, style: &ComputedStyle) {
        self.open_frame_at(id, style, None, None);
    }

    /// Opens a block. A float passes its width and where it goes, with its margins.
    fn open_frame_at(
        &mut self,
        id: NodeId,
        style: &ComputedStyle,
        forced_width: Option<f32>,
        float_place: Option<(f32, f32)>,
    ) {
        let span = self.start_span(style);
        let parent = self.frames.last().expect("the root frame stays");
        let (parent_x, available) = (parent.content_x, parent.content_width);

        let padding = |size| resolve(size, available).unwrap_or(0.0);
        let padding_left = padding(style.padding.left);
        let padding_right = padding(style.padding.right);
        let padding_top = padding(style.padding.top);
        let padding_bottom = padding(style.padding.bottom);
        let border = style.border_width;
        let insets = padding_left + padding_right + border.left + border.right;

        let margin_left = resolve(style.margin.left, available);
        let margin_right = resolve(style.margin.right, available);
        let mut wanted = forced_width.or_else(|| {
            resolve(style.width, available).map(|width| {
                if style.box_sizing == ron::css::BoxSizing::BorderBox {
                    (width - insets).max(0.0)
                } else {
                    width
                }
            })
        });
        if let Some(most) = resolve(style.max_width, available)
            && wanted.is_some_and(|width| width > most)
        {
            wanted = Some(most);
        }
        if let Some(least) = resolve(style.min_width, available)
            && let Some(width) = wanted
            && width < least
        {
            wanted = Some(least);
        }
        let (offset, content_width) = match wanted {
            None => {
                let left = margin_left.unwrap_or(0.0);
                let right = margin_right.unwrap_or(0.0);
                let mut width = (available - left - right - insets).max(0.0);
                if let Some(most) = resolve(style.max_width, available) {
                    width = width.min(most);
                }
                if let Some(least) = resolve(style.min_width, available) {
                    width = width.max(least);
                }
                (left, width)
            }
            Some(width) => {
                let free = available - width - insets;
                let left = match (margin_left, margin_right) {
                    (None, None) => (free / 2.0).max(0.0),
                    (None, Some(right)) => (free - right).max(0.0),
                    (Some(left), _) => left,
                };
                (left, width)
            }
        };
        let x = float_place.map_or(parent_x + offset, |(float_x, _)| float_x);
        let number = self.next_list_number(id, style);
        let margin_top = resolve(style.margin.top, available).unwrap_or(0.0);
        if float_place.is_none() {
            self.margin.add(margin_top);
            self.apply_clear(style.clear);
        }

        // What is hidden keeps its place and what it holds can show, but it draws nothing itself.
        let paints_background =
            self.canvas_node != Some(id) && style.visibility == Visibility::Visible;
        let background_item = (paints_background && style.background_color.alpha != 0).then(|| {
            self.push_rect(x, 0.0, content_width + insets, 0.0, style.background_color);
            self.out.items.len() - 1
        });
        let background_tile = (paints_background && style.background_image.is_some()).then(|| {
            self.push_rect(x, 0.0, 0.0, 0.0, Color::TRANSPARENT);
            self.out.items.len() - 1
        });
        let height = resolve(style.height, 0.0).filter(|_| matches!(style.height, Size::Px(_)));
        let top = float_place.map(|(_, y)| y);
        self.frames.push(Frame {
            node: id,
            x,
            width: content_width + insets,
            top,
            content_x: x + border.left + padding_left,
            content_width,
            bottom_inset: padding_bottom + border.bottom,
            border_top: border.top,
            contains_floats: style.overflow != Overflow::Visible || float_place.is_some(),
            lowest_float: 0.0,
            float_return: None,
            inline_return: None,
            cell_return: None,
            flex_return: None,
            out_return: None,
            span,
            background_tile,
            padding_top,
            margin_bottom: resolve(style.margin.bottom, available).unwrap_or(0.0),
            height,
            min_height: resolve(style.min_height, 0.0).unwrap_or(0.0),
            max_height: match style.max_height {
                Size::Px(px) => Some(px),
                _ => None,
            },
            background_item,
            list_number: self.start_of_list(id),
        });
        if let Some(y) = top {
            if let Some(index) = background_item {
                self.out.items[index].y = y;
            }
            self.cursor = y + border.top + padding_top;
        } else if border.top > 0.0 || padding_top > 0.0 {
            self.commit_margins();
        }
        if style.display == Display::ListItem {
            self.prepare_marker(id, style, number);
        }
    }

    fn push_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        self.out.items.push(Item {
            kind: ItemKind::Rect,
            x,
            y,
            width,
            height,
            baseline: 0.0,
            color,
            font: 0,
            decoration: 0,
            start: 0,
            end: 0,
        });
    }

    /// Moves past floats that an element must stay clear of.
    fn apply_clear(&mut self, clear: Clear) {
        if clear == Clear::None {
            return;
        }
        let below = self
            .floats
            .iter()
            .filter(|float| match clear {
                Clear::Left => float.is_left,
                Clear::Right => !float.is_left,
                _ => true,
            })
            .map(|float| float.bottom)
            .fold(f32::MIN, f32::max);
        if below > self.cursor + self.margin.value() {
            self.cursor = below;
            self.margin = Margin::default();
        }
    }

    /// Like [`Self::apply_clear`], for a `<br clear>` between lines.
    fn clear_floats(&mut self, clear: Clear) {
        self.apply_clear(clear);
    }

    /// Between which positions there is room at these heights, inside the given limits.
    fn room_at(&self, top: f32, bottom: f32, mut left: f32, mut right: f32) -> (f32, f32) {
        for float in &self.floats {
            if float.top < bottom && float.bottom > top {
                if float.is_left {
                    left = left.max(float.right);
                } else {
                    right = right.min(float.left);
                }
            }
        }
        (left, right)
    }

    /// Finds where a float of this size goes, at this height or further down.
    fn place_float(
        &self,
        is_left: bool,
        width: f32,
        height: f32,
        top: f32,
        limits: (f32, f32),
    ) -> (f32, f32) {
        // A float never goes above one that came before it.
        let mut y = self.floats.last().map_or(top, |float| top.max(float.top));
        loop {
            let (left, right) = self.room_at(y, y + height.max(1.0), limits.0, limits.1);
            let next = self
                .floats
                .iter()
                .filter(|float| float.top < y + height.max(1.0) && float.bottom > y)
                .map(|float| float.bottom)
                .fold(f32::MAX, f32::min);
            if right - left >= width || next == f32::MAX {
                let x = if is_left { left } else { right - width };
                return (x, y);
            }
            y = next;
        }
    }

    /// Remembers the room a float takes. Those long past are let go of, and there is a limit on
    /// how many count at once, so that hostile pages do not make every line slow.
    fn add_float(&mut self, float: FloatBox) {
        if self.in_float == 0 {
            let cursor = self.cursor;
            self.floats.retain(|other| other.bottom > cursor);
        }
        if self.floats.len() >= MAX_ACTIVE_FLOATS {
            self.floats.remove(0);
        }
        self.floats.push(float);
        self.lowest_float = self.lowest_float.max(float.bottom);
        for frame in &mut self.frames {
            if frame.contains_floats {
                frame.lowest_float = frame.lowest_float.max(float.bottom);
            }
        }
        self.out.width = self.out.width.max(float.right);
    }

    /// Starts a float: a block at the side, which the normal flow then goes around.
    fn open_float(&mut self, id: NodeId, style: &ComputedStyle) {
        let parent = self.frames.last().expect("the root frame stays");
        let (limit_left, limit_right) = (parent.content_x, parent.content_x + parent.content_width);
        let available = parent.content_width;
        let depth = self.inline_box_depth;
        let margin = |size| resolve(size, available).unwrap_or(0.0);
        let margin_left = margin(style.margin.left);
        let margin_right = margin(style.margin.right);
        let margin_top = margin(style.margin.top);
        let margin_bottom = margin(style.margin.bottom);
        let padding = |size| resolve(size, available).unwrap_or(0.0);
        let insets = padding(style.padding.left)
            + padding(style.padding.right)
            + style.border_width.left
            + style.border_width.right;
        let extra = margin_left + margin_right + insets;
        // Without a set width, a float is as wide as what it holds, as far as there is room.
        let content_width = resolve(style.width, available).unwrap_or_else(|| {
            self.preferred_width(id, depth)
                .min((available - extra).max(0.0))
        });
        let is_left = style.float == Float::Left;
        let saved = FloatReturn {
            cursor: self.cursor,
            margin: self.margin,
            is_left,
            margin_right,
            margin_left,
            margin_top,
            margin_bottom,
        };
        self.apply_clear(style.clear);
        let top = self.cursor + self.margin.value();
        let (x, y) = self.place_float(
            is_left,
            content_width + extra,
            1.0,
            top,
            (limit_left, limit_right),
        );
        self.margin = Margin::default();
        self.in_float += 1;
        self.open_frame_at(
            id,
            style,
            Some(content_width),
            Some((x + margin_left, y + margin_top)),
        );
        if let Some(frame) = self.frames.last_mut() {
            frame.float_return = Some(saved);
        }
    }

    /// The width of a block's content without any wrapping.
    fn preferred_width(&mut self, id: NodeId, depth: usize) -> f32 {
        self.content_widths(id, depth, false).1
    }

    /// How narrow a block's content can get by wrapping, if `want_least` asks for it, as that takes
    /// measuring each word, and how wide it is without any wrapping. Walks the tree without
    /// recursion, apart from what has a size of its own.
    fn content_widths(&mut self, id: NodeId, depth: usize, want_least: bool) -> (f32, f32) {
        if let Some((least, wanted)) = self.own_layout_widths(id) {
            return (least.ceil(), wanted.ceil());
        }
        let document = self.document;
        let mut least = 0.0_f32;
        let mut widest = 0.0_f32;
        let mut line = 0.0_f32;
        // What the blocks open around the line take across.
        let mut around = 0.0_f32;
        // A space counts once something follows it on the line.
        let mut space = 0.0_f32;
        let mut current = document.first_child(id);
        while let Some(node) = current {
            let mut descend = false;
            match document.kind(node) {
                NodeKind::Element => {
                    let style = self.styles.get(node);
                    // What is out of the flow takes no room in it.
                    if style.display != Display::None
                        && !is_replaced(document, node)
                        && !positioned::is_out_of_flow(style)
                    {
                        let sides = BoxSides::of(style, 0.0);
                        if document.tag(node) == Tag::Img {
                            // A floating image also wants its room beside the text.
                            let (width, _) = self.image_size(node, style, 0.0);
                            line += space + width + sides.horizontal();
                            space = 0.0;
                            least = least.max(width + sides.horizontal() + around);
                        } else if style.float == Float::None
                            && self.becomes_inline_box(node, style)
                            && depth + 1 < MAX_INLINE_BOX_DEPTH
                        {
                            let (smallest, width) = match resolve(style.width, 0.0) {
                                Some(width) => (width, width),
                                None => self.content_widths(node, depth + 1, want_least),
                            };
                            line += space + width + sides.horizontal();
                            space = 0.0;
                            least = least.max(smallest + sides.horizontal() + around);
                        } else if is_block(style.display)
                            && (table::is_table(style.display)
                                || flex::is_flex(style.display)
                                || matches!(style.width, Size::Px(_)))
                        {
                            // It has a size of its own, whatever it holds.
                            widest = widest.max(line + around);
                            line = 0.0;
                            space = 0.0;
                            let (mut smallest, mut width) =
                                self.own_layout_widths(node).unwrap_or((0.0, 0.0));
                            if let Size::Px(own) = style.width {
                                smallest = smallest.max(own);
                                width = smallest;
                            }
                            least = least.max(smallest + sides.horizontal() + around);
                            widest = widest.max(width + sides.horizontal() + around);
                        } else if document.tag(node) == Tag::Br {
                            widest = widest.max(line + around);
                            line = 0.0;
                            space = 0.0;
                            descend = true;
                        } else if is_block(style.display) {
                            widest = widest.max(line + around);
                            line = 0.0;
                            space = 0.0;
                            around += sides.horizontal();
                            descend = true;
                        } else {
                            descend = true;
                        }
                    }
                }
                NodeKind::Text => {
                    let style = self.styles.get(node);
                    let font = self.font_of(style);
                    let text = document.decoded_text(node);
                    let keeps_spaces =
                        matches!(style.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap);
                    if keeps_spaces {
                        for part in text.split('\n') {
                            line += space + self.measurer.width(font, part);
                            space = 0.0;
                            widest = widest.max(line + around);
                            least = least.max(line + around);
                        }
                    } else {
                        if want_least && style.white_space != WhiteSpace::Nowrap {
                            for word in text.split_ascii_whitespace() {
                                least = least.max(self.measured_word(font, word) + around);
                            }
                        }
                        let collapsed = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
                        let space_width = self.measurer.width(font, " ");
                        if text.starts_with(|c: char| c.is_ascii_whitespace()) && line > 0.0 {
                            space = space_width;
                        }
                        if !collapsed.is_empty() {
                            line += space + self.measurer.width(font, &collapsed);
                            space = 0.0;
                            if text.ends_with(|c: char| c.is_ascii_whitespace()) {
                                space = space_width;
                            }
                            if style.white_space == WhiteSpace::Nowrap {
                                least = least.max(line + around);
                            }
                        }
                    }
                }
                _ => {}
            }
            let child = if descend {
                document.first_child(node)
            } else {
                None
            };
            current = child.or_else(|| {
                let mut up = node;
                loop {
                    if let Some(sibling) = document.next_sibling(up) {
                        return Some(sibling);
                    }
                    up = document.parent(up)?;
                    if up == id {
                        return None;
                    }
                    let style = self.styles.get(up);
                    if is_block(style.display) {
                        widest = widest.max(line + around);
                        line = 0.0;
                        space = 0.0;
                        around -= BoxSides::of(style, 0.0).horizontal();
                    }
                }
            });
        }
        (least.ceil(), widest.max(line + around).ceil())
    }

    /// How narrow and how wide an element is that lays out what it holds in a way of its own.
    fn own_layout_widths(&mut self, id: NodeId) -> Option<(f32, f32)> {
        let display = self.styles.get(id).display;
        if table::is_table(display) {
            self.table_widths(id)
        } else if flex::is_flex(display) {
            self.flex_widths(id)
        } else {
            None
        }
    }

    /// The width of a word, remembered per font.
    fn measured_word(&mut self, font: u32, word: &str) -> f32 {
        let widths = self.word_widths.entry(font).or_default();
        if let Some(&width) = widths.get(word) {
            return width;
        }
        let width = self.measurer.width(font, word);
        widths.insert(word.into(), width);
        width
    }

    /// The size of an image's content box: what is set, or what the file has, keeping its shape.
    fn image_size(&self, id: NodeId, style: &ComputedStyle, available: f32) -> (f32, f32) {
        let natural = self.image_of(id).and_then(|number| self.images.get(number));
        #[allow(clippy::cast_precision_loss)]
        let (own_width, own_height) = natural.map_or((0.0, 0.0), |image| {
            (image.width as f32, image.height as f32)
        });
        let width = resolve(style.width, available);
        let height = match style.height {
            Size::Px(px) => Some(px),
            _ => None,
        };
        match (width, height) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) if own_width > 0.0 => (width, width * own_height / own_width),
            (None, Some(height)) if own_height > 0.0 => (height * own_width / own_height, height),
            (Some(width), None) => (width, 0.0),
            (None, Some(height)) => (0.0, height),
            (None, None) => (own_width, own_height),
        }
    }

    fn image_of(&self, id: NodeId) -> Option<u32> {
        let source = self
            .document
            .attributes(id)
            .find(|attribute| attribute.name.eq_ignore_ascii_case("src"))?
            .decoded_value();
        self.images.number(&source)
    }

    /// Adds an image: to the line, as a block, or as a float. One that is missing shows its
    /// alternative text, or takes up only the room asked for.
    fn add_image(&mut self, id: NodeId, style: &ComputedStyle) {
        let image = self.image_of(id);
        let available = self.frames.last().map_or(0.0, |frame| frame.content_width);
        let (width, height) = self.image_size(id, style, available);
        if image.is_none() && (width == 0.0 || height == 0.0) {
            let alternative = self
                .document
                .attributes(id)
                .find(|attribute| attribute.name.eq_ignore_ascii_case("alt"))
                .map(|attribute| attribute.decoded_value().into_owned());
            if let Some(text) = alternative
                && let Some(start) = self.add_collapsed(style, &text, false)
            {
                let end = self.out.text.len();
                let run = self.make_run(style, start, end);
                self.push_run(run);
                self.has_inline_content = true;
            }
            return;
        }
        let atom = Atom {
            node: id,
            image,
            inline_box: None,
            width,
            height,
        };
        if style.float != Float::None {
            self.place_image_float(atom, style);
        } else if is_block(style.display) {
            self.flush_inline();
            self.add_block_image(atom, style);
        } else {
            self.flush_pending_space();
            let start = self.out.text.len();
            let mut run = self.make_run(style, start, start);
            run.atom = Some(atom);
            self.runs.push(run);
            self.has_inline_content = true;
        }
    }

    /// A float that is an image goes in place at once, level with the line being collected.
    fn place_image_float(&mut self, atom: Atom, style: &ComputedStyle) {
        let frame = self.frames.last().expect("the root frame stays");
        let (limit_left, limit_right) = (frame.content_x, frame.content_x + frame.content_width);
        let available = frame.content_width;
        let sides = BoxSides::of(style, available);
        let total_width = atom.width + sides.horizontal();
        let total_height = atom.height + sides.vertical();
        let is_left = style.float == Float::Left;
        self.apply_clear(style.clear);
        let top = self.cursor + self.margin.value();
        let (x, y) = self.place_float(
            is_left,
            total_width,
            total_height,
            top,
            (limit_left, limit_right),
        );
        self.push_link_area(self.current_link(), x, y, total_width, total_height);
        self.draw_image_box(atom, style, &sides, x, y);
        self.add_float(FloatBox {
            is_left,
            left: x,
            right: x + total_width,
            top: y,
            bottom: y + total_height,
        });
    }

    fn add_block_image(&mut self, atom: Atom, style: &ComputedStyle) {
        let frame = self.frames.last().expect("the root frame stays");
        let (left, available) = (frame.content_x, frame.content_width);
        let sides = BoxSides::of(style, available);
        self.margin.add(sides.margin_top);
        self.apply_clear(style.clear);
        self.commit_margins();
        let top = self.cursor - sides.margin_top;
        let outer_width = atom.width + sides.horizontal();
        let outer_height = atom.height + sides.vertical();
        self.push_link_area(self.current_link(), left, top, outer_width, outer_height);
        self.draw_image_box(atom, style, &sides, left, top);
        self.cursor =
            top + sides.margin_top + sides.vertical() - sides.margin_top - sides.margin_bottom
                + atom.height;
        self.margin.add(sides.margin_bottom);
    }

    /// Draws an image with its border, at the outer corner of its margins.
    fn draw_image_box(
        &mut self,
        atom: Atom,
        style: &ComputedStyle,
        sides: &BoxSides,
        x: f32,
        y: f32,
    ) {
        if style.visibility != Visibility::Visible {
            return;
        }
        let left = x + sides.margin_left;
        let top = y + sides.margin_top;
        let outer_width = atom.width
            + sides.padding_left
            + sides.padding_right
            + sides.border_left
            + sides.border_right;
        let outer_height = atom.height
            + sides.padding_top
            + sides.padding_bottom
            + sides.border_top
            + sides.border_bottom;
        if style.background_color.alpha != 0 {
            self.push_rect(left, top, outer_width, outer_height, style.background_color);
        }
        self.push_borders(style, left, top, outer_width, top + outer_height);
        let content_left = left + sides.border_left + sides.padding_left;
        let content_top = top + sides.border_top + sides.padding_top;
        self.add_image_map(atom, content_left, content_top);
        if let Some(number) = atom.image {
            self.out.items.push(Item {
                kind: ItemKind::Image,
                x: content_left,
                y: content_top,
                width: atom.width,
                height: atom.height,
                baseline: 0.0,
                color: Color::TRANSPARENT,
                font: 0,
                decoration: 0,
                start: number,
                end: number,
            });
        }
    }

    /// Places the block's top, and those of the blocks around it still waiting, for good.
    fn commit_margins(&mut self) {
        let y = self.cursor + self.margin.value();
        self.cursor = y;
        self.margin = Margin::default();
        let mut advance = 0.0;
        let mut waiting = self.frames.len();
        while waiting > 0 && self.frames[waiting - 1].top.is_none() {
            waiting -= 1;
        }
        for frame in &mut self.frames[waiting..] {
            frame.top = Some(y + advance);
            if let Some(index) = frame.background_item {
                self.out.items[index].y = y + advance;
            }
            advance += frame.border_top + frame.padding_top;
        }
        self.cursor = y + advance;
    }

    fn close_frame(&mut self) {
        if self.frames.len() < 2 {
            return;
        }
        let needs_room = {
            let frame = self.frames.last().expect("checked above");
            frame.bottom_inset > 0.0
                || frame.height.is_some()
                || frame.min_height > 0.0
                || frame.lowest_float > 0.0
                || frame.cell_return.is_some()
                || frame.flex_return.is_some()
                || frame.out_return.is_some()
        };
        if needs_room {
            if self.frames.last().is_some_and(|frame| frame.top.is_some()) {
                self.cursor += self.margin.value();
                self.margin = Margin::default();
            } else {
                self.commit_margins();
            }
        }
        let mut frame = self.frames.pop().expect("checked above");
        let node = frame.node;
        let Some(top) = frame.top else {
            // Nothing took up room, so its margins collapse together.
            self.margin.add(frame.margin_bottom);
            let y = self.cursor + self.margin.value();
            if let Some(index) = frame.background_item {
                self.out.items[index].y = y;
            }
            let depth = self.frames.len();
            self.place_boxes_of(depth, &frame, y, y);
            return;
        };
        let content_top = top + frame.border_top + frame.padding_top;
        let mut content_height = (self.cursor - content_top).max(0.0);
        if frame.contains_floats {
            content_height = content_height.max(frame.lowest_float - content_top);
        }
        if let Some(height) = frame.height {
            content_height = height;
        }
        content_height = content_height.max(frame.min_height);
        if let Some(most) = frame.max_height {
            content_height = content_height.min(most);
        }
        let bottom = content_top + content_height + frame.bottom_inset;
        let depth = self.frames.len();
        self.place_boxes_of(depth, &frame, top, bottom);
        if let Some(saved) = frame.cell_return.take() {
            self.finish_span(&frame, top, bottom);
            self.close_cell(&frame, *saved, top, bottom);
            return;
        }
        if let Some(saved) = frame.flex_return.take() {
            self.finish_span(&frame, top, bottom);
            self.close_flex_item(&frame, *saved, top, bottom);
            return;
        }
        if let Some(saved) = frame.out_return.take() {
            self.close_out_of_flow(&frame, *saved, top, bottom);
            return;
        }
        self.cursor = bottom;
        if let Some(index) = frame.background_item {
            self.out.items[index].height = bottom - top;
        }
        if let Some(index) = frame.background_tile {
            self.finish_background_tile(node, index, frame.x, top, frame.width, bottom - top);
        }
        self.push_borders(self.styles.get(node), frame.x, top, frame.width, bottom);
        self.out.width = self.out.width.max(frame.x + frame.width);
        self.finish_span(&frame, top, bottom);
        self.shift_relative(&frame);
        if let Some(saved) = frame.inline_return {
            self.close_inline_box(node, *saved, (frame.x, frame.width), (top, bottom));
            return;
        }
        if let Some(saved) = frame.float_return {
            self.in_float -= 1;
            let (left, right) = (
                frame.x - saved.margin_left,
                frame.x + frame.width + saved.margin_right,
            );
            self.add_float(FloatBox {
                is_left: saved.is_left,
                left,
                right,
                top: top - saved.margin_top,
                bottom: bottom + saved.margin_bottom,
            });
            self.cursor = saved.cursor;
            self.margin = saved.margin;
            return;
        }
        self.margin.add(frame.margin_bottom);
    }

    /// Fills in the item held for a background image, now that the box is known.
    fn finish_background_tile(
        &mut self,
        node: NodeId,
        index: usize,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        let Some((tile, cover_x, cover_y, cover_width, cover_height)) =
            background_tile(self.styles.get(node), self.images, x, y, width, height)
        else {
            return;
        };
        let number = offset(self.out.tiles.len());
        self.out.tiles.push(tile);
        let item = &mut self.out.items[index];
        item.kind = ItemKind::Tile;
        item.x = cover_x;
        item.y = cover_y;
        item.width = cover_width;
        item.height = cover_height;
        item.start = number;
        item.end = number;
    }

    fn push_borders(&mut self, style: &ComputedStyle, x: f32, top: f32, width: f32, bottom: f32) {
        if style.visibility != Visibility::Visible {
            return;
        }
        let widths = style.border_width;
        let colors = style.border_color;
        let lines = style.border_style;
        let sides = [
            (
                x,
                top,
                width,
                widths.top,
                shaded(colors.top, lines.top, true),
            ),
            (
                x,
                bottom - widths.bottom,
                width,
                widths.bottom,
                shaded(colors.bottom, lines.bottom, false),
            ),
            (
                x,
                top,
                widths.left,
                bottom - top,
                shaded(colors.left, lines.left, true),
            ),
            (
                x + width - widths.right,
                top,
                widths.right,
                bottom - top,
                shaded(colors.right, lines.right, false),
            ),
        ];
        for (x, y, width, height, color) in sides {
            if width > 0.0 && height > 0.0 && color.alpha != 0 {
                self.push_rect(x, y, width, height, color);
            }
        }
    }

    /// The number before the first item of a list, from its `start` attribute.
    fn start_of_list(&self, id: NodeId) -> i64 {
        if self.document.tag(id) != Tag::Ol {
            return 0;
        }
        self.document
            .attributes(id)
            .find(|attribute| attribute.name.eq_ignore_ascii_case("start"))
            .and_then(|attribute| attribute.value)
            .and_then(|value| value.trim().parse::<i64>().ok())
            .map_or(0, |start| start.saturating_sub(1))
    }

    fn next_list_number(&mut self, id: NodeId, style: &ComputedStyle) -> i64 {
        if style.display != Display::ListItem {
            return 0;
        }
        let value = self
            .document
            .attributes(id)
            .find(|attribute| attribute.name.eq_ignore_ascii_case("value"))
            .and_then(|attribute| attribute.value)
            .and_then(|value| value.trim().parse::<i64>().ok());
        let parent = self.frames.last_mut().expect("the root frame stays");
        parent.list_number = value.unwrap_or(parent.list_number.saturating_add(1));
        parent.list_number
    }

    fn font_of(&mut self, style: &ComputedStyle) -> u32 {
        let key = std::ptr::from_ref(style) as usize;
        if let Some(&font) = self.fonts.get(&key) {
            return font;
        }
        let spec = FontSpec {
            family: style.font_family.to_string(),
            size: style.font_size,
            weight: style.font_weight,
            italic: style.font_style != ron::css::FontStyle::Normal,
        };
        let font = self.measurer.font(&spec);
        self.fonts.insert(key, font);
        font
    }

    fn prepare_marker(&mut self, id: NodeId, style: &ComputedStyle, number: i64) {
        if style.visibility != Visibility::Visible {
            return;
        }
        let Some(text) = marker_text(style.list_style_type, number) else {
            return;
        };
        let font = self.font_of(style);
        let width = self.measurer.width(font, &text);
        let gap = if text.ends_with('.') {
            self.measurer.width(font, " ")
        } else {
            style.font_size * MARKER_GAP
        };
        let text_start = self.out.marker_text.len();
        self.out.marker_text.push_str(&text);
        let right = self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.node == id)
            .map_or(0.0, |frame| frame.content_x);
        self.marker = Some(Marker {
            text_start,
            text_end: self.out.marker_text.len(),
            font,
            color: style.color,
            width,
            right,
            gap,
        });
    }

    fn add_line_break(&mut self, style: &ComputedStyle) {
        self.pending_space = None;
        self.begin_text();
        let start = self.out.text.len();
        self.out.text.push('\n');
        let run = self.make_run(style, start, start + 1);
        self.runs.push(run);
        self.has_inline_content = true;
    }

    /// Before the first text of a block, separates it from the text before, as far as
    /// line ends from `<br>` have not already done so.
    fn begin_text(&mut self) {
        if std::mem::replace(&mut self.text_begun, true) || self.out.text.is_empty() {
            return;
        }
        let gap = self
            .last_line_bottom
            .map_or(0.0, |bottom| self.cursor + self.margin.value() - bottom);
        // Blocks with room between them read as paragraphs when copied.
        let wanted = if gap >= 4.0 { 2 } else { 1 };
        let present = self
            .out
            .text
            .bytes()
            .rev()
            .take_while(|&byte| byte == b'\n')
            .count();
        for _ in present..wanted {
            self.out.text.push('\n');
        }
    }

    fn make_run(&mut self, style: &ComputedStyle, start: usize, end: usize) -> Run {
        let font = self.font_of(style);
        let ascent = self.measurer.ascent(font);
        let descent = self.measurer.descent(font);
        let line_height = match style.line_height {
            LineHeight::Normal => ascent + descent,
            LineHeight::Multiple(multiple) => multiple * style.font_size,
            LineHeight::Px(px) => px,
        };
        let shift = match style.vertical_align {
            VerticalPosition::Keyword(VerticalAlign::Sub) => style.font_size * 0.25,
            VerticalPosition::Keyword(VerticalAlign::Super) => -style.font_size * 0.5,
            VerticalPosition::Px(px) => -px,
            VerticalPosition::Percent(percent) => -line_height * percent / 100.0,
            VerticalPosition::Keyword(_) => 0.0,
        };
        Run {
            start,
            end,
            font,
            color: style.color,
            decoration: self.current_decoration() | style.text_decoration,
            wraps: !matches!(style.white_space, WhiteSpace::Pre | WhiteSpace::Nowrap),
            keeps_spaces: matches!(style.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap),
            ascent,
            descent,
            line_height,
            shift,
            // A block paints its own background, once, behind all its lines.
            background: if is_block(style.display)
                || (self.inline_box_depth > 0 && !is_inline_level_text(style.display))
            {
                Color::TRANSPARENT
            } else {
                style.background_color
            },
            atom: None,
            direct: false,
            link: self.current_link(),
        }
    }

    fn add_text(&mut self, id: NodeId) {
        let style = self.styles.get(id);
        if style.visibility != Visibility::Visible {
            return;
        }
        let decoded = self.document.decoded_text(id);
        let transformed = apply_transform(&decoded, style.text_transform);
        let start = match style.white_space {
            WhiteSpace::Pre | WhiteSpace::PreWrap => {
                if transformed.is_empty() {
                    return;
                }
                self.begin_text();
                self.flush_pending_space();
                let start = self.out.text.len();
                self.out.text.push_str(&transformed);
                self.has_inline_content = true;
                start
            }
            WhiteSpace::Normal | WhiteSpace::Nowrap | WhiteSpace::PreLine => {
                let keeps_lines = style.white_space == WhiteSpace::PreLine;
                let Some(start) = self.add_collapsed(style, &transformed, keeps_lines) else {
                    return;
                };
                start
            }
        };
        let end = self.out.text.len();
        let mut run = self.make_run(style, start, end);
        run.direct = self.document.parent(id) == self.frames.last().map(|frame| frame.node);
        self.push_run(run);
    }

    /// Adds the space waiting from earlier text, which never starts a block or a line.
    fn flush_pending_space(&mut self) {
        let Some(mut run) = self.pending_space.take() else {
            return;
        };
        if !self.has_inline_content || self.out.text.ends_with('\n') {
            return;
        }
        run.start = self.out.text.len();
        self.out.text.push(' ');
        run.end = self.out.text.len();
        self.push_run(run);
    }

    /// Adds a run, joining it to the last one when it looks the same and directly follows.
    fn push_run(&mut self, run: Run) {
        if let Some(last) = self.runs.last_mut()
            && last.end == run.start
            && last.same_look(&run)
        {
            last.end = run.end;
        } else {
            self.runs.push(run);
        }
    }

    /// Adds text with its white space collapsed. Returns where it starts, or `None` when
    /// it added nothing.
    fn add_collapsed(
        &mut self,
        style: &ComputedStyle,
        text: &str,
        keeps_lines: bool,
    ) -> Option<usize> {
        let mut start = None;
        let mut space_inside = false;
        for character in text.chars() {
            let is_line_end = matches!(character, '\n' | '\r');
            if matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0c')
                && !(keeps_lines && is_line_end)
            {
                if start.is_some() {
                    space_inside = true;
                } else if self.pending_space.is_none() {
                    self.pending_space = Some(self.make_run(style, 0, 0));
                }
                continue;
            }
            if start.is_none() {
                self.begin_text();
                self.flush_pending_space();
                start = Some(self.out.text.len());
                self.has_inline_content = true;
            } else if std::mem::take(&mut space_inside) {
                self.out.text.push(' ');
            }
            self.out.text.push(character);
        }
        if space_inside {
            self.pending_space = Some(self.make_run(style, 0, 0));
        }
        start
    }

    /// Breaks the collected text of a block into lines.
    fn flush_inline(&mut self) {
        self.pending_space = None;
        self.has_inline_content = false;
        self.text_begun = false;
        if self.runs.is_empty() {
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        let frame = self.frames.last().expect("the root frame stays");
        let (limit_left, available, node) = (frame.content_x, frame.content_width, frame.node);
        let style = self.styles.get(node);
        let indent = resolve(style.text_indent, available).unwrap_or(0.0);
        let align = style.text_align;
        // Lines are at least as high as the block's own font, except in quirks mode where
        // a line holds no text.
        let strut = self.make_run(style, 0, 0);
        let mut block = InlineBlock {
            align,
            limit_left,
            limit_right: limit_left + available,
            left: limit_left,
            available,
            indent,
            strut,
            quirks: self.document.is_quirks(),
            runs,
            line: Vec::new(),
            line_width: 0.0,
            word: Vec::new(),
            word_width: 0.0,
        };
        self.commit_margins();
        self.update_room(&mut block);
        for index in 0..block.runs.len() {
            self.break_run(&mut block, index);
        }
        self.add_word(&mut block);
        if !block.line.is_empty() {
            let end = block.runs.last().map_or(0, |run| run.end);
            self.finish_line(&mut block, None, end);
        }
    }

    /// Finds the room for a line at the current height, around floats.
    fn update_room(&self, block: &mut InlineBlock) {
        let height = block.strut.line_height.max(1.0);
        let (left, right) = self.room_at(
            self.cursor,
            self.cursor + height,
            block.limit_left,
            block.limit_right,
        );
        block.left = left;
        block.available = (right - left).max(0.0);
    }

    /// When a line has no room for the word beside floats, moves it below the nearest float.
    fn move_below_float(&mut self, block: &mut InlineBlock, needed: f32) {
        loop {
            self.update_room(block);
            if block.available - block.indent >= needed {
                return;
            }
            let height = block.strut.line_height.max(1.0);
            let next = self
                .floats
                .iter()
                .filter(|float| float.top < self.cursor + height && float.bottom > self.cursor)
                .map(|float| float.bottom)
                .fold(f32::MAX, f32::min);
            if next == f32::MAX {
                return;
            }
            self.cursor = next;
        }
    }

    /// Splits a run at its spaces and line ends, adding what is between to the line.
    fn break_run(&mut self, block: &mut InlineBlock, index: usize) {
        let run = block.runs[index];
        if let Some(atom) = run.atom {
            // An image is a word of its own: a line can break on either side.
            self.add_word(block);
            let width = self.atom_width(atom);
            block.word_width = width;
            block.word.push(Piece {
                run: index,
                start: run.start,
                end: run.start,
                width,
            });
            self.add_word(block);
            return;
        }
        let mut position = run.start;
        while position < run.end {
            let keeps_spaces = run.keeps_spaces;
            let rest = &self.out.text[position..run.end];
            if rest.starts_with('\n') {
                self.add_word(block);
                self.finish_line(block, Some(index), position);
                position += 1;
            } else if rest.starts_with(' ') && !keeps_spaces {
                self.add_word(block);
                let width = self.word_width(run.font, position, position + 1);
                block.line.push(Piece {
                    run: index,
                    start: position,
                    end: position + 1,
                    width,
                });
                block.line_width += width;
                position += 1;
            } else {
                let length = rest
                    .find(|c| c == '\n' || (c == ' ' && !keeps_spaces))
                    .unwrap_or(rest.len());
                let width = self.word_width(run.font, position, position + length);
                block.word_width += width;
                block.word.push(Piece {
                    run: index,
                    start: position,
                    end: position + length,
                    width,
                });
                position += length;
            }
        }
    }

    /// The width of a range of the text, remembered per font.
    fn word_width(&mut self, font: u32, start: usize, end: usize) -> f32 {
        let word = &self.out.text[start..end];
        let widths = self.word_widths.entry(font).or_default();
        if let Some(&width) = widths.get(word) {
            return width;
        }
        let width = self.measurer.width(font, word);
        widths.insert(word.into(), width);
        width
    }

    fn atom_sides(&self, atom: Atom) -> BoxSides {
        let available = self.frames.last().map_or(0.0, |frame| frame.content_width);
        BoxSides::of(self.styles.get(atom.node), available)
    }

    fn atom_width(&self, atom: Atom) -> f32 {
        atom.width + self.atom_sides(atom).horizontal()
    }

    /// How far an image on a line reaches above and below the baseline.
    fn atom_extent(&self, atom: Atom) -> (f32, f32) {
        let style = self.styles.get(atom.node);
        let height = atom.height + self.atom_sides(atom).vertical();
        match style.vertical_align {
            VerticalPosition::Keyword(VerticalAlign::Middle) => {
                let middle = style.font_size * 0.25;
                (height / 2.0 + middle, (height / 2.0 - middle).max(0.0))
            }
            VerticalPosition::Keyword(VerticalAlign::Top | VerticalAlign::TextTop) => {
                let top = style.font_size;
                (top, (height - top).max(0.0))
            }
            VerticalPosition::Px(px) => (height + px, (-px).max(0.0)),
            _ => {
                // An inline block stands on the line of its last text, if it has any.
                let baseline = atom
                    .inline_box
                    .and_then(|number| self.inline_boxes.get(number as usize))
                    .and_then(|inline_box| inline_box.baseline);
                match baseline {
                    Some(baseline) => {
                        let above = self.atom_sides(atom).margin_top + baseline;
                        (above, (height - above).max(0.0))
                    }
                    None => (height, 0.0),
                }
            }
        }
    }

    fn draw_atom(&mut self, atom: Atom, x: f32, baseline: f32) {
        let sides = self.atom_sides(atom);
        let (above, _) = self.atom_extent(atom);
        if let Some(number) = atom.inline_box {
            let top = baseline - above + sides.margin_top;
            self.move_inline_box(number, x + sides.margin_left, top);
            return;
        }
        let style = self.styles.get(atom.node);
        self.draw_image_box(atom, style, &sides, x, baseline - above);
    }

    /// Moves the word being collected onto the line, first wrapping when it does not fit.
    fn add_word(&mut self, block: &mut InlineBlock) {
        let Some(first) = block.word.first() else {
            return;
        };
        let wraps = block.runs[first.run].wraps;
        if wraps
            && !block.line.is_empty()
            && block.line_width + block.word_width > block.available - block.indent
        {
            let end = block.line.last().map_or(0, |piece| piece.end);
            self.finish_line(block, None, end);
        }
        if block.line.is_empty() {
            self.move_below_float(block, block.word_width);
        }
        block.line.append(&mut block.word);
        block.line_width += block.word_width;
        block.word_width = 0.0;
    }

    /// Places one line of text and starts the next. An empty line takes its height from
    /// `empty_run`, and its place in the text from `position`.
    fn finish_line(&mut self, block: &mut InlineBlock, empty_run: Option<usize>, position: usize) {
        // A collapsible space at the end of a line stays in the text but takes no room.
        while block.line.last().is_some_and(|piece| {
            let run = &block.runs[piece.run];
            run.atom.is_none() && !run.keeps_spaces && &self.out.text[piece.start..piece.end] == " "
        }) {
            block.line.pop();
        }
        // What touches within a run becomes one item.
        let mut stretches: Vec<Piece> = Vec::new();
        for piece in &block.line {
            match stretches.last_mut() {
                Some(last)
                    if last.run == piece.run
                        && last.end == piece.start
                        && block.runs[piece.run].atom.is_none() =>
                {
                    last.end = piece.end;
                    last.width += piece.width;
                }
                _ => stretches.push(*piece),
            }
        }
        let line_width: f32 = stretches.iter().map(|piece| piece.width).sum();
        let free = (block.available - block.indent - line_width).max(0.0);
        let line_left = block.left + block.indent;
        let mut x = block.left
            + block.indent
            + match block.align {
                TextAlign::Center => free / 2.0,
                TextAlign::Right | TextAlign::End => free,
                _ => 0.0,
            };

        let mut above = 0.0_f32;
        let mut below = 0.0_f32;
        let measured = stretches
            .iter()
            .map(|piece| piece.run)
            .chain(if stretches.is_empty() {
                empty_run
            } else {
                None
            });
        let mut has_direct_text = stretches.is_empty();
        for index in measured {
            let run = &block.runs[index];
            if let Some(atom) = run.atom {
                let (atom_above, atom_below) = self.atom_extent(atom);
                above = above.max(atom_above);
                below = below.max(atom_below);
                continue;
            }
            has_direct_text |= run.direct;
            let leading = (run.line_height - run.ascent - run.descent) / 2.0;
            above = above.max(run.ascent + leading + (-run.shift).max(0.0));
            below = below.max(run.descent + leading + run.shift.max(0.0));
        }
        // In quirks mode, only text right in the block makes a line as high as the block's font.
        if has_direct_text || !block.quirks {
            let strut = &block.strut;
            let leading = (strut.line_height - strut.ascent - strut.descent) / 2.0;
            above = above.max(strut.ascent + leading);
            below = below.max(strut.descent + leading);
        }
        let top = self.cursor;
        let baseline = top + above;
        let bottom = top + above + below;

        if let Some(marker) = self.marker.take() {
            self.out.items.push(Item {
                kind: ItemKind::Marker,
                x: (marker.right - marker.width - marker.gap).max(0.0),
                y: top,
                width: marker.width,
                height: bottom - top,
                baseline,
                color: marker.color,
                font: marker.font,
                decoration: 0,
                start: offset(marker.text_start),
                end: offset(marker.text_end),
            });
        }
        // Backgrounds go first, so that the items of the line stay together.
        let mut text_x = x;
        for piece in &stretches {
            let run = &block.runs[piece.run];
            self.push_link_area(run.link, text_x, top, piece.width, bottom - top);
            if let Some(atom) = run.atom {
                self.draw_atom(atom, text_x, baseline);
            } else if run.background.alpha != 0 {
                self.out.items.push(Item {
                    kind: ItemKind::Rect,
                    x: text_x,
                    y: top,
                    width: piece.width,
                    height: bottom - top,
                    baseline: 0.0,
                    color: run.background,
                    font: 0,
                    decoration: 0,
                    start: 0,
                    end: 0,
                });
            }
            text_x += piece.width;
        }
        let first_item = offset(self.out.items.len());
        let mut text_items = 0;
        for piece in &stretches {
            let run = &block.runs[piece.run];
            if run.atom.is_some() {
                x += piece.width;
                continue;
            }
            text_items += 1;
            self.out.items.push(Item {
                kind: ItemKind::Text,
                x,
                y: top,
                width: piece.width,
                height: bottom - top,
                baseline: baseline + run.shift,
                color: run.color,
                font: run.font,
                decoration: run.decoration,
                start: offset(piece.start),
                end: offset(piece.end),
            });
            x += piece.width;
        }
        let mut text_pieces = stretches
            .iter()
            .filter(|piece| block.runs[piece.run].atom.is_none());
        let first_text = text_pieces.next();
        let (start, end) = match (first_text, text_pieces.next_back().or(first_text)) {
            (Some(first), Some(last)) => (first.start, last.end),
            _ => (position, position),
        };
        self.out.lines.push(Line {
            top,
            bottom,
            left: line_left,
            right: x,
            first_item,
            item_count: text_items,
            start: offset(start),
            end: offset(end),
        });
        self.out.width = self.out.width.max(x);
        self.cursor = bottom;
        self.last_line_bottom = Some(bottom);
        self.last_baseline = Some(baseline);
        block.line.clear();
        block.line_width = 0.0;
        block.indent = 0.0;
        self.update_room(block);
    }
}

/// Text ranges are kept in 32 bits; the parser limits pages to less than that.
fn offset(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// How far a bullet stands from the text, in font sizes. Numbers get the width of a space instead.
const MARKER_GAP: f32 = 0.5;
/// See [`Layouter::add_float`].
const MAX_ACTIVE_FLOATS: usize = 1000;
/// Each costs time on every move of the mouse.
const MAX_MAP_AREAS: usize = 100_000;
/// Finding how wide an inline block wants to be walks all it holds, for each block around it, so
/// deeper ones stay ordinary inline elements.
const MAX_INLINE_BOX_DEPTH: usize = 32;
/// Each level of tables and flexible boxes measures all it holds, so deeper ones stack as blocks.
const MAX_NESTED_LAYOUTS: usize = 32;

/// The margins, borders and padding of a box.
struct BoxSides {
    margin_left: f32,
    margin_right: f32,
    margin_top: f32,
    margin_bottom: f32,
    border_left: f32,
    border_right: f32,
    border_top: f32,
    border_bottom: f32,
    padding_left: f32,
    padding_right: f32,
    padding_top: f32,
    padding_bottom: f32,
}

impl BoxSides {
    fn of(style: &ComputedStyle, whole: f32) -> Self {
        let size = |value| resolve(value, whole).unwrap_or(0.0);
        Self {
            margin_left: size(style.margin.left),
            margin_right: size(style.margin.right),
            margin_top: size(style.margin.top),
            margin_bottom: size(style.margin.bottom),
            border_left: style.border_width.left,
            border_right: style.border_width.right,
            border_top: style.border_width.top,
            border_bottom: style.border_width.bottom,
            padding_left: size(style.padding.left),
            padding_right: size(style.padding.right),
            padding_top: size(style.padding.top),
            padding_bottom: size(style.padding.bottom),
        }
    }

    fn horizontal(&self) -> f32 {
        self.margin_left
            + self.margin_right
            + self.border_left
            + self.border_right
            + self.padding_left
            + self.padding_right
    }

    /// What borders and padding take across.
    fn insets(&self) -> f32 {
        self.border_left + self.border_right + self.padding_left + self.padding_right
    }

    /// What borders and padding take down.
    fn vertical_insets(&self) -> f32 {
        self.border_top + self.border_bottom + self.padding_top + self.padding_bottom
    }

    fn vertical(&self) -> f32 {
        self.margin_top
            + self.margin_bottom
            + self.border_top
            + self.border_bottom
            + self.padding_top
            + self.padding_bottom
    }
}

/// The text of a block on its way into lines.
struct InlineBlock {
    align: TextAlign,
    /// The content box of the block.
    limit_left: f32,
    limit_right: f32,
    /// The room for the line being built, around floats.
    left: f32,
    available: f32,
    strut: Run,
    quirks: bool,
    /// The indent of the line being built, which only the first line has.
    indent: f32,
    runs: Vec<Run>,
    line: Vec<Piece>,
    line_width: f32,
    /// A word can span runs, as in `a<b>b</b>`, and is only placed once it is complete.
    word: Vec<Piece>,
    word_width: f32,
}

fn apply_transform(text: &str, transform: TextTransform) -> std::borrow::Cow<'_, str> {
    match transform {
        TextTransform::None => std::borrow::Cow::Borrowed(text),
        TextTransform::Uppercase => std::borrow::Cow::Owned(text.to_uppercase()),
        TextTransform::Lowercase => std::borrow::Cow::Owned(text.to_lowercase()),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(text.len());
            let mut at_start = true;
            for character in text.chars() {
                if at_start {
                    out.extend(character.to_uppercase());
                } else {
                    out.push(character);
                }
                at_start = character.is_whitespace();
            }
            std::borrow::Cow::Owned(out)
        }
    }
}

fn marker_text(kind: ListStyleType, number: i64) -> Option<String> {
    Some(match kind {
        ListStyleType::None => return None,
        ListStyleType::Disc => "\u{2022}".to_owned(),
        ListStyleType::Circle => "\u{25e6}".to_owned(),
        ListStyleType::Square => "\u{25aa}".to_owned(),
        ListStyleType::Decimal => format!("{number}."),
        ListStyleType::LowerAlpha | ListStyleType::LowerLatin => format!("{}.", alpha(number)),
        ListStyleType::UpperAlpha | ListStyleType::UpperLatin => {
            format!("{}.", alpha(number).to_uppercase())
        }
        ListStyleType::LowerRoman => format!("{}.", roman(number)),
        ListStyleType::UpperRoman => format!("{}.", roman(number).to_uppercase()),
    })
}

fn alpha(number: i64) -> String {
    if number < 1 {
        return number.to_string();
    }
    let mut value = number;
    let mut out = Vec::new();
    while value > 0 {
        value -= 1;
        let digit = u8::try_from(value % 26).unwrap_or(0);
        out.push(char::from(b'a' + digit));
        value /= 26;
    }
    out.iter().rev().collect()
}

fn roman(number: i64) -> String {
    if !(1..4000).contains(&number) {
        return number.to_string();
    }
    let mut value = number;
    let mut out = String::new();
    for (amount, letters) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while value >= amount {
            out.push_str(letters);
            value -= amount;
        }
    }
    out
}
