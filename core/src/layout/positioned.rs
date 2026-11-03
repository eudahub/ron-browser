//! Positioned boxes, and what is cut off by `overflow`.
//!
//! An absolute box is laid out by itself, starting where it would have been. Where it ends up can
//! depend on how high the box it goes by turns out, so it moves there when that one ends. Boxes that
//! go by the page go by the first screen of it, which is also what fixed ones do, as they do not stay
//! in place when scrolling yet. Sticky ones stay where they are. Only blocks can be what others go by,
//! and shift themselves, as `position: relative` on inline text does nothing yet.
//!
//! What draws above what is settled when drawing, from [`PaintLayer`]s, as the order of items also
//! is that of the text. All layers count together, without stacking contexts within each other.

use std::ops::Range;

use ron::css::{ComputedStyle, Overflow, Position, Size};
use ron::html::{NodeId, Tag};

use super::{
    BoxSides, Color, Frame, InlineReturn, Item, ItemKind, Layout, Layouter, Margin, resolve,
};

/// Whether a box is taken out of the flow.
pub(super) fn is_out_of_flow(style: &ComputedStyle) -> bool {
    matches!(style.position, Position::Absolute | Position::Fixed)
}

fn goes_above(style: &ComputedStyle) -> bool {
    (style.position != Position::Static && style.position != Position::Sticky)
        || style.transform.is_some()
}

/// How far a box of this size is moved by its `transform`. Percentages are of its own size.
pub(super) fn translation(style: &ComputedStyle, width: f32, height: f32) -> (f32, f32) {
    style.transform.map_or((0.0, 0.0), |(across, down)| {
        (
            resolve(across, width).unwrap_or(0.0),
            resolve(down, height).unwrap_or(0.0),
        )
    })
}

/// Whether a box hides what goes beyond it. Boxes that scroll cannot do that yet, as there would be no
/// way to reach what they hide.
fn cuts_off(style: &ComputedStyle) -> bool {
    matches!(style.overflow, Overflow::Hidden | Overflow::Clip)
}

/// A range of items that draw differently from the others.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintLayer {
    start: u32,
    end: u32,
    /// Whether it draws above what is not positioned, or below if it asks for a negative `z-index`.
    positioned: bool,
    z_index: i32,
    /// How much of what it holds shows. Images only show or do not, as the toolkit is not told.
    opacity: f32,
    /// The item that holds the box that this range is cut off by, which moves along with the rest.
    clip: Option<u32>,
    /// The layer that this one is within.
    parent: Option<u32>,
}

/// Where a box started, in what it puts in the layout, for what is to be done with that.
pub(super) struct FrameSpan {
    items: usize,
    lines: usize,
    link_areas: usize,
    area_shapes: usize,
    pub(super) clip_marker: Option<usize>,
}

/// What goes on around an absolute box, while it is laid out.
pub(super) struct PositionedReturn {
    around: InlineReturn,
    has_inline_content: bool,
    text_begun: bool,
    /// An item that tells where the box is, whatever has moved it on the way.
    marker: usize,
    /// Which of the open boxes it goes by.
    depth_of_frame: usize,
    /// The start of all it holds, with its marker and background.
    items: usize,
    lines: usize,
    link_areas: usize,
    area_shapes: usize,
    /// Text and images, which have nothing of their own to draw when they end.
    is_plain: bool,
}

/// An absolute box that waits for the box it goes by to end.
pub(super) struct PendingBox {
    node: NodeId,
    depth_of_frame: usize,
    marker: usize,
    x: f32,
    width: f32,
    height: f32,
    background_item: Option<usize>,
    background_tile: Option<usize>,
    /// Whether its height depends on where it ends up, so that its borders have to wait for that.
    borders_wait: bool,
    is_plain: bool,
    /// All it has put in the layout, with what comes after not in it.
    ranges: [Range<usize>; 4],
}

impl Layout {
    /// The items within this part of the page, in the order they are to be drawn, without what is
    /// cut off or has no place yet.
    #[must_use]
    pub fn visible_items(&self, top: f32, bottom: f32) -> Vec<VisibleItem> {
        let placed = self
            .unplaced_items
            .map_or(self.items.len(), |first| first as usize);
        let within = self.items[..placed]
            .iter()
            .enumerate()
            .filter(|(_, item)| item.y <= bottom && item.y + item.height >= top);
        if self.paint_layers.is_empty() {
            return within
                .map(|(index, item)| VisibleItem {
                    index,
                    item: *item,
                    clip: None,
                })
                .collect();
        }
        let mut kept: Vec<((i8, i32), VisibleItem)> = Vec::new();
        for (index, item) in within {
            if let Some(found) = self.painted(index, *item) {
                kept.push(found);
            }
        }
        kept.sort_by_key(|(rank, visible)| (*rank, visible.index));
        kept.into_iter().map(|(_, visible)| visible).collect()
    }

    /// What an item comes to with all layers around it, and how far up it goes.
    fn painted(&self, index: usize, mut item: Item) -> Option<((i8, i32), VisibleItem)> {
        let at = u32::try_from(index).unwrap_or(u32::MAX);
        let mut clip = None;
        let after = self.paint_layers.partition_point(|layer| layer.start <= at);
        let mut layer = after.checked_sub(1).map(|found| &self.paint_layers[found]);
        // The last that starts before it may have ended, and then one around that may still count.
        let mut rank = None;
        while let Some(current) = layer {
            if current.end > at {
                if current.opacity <= 0.0 {
                    return None;
                }
                if current.opacity < 1.0 {
                    let alpha = f32::from(item.color.alpha) * current.opacity;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    {
                        item.color.alpha = alpha.round() as u8;
                    }
                }
                if rank.is_none() && current.positioned {
                    rank = Some((if current.z_index < 0 { -1 } else { 1 }, current.z_index));
                }
                if let Some(area_marker) = current.clip {
                    let area = self.items.get(area_marker as usize)?;
                    if area_marker != at && !cut(&mut item, area, &mut clip) {
                        return None;
                    }
                }
            }
            layer = current
                .parent
                .map(|parent| &self.paint_layers[parent as usize]);
        }
        Some((rank.unwrap_or((0, 0)), VisibleItem { index, item, clip }))
    }
}

/// An item to draw, as [`Layout::visible_items`] gives them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibleItem {
    /// Its place among the items of the layout.
    pub index: usize,
    pub item: Item,
    /// What it is to be cut to when drawing, for what cannot be made smaller.
    pub clip: Option<ClipArea>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Keeps what is within the area. Returns whether there is anything left. Rectangles can just get
/// smaller, while text and images need cutting when they are drawn. A box of hardly any size is how
/// text gets hidden from all but screen readers, of which nothing should show.
fn cut(item: &mut Item, area: &Item, clip: &mut Option<ClipArea>) -> bool {
    let inside = area.width > 1.0
        && area.height > 1.0
        && item.x < area.x + area.width
        && item.x + item.width > area.x
        && item.y < area.y + area.height
        && item.y + item.height > area.y;
    if !inside {
        return false;
    }
    if item.kind == ItemKind::Rect {
        let right = (item.x + item.width).min(area.x + area.width);
        let down = (item.y + item.height).min(area.y + area.height);
        item.x = item.x.max(area.x);
        item.y = item.y.max(area.y);
        item.width = right - item.x;
        item.height = down - item.y;
        return true;
    }
    let (left, top) = (area.x, area.y);
    let (right, bottom) = (area.x + area.width, area.y + area.height);
    *clip = Some(match *clip {
        None => ClipArea {
            x: left,
            y: top,
            width: area.width,
            height: area.height,
        },
        Some(other) => {
            let x = other.x.max(left);
            let y = other.y.max(top);
            ClipArea {
                x,
                y,
                width: ((other.x + other.width).min(right) - x).max(0.0),
                height: ((other.y + other.height).min(bottom) - y).max(0.0),
            }
        }
    });
    true
}

/// Puts layers in order, and finds which is within which, as ranges either nest or stand apart.
pub(super) fn order_layers(layers: &mut [PaintLayer]) {
    layers.sort_by_key(|layer| (layer.start, std::cmp::Reverse(layer.end)));
    let mut open: Vec<u32> = Vec::new();
    for index in 0..layers.len() {
        let start = layers[index].start;
        while open
            .last()
            .is_some_and(|&last| layers[last as usize].end <= start)
        {
            open.pop();
        }
        layers[index].parent = open.last().copied();
        open.push(u32::try_from(index).unwrap_or(u32::MAX));
    }
}

impl Layouter<'_> {
    /// Notes where a box starts, if something is to be done with what it holds when it ends.
    pub(super) fn start_span(&mut self, style: &ComputedStyle) -> Option<Box<FrameSpan>> {
        let clips = cuts_off(style);
        if !clips && !goes_above(style) && style.opacity >= 1.0 {
            return None;
        }
        let mut span = Box::new(FrameSpan {
            items: self.out.items.len(),
            lines: self.out.lines.len(),
            link_areas: self.out.link_areas.len(),
            area_shapes: self.out.area_shapes.len(),
            clip_marker: None,
        });
        if clips {
            self.push_rect(0.0, 0.0, 0.0, 0.0, Color::TRANSPARENT);
            span.clip_marker = Some(self.out.items.len() - 1);
        }
        Some(span)
    }

    /// Ends what was noted about a box, at its place for now.
    pub(super) fn finish_span(&mut self, frame: &Frame, top: f32, bottom: f32) {
        let Some(span) = &frame.span else {
            return;
        };
        let style = self.styles.get(frame.node);
        if let Some(marker) = span.clip_marker {
            let area = &mut self.out.items[marker];
            area.x = frame.x;
            area.y = top;
            area.width = frame.width;
            area.height = bottom - top;
            // What cannot be seen cannot be clicked either.
            for link in &mut self.out.link_areas[span.link_areas..] {
                let right = (link.x + link.width).min(frame.x + frame.width);
                let down = (link.y + link.height).min(bottom);
                link.x = link.x.max(frame.x);
                link.y = link.y.max(top);
                link.width = (right - link.x).max(0.0);
                link.height = (down - link.y).max(0.0);
            }
        }
        let end = u32::try_from(self.out.items.len()).unwrap_or(u32::MAX);
        self.out.paint_layers.push(PaintLayer {
            start: u32::try_from(span.items).unwrap_or(u32::MAX),
            end,
            positioned: goes_above(style),
            z_index: style.z_index.unwrap_or(0),
            opacity: style.opacity,
            clip: span
                .clip_marker
                .map(|marker| u32::try_from(marker).unwrap_or(u32::MAX)),
            parent: None,
        });
    }

    /// Shifts a relative box from where it is, which leaves its place in the flow as it was.
    pub(super) fn shift_relative(&mut self, frame: &Frame, height: f32) {
        let Some(span) = &frame.span else {
            return;
        };
        let style = self.styles.get(frame.node);
        let (mut dx, mut dy) = translation(style, frame.width, height);
        if style.position == Position::Relative {
            let whole = self
                .frames
                .last()
                .map_or(frame.width, |parent| parent.content_width);
            let offsets = style.offsets;
            dx += resolve(offsets.left, whole)
                .or_else(|| resolve(offsets.right, whole).map(|right| -right))
                .unwrap_or(0.0);
            // A percentage of a height that is not known counts as nothing.
            let down = |size: Size| match size {
                Size::Px(px) => Some(px),
                _ => None,
            };
            dy += down(offsets.top)
                .or_else(|| down(offsets.bottom).map(|bottom| -bottom))
                .unwrap_or(0.0);
        }
        if dx != 0.0 || dy != 0.0 {
            let ranges = [
                span.items..self.out.items.len(),
                span.lines..self.out.lines.len(),
                span.link_areas..self.out.link_areas.len(),
                span.area_shapes..self.out.area_shapes.len(),
            ];
            self.move_ranges(ranges, dx, dy);
        }
    }

    /// Which of the open boxes this one goes by: the nearest positioned one, or else the page.
    fn frame_it_goes_by(&self, style: &ComputedStyle) -> usize {
        if style.position == Position::Fixed {
            return 0;
        }
        self.frames
            .iter()
            .rposition(|frame| {
                let around = self.styles.get(frame.node);
                around.position != Position::Static || around.transform.is_some()
            })
            .filter(|&index| index > 0)
            .unwrap_or(0)
    }

    /// The inside of a box as others go by it, from the left and how wide, within borders.
    fn inside_across(&self, depth: usize) -> (f32, f32) {
        let frame = &self.frames[depth];
        if depth == 0 {
            return (frame.x, frame.width);
        }
        let border = self.styles.get(frame.node).border_width;
        (
            frame.x + border.left,
            frame.width - border.left - border.right,
        )
    }

    /// Opens an absolute box. It goes on with what it holds, or is done with an image.
    pub(super) fn enter_out_of_flow(&mut self, id: NodeId, style: &ComputedStyle) -> bool {
        let depth = self.frame_it_goes_by(style);
        let (inside_x, inside_width) = self.inside_across(depth);
        let sides = BoxSides::of(style, inside_width);
        let left = resolve(style.offsets.left, inside_width);
        let right = resolve(style.offsets.right, inside_width);
        let is_image = self.document.tag(id) == Tag::Img;
        let insets = sides.insets();
        let margins = sides.margin_left + sides.margin_right;
        let room =
            (inside_width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - margins - insets).max(0.0);
        let own = resolve(style.width, inside_width).map(|width| {
            if style.box_sizing == ron::css::BoxSizing::BorderBox {
                (width - insets).max(0.0)
            } else {
                width
            }
        });
        let content_width = if is_image {
            self.image_size(id, style, inside_width).0
        } else {
            own.unwrap_or_else(|| {
                if left.is_some() && right.is_some() {
                    room
                } else {
                    self.preferred_width(id, self.inline_box_depth).min(room)
                }
            })
        };
        let own_width = content_width + insets;
        // Two offsets and a width leave what automatic margins share out.
        let free = inside_width - left.unwrap_or(0.0) - right.unwrap_or(0.0) - own_width;
        let centered = left.is_some()
            && right.is_some()
            && style.margin.left == Size::Auto
            && style.margin.right == Size::Auto;
        let static_x = self.frames.last().map_or(inside_x, |frame| frame.content_x);
        let x = if centered {
            inside_x + left.unwrap_or(0.0) + free.max(0.0) / 2.0
        } else if let Some(left) = left {
            inside_x + left + sides.margin_left
        } else if let Some(right) = right {
            inside_x + inside_width - right - sides.margin_right - own_width
        } else {
            static_x + sides.margin_left
        };
        let static_top = self.cursor + self.margin.value();
        let start = [
            self.out.items.len(),
            self.out.lines.len(),
            self.out.link_areas.len(),
            self.out.area_shapes.len(),
        ];
        self.push_rect(x, static_top, 0.0, 0.0, Color::TRANSPARENT);
        let marker = self.out.items.len() - 1;
        let around = InlineReturn {
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
        let saved = Box::new(PositionedReturn {
            around,
            has_inline_content: self.has_inline_content,
            text_begun: self.text_begun,
            marker,
            depth_of_frame: depth,
            items: start[0],
            lines: start[1],
            link_areas: start[2],
            area_shapes: start[3],
            is_plain: is_image,
        });
        self.margin = Margin::default();
        self.has_inline_content = false;
        self.lowest_float = 0.0;
        self.text_begun = false;
        if is_image {
            let outer = own_width + margins;
            self.open_plain_frame(id, x - sides.margin_left, static_top, outer);
            if let Some(frame) = self.frames.last_mut() {
                frame.out_return = Some(saved);
            }
            self.add_image(id, style);
            self.flush_inline();
            self.close_frame();
            return false;
        }
        self.open_frame_at(
            id,
            style,
            Some(content_width),
            Some((x, static_top + sides.margin_top)),
        );
        if let Some(frame) = self.frames.last_mut() {
            frame.out_return = Some(saved);
        }
        true
    }

    /// Ends an absolute box, which goes to its place now, or when the box it goes by ends.
    pub(super) fn close_out_of_flow(
        &mut self,
        frame: &Frame,
        saved: PositionedReturn,
        top: f32,
        bottom: f32,
    ) {
        let style = self.styles.get(frame.node);
        let around = saved.around;
        self.cursor = around.cursor;
        self.margin = around.margin;
        self.runs = around.runs;
        self.pending_space = None;
        self.marker = around.marker;
        self.floats = around.floats;
        self.lowest_float = around.lowest_float;
        self.last_line_bottom = around.last_line_bottom;
        self.last_baseline = around.last_baseline;
        self.has_inline_content = saved.has_inline_content;
        self.text_begun = saved.text_begun;
        if let Some(index) = frame.background_item {
            self.out.items[index].height = bottom - top;
        }
        let stretches = style.height == Size::Auto
            && style.offsets.top != Size::Auto
            && style.offsets.bottom != Size::Auto;
        let is_percent = matches!(style.height, Size::Percent(_) | Size::Calc(_));
        let borders_wait = stretches || is_percent;
        if !saved.is_plain && !borders_wait {
            self.push_borders(style, frame.x, top, frame.width, bottom);
        }
        self.finish_span(frame, top, bottom);
        let pending = PendingBox {
            node: frame.node,
            depth_of_frame: saved.depth_of_frame,
            marker: saved.marker,
            x: frame.x,
            width: frame.width,
            height: bottom - top,
            background_item: frame.background_item,
            background_tile: frame.background_tile,
            borders_wait,
            is_plain: saved.is_plain,
            ranges: [
                saved.items..self.out.items.len(),
                saved.lines..self.out.lines.len(),
                saved.link_areas..self.out.link_areas.len(),
                saved.area_shapes..self.out.area_shapes.len(),
            ],
        };
        if pending.depth_of_frame == 0 {
            let height = self.view_height;
            self.place_box(pending, 0.0, height);
        } else {
            self.pending_boxes.push(pending);
        }
    }

    /// Now that a box has ended, those going by it can go to their places. `top` and `bottom` are
    /// where its borders are.
    pub(super) fn place_boxes_of(&mut self, depth: usize, frame: &Frame, top: f32, bottom: f32) {
        if self
            .pending_boxes
            .last()
            .is_none_or(|last| last.depth_of_frame != depth)
        {
            return;
        }
        let border = self.styles.get(frame.node).border_width;
        let inside_top = top + border.top;
        let inside_height = (bottom - border.bottom - inside_top).max(0.0);
        while self
            .pending_boxes
            .last()
            .is_some_and(|last| last.depth_of_frame == depth)
        {
            if let Some(pending) = self.pending_boxes.pop() {
                self.place_box(pending, inside_top, inside_height);
            }
        }
    }

    /// Puts a box at its place down the box it goes by, and finishes what depends on that.
    fn place_box(&mut self, pending: PendingBox, inside_top: f32, inside_height: f32) {
        let style = self.styles.get(pending.node);
        let sides = BoxSides::of(style, pending.width);
        // Percentages in a height that is known by now.
        let offset = |size: Size| resolve(size, inside_height);
        let top = offset(style.offsets.top);
        let bottom = offset(style.offsets.bottom);
        let mut height = pending.height;
        match style.height {
            Size::Percent(percent) => {
                height = inside_height * percent / 100.0;
            }
            Size::Calc(calc) => height = calc.of(inside_height).max(0.0),
            Size::Auto => {
                if let (Some(top), Some(bottom)) = (top, bottom) {
                    height =
                        (inside_height - top - bottom - sides.margin_top - sides.margin_bottom)
                            .max(0.0);
                }
            }
            Size::Px(_) => {}
        }
        // An image has its margins in what it has laid out.
        let (margin_top, margin_bottom) = if pending.is_plain {
            (0.0, 0.0)
        } else {
            (sides.margin_top, sides.margin_bottom)
        };
        let from = self.out.items[pending.marker].y + margin_top;
        let free = inside_height - top.unwrap_or(0.0) - bottom.unwrap_or(0.0) - height;
        let target = match (top, bottom) {
            (Some(top), Some(_))
                if style.margin.top == Size::Auto && style.margin.bottom == Size::Auto =>
            {
                inside_top + top + free.max(0.0) / 2.0
            }
            (Some(top), _) => inside_top + top + margin_top,
            (None, Some(bottom)) => inside_top + inside_height - bottom - margin_bottom - height,
            (None, None) => from,
        };
        let (across, moved_down) = translation(style, pending.width, height);
        let target = target + moved_down;
        let x = pending.x + across;
        let down = target - from;
        if down != 0.0 || across != 0.0 {
            self.move_ranges(pending.ranges, across, down);
        }
        if let Some(index) = pending.background_item {
            self.out.items[index].height = height;
        }
        if !pending.is_plain {
            if let Some(index) = pending.background_tile {
                self.finish_background_tile(pending.node, index, x, target, pending.width, height);
            }
            if pending.borders_wait {
                self.push_borders(style, x, target, pending.width, target + height);
            }
        }
        self.out.width = self.out.width.max(x + pending.width);
        self.lowest_positioned = self.lowest_positioned.max(target + height + margin_bottom);
    }
}
