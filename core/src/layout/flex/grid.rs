//! Grids. Items are laid out the way those of a flexible box are, as in [`super`]: all of a grid
//! has to be there, each item gets its place across from the columns, and goes to its row once all
//! are done and it is known how high they are.
//!
//! Tracks go from what they hold to what they may take, then share what is left by `fr`, and then
//! `auto` ones take the rest. Not handled: `grid-auto-flow: column` and `dense`, `grid-auto-columns`,
//! named lines other than those of areas, and subgrids. The `grid` and `grid-template` shorthands are
//! not read.

use ron::css::{
    ComputedStyle, FlexAlign, GridAreas, GridLine, GridTrack, JustifyContent, Size, TrackSize,
};

use super::super::{BoxSides, Layouter, resolve};
use super::{FlexContent, FlexState, ItemContent, ItemKind, ItemPlace, align_stretches, justified};

/// However many tracks a page asks for, this many are made.
const MAX_TRACKS: usize = 1000;
/// With more items than this, those that place themselves stop looking for overlaps, which would
/// otherwise take as long as the square of the number of items.
const MAX_CHECKED_ITEMS: usize = 4096;

/// Which tracks an item goes in, from the start of the first to the start of the one after the last.
#[derive(Clone, Copy, Debug)]
struct Cells {
    row_start: usize,
    row_end: usize,
    column_start: usize,
    column_end: usize,
}

impl Cells {
    fn overlaps(&self, other: &Self) -> bool {
        self.row_start < other.row_end
            && other.row_start < self.row_end
            && self.column_start < other.column_end
            && other.column_start < self.column_end
    }
}

/// What a grid remembers, between placing its items across and placing them down.
pub(super) struct GridPlan {
    /// The node of each item, with where it goes.
    placed: Vec<(usize, Cells)>,
    rows: usize,
}

/// The tracks of a list, with those that repeat as often as there is room.
fn expanded(
    list: Option<&[GridTrack]>,
    room: Option<f32>,
    gap: f32,
    items: usize,
) -> Vec<TrackSize> {
    let Some(list) = list else {
        return Vec::new();
    };
    let fixed: f32 = list
        .iter()
        .map(|track| match track {
            GridTrack::One(size) => track_least(size, room) + gap,
            GridTrack::AutoRepeat { .. } => 0.0,
        })
        .sum();
    let mut tracks = Vec::new();
    for track in list {
        match track {
            GridTrack::One(size) => tracks.push(size.clone()),
            GridTrack::AutoRepeat {
                fit,
                tracks: repeated,
            } => {
                let each: f32 = repeated
                    .iter()
                    .map(|size| track_least(size, room).max(1.0) + gap)
                    .sum();
                let count = room.map_or(1.0, |room| ((room - fixed + gap) / each).floor());
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let mut count = (count.max(1.0) as usize).min(MAX_TRACKS);
                // Those that nothing goes in collapse, which leaves room for the others.
                if *fit {
                    count = count.min(items.max(1));
                }
                for _ in 0..count {
                    tracks.extend(repeated.iter().cloned());
                }
            }
        }
        if tracks.len() >= MAX_TRACKS {
            tracks.truncate(MAX_TRACKS);
            break;
        }
    }
    tracks
}

/// What a track takes for sure, without what it holds.
fn track_least(size: &TrackSize, room: Option<f32>) -> f32 {
    match size {
        TrackSize::Px(px) => *px,
        TrackSize::Percent(percent) => room.map_or(0.0, |room| room * percent / 100.0),
        TrackSize::MinMax(least, most) => {
            let least = track_least(least, room);
            if least > 0.0 {
                least
            } else {
                track_least(most, room)
            }
        }
        _ => 0.0,
    }
}

/// Where one end of an item goes, from the start of the tracks. Nothing if it is up to the item.
fn line_place(
    line: &GridLine,
    explicit: usize,
    areas: Option<&GridAreas>,
    is_row: bool,
    is_start: bool,
) -> Option<usize> {
    match line {
        GridLine::Auto | GridLine::Span(_) => None,
        #[allow(clippy::cast_sign_loss)]
        GridLine::Line(number) => {
            let number = i64::from(*number);
            let place = if number > 0 {
                number - 1
            } else {
                i64::try_from(explicit).unwrap_or(0) + 1 + number
            };
            Some((place.max(0) as usize).min(MAX_TRACKS))
        }
        GridLine::Name(name) => {
            let area = areas?.areas.iter().find(|area| area.name == *name)?;
            let place = match (is_row, is_start) {
                (true, true) => area.row_start,
                (true, false) => area.row_end,
                (false, true) => area.column_start,
                (false, false) => area.column_end,
            };
            Some(place as usize)
        }
    }
}

fn span_of(line: &GridLine) -> usize {
    match line {
        GridLine::Span(count) => (*count as usize).clamp(1, MAX_TRACKS),
        _ => 1,
    }
}

/// Where an item goes along one way, if it says, or else how many tracks it spans.
fn resolved(
    start: &GridLine,
    end: &GridLine,
    explicit: usize,
    areas: Option<&GridAreas>,
    is_row: bool,
) -> Result<(usize, usize), usize> {
    let from = line_place(start, explicit, areas, is_row, true);
    let to = line_place(end, explicit, areas, is_row, false);
    match (from, to) {
        (Some(from), Some(to)) => {
            if from == to {
                Ok((from, from + 1))
            } else {
                Ok((from.min(to), from.max(to)))
            }
        }
        (Some(from), None) => Ok((from, (from + span_of(end)).min(MAX_TRACKS + 1))),
        (None, Some(to)) => {
            let to = to.max(1);
            Ok((to.saturating_sub(span_of(start)), to))
        }
        (None, None) => Err(span_of(start).max(span_of(end))),
    }
}

/// Finds where each item goes, and how many columns and rows that takes in all.
fn place_cells(
    order: &[usize],
    styles: &dyn Fn(usize) -> Option<(GridLine, GridLine, GridLine, GridLine)>,
    (columns, rows): (usize, usize),
    areas: Option<&GridAreas>,
) -> (Vec<(usize, Cells)>, usize, usize) {
    let mut placed: Vec<(usize, Cells)> = Vec::with_capacity(order.len());
    let mut taken: Vec<Cells> = Vec::new();
    let mut column_count = columns.max(1);
    let mut wanting = Vec::new();
    for &index in order {
        let lines = styles(index);
        let (column_start, column_end, row_start, row_end) = lines.unwrap_or((
            GridLine::Auto,
            GridLine::Auto,
            GridLine::Auto,
            GridLine::Auto,
        ));
        let across = resolved(&column_start, &column_end, columns, areas, false);
        let down = resolved(&row_start, &row_end, rows, areas, true);
        if let (Ok(across), Ok(down)) = (across, down) {
            let cells = Cells {
                row_start: down.0,
                row_end: down.1,
                column_start: across.0,
                column_end: across.1,
            };
            column_count = column_count.max(across.1);
            taken.push(cells);
            placed.push((index, cells));
        } else {
            if let Ok(across) = across {
                column_count = column_count.max(across.1);
            }
            if let Err(span) = across {
                column_count = column_count.max(span);
            }
            wanting.push((index, across, down));
        }
    }
    let check = order.len() <= MAX_CHECKED_ITEMS;
    let free =
        |cells: &Cells, taken: &[Cells]| !check || !taken.iter().any(|other| other.overlaps(cells));
    let (mut row, mut column) = (0, 0);
    for (index, across, down) in wanting {
        let cells = match (across, down) {
            // Its row is given, so it looks along that.
            (Err(span), Ok(down)) => {
                let mut start = 0;
                loop {
                    let cells = Cells {
                        row_start: down.0,
                        row_end: down.1,
                        column_start: start,
                        column_end: start + span,
                    };
                    if free(&cells, &taken) || start > MAX_TRACKS {
                        column_count = column_count.max(start + span);
                        break cells;
                    }
                    start += 1;
                }
            }
            // Its column is given, so it looks down that.
            (Ok(across), Err(span)) => {
                let mut start = 0;
                loop {
                    let cells = Cells {
                        row_start: start,
                        row_end: start + span,
                        column_start: across.0,
                        column_end: across.1,
                    };
                    if free(&cells, &taken) {
                        break cells;
                    }
                    start += 1;
                }
            }
            // Neither is, so it goes on from where the last of these went.
            (Err(columns_spanned), Err(rows_spanned)) => loop {
                if column + columns_spanned > column_count {
                    column = 0;
                    row += 1;
                }
                let cells = Cells {
                    row_start: row,
                    row_end: row + rows_spanned,
                    column_start: column,
                    column_end: column + columns_spanned,
                };
                column += 1;
                if free(&cells, &taken) {
                    column = cells.column_end;
                    break cells;
                }
            },
            (Ok(across), Ok(down)) => Cells {
                row_start: down.0,
                row_end: down.1,
                column_start: across.0,
                column_end: across.1,
            },
        };
        if check {
            taken.push(cells);
        }
        placed.push((index, cells));
    }
    let row_count = placed
        .iter()
        .map(|(_, cells)| cells.row_end)
        .max()
        .unwrap_or(0)
        .max(rows);
    (placed, column_count, row_count)
}

/// How narrow and how wide an item can be, with what it has around it.
struct Span {
    start: usize,
    end: usize,
    least: f32,
    wanted: f32,
}

/// Works out how large each track is. With no room given, tracks get what they would like.
fn track_sizes(tracks: &[TrackSize], spans: &[Span], room: Option<f32>, gap: f32) -> Vec<f32> {
    let count = tracks.len();
    let mut least_held = vec![0.0_f32; count];
    let mut wanted_held = vec![0.0_f32; count];
    for span in spans.iter().filter(|span| span.end == span.start + 1) {
        if let (Some(least), Some(wanted)) = (
            least_held.get_mut(span.start),
            wanted_held.get_mut(span.start),
        ) {
            *least = least.max(span.least);
            *wanted = wanted.max(span.wanted);
        }
    }
    let at_least = |size: &TrackSize, index: usize| match size {
        TrackSize::Px(px) => *px,
        TrackSize::Percent(percent) => {
            room.map_or(least_held[index], |room| room * percent / 100.0)
        }
        TrackSize::MaxContent => wanted_held[index],
        _ => least_held[index],
    };
    let at_most = |size: &TrackSize, index: usize| match size {
        TrackSize::Px(px) => Some(*px),
        TrackSize::Percent(percent) => room.map(|room| room * percent / 100.0),
        TrackSize::MinContent => Some(least_held[index]),
        TrackSize::Fr(_) => None,
        _ => Some(wanted_held[index]),
    };
    let mut bases = vec![0.0_f32; count];
    let mut limits = vec![None; count];
    let mut shares = vec![0.0_f32; count];
    let mut grows_alone = vec![false; count];
    for (index, size) in tracks.iter().enumerate() {
        let (least, most) = match size {
            TrackSize::MinMax(least, most) => (&**least, &**most),
            other => (other, other),
        };
        // A share of what is left says nothing of how small it can be, unless it is alone.
        let lowest = match (size, least) {
            (TrackSize::MinMax(..), TrackSize::Fr(_)) => 0.0,
            _ => at_least(least, index),
        };
        bases[index] = lowest;
        limits[index] = at_most(most, index);
        if let TrackSize::Fr(share) = most {
            shares[index] = *share;
        }
        grows_alone[index] = matches!(size, TrackSize::Auto);
    }
    // Items over several tracks add what is missing, to those that can still change.
    let between = |start: usize, end: usize| (end - start).saturating_sub(1) as f32 * gap;
    for span in spans.iter().filter(|span| span.end > span.start + 1) {
        let end = span.end.min(count);
        if span.start >= end {
            continue;
        }
        let open: Vec<usize> = (span.start..end)
            .filter(|&index| !matches!(tracks[index], TrackSize::Px(_)))
            .collect();
        let has = bases[span.start..end].iter().sum::<f32>() + between(span.start, end);
        if span.least > has && !open.is_empty() {
            #[allow(clippy::cast_precision_loss)]
            let each = (span.least - has) / open.len() as f32;
            for &index in &open {
                bases[index] += each;
            }
        }
        let could: f32 = (span.start..end)
            .map(|index| limits[index].unwrap_or(bases[index]))
            .sum::<f32>()
            + between(span.start, end);
        if span.wanted > could && !open.is_empty() {
            #[allow(clippy::cast_precision_loss)]
            let each = (span.wanted - could) / open.len() as f32;
            for &index in &open {
                if let Some(limit) = &mut limits[index] {
                    *limit += each;
                } else {
                    wanted_held[index] += each;
                }
            }
        }
    }
    let Some(room) = room else {
        // Without a limit, a share of what is left takes what it would like.
        return (0..count)
            .map(|index| {
                limits[index]
                    .unwrap_or(wanted_held[index])
                    .max(bases[index])
            })
            .collect();
    };
    #[allow(clippy::cast_precision_loss)]
    let gaps = count.saturating_sub(1) as f32 * gap;
    let room = room - gaps;
    let mut sizes = bases;
    // First all may grow as far as they like, if there is room.
    let mut free = room - sizes.iter().sum::<f32>();
    let wish: f32 = (0..count)
        .map(|index| limits[index].map_or(0.0, |limit: f32| (limit - sizes[index]).max(0.0)))
        .sum();
    if free > 0.0 && wish > 0.0 {
        let part = (free / wish).min(1.0);
        for index in 0..count {
            if let Some(limit) = limits[index] {
                sizes[index] += (limit - sizes[index]).max(0.0) * part;
            }
        }
        free = room - sizes.iter().sum::<f32>();
    }
    // Then what is left goes by shares, of which a few can be held up by what they hold.
    let mut open: Vec<usize> = (0..count).filter(|&index| shares[index] > 0.0).collect();
    while free > 0.0 && !open.is_empty() {
        let around: f32 = (0..count)
            .filter(|index| !open.contains(index))
            .map(|index| sizes[index])
            .sum();
        let all: f32 = open
            .iter()
            .map(|&index| shares[index])
            .sum::<f32>()
            .max(1.0);
        let unit = (room - around) / all;
        let held: Vec<usize> = open
            .iter()
            .copied()
            .filter(|&index| sizes[index] > unit * shares[index])
            .collect();
        if held.is_empty() {
            for &index in &open {
                sizes[index] = unit * shares[index];
            }
            free = 0.0;
        } else {
            open.retain(|index| !held.contains(index));
        }
    }
    // And the rest goes to those that have no size of their own.
    let free = room - sizes.iter().sum::<f32>();
    let alone = grows_alone.iter().filter(|grows| **grows).count();
    if free > 0.0 && alone > 0 {
        #[allow(clippy::cast_precision_loss)]
        let each = free / alone as f32;
        for index in 0..count {
            if grows_alone[index] {
                sizes[index] += each;
            }
        }
    }
    sizes
}

impl Layouter<'_> {
    /// The lines of an item of this content.
    fn item_lines(&self, item: &ItemContent) -> Option<(GridLine, GridLine, GridLine, GridLine)> {
        if item.kind == ItemKind::Text {
            return None;
        }
        let style = self.styles.get(item.node);
        Some((
            style.grid_column_start.clone(),
            style.grid_column_end.clone(),
            style.grid_row_start.clone(),
            style.grid_row_end.clone(),
        ))
    }

    /// How narrow and how wide an item is, with its margins, borders and padding.
    fn item_widths(&self, item: &ItemContent) -> (f32, f32) {
        if item.kind == ItemKind::Text {
            return (item.least, item.wanted);
        }
        let style = self.styles.get(item.node);
        let sides = BoxSides::of(style, 0.0).horizontal();
        match style.width {
            Size::Px(own) if item.kind == ItemKind::Element => {
                let own = if style.box_sizing == ron::css::BoxSizing::BorderBox {
                    own - BoxSides::of(style, 0.0).insets() + sides
                } else {
                    own + sides
                };
                (own, own)
            }
            _ => (item.least + sides, item.wanted + sides),
        }
    }

    fn grid_places(
        &self,
        style: &ComputedStyle,
        content: &FlexContent,
        order: &[usize],
        room: Option<f32>,
    ) -> (Vec<(usize, Cells)>, Vec<TrackSize>, usize) {
        let gap = style.column_gap;
        let areas = style.grid_template_areas.as_deref();
        let mut columns = expanded(
            style.grid_template_columns.as_deref(),
            room,
            gap,
            order.len(),
        );
        let area_columns = areas.map_or(0, |areas| areas.columns as usize);
        let area_rows = areas.map_or(0, |areas| areas.rows as usize);
        let written_rows =
            expanded(style.grid_template_rows.as_deref(), None, style.row_gap, 0).len();
        let explicit = columns.len().max(area_columns);
        let rows = written_rows.max(area_rows);
        let lines = |index: usize| self.item_lines(&content.items[index]);
        let (placed, column_count, row_count) = place_cells(order, &lines, (explicit, rows), areas);
        // Beyond what is written, columns are as large as what they hold.
        columns.resize(column_count.min(MAX_TRACKS), TrackSize::Auto);
        (placed, columns, row_count)
    }

    /// How narrow and how wide the content of a grid can be.
    pub(super) fn grid_widths(&self, style: &ComputedStyle, content: &FlexContent) -> (f32, f32) {
        let order: Vec<usize> = (0..content.items.len()).collect();
        let (placed, columns, _) = self.grid_places(style, content, &order, None);
        let spans = self.column_spans(content, &placed);
        let gap = style.column_gap;
        #[allow(clippy::cast_precision_loss)]
        let gaps = columns.len().saturating_sub(1) as f32 * gap;
        let wanted: f32 = track_sizes(&columns, &spans, None, gap).iter().sum();
        // Without room, only what cannot get any smaller is left.
        let least: f32 = track_sizes(&columns, &spans, Some(0.0), gap).iter().sum();
        (least + gaps, wanted + gaps)
    }

    fn column_spans(&self, content: &FlexContent, placed: &[(usize, Cells)]) -> Vec<Span> {
        placed
            .iter()
            .map(|&(index, cells)| {
                let (least, wanted) = self.item_widths(&content.items[index]);
                Span {
                    start: cells.column_start,
                    end: cells.column_end,
                    least,
                    wanted,
                }
            })
            .collect()
    }

    /// Works out where items go across, and what is left for when they are done.
    pub(super) fn plan_grid(
        &self,
        style: &ComputedStyle,
        content: &FlexContent,
        order: &[usize],
        left: f32,
        room: f32,
    ) -> (Vec<(usize, ItemPlace)>, GridPlan) {
        let gap = style.column_gap;
        let (placed, columns, row_count) = self.grid_places(style, content, order, Some(room));
        let spans = self.column_spans(content, &placed);
        let sizes = track_sizes(&columns, &spans, Some(room), gap);
        #[allow(clippy::cast_precision_loss)]
        let gaps = sizes.len().saturating_sub(1) as f32 * gap;
        let free = room - sizes.iter().sum::<f32>() - gaps;
        let (start, between) = if style.justify_content == JustifyContent::Normal {
            (0.0, 0.0)
        } else {
            justified(style.justify_content, free, sizes.len(), false)
        };
        let mut starts = Vec::with_capacity(sizes.len() + 1);
        let mut x = left + start;
        for &size in &sizes {
            starts.push(x);
            x += size + gap + between;
        }
        let mut places = Vec::with_capacity(placed.len());
        let mut remembered = Vec::with_capacity(placed.len());
        for &(index, cells) in &placed {
            let item = &content.items[index];
            let first = cells.column_start.min(sizes.len().saturating_sub(1));
            let last = cells
                .column_end
                .saturating_sub(1)
                .min(sizes.len().saturating_sub(1));
            let (area_x, area_width) = match (starts.get(first), starts.get(last)) {
                (Some(&from), Some(&to)) => (from, to + sizes[last] - from),
                _ => (left, room),
            };
            remembered.push((item.node.index(), cells));
            places.push((
                item.node.index(),
                self.place_across(style, item, area_x, area_width),
            ));
        }
        (
            places,
            GridPlan {
                placed: remembered,
                rows: row_count,
            },
        )
    }

    fn place_across(
        &self,
        around: &ComputedStyle,
        item: &ItemContent,
        x: f32,
        width: f32,
    ) -> ItemPlace {
        if item.kind == ItemKind::Text {
            return ItemPlace { x, width };
        }
        let style = self.styles.get(item.node);
        let sides = BoxSides::of(style, width);
        let margin_left = resolve(style.margin.left, width);
        let margin_right = resolve(style.margin.right, width);
        let margins = margin_left.unwrap_or(0.0) + margin_right.unwrap_or(0.0);
        let insets = sides.insets();
        let align = match style.justify_self {
            FlexAlign::Auto => around.justify_items,
            own => own,
        };
        let own = if item.kind == ItemKind::Image {
            Some(item.wanted)
        } else {
            resolve(style.width, width).map(|own| {
                if style.box_sizing == ron::css::BoxSizing::BorderBox {
                    (own - insets).max(0.0)
                } else {
                    own
                }
            })
        };
        let stretches = align_stretches(align) && margin_left.is_some() && margin_right.is_some();
        let mut inner = own.unwrap_or(if stretches {
            width - margins - insets
        } else {
            item.wanted.min(width - margins - insets)
        });
        if let Some(most) = resolve(style.max_width, width) {
            inner = inner.min(most);
        }
        if let Some(minimum) = resolve(style.min_width, width) {
            inner = inner.max(minimum);
        }
        let inner = inner.max(0.0);
        let free = width - inner - insets - margins;
        let offset = match (margin_left, margin_right) {
            (None, None) => free.max(0.0) / 2.0,
            (None, Some(_)) => free.max(0.0),
            _ => match align {
                FlexAlign::Center => free / 2.0,
                FlexAlign::End | FlexAlign::FlexEnd | FlexAlign::SelfEnd => free,
                _ => 0.0,
            },
        };
        ItemPlace {
            x: x + offset + margin_left.unwrap_or(0.0),
            width: inner + insets,
        }
    }

    /// Now that all items are laid out, puts the rows in place, and returns where they end.
    pub(super) fn place_grid(
        &mut self,
        flex: &mut FlexState,
        style: &ComputedStyle,
        height: Option<f32>,
    ) -> f32 {
        let Some(plan) = flex.grid.take() else {
            return flex.top;
        };
        let gap = style.row_gap;
        let mut rows = expanded(style.grid_template_rows.as_deref(), height, gap, 0);
        let implicit = expanded(style.grid_auto_rows.as_deref(), height, gap, 0);
        let written = rows.len();
        for index in written..plan.rows.max(written) {
            rows.push(if implicit.is_empty() {
                TrackSize::Auto
            } else {
                implicit[(index - written) % implicit.len()].clone()
            });
        }
        let mut spans = Vec::with_capacity(plan.placed.len());
        for (node, cells) in &plan.placed {
            if let Some(item) = flex.laid.get(node) {
                let (top, bottom) = self.cross_margins(item);
                let outer = item.bottom - item.top + top + bottom;
                spans.push(Span {
                    start: cells.row_start,
                    end: cells.row_end,
                    least: outer,
                    wanted: outer,
                });
            }
        }
        let sizes = track_sizes(&rows, &spans, height, gap);
        let mut starts = Vec::with_capacity(sizes.len());
        let mut y = flex.top;
        for &size in &sizes {
            starts.push(y);
            y += size + gap;
        }
        for (node, cells) in plan.placed {
            let Some(item) = flex.laid.remove(&node) else {
                continue;
            };
            let first = cells.row_start.min(sizes.len().saturating_sub(1));
            let last = cells
                .row_end
                .saturating_sub(1)
                .min(sizes.len().saturating_sub(1));
            let (area_top, area_height) = match (starts.get(first), starts.get(last)) {
                (Some(&from), Some(&to)) => (from, to + sizes[last] - from),
                _ => (flex.top, 0.0),
            };
            // This goes the way the items in a line of a flexible box do.
            self.place_row_item(item, style, area_top, area_height);
        }
        if sizes.is_empty() { flex.top } else { y - gap }
    }
}
