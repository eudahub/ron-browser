//! Flexible boxes. How wide items in a row get depends on all of them, so a flexible box is laid out
//! once all of it is there. Each item is laid out on its own at its place across, and only once all
//! are done is it known how high lines are, or where items in a column go, which then moves them.
//!
//! Not handled: columns that wrap, `align-content`, which counts as stretching, and baselines, which
//! count as the start. Only widths are shrunk, so a column that is too high runs over.

mod grid;

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ron::css::{
    BoxSizing, ComputedStyle, Display, FlexAlign, FlexDirection, FlexWrap, JustifyContent,
    Overflow, Size, WhiteSpace,
};
use ron::html::{NodeId, NodeKind, Tag};

use self::grid::GridPlan;
use super::positioned::{is_out_of_flow, translation};
use super::{
    BoxSides, Frame, InlineReturn, Layouter, MAX_NESTED_LAYOUTS, Margin, is_replaced, resolve,
};

/// Whether a display makes an element a flexible box.
pub(super) fn is_flex(display: Display) -> bool {
    matches!(
        display,
        Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
    )
}

/// Whether it is a grid, which goes on from here with [`super::grid`].
fn is_grid(display: Display) -> bool {
    matches!(display, Display::Grid | Display::InlineGrid)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ItemKind {
    /// An element, with a box as its style says.
    Element,
    /// Text directly in the flexible box.
    Text,
    Image,
}

/// How wide what an item holds can be, which does not depend on the room there is.
#[derive(Clone, Copy)]
struct ItemContent {
    node: NodeId,
    kind: ItemKind,
    least: f32,
    wanted: f32,
}

/// What a flexible box needs to know of its items before it is laid out.
pub(super) struct FlexContent {
    items: Vec<ItemContent>,
    /// How narrow and how wide the content of the box itself can be.
    least: f32,
    wanted: f32,
}

/// Where an item goes across its box, and how wide it is, without its margins.
#[derive(Clone, Copy)]
struct ItemPlace {
    x: f32,
    width: f32,
}

/// An item that has been laid out, which waits for how all of them turn out.
struct LaidItem {
    node: NodeId,
    is_plain: bool,
    x: f32,
    width: f32,
    top: f32,
    bottom: f32,
    background_item: Option<usize>,
    background_tile: Option<usize>,
    clip_marker: Option<usize>,
    items: Range<usize>,
    lines: Range<usize>,
    link_areas: Range<usize>,
    area_shapes: Range<usize>,
}

/// A flexible box being laid out.
pub(super) struct FlexState {
    node: NodeId,
    is_row: bool,
    /// Where the content of the box starts going down.
    top: f32,
    places: HashMap<usize, ItemPlace>,
    /// The items of each line, by node, in the order that they show.
    lines: Vec<Vec<usize>>,
    laid: HashMap<usize, LaidItem>,
    /// What else there is to know when it is a grid.
    grid: Option<GridPlan>,
}

/// What an item is across its box, as worked out from its style and the room there is.
struct Sizing {
    index: usize,
    margin_left: Option<f32>,
    margin_right: Option<f32>,
    insets: f32,
    /// In content widths.
    base: f32,
    least: f32,
    most: f32,
    grow: f32,
    shrink: f32,
    /// How wide it ends up, also a content width.
    width: f32,
    frozen: bool,
}

impl Sizing {
    fn margins(&self) -> f32 {
        self.margin_left.unwrap_or(0.0) + self.margin_right.unwrap_or(0.0)
    }

    fn outer(&self, content: f32) -> f32 {
        content + self.insets + self.margins()
    }
}

fn content_size(size: Size, whole: f32, style: &ComputedStyle, insets: f32) -> Option<f32> {
    resolve(size, whole).map(|value| {
        if style.box_sizing == BoxSizing::BorderBox {
            (value - insets).max(0.0)
        } else {
            value
        }
    })
}

/// Shares out what is free among those in a line, as long as limits leave anything to share.
fn resolve_line(line: &mut [Sizing], room: f32) {
    for item in line.iter_mut() {
        item.width = item.base.clamp(item.least, item.most.max(item.least));
        item.frozen = false;
    }
    let used: f32 = line.iter().map(|item| item.outer(item.base)).sum();
    let growing = used < room;
    // Each round fixes at least one, so this ends, and a few rounds do for almost any line.
    for _ in 0..line.len().min(16) {
        let open: Vec<usize> = (0..line.len()).filter(|&i| !line[i].frozen).collect();
        if open.is_empty() {
            return;
        }
        let fixed: f32 = line
            .iter()
            .map(|item| {
                if item.frozen {
                    item.outer(item.width)
                } else {
                    item.outer(item.base)
                }
            })
            .sum();
        let free = room - fixed;
        let weight = |item: &Sizing| {
            if growing {
                item.grow
            } else {
                item.shrink * item.base
            }
        };
        let all: f32 = open.iter().map(|&i| weight(&line[i])).sum();
        if all <= 0.0 {
            return;
        }
        // Less than one in all takes that part of what is free.
        let share = if growing && all < 1.0 {
            free * all
        } else {
            free
        };
        let mut wrong = false;
        for &i in &open {
            let item = &mut line[i];
            let wanted = item.base + share * weight(item) / all;
            let limited = wanted.clamp(item.least, item.most.max(item.least));
            item.width = limited;
            if (limited - wanted).abs() > 0.001 {
                item.frozen = true;
                wrong = true;
            }
        }
        if !wrong {
            return;
        }
    }
}

/// Where the first item goes and how much goes between them, for what is left free in a line.
fn justified(justify: JustifyContent, free: f32, count: usize, reversed: bool) -> (f32, f32) {
    #[allow(clippy::cast_precision_loss)]
    let count_f = count as f32;
    use JustifyContent as J;
    let free_side = free.max(0.0);
    match justify {
        J::Center => (free / 2.0, 0.0),
        J::End | J::FlexEnd | J::Right if !reversed => (free, 0.0),
        J::Normal | J::Start | J::FlexStart | J::Left if reversed => (free, 0.0),
        J::SpaceBetween if count > 1 => (0.0, free_side / (count_f - 1.0)),
        J::SpaceAround => (free_side / count_f / 2.0, free_side / count_f),
        J::SpaceEvenly => (free_side / (count_f + 1.0), free_side / (count_f + 1.0)),
        _ => (0.0, 0.0),
    }
}

impl Layouter<'_> {
    /// Whether what comes now is an item of a flexible box, and not within one.
    pub(super) fn in_flex_structure(&self) -> bool {
        match (self.flexes.last(), self.frames.last()) {
            (Some(flex), Some(frame)) => flex.node == frame.node,
            _ => false,
        }
    }

    fn text_widths(&mut self, node: NodeId) -> (f32, f32) {
        let style = self.styles.get(node);
        let font = self.font_of(style);
        let document = self.document;
        let text = document.decoded_text(node);
        let whole = text.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
        let wanted = self.measurer.width(font, &whole);
        if style.white_space == WhiteSpace::Nowrap {
            return (wanted, wanted);
        }
        let mut least = 0.0_f32;
        for word in text.split_ascii_whitespace() {
            least = least.max(self.measured_word(font, word));
        }
        (least, wanted)
    }

    /// Finds the items of a flexible box and what each of them holds. Nothing when it lies too deep.
    pub(super) fn flex_content(&mut self, id: NodeId) -> Option<Rc<FlexContent>> {
        if let Some(found) = self.flex_contents.get(&id.index()) {
            return found.clone();
        }
        let found = if self.tables.len() + self.flexes.len() + self.grid_depth >= MAX_NESTED_LAYOUTS
        {
            None
        } else {
            self.grid_depth += 1;
            let content = self.measure_flex(id);
            self.grid_depth -= 1;
            Some(Rc::new(content))
        };
        self.flex_contents.insert(id.index(), found.clone());
        found
    }

    /// What stands in a box, where what an element holds takes its place if it makes no box itself.
    fn items_within(&self, id: NodeId) -> Vec<NodeId> {
        let document = self.document;
        let mut found = Vec::new();
        let mut waiting: Vec<NodeId> = document.children(id).collect();
        waiting.reverse();
        while let Some(node) = waiting.pop() {
            if document.kind(node) == NodeKind::Element
                && self.styles.get(node).display == Display::Contents
            {
                let at = waiting.len();
                waiting.extend(document.children(node));
                waiting[at..].reverse();
            } else {
                found.push(node);
            }
        }
        found
    }

    fn measure_flex(&mut self, id: NodeId) -> FlexContent {
        let document = self.document;
        let style = self.styles.get(id);
        let is_row = matches!(
            style.flex_direction,
            FlexDirection::Row | FlexDirection::RowReverse
        );
        let wraps = style.flex_wrap != FlexWrap::Nowrap;
        let depth = self.inline_box_depth;
        let mut items = Vec::new();
        let (mut least, mut wanted) = (0.0_f32, 0.0_f32);
        for child in self.items_within(id) {
            let (kind, inner_least, inner_wanted, sides) = match document.kind(child) {
                NodeKind::Text => {
                    if document
                        .decoded_text(child)
                        .bytes()
                        .all(|byte| byte.is_ascii_whitespace())
                    {
                        continue;
                    }
                    let (least, wanted) = self.text_widths(child);
                    (ItemKind::Text, least, wanted, 0.0)
                }
                NodeKind::Element => {
                    let style = self.styles.get(child);
                    if style.display == Display::None
                        || is_replaced(document, child)
                        || is_out_of_flow(style)
                    {
                        continue;
                    }
                    let sides = BoxSides::of(style, 0.0).horizontal();
                    if document.tag(child) == Tag::Img {
                        let (width, _) = self.image_size(child, style, 0.0);
                        (ItemKind::Image, width, width, sides)
                    } else {
                        let (inner_least, inner_wanted) = self.content_widths(child, depth, true);
                        (ItemKind::Element, inner_least, inner_wanted, sides)
                    }
                }
                _ => continue,
            };
            items.push(ItemContent {
                node: child,
                kind,
                least: inner_least,
                wanted: inner_wanted,
            });
            let (mut outer_least, mut outer_wanted) = (inner_least, inner_wanted);
            if kind == ItemKind::Element {
                let style = self.styles.get(child);
                if let Size::Px(own) = style.width {
                    let own = if style.box_sizing == BoxSizing::BorderBox {
                        (own - BoxSides::of(style, 0.0).insets()).max(0.0)
                    } else {
                        own
                    };
                    outer_wanted = own;
                    if style.min_width != Size::Auto || style.overflow != Overflow::Visible {
                        outer_least = own.min(outer_least);
                    }
                }
            }
            outer_least += sides;
            outer_wanted += sides;
            if is_row {
                wanted += outer_wanted;
                least = if wraps {
                    least.max(outer_least)
                } else {
                    least + outer_least
                };
            } else {
                wanted = wanted.max(outer_wanted);
                least = least.max(outer_least);
            }
        }
        if is_row && items.len() > 1 {
            #[allow(clippy::cast_precision_loss)]
            let gaps = (items.len() - 1) as f32 * style.column_gap;
            wanted += gaps;
            if !wraps {
                least += gaps;
            }
        }
        let mut content = FlexContent {
            items,
            least,
            wanted,
        };
        if is_grid(style.display) {
            (content.least, content.wanted) = self.grid_widths(style, &content);
        }
        content
    }

    /// How narrow and how wide a flexible box can be.
    pub(super) fn flex_widths(&mut self, id: NodeId) -> Option<(f32, f32)> {
        let content = self.flex_content(id)?;
        Some((content.least, content.wanted))
    }

    fn item_sizing(
        &self,
        index: usize,
        item: &ItemContent,
        room: f32,
        stretch_base: Option<f32>,
    ) -> Sizing {
        if item.kind == ItemKind::Text {
            return Sizing {
                index,
                margin_left: Some(0.0),
                margin_right: Some(0.0),
                insets: 0.0,
                base: stretch_base.unwrap_or(item.wanted),
                least: item.least,
                most: f32::INFINITY,
                grow: 0.0,
                shrink: 1.0,
                width: 0.0,
                frozen: false,
            };
        }
        let style = self.styles.get(item.node);
        let insets = BoxSides::of(style, room).insets();
        let own_width = if item.kind == ItemKind::Image {
            Some(item.wanted)
        } else {
            content_size(style.width, room, style, insets)
        };
        let basis = content_size(style.flex_basis, room, style, insets);
        let base = stretch_base.or(basis).or(own_width).unwrap_or(item.wanted);
        // An item is not narrower than what it holds, unless it says so or hides the rest.
        let floor = match own_width {
            Some(own) => own.min(item.least),
            None => item.least,
        };
        let least = match style.min_width {
            Size::Auto if style.overflow == Overflow::Visible => floor,
            Size::Auto => 0.0,
            size => content_size(size, room, style, insets).unwrap_or(0.0),
        };
        let most = content_size(style.max_width, room, style, insets).unwrap_or(f32::INFINITY);
        Sizing {
            index,
            margin_left: resolve(style.margin.left, room),
            margin_right: resolve(style.margin.right, room),
            insets,
            base,
            least,
            most,
            grow: style.flex_grow,
            shrink: style.flex_shrink,
            width: 0.0,
            frozen: false,
        }
    }

    /// Starts on the items of a flexible box, whose own box has been opened.
    pub(super) fn begin_flex(&mut self, id: NodeId) {
        if self
            .frames
            .last()
            .is_none_or(|frame: &Frame| frame.node != id)
        {
            return;
        }
        let Some(content) = self.flex_content(id) else {
            return;
        };
        if self.frames.last().is_some_and(|frame| frame.top.is_none()) {
            self.commit_margins();
        }
        let top = self.cursor + self.margin.value();
        self.cursor = top;
        self.margin = Margin::default();
        let frame = self.frames.last().expect("checked above");
        let (left, room) = (frame.content_x, frame.content_width);
        let style = self.styles.get(id);
        let is_row = matches!(
            style.flex_direction,
            FlexDirection::Row | FlexDirection::RowReverse
        );
        let mut order: Vec<usize> = (0..content.items.len()).collect();
        // Text has no order of its own.
        order.sort_by_key(|&index| match content.items[index].kind {
            ItemKind::Text => 0,
            _ => self.styles.get(content.items[index].node).order,
        });
        let mut grid = None;
        let (places, lines) = if is_grid(style.display) {
            let (places, plan) = self.plan_grid(style, &content, &order, left, room);
            grid = Some(plan);
            (places.into_iter().collect(), Vec::new())
        } else if is_row {
            self.plan_row(style, &content, &order, left, room)
        } else {
            self.plan_column(style, &content, &order, left, room)
        };
        self.flexes.push(FlexState {
            node: id,
            is_row: is_row && grid.is_none(),
            top,
            places,
            lines,
            laid: HashMap::with_capacity(content.items.len()),
            grid,
        });
    }

    fn plan_row(
        &self,
        style: &ComputedStyle,
        content: &FlexContent,
        order: &[usize],
        left: f32,
        room: f32,
    ) -> (HashMap<usize, ItemPlace>, Vec<Vec<usize>>) {
        let gap = style.column_gap;
        let reversed = style.flex_direction == FlexDirection::RowReverse;
        // Lines fill by what items start out with.
        let mut sizings: Vec<Vec<Sizing>> = vec![Vec::new()];
        let mut used = 0.0_f32;
        for &index in order {
            let sizing = self.item_sizing(index, &content.items[index], room, None);
            let outer = sizing.outer(
                sizing
                    .base
                    .clamp(sizing.least, sizing.most.max(sizing.least)),
            );
            let line = sizings.last_mut().expect("there is always a line");
            if style.flex_wrap != FlexWrap::Nowrap && !line.is_empty() && used + gap + outer > room
            {
                sizings.push(vec![sizing]);
                used = outer;
            } else {
                if !line.is_empty() {
                    used += gap;
                }
                used += outer;
                line.push(sizing);
            }
        }
        let mut places = HashMap::with_capacity(content.items.len());
        let mut lines = Vec::with_capacity(sizings.len());
        for mut line in sizings {
            #[allow(clippy::cast_precision_loss)]
            let gaps = line.len().saturating_sub(1) as f32 * gap;
            resolve_line(&mut line, room - gaps);
            let taken: f32 = line.iter().map(|item| item.outer(item.width)).sum::<f32>() + gaps;
            let mut free = room - taken;
            // Auto margins take what is free before anything else does.
            let autos = line
                .iter()
                .map(|item| {
                    u32::from(item.margin_left.is_none()) + u32::from(item.margin_right.is_none())
                })
                .sum::<u32>();
            #[allow(clippy::cast_precision_loss)]
            let each_auto = if autos > 0 && free > 0.0 {
                let each = free / autos as f32;
                free = 0.0;
                each
            } else {
                0.0
            };
            let (start, between) = justified(style.justify_content, free, line.len(), reversed);
            let mut x = left + start;
            let mut shown = Vec::with_capacity(line.len());
            let count = line.len();
            for position in 0..count {
                // A reversed row goes from the far end.
                let item = if reversed {
                    &line[count - 1 - position]
                } else {
                    &line[position]
                };
                let margin_left = item.margin_left.unwrap_or(each_auto);
                let margin_right = item.margin_right.unwrap_or(each_auto);
                x += margin_left;
                let node = content.items[item.index].node.index();
                places.insert(
                    node,
                    ItemPlace {
                        x,
                        width: item.width + item.insets,
                    },
                );
                shown.push(node);
                x += item.width + item.insets + margin_right + between + gap;
            }
            lines.push(shown);
        }
        (places, lines)
    }

    fn plan_column(
        &self,
        style: &ComputedStyle,
        content: &FlexContent,
        order: &[usize],
        left: f32,
        room: f32,
    ) -> (HashMap<usize, ItemPlace>, Vec<Vec<usize>>) {
        let mut places = HashMap::with_capacity(content.items.len());
        let mut line = Vec::with_capacity(order.len());
        for &index in order {
            let item = &content.items[index];
            let node = item.node.index();
            line.push(node);
            if item.kind == ItemKind::Text {
                places.insert(
                    node,
                    ItemPlace {
                        x: left,
                        width: room,
                    },
                );
                continue;
            }
            let own = self.styles.get(item.node);
            let sides = BoxSides::of(own, room);
            let margin_left = resolve(own.margin.left, room);
            let margin_right = resolve(own.margin.right, room);
            let margins = margin_left.unwrap_or(0.0) + margin_right.unwrap_or(0.0);
            let insets = sides.insets();
            let align = flex_align(own, style);
            let stretches =
                align_stretches(align) && margin_left.is_some() && margin_right.is_some();
            let wanted = if item.kind == ItemKind::Image {
                Some(item.wanted)
            } else {
                content_size(own.width, room, own, insets)
            };
            let mut width = wanted.unwrap_or(if stretches {
                room - margins - insets
            } else {
                item.wanted.min(room - margins - insets).max(item.least)
            });
            if let Some(most) = content_size(own.max_width, room, own, insets) {
                width = width.min(most);
            }
            if let Some(minimum) = content_size(own.min_width, room, own, insets) {
                width = width.max(minimum);
            }
            let free = room - width - insets - margins;
            let x = match (margin_left, margin_right) {
                (None, None) => free.max(0.0) / 2.0,
                (None, Some(right)) => free.max(0.0) + right,
                (Some(gap), None) => gap,
                (Some(gap), Some(_)) => {
                    gap + match align {
                        FlexAlign::Center => free / 2.0,
                        FlexAlign::End | FlexAlign::FlexEnd | FlexAlign::SelfEnd => free,
                        _ => 0.0,
                    }
                }
            };
            places.insert(
                node,
                ItemPlace {
                    x: left + x,
                    width: width.max(0.0) + insets,
                },
            );
        }
        (places, vec![line])
    }

    /// A box without anything of its own around what it holds, for what has no box as such.
    pub(super) fn open_plain_frame(&mut self, node: NodeId, x: f32, top: f32, width: f32) {
        self.frames.push(Frame {
            node,
            x,
            width,
            top: Some(top),
            content_x: x,
            content_width: width,
            bottom_inset: 0.0,
            border_top: 0.0,
            contains_floats: true,
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
        });
        self.cursor = top;
    }

    /// What goes on around an item while it is laid out on its own.
    fn saved_for_item(&mut self) -> InlineReturn {
        let saved = InlineReturn {
            cursor: self.cursor,
            margin: self.margin,
            runs: std::mem::take(&mut self.runs),
            marker: self.marker.take(),
            floats: std::mem::take(&mut self.floats),
            lowest_float: self.lowest_float,
            last_line_bottom: self.last_line_bottom,
            last_baseline: self.last_baseline.take(),
            items_start: 0,
            lines_start: 0,
            link_areas_start: 0,
            area_shapes_start: 0,
        };
        self.margin = Margin::default();
        self.has_inline_content = false;
        self.text_begun = false;
        self.lowest_float = 0.0;
        saved
    }

    fn mark_item_start(&self, saved: &mut InlineReturn) {
        saved.items_start = self.out.items.len();
        saved.lines_start = self.out.lines.len();
        saved.link_areas_start = self.out.link_areas.len();
        saved.area_shapes_start = self.out.area_shapes.len();
    }

    /// Lays out text that stands directly in a flexible box, as an item of its own.
    pub(super) fn add_flex_text(&mut self, id: NodeId) {
        let is_blank = self
            .document
            .decoded_text(id)
            .bytes()
            .all(|byte| byte.is_ascii_whitespace());
        let Some(flex) = self.flexes.last() else {
            return;
        };
        let top = flex.top;
        let Some(&place) = flex.places.get(&id.index()) else {
            return;
        };
        if is_blank {
            return;
        }
        let mut saved = self.saved_for_item();
        self.mark_item_start(&mut saved);
        self.open_plain_frame(id, place.x, top, place.width);
        self.add_text(id);
        self.flush_inline();
        self.close_plain_item(saved);
    }

    fn close_plain_item(&mut self, saved: InlineReturn) {
        let mut frame = self.frames.pop().expect("it was opened just before");
        frame.top = frame.top.or(Some(self.cursor));
        let top = frame.top.unwrap_or(self.cursor);
        let bottom = (self.cursor + self.margin.value()).max(self.lowest_float);
        self.close_flex_item_at(&frame, saved, top, bottom, true);
    }

    /// Opens an item of a flexible box. Returns whether to go on to what it holds.
    pub(super) fn enter_flex_item(&mut self, id: NodeId, style: &ComputedStyle) -> bool {
        let Some(flex) = self.flexes.last() else {
            return false;
        };
        let top = flex.top;
        let Some(&place) = flex.places.get(&id.index()) else {
            return false;
        };
        let mut saved = self.saved_for_item();
        if self.document.tag(id) == Tag::Img {
            let sides = BoxSides::of(style, place.width);
            let x = place.x - sides.margin_left;
            let width = place.width + sides.margin_left + sides.margin_right;
            self.mark_item_start(&mut saved);
            self.open_plain_frame(id, x, top, width);
            self.add_image(id, style);
            self.flush_inline();
            self.close_plain_item(saved);
            return false;
        }
        let insets = BoxSides::of(style, place.width).insets();
        // Before its box opens, as all of that moves with it.
        self.mark_item_start(&mut saved);
        self.open_frame_at(
            id,
            style,
            Some((place.width - insets).max(0.0)),
            Some((place.x, top)),
        );
        // Stretching to a height that is known already is known here too, so that what the item
        // holds can count on it, as a column within it that puts something at its bottom does.
        let sides = BoxSides::of(style, place.width);
        let around = self.flexes.last().map(|flex| flex.node);
        let stretched_to = around.and_then(|node| self.stretch_height(node, style));
        if let Some(frame) = self.frames.last_mut() {
            frame.flex_return = Some(Box::new(saved));
            if let Some(height) = stretched_to {
                frame.min_height = frame.min_height.max(height - sides.vertical());
            }
        }
        true
    }

    /// How high an item gets by stretching in a row of a known height, if it does.
    fn stretch_height(&self, around: NodeId, style: &ComputedStyle) -> Option<f32> {
        let flex = self.flexes.last()?;
        let row_style = self.styles.get(around);
        if !flex.is_row
            || flex.lines.len() != 1
            || style.height != Size::Auto
            || !align_stretches(flex_align(style, row_style))
        {
            return None;
        }
        // Frames go from outermost, and the box of the item was opened just now.
        let frame = self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.node == around)?;
        frame
            .height
            .or((frame.min_height > 0.0).then_some(frame.min_height))
    }

    /// Ends an item, whose place down the box comes once all are done.
    pub(super) fn close_flex_item(
        &mut self,
        frame: &Frame,
        saved: InlineReturn,
        top: f32,
        bottom: f32,
    ) {
        self.close_flex_item_at(frame, saved, top, bottom, false);
    }

    fn close_flex_item_at(
        &mut self,
        frame: &Frame,
        saved: InlineReturn,
        top: f32,
        bottom: f32,
        is_plain: bool,
    ) {
        let item = LaidItem {
            node: frame.node,
            is_plain,
            x: frame.x,
            width: frame.width,
            top,
            bottom,
            background_item: frame.background_item,
            background_tile: frame.background_tile,
            clip_marker: frame.span.as_ref().and_then(|span| span.clip_marker),
            items: saved.items_start..self.out.items.len(),
            lines: saved.lines_start..self.out.lines.len(),
            link_areas: saved.link_areas_start..self.out.link_areas.len(),
            area_shapes: saved.area_shapes_start..self.out.area_shapes.len(),
        };
        self.cursor = saved.cursor;
        self.margin = saved.margin;
        self.runs = saved.runs;
        self.pending_space = None;
        self.marker = saved.marker;
        self.floats = saved.floats;
        self.lowest_float = saved.lowest_float;
        self.last_line_bottom = saved.last_line_bottom;
        self.last_baseline = saved.last_baseline;
        self.has_inline_content = false;
        self.text_begun = false;
        if let Some(flex) = self.flexes.last_mut() {
            flex.laid.insert(frame.node.index(), item);
        }
    }

    /// Now that all items are laid out, puts them in place, before the box's own end.
    pub(super) fn finish_flex(&mut self, id: NodeId) {
        let Some(mut flex) = self.flexes.pop() else {
            return;
        };
        let style = self.styles.get(id);
        let frame = self.frames.last().expect("it is still open");
        let height = frame
            .height
            .or((frame.min_height > 0.0).then_some(frame.min_height));
        let bottom = if flex.grid.is_some() {
            self.place_grid(&mut flex, style, height)
        } else if flex.is_row {
            self.place_row_lines(&mut flex, style, height)
        } else {
            self.place_column(&mut flex, style, height)
        };
        self.cursor = bottom;
        self.flex_contents.remove(&id.index());
        self.text_begun = false;
    }

    fn place_row_lines(
        &mut self,
        flex: &mut FlexState,
        style: &ComputedStyle,
        height: Option<f32>,
    ) -> f32 {
        let lines = std::mem::take(&mut flex.lines);
        let gap = style.row_gap;
        let mut crosses: Vec<f32> = lines
            .iter()
            .map(|line| {
                line.iter()
                    .filter_map(|node| flex.laid.get(node))
                    .map(|item| {
                        let (top, bottom) = self.cross_margins(item);
                        item.bottom - item.top + top + bottom
                    })
                    .fold(0.0, f32::max)
            })
            .collect();
        // Lines share out the height the box has.
        if let Some(height) = height {
            #[allow(clippy::cast_precision_loss)]
            let gaps = crosses.len().saturating_sub(1) as f32 * gap;
            let free = height - crosses.iter().sum::<f32>() - gaps;
            if free > 0.0 {
                #[allow(clippy::cast_precision_loss)]
                let each = free / crosses.len().max(1) as f32;
                for cross in &mut crosses {
                    *cross += each;
                }
            }
        }
        let mut line_top = flex.top;
        for (line, &cross) in lines.iter().zip(&crosses) {
            for node in line {
                if let Some(item) = flex.laid.remove(node) {
                    self.place_row_item(item, style, line_top, cross);
                }
            }
            line_top += cross + gap;
        }
        if crosses.is_empty() {
            flex.top
        } else {
            line_top - gap
        }
    }

    /// The margins of an item down its line, where those that are `auto` count as nothing.
    fn cross_margins(&self, item: &LaidItem) -> (f32, f32) {
        if item.is_plain {
            return (0.0, 0.0);
        }
        let style = self.styles.get(item.node);
        (
            resolve(style.margin.top, 0.0).unwrap_or(0.0),
            resolve(style.margin.bottom, 0.0).unwrap_or(0.0),
        )
    }

    fn place_row_item(
        &mut self,
        item: LaidItem,
        around: &ComputedStyle,
        line_top: f32,
        cross: f32,
    ) {
        let height = item.bottom - item.top;
        let (margin_top, margin_bottom) = self.cross_margins(&item);
        let (mut new_top, mut new_height) = (line_top + margin_top, height);
        if !item.is_plain {
            let style = self.styles.get(item.node);
            let free = cross - height - margin_top - margin_bottom;
            let top_auto = matches!(style.margin.top, Size::Auto);
            let bottom_auto = matches!(style.margin.bottom, Size::Auto);
            if top_auto || bottom_auto {
                if top_auto && bottom_auto {
                    new_top += free.max(0.0) / 2.0;
                } else if top_auto {
                    new_top += free.max(0.0);
                }
            } else {
                let align = flex_align(style, around);
                match align {
                    FlexAlign::Center => new_top += free / 2.0,
                    FlexAlign::End | FlexAlign::FlexEnd | FlexAlign::SelfEnd => new_top += free,
                    _ if align_stretches(align) && style.height == Size::Auto => {
                        new_height = (cross - margin_top - margin_bottom).max(height);
                    }
                    _ => {}
                }
            }
        } else {
            // Without a box of its own it goes in the line as a whole: an image has its margins in
            // it, and its own wish, while text only has what the flexible box says.
            let align = if self.document.kind(item.node) == NodeKind::Element {
                flex_align(self.styles.get(item.node), around)
            } else {
                around.align_items
            };
            match align {
                FlexAlign::Center => new_top += (cross - height) / 2.0,
                FlexAlign::End | FlexAlign::FlexEnd | FlexAlign::SelfEnd => {
                    new_top += cross - height;
                }
                _ => {}
            }
        }
        self.place_item(item, new_top, new_height);
    }

    fn place_column(
        &mut self,
        flex: &mut FlexState,
        style: &ComputedStyle,
        height: Option<f32>,
    ) -> f32 {
        let line = flex.lines.pop().unwrap_or_default();
        let reversed = style.flex_direction == FlexDirection::ColumnReverse;
        let gap = style.row_gap;
        let items: Vec<LaidItem> = line
            .iter()
            .filter_map(|node| flex.laid.remove(node))
            .collect();
        let mut sizes: Vec<f32> = items.iter().map(|item| item.bottom - item.top).collect();
        let margins: Vec<(f32, f32)> = items.iter().map(|item| self.cross_margins(item)).collect();
        #[allow(clippy::cast_precision_loss)]
        let gaps = items.len().saturating_sub(1) as f32 * gap;
        let taken = sizes.iter().sum::<f32>()
            + margins
                .iter()
                .map(|(top, bottom)| top + bottom)
                .sum::<f32>()
            + gaps;
        let mut free = height.map_or(0.0, |height| height - taken);
        // What grows takes what is free first.
        if free > 0.0 {
            let grow: f32 = items
                .iter()
                .filter(|item| !item.is_plain)
                .map(|item| self.styles.get(item.node).flex_grow)
                .sum();
            if grow > 0.0 {
                let share = if grow < 1.0 { free * grow } else { free };
                for (size, item) in sizes.iter_mut().zip(&items) {
                    if !item.is_plain {
                        *size += share * self.styles.get(item.node).flex_grow / grow;
                    }
                }
                free -= share;
            }
        }
        let auto_count: u32 = items
            .iter()
            .filter(|item| !item.is_plain)
            .map(|item| {
                let style = self.styles.get(item.node);
                u32::from(style.margin.top == Size::Auto)
                    + u32::from(style.margin.bottom == Size::Auto)
            })
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let each_auto = if auto_count > 0 && free > 0.0 {
            let each = free / auto_count as f32;
            free = 0.0;
            each
        } else {
            0.0
        };
        let (start, between) = justified(style.justify_content, free, items.len(), reversed);
        let mut y = flex.top + start;
        let count = items.len();
        let mut ordered: Vec<(LaidItem, f32, (f32, f32))> = items
            .into_iter()
            .zip(sizes)
            .zip(margins)
            .map(|((item, size), margins)| (item, size, margins))
            .collect();
        if reversed {
            ordered.reverse();
        }
        for (position, (item, size, (margin_top, margin_bottom))) in ordered.into_iter().enumerate()
        {
            let (top_auto, bottom_auto) = if item.is_plain {
                (false, false)
            } else {
                let style = self.styles.get(item.node);
                (
                    style.margin.top == Size::Auto,
                    style.margin.bottom == Size::Auto,
                )
            };
            y += margin_top + if top_auto { each_auto } else { 0.0 };
            self.place_item(item, y, size);
            y += size + margin_bottom + if bottom_auto { each_auto } else { 0.0 };
            if position + 1 < count {
                y += gap + between;
            }
        }
        y
    }

    /// Puts an item at its place and height, and draws what depends on that.
    fn place_item(&mut self, item: LaidItem, top: f32, height: f32) {
        // Text has the style of what is around it, and so nothing of its own to move it.
        let (across, moved_down) = if item.is_plain {
            (0.0, 0.0)
        } else {
            translation(self.styles.get(item.node), item.width, height)
        };
        let top = top + moved_down;
        let x = item.x + across;
        let down = top - item.top;
        if down != 0.0 || across != 0.0 {
            self.move_ranges(
                [item.items, item.lines, item.link_areas, item.area_shapes],
                across,
                down,
            );
        }
        let bottom = top + height;
        if let Some(index) = item.clip_marker {
            self.out.items[index].height = height;
        }
        if let Some(index) = item.background_item {
            let background = &mut self.out.items[index];
            background.y = top;
            background.height = height;
        }
        if !item.is_plain {
            if let Some(index) = item.background_tile {
                self.finish_background_tile(item.node, index, x, top, item.width, height);
            }
            self.push_borders(self.styles.get(item.node), x, top, item.width, bottom);
        }
        self.out.width = self.out.width.max(x + item.width);
    }
}

fn flex_align(item: &ComputedStyle, around: &ComputedStyle) -> FlexAlign {
    match item.align_self {
        FlexAlign::Auto => around.align_items,
        own => own,
    }
}

fn align_stretches(align: FlexAlign) -> bool {
    matches!(
        align,
        FlexAlign::Auto | FlexAlign::Normal | FlexAlign::Stretch
    )
}
