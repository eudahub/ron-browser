//! Turns a styled tree into positioned rectangles and text. Blocks stack, text wraps into
//! lines. Walks the tree without recursion, as elements can nest very deeply.
//!
//! All visible text goes into one string in document order, and everything on screen refers
//! to ranges of it. Selection and copying work on that string, so they follow the text and
//! not the geometry. List markers live apart from it, so they cannot be selected.
//!
//! Not handled yet: floats, tables as grids, images, and positioned boxes, which stay in the
//! flow. Table parts stack as blocks for now.

use std::collections::HashMap;

use ron::css::{
    BorderStyle, Color, ComputedStyle, Display, LineHeight, ListStyleType, Size, Styles, TextAlign,
    TextTransform, VerticalAlign, VerticalPosition, Visibility, WhiteSpace,
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
    /// The text items of this line, from left to right.
    pub first_item: u32,
    pub item_count: u32,
    /// The range of the text on this line, without the space hidden at a wrap.
    pub start: u32,
    pub end: u32,
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
}

struct Layouter<'a> {
    document: &'a Document,
    styles: &'a Styles,
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
    last_line_bottom: Option<f32>,
}

/// Lays out a document in a view of this width.
pub fn layout(
    document: &Document,
    styles: &Styles,
    width: f32,
    measurer: &mut dyn TextMeasurer,
) -> Layout {
    let mut layouter = Layouter {
        document,
        styles,
        measurer,
        fonts: HashMap::new(),
        word_widths: HashMap::new(),
        out: Layout::default(),
        frames: vec![Frame {
            node: document.root(),
            x: 0.0,
            width,
            top: Some(0.0),
            content_x: 0.0,
            content_width: width,
            bottom_inset: 0.0,
            border_top: 0.0,
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
        last_line_bottom: None,
    };
    layouter.run();
    let mut result = layouter.out;
    result.canvas = canvas_color(document, styles);
    result.width = result.width.max(width);
    result.text.shrink_to_fit();
    result.items.shrink_to_fit();
    result.lines.shrink_to_fit();
    result
}

/// The background of the root element, or of `<body>` when the root has none.
fn canvas_color(document: &Document, styles: &Styles) -> Option<Color> {
    let html = document
        .children(document.root())
        .find(|&id| document.kind(id) == NodeKind::Element)?;
    let color = styles.get(html).background_color;
    if color.alpha != 0 {
        return Some(color);
    }
    let body = document
        .children(html)
        .find(|&id| document.tag(id) == Tag::Body)?;
    let color = styles.get(body).background_color;
    (color.alpha != 0).then_some(color)
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
    }
}

/// Elements drawn from something else, which there is no support for yet. Their content is
/// a fallback for browsers that lack them, or not for showing at all.
fn is_replaced(document: &Document, id: NodeId) -> bool {
    matches!(
        document.tag(id),
        Tag::Img | Tag::Embed | Tag::Iframe | Tag::Applet | Tag::Object
    ) || (document.tag(id) == Tag::Other
        && matches!(document.name(id), "video" | "audio" | "canvas"))
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
    fn run(&mut self) {
        let document = self.document;
        let mut current = document.first_child(document.root());
        while let Some(id) = current {
            let descend = match document.kind(id) {
                NodeKind::Element => self.enter_element(id),
                NodeKind::Text => {
                    self.add_text(id);
                    false
                }
                _ => false,
            };
            let child = if descend {
                document.first_child(id)
            } else {
                None
            };
            current = child.or_else(|| self.leave_until_next(id, descend));
        }
        self.flush_inline();
        self.out.height = self.cursor + self.margin.value().max(0.0);
    }

    /// Finds the next node once `id` is done, leaving the elements that end on the way.
    /// `entered` tells whether `id` itself was entered.
    fn leave_until_next(&mut self, id: NodeId, entered: bool) -> Option<NodeId> {
        let document = self.document;
        let mut node = id;
        let mut inside = entered;
        loop {
            if inside {
                self.leave_element(node);
            }
            if let Some(sibling) = document.next_sibling(node) {
                return Some(sibling);
            }
            node = document.parent(node)?;
            inside = true;
            if node == document.root() {
                return None;
            }
        }
    }

    /// Returns whether to go on to the element's children.
    fn enter_element(&mut self, id: NodeId) -> bool {
        let style = self.styles.get(id);
        if style.display == Display::None || is_replaced(self.document, id) {
            return false;
        }
        if style.text_decoration != 0 {
            let all = self.current_decoration() | style.text_decoration;
            self.decorations.push((id, all));
        }
        if is_block(style.display) {
            self.flush_inline();
            self.open_frame(id, style);
        } else if self.document.tag(id) == Tag::Br {
            self.add_line_break(style);
        }
        true
    }

    fn leave_element(&mut self, id: NodeId) {
        if self.decorations.last().is_some_and(|&(node, _)| node == id) {
            self.decorations.pop();
        }
        if self.frames.last().is_some_and(|frame| frame.node == id) {
            self.flush_inline();
            self.close_frame();
        }
    }

    fn current_decoration(&self) -> u8 {
        self.decorations.last().map_or(0, |&(_, lines)| lines)
    }

    fn open_frame(&mut self, id: NodeId, style: &ComputedStyle) {
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
        let mut wanted = resolve(style.width, available).map(|width| {
            if style.box_sizing == ron::css::BoxSizing::BorderBox {
                (width - insets).max(0.0)
            } else {
                width
            }
        });
        if let Some(most) = resolve(style.max_width, available)
            && wanted.is_none_or(|width| width > most)
            && wanted.is_some()
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
        let x = parent_x + offset;
        let number = self.next_list_number(id, style);

        self.margin
            .add(resolve(style.margin.top, available).unwrap_or(0.0));
        let background_item = (style.background_color.alpha != 0).then(|| {
            self.out.items.push(Item {
                kind: ItemKind::Rect,
                x,
                y: 0.0,
                width: content_width + insets,
                height: 0.0,
                baseline: 0.0,
                color: style.background_color,
                font: 0,
                decoration: 0,
                start: 0,
                end: 0,
            });
            self.out.items.len() - 1
        });
        let height = resolve(style.height, 0.0).filter(|_| matches!(style.height, Size::Px(_)));
        self.frames.push(Frame {
            node: id,
            x,
            width: content_width + insets,
            top: None,
            content_x: x + border.left + padding_left,
            content_width,
            bottom_inset: padding_bottom + border.bottom,
            border_top: border.top,
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
        if border.top > 0.0 || padding_top > 0.0 {
            self.commit_margins();
        }
        if style.display == Display::ListItem {
            self.prepare_marker(id, style, number);
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
            frame.bottom_inset > 0.0 || frame.height.is_some() || frame.min_height > 0.0
        };
        if needs_room {
            if self.frames.last().is_some_and(|frame| frame.top.is_some()) {
                self.cursor += self.margin.value();
                self.margin = Margin::default();
            } else {
                self.commit_margins();
            }
        }
        let frame = self.frames.pop().expect("checked above");
        let node = frame.node;
        let Some(top) = frame.top else {
            // Nothing took up room, so its margins collapse together.
            self.margin.add(frame.margin_bottom);
            let y = self.cursor + self.margin.value();
            if let Some(index) = frame.background_item {
                self.out.items[index].y = y;
            }
            return;
        };
        let content_top = top + frame.border_top + frame.padding_top;
        let mut content_height = (self.cursor - content_top).max(0.0);
        if let Some(height) = frame.height {
            content_height = height;
        }
        content_height = content_height.max(frame.min_height);
        if let Some(most) = frame.max_height {
            content_height = content_height.min(most);
        }
        let bottom = content_top + content_height + frame.bottom_inset;
        self.cursor = bottom;
        if let Some(index) = frame.background_item {
            self.out.items[index].height = bottom - top;
        }
        self.draw_borders(node, &frame, top, bottom);
        self.margin.add(frame.margin_bottom);
        self.out.width = self.out.width.max(frame.x + frame.width);
    }

    fn draw_borders(&mut self, node: NodeId, frame: &Frame, top: f32, bottom: f32) {
        let style = self.styles.get(node);
        let widths = style.border_width;
        let colors = style.border_color;
        let (x, width) = (frame.x, frame.width);
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
        let Some(text) = marker_text(style.list_style_type, number) else {
            return;
        };
        let font = self.font_of(style);
        let width = self.measurer.width(font, &text);
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
        });
    }

    fn add_line_break(&mut self, style: &ComputedStyle) {
        self.pending_space = None;
        self.begin_text();
        let start = self.out.text.len();
        self.out.text.push('\n');
        let run = self.make_run(style, start, start + 1);
        self.runs.push(run);
    }

    /// Before the first text of a block, separates it from the text before, as far as
    /// line ends from `<br>` have not already done so.
    fn begin_text(&mut self) {
        if !self.runs.is_empty() || self.out.text.is_empty() {
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
            background: if is_block(style.display) {
                Color::TRANSPARENT
            } else {
                style.background_color
            },
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
                self.flush_pending_space();
                self.begin_text();
                let start = self.out.text.len();
                self.out.text.push_str(&transformed);
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
        let run = self.make_run(style, start, end);
        self.push_run(run);
    }

    /// Adds the space waiting from earlier text, which never starts a block or a line.
    fn flush_pending_space(&mut self) {
        let Some(mut run) = self.pending_space.take() else {
            return;
        };
        if self.runs.is_empty() || self.out.text.ends_with('\n') {
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
                self.flush_pending_space();
                self.begin_text();
                start = Some(self.out.text.len());
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
        if self.runs.is_empty() {
            return;
        }
        let runs = std::mem::take(&mut self.runs);
        let frame = self.frames.last().expect("the root frame stays");
        let (left, available, node) = (frame.content_x, frame.content_width, frame.node);
        let style = self.styles.get(node);
        let indent = resolve(style.text_indent, available).unwrap_or(0.0);
        let mut block = InlineBlock {
            align: style.text_align,
            left,
            available,
            indent,
            runs,
            line: Vec::new(),
            line_width: 0.0,
            word: Vec::new(),
            word_width: 0.0,
        };
        self.commit_margins();
        for index in 0..block.runs.len() {
            self.break_run(&mut block, index);
        }
        self.add_word(&mut block);
        if !block.line.is_empty() {
            let end = block.runs.last().map_or(0, |run| run.end);
            self.finish_line(&mut block, None, end);
        }
    }

    /// Splits a run at its spaces and line ends, adding what is between to the line.
    fn break_run(&mut self, block: &mut InlineBlock, index: usize) {
        let run = block.runs[index];
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

    /// Moves the word being collected onto the line, first wrapping when it does not fit.
    fn add_word(&mut self, block: &mut InlineBlock) {
        let Some(first) = block.word.first() else {
            return;
        };
        let room = block.available - block.indent;
        if block.runs[first.run].wraps
            && !block.line.is_empty()
            && block.line_width + block.word_width > room
        {
            let end = block.line.last().map_or(0, |piece| piece.end);
            self.finish_line(block, None, end);
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
            !block.runs[piece.run].keeps_spaces && &self.out.text[piece.start..piece.end] == " "
        }) {
            block.line.pop();
        }
        // What touches within a run becomes one item.
        let mut stretches: Vec<Piece> = Vec::new();
        for piece in &block.line {
            match stretches.last_mut() {
                Some(last) if last.run == piece.run && last.end == piece.start => {
                    last.end = piece.end;
                    last.width += piece.width;
                }
                _ => stretches.push(*piece),
            }
        }
        let line_width: f32 = stretches.iter().map(|piece| piece.width).sum();
        let free = (block.available - block.indent - line_width).max(0.0);
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
        for index in measured {
            let run = &block.runs[index];
            let leading = (run.line_height - run.ascent - run.descent) / 2.0;
            above = above.max(run.ascent + leading + (-run.shift).max(0.0));
            below = below.max(run.descent + leading + run.shift.max(0.0));
        }
        let top = self.cursor;
        let baseline = top + above;
        let bottom = top + above + below;

        if let Some(marker) = self.marker.take() {
            self.out.items.push(Item {
                kind: ItemKind::Marker,
                x: (marker.right - marker.width - MARKER_GAP).max(0.0),
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
            if run.background.alpha != 0 {
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
        for piece in &stretches {
            let run = &block.runs[piece.run];
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
        let (start, end) = match (stretches.first(), stretches.last()) {
            (Some(first), Some(last)) => (first.start, last.end),
            _ => (position, position),
        };
        self.out.lines.push(Line {
            top,
            bottom,
            first_item,
            item_count: offset(stretches.len()),
            start: offset(start),
            end: offset(end),
        });
        self.out.width = self.out.width.max(x);
        self.cursor = bottom;
        self.last_line_bottom = Some(bottom);
        block.line.clear();
        block.line_width = 0.0;
        block.indent = 0.0;
    }
}

/// Text ranges are kept in 32 bits; the parser limits pages to less than that.
fn offset(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

const MARKER_GAP: f32 = 8.0;

/// The text of a block on its way into lines.
struct InlineBlock {
    align: TextAlign,
    left: f32,
    available: f32,
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
