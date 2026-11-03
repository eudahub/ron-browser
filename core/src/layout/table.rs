//! Tables as grids. The width of a column depends on all of its cells, so a table is laid out once
//! all of it is there. Each cell is then laid out at its place in its column, starting at the top of
//! its row. Only when a row is complete is it known how high it is, which decides where the content
//! of its shorter cells goes, and how far their backgrounds and borders reach.
//!
//! What does not fit the structure of a table, as `display: table` around ordinary blocks, is not
//! made into one, and stacks as blocks. Not handled: `table-layout: fixed`, footers that come before
//! the body, row backgrounds that are images and aligning cells on their baselines, which go to the
//! top instead. Collapsed borders overlap by the widest border of the table, which is only exact when
//! the cells have the same.

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ron::css::{
    BorderCollapse, BoxSizing, ComputedStyle, Display, Size, VerticalAlign, VerticalPosition,
};
use ron::html::{NodeId, NodeKind};

use super::positioned::is_out_of_flow;
use super::{BoxSides, Color, Frame, InlineReturn, Layouter, MAX_NESTED_LAYOUTS, Margin, resolve};

/// As in the HTML standard.
const MAX_COLUMN_SPAN: u32 = 1000;
const MAX_ROW_SPAN: u32 = 65534;
/// More columns than this are put together in the last, so that a page cannot ask for any amount of
/// memory for each table.
const MAX_COLUMNS: usize = 4000;

/// Whether a display makes an element a table.
pub(super) fn is_table(display: Display) -> bool {
    matches!(display, Display::Table | Display::InlineTable)
}

#[derive(Clone, Copy)]
struct CellPlace {
    row: u32,
    column: u32,
    columns: u32,
    rows: u32,
}

/// What a table needs to know before any of it is laid out.
pub(super) struct TableGrid {
    /// By the position of the cell's node.
    cells: HashMap<usize, CellPlace>,
    /// How narrow each column can get, and how wide it would be without wrapping.
    least: Vec<f32>,
    wanted: Vec<f32>,
    /// What a column asks for in percent of the table, or 0.
    percent: Vec<f32>,
    /// Whether a column asks for a width of its own, and so takes no room that is left over.
    fixed: Vec<bool>,
    gap_x: f32,
    gap_y: f32,
    /// How far the cells are from the sides of the table.
    edge_x: f32,
    edge_y: f32,
    /// How narrow the captions can get, which the table cannot be narrower than.
    caption_least: f32,
}

impl TableGrid {
    /// What the room between the columns takes.
    fn gaps(&self) -> f32 {
        match self.least.len() {
            0 => 0.0,
            #[allow(clippy::cast_precision_loss)]
            count => (count - 1) as f32 * self.gap_x + 2.0 * self.edge_x,
        }
    }

    /// How narrow the content of the table can get.
    fn least_width(&self) -> f32 {
        (self.least.iter().sum::<f32>() + self.gaps())
            .max(self.caption_least)
            .max(0.0)
    }

    /// How wide the content of the table would be without wrapping. A column in percent counts as
    /// what it takes for that many percent to hold it.
    fn wanted_width(&self) -> f32 {
        let all_percent: f32 = self.percent.iter().sum::<f32>().min(100.0);
        let others: f32 = self
            .wanted
            .iter()
            .zip(&self.percent)
            .filter(|&(_, &percent)| percent == 0.0)
            .map(|(&wanted, _)| wanted)
            .sum();
        let mut columns = if all_percent < 100.0 {
            others * 100.0 / (100.0 - all_percent)
        } else {
            others
        };
        for (&wanted, &percent) in self.wanted.iter().zip(&self.percent) {
            if percent > 0.0 {
                columns = columns.max(wanted * 100.0 / percent);
            }
        }
        (columns + self.gaps()).max(self.caption_least).max(0.0)
    }

    /// How wide each column is in a table with this much room for its content.
    fn column_widths(&self, width: f32) -> Vec<f32> {
        let room = (width - self.gaps()).max(0.0);
        let goals: Vec<f32> = self
            .wanted
            .iter()
            .zip(&self.least)
            .zip(&self.percent)
            .map(|((&wanted, &least), &percent)| {
                if percent > 0.0 {
                    (room * percent / 100.0).max(least)
                } else {
                    wanted
                }
            })
            .collect();
        let all: f32 = goals.iter().sum();
        let least: f32 = self.least.iter().sum();
        if all <= room {
            return self.with_room_left(goals, room - all);
        }
        if least >= room || all <= least {
            return self.least.clone();
        }
        let share = (room - least) / (all - least);
        goals
            .iter()
            .zip(&self.least)
            .map(|(&goal, &least)| least + (goal - least) * share)
            .collect()
    }

    /// Hands out what is left, to columns that do not ask for a width if there are any, in the
    /// measure of what they take already.
    fn with_room_left(&self, mut widths: Vec<f32>, left: f32) -> Vec<f32> {
        let free = |index: usize| self.percent[index] == 0.0 && !self.fixed[index];
        let any_free = (0..widths.len()).any(free);
        let takes = |index: usize| !any_free || free(index);
        let among: f32 = (0..widths.len())
            .filter(|&index| takes(index))
            .map(|index| widths[index])
            .sum();
        #[allow(clippy::cast_precision_loss)]
        let count = (0..widths.len()).filter(|&index| takes(index)).count() as f32;
        for (index, width) in widths.iter_mut().enumerate() {
            if takes(index) {
                *width += if among > 0.0 {
                    left * *width / among
                } else {
                    left / count
                };
            }
        }
        widths
    }
}

/// A cell that has been laid out, and waits for how high its row turns out to be.
struct LaidCell {
    node: NodeId,
    last_row: u32,
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

/// A table being laid out.
pub(super) struct TableState {
    node: NodeId,
    grid: Rc<TableGrid>,
    /// Where each column starts, and how wide it is.
    xs: Vec<f32>,
    widths: Vec<f32>,
    row: Option<u32>,
    row_top: f32,
    row_least_height: f32,
    cells_in_row: u32,
    waiting: Vec<LaidCell>,
}

/// The rows of a table with the cells of each, and its columns as far as they are written out.
#[derive(Default)]
struct Structure {
    rows: Vec<Vec<NodeId>>,
    columns: Vec<NodeId>,
    captions: Vec<NodeId>,
}

fn number_attribute(layouter: &Layouter<'_>, id: NodeId, name: &str) -> Option<u32> {
    layouter
        .document
        .attributes(id)
        .find(|attribute| attribute.name.eq_ignore_ascii_case(name))
        .and_then(|attribute| attribute.decoded_value().trim().parse().ok())
}

impl Layouter<'_> {
    /// Whether what comes now is a part of a table and not within a cell: there, only cells and
    /// captions make boxes, and white space does not count.
    pub(super) fn in_table_structure(&self) -> bool {
        match (self.tables.last(), self.frames.last()) {
            (Some(table), Some(frame)) => table.node == frame.node,
            _ => false,
        }
    }

    /// Whether an element is something that a table is made of.
    pub(super) fn is_table_part(display: Display) -> bool {
        matches!(
            display,
            Display::TableRow
                | Display::TableRowGroup
                | Display::TableHeaderGroup
                | Display::TableFooterGroup
                | Display::TableCell
                | Display::TableCaption
        )
    }

    fn is_blank_text(&self, id: NodeId) -> bool {
        self.document
            .decoded_text(id)
            .bytes()
            .all(|byte| byte.is_ascii_whitespace())
    }

    /// Takes in what is inside a row or a group of rows. Returns false when it is something else
    /// than a table is made of.
    fn add_row_parts(&self, parent: NodeId, is_row: bool, structure: &mut Structure) -> bool {
        let document = self.document;
        let mut in_row = is_row;
        if is_row {
            structure.rows.push(Vec::new());
        }
        for child in document.children(parent) {
            match document.kind(child) {
                NodeKind::Text if !self.is_blank_text(child) => return false,
                NodeKind::Element => {}
                _ => continue,
            }
            if is_out_of_flow(self.styles.get(child)) {
                continue;
            }
            match self.styles.get(child).display {
                Display::None => {}
                Display::TableCell => {
                    // Cells without a row around them make one together.
                    if !in_row {
                        structure.rows.push(Vec::new());
                        in_row = true;
                    }
                    if let Some(row) = structure.rows.last_mut() {
                        row.push(child);
                    }
                }
                Display::TableRow if !is_row => {
                    in_row = false;
                    if !self.add_row_parts(child, true, structure) {
                        return false;
                    }
                }
                _ if document.first_child(child).is_some() => return false,
                _ => {}
            }
        }
        true
    }

    /// Finds the rows of a table, if it is made up as a table has to be.
    fn table_structure(&self, table: NodeId) -> Option<Structure> {
        let document = self.document;
        let mut structure = Structure::default();
        let mut in_row = false;
        for child in document.children(table) {
            match document.kind(child) {
                NodeKind::Text if !self.is_blank_text(child) => return None,
                NodeKind::Element => {}
                _ => continue,
            }
            if is_out_of_flow(self.styles.get(child)) {
                continue;
            }
            match self.styles.get(child).display {
                Display::None => {}
                Display::TableCaption => structure.captions.push(child),
                Display::TableColumn => structure.columns.push(child),
                Display::TableColumnGroup => {
                    let before = structure.columns.len();
                    structure
                        .columns
                        .extend(document.children(child).filter(|&column| {
                            self.styles.get(column).display == Display::TableColumn
                                && document.kind(column) == NodeKind::Element
                        }));
                    if structure.columns.len() == before {
                        structure.columns.push(child);
                    }
                }
                Display::TableRow => {
                    in_row = false;
                    self.add_row_parts(child, true, &mut structure)
                        .then_some(())?;
                }
                Display::TableRowGroup | Display::TableHeaderGroup | Display::TableFooterGroup => {
                    in_row = false;
                    self.add_row_parts(child, false, &mut structure)
                        .then_some(())?;
                }
                Display::TableCell => {
                    if !in_row {
                        structure.rows.push(Vec::new());
                        in_row = true;
                    }
                    if let Some(row) = structure.rows.last_mut() {
                        row.push(child);
                    }
                }
                _ if document.first_child(child).is_some() => return None,
                _ => {}
            }
        }
        Some(structure)
    }

    /// The grid of a table, or nothing when it is not made up as one or lies too deep.
    pub(super) fn table_grid(&mut self, table: NodeId) -> Option<Rc<TableGrid>> {
        if let Some(found) = self.table_grids.get(&table.index()) {
            return found.clone();
        }
        let grid = if self.tables.len() + self.flexes.len() + self.grid_depth >= MAX_NESTED_LAYOUTS
        {
            None
        } else {
            self.grid_depth += 1;
            let grid = self
                .table_structure(table)
                .map(|structure| Rc::new(self.build_grid(table, &structure)));
            self.grid_depth -= 1;
            grid
        };
        self.table_grids.insert(table.index(), grid.clone());
        grid
    }

    fn build_grid(&mut self, table: NodeId, structure: &Structure) -> TableGrid {
        let places = self.place_cells(structure);
        let count = places
            .iter()
            .map(|(_, place)| (place.column + place.columns) as usize)
            .max()
            .unwrap_or(0);
        let mut grid = TableGrid {
            cells: HashMap::with_capacity(places.len()),
            least: vec![0.0; count],
            wanted: vec![0.0; count],
            percent: vec![0.0; count],
            fixed: vec![false; count],
            gap_x: 0.0,
            gap_y: 0.0,
            edge_x: 0.0,
            edge_y: 0.0,
            caption_least: 0.0,
        };
        self.set_gaps(table, &places, &mut grid);
        self.apply_written_columns(structure, &mut grid);
        let depth = self.inline_box_depth;
        for &caption in &structure.captions {
            let insets = BoxSides::of(self.styles.get(caption), 0.0).horizontal();
            let (least, _) = self.content_widths(caption, depth, true);
            grid.caption_least = grid.caption_least.max(least + insets);
        }
        // Those that take one column decide first, and those over several add what is missing.
        let mut spanning = Vec::new();
        for &(cell, place) in &places {
            grid.cells.insert(cell.index(), place);
            let style = self.styles.get(cell);
            let insets = BoxSides::of(style, 0.0).insets();
            let (least, wanted) = self.content_widths(cell, depth, true);
            let own = match style.width {
                Size::Px(px) if style.box_sizing == BoxSizing::BorderBox => Some(px),
                Size::Px(px) => Some(px + insets),
                _ => None,
            };
            let percent = match style.width {
                Size::Percent(percent) => percent.clamp(0.0, 100.0),
                _ => 0.0,
            };
            let least = least + insets;
            let wanted = own.unwrap_or(wanted + insets).max(least);
            if place.columns > 1 {
                spanning.push((place, least, wanted, own.is_some()));
                continue;
            }
            let column = place.column as usize;
            grid.least[column] = grid.least[column].max(least);
            grid.wanted[column] = grid.wanted[column].max(wanted);
            grid.percent[column] = grid.percent[column].max(percent);
            grid.fixed[column] |= own.is_some();
        }
        spanning.sort_by_key(|(place, ..)| place.columns);
        for (place, least, wanted, fixed) in spanning {
            let range = place.column as usize..(place.column + place.columns) as usize;
            #[allow(clippy::cast_precision_loss)]
            let between = (place.columns - 1) as f32 * grid.gap_x;
            #[allow(clippy::cast_precision_loss)]
            let share = |missing: f32| missing.max(0.0) / place.columns as f32;
            let more_least = share(least - between - grid.least[range.clone()].iter().sum::<f32>());
            let more_wanted =
                share(wanted - between - grid.wanted[range.clone()].iter().sum::<f32>());
            for column in range {
                grid.least[column] += more_least;
                grid.wanted[column] += more_wanted;
                grid.wanted[column] = grid.wanted[column].max(grid.least[column]);
                grid.fixed[column] |= fixed;
            }
        }
        grid
    }

    /// Finds which row and column each cell goes in. A cell spanning rows keeps its columns from
    /// the cells of the rows below.
    fn place_cells(&self, structure: &Structure) -> Vec<(NodeId, CellPlace)> {
        let row_count = u32::try_from(structure.rows.len()).unwrap_or(u32::MAX);
        let mut taken: Vec<u32> = Vec::new();
        let mut places = Vec::new();
        for (row, cells) in (0_u32..).zip(&structure.rows) {
            let mut column = 0;
            for &cell in cells {
                while taken.get(column).is_some_and(|&rows| rows > 0) {
                    column += 1;
                }
                let columns = number_attribute(self, cell, "colspan")
                    .unwrap_or(1)
                    .clamp(1, MAX_COLUMN_SPAN) as usize;
                // Zero means all the rest of the rows.
                let left = row_count - row;
                let rows = match number_attribute(self, cell, "rowspan").unwrap_or(1) {
                    0 => left,
                    rows => rows.min(MAX_ROW_SPAN).min(left),
                };
                let first = column.min(MAX_COLUMNS - 1);
                let columns = columns.min(MAX_COLUMNS - first);
                if taken.len() < first + columns {
                    taken.resize(first + columns, 0);
                }
                for slot in &mut taken[first..first + columns] {
                    *slot = (*slot).max(rows);
                }
                places.push((
                    cell,
                    CellPlace {
                        row,
                        column: u32::try_from(first).unwrap_or(0),
                        columns: u32::try_from(columns).unwrap_or(1),
                        rows,
                    },
                ));
                column = first + columns;
            }
            for slot in &mut taken {
                *slot = slot.saturating_sub(1);
            }
        }
        places
    }

    fn set_gaps(&self, table: NodeId, places: &[(NodeId, CellPlace)], grid: &mut TableGrid) {
        let style = self.styles.get(table);
        if style.border_collapse == BorderCollapse::Collapse {
            let (mut across, mut down) = (0.0_f32, 0.0_f32);
            for &(cell, _) in places {
                let border = self.styles.get(cell).border_width;
                across = across.max(border.left).max(border.right);
                down = down.max(border.top).max(border.bottom);
            }
            let own = style.border_width;
            grid.gap_x = -across;
            grid.gap_y = -down;
            grid.edge_x = -across.min(own.left.max(own.right));
            grid.edge_y = -down.min(own.top.max(own.bottom));
        } else {
            grid.gap_x = style.border_spacing;
            grid.gap_y = style.border_spacing;
            grid.edge_x = style.border_spacing;
            grid.edge_y = style.border_spacing;
        }
    }

    /// Takes in what `<col>` elements ask for.
    fn apply_written_columns(&self, structure: &Structure, grid: &mut TableGrid) {
        let mut column = 0;
        for &written in &structure.columns {
            let span = number_attribute(self, written, "span")
                .unwrap_or(1)
                .clamp(1, MAX_COLUMN_SPAN) as usize;
            for _ in 0..span {
                if column >= grid.least.len() {
                    return;
                }
                match self.styles.get(written).width {
                    Size::Px(px) => {
                        grid.wanted[column] = px;
                        grid.fixed[column] = true;
                    }
                    Size::Percent(percent) => grid.percent[column] = percent.clamp(0.0, 100.0),
                    Size::Auto | Size::Calc(_) => {}
                }
                column += 1;
            }
        }
    }

    /// How much a table holds across, or nothing if it is not one.
    pub(super) fn table_content_width(&mut self, id: NodeId, style: &ComputedStyle) -> Option<f32> {
        if !is_table(style.display) {
            return None;
        }
        let grid = self.table_grid(id)?;
        let available = self.frames.last().map_or(0.0, |frame| frame.content_width);
        let sides = BoxSides::of(style, available);
        let width = match resolve(style.width, available) {
            Some(width) if style.box_sizing == BoxSizing::BorderBox => width - sides.insets(),
            Some(width) => width,
            None => grid.wanted_width().min(available - sides.horizontal()),
        };
        Some(width.max(grid.least_width()).max(0.0))
    }

    /// How narrow and how wide a table can be, if it is one.
    pub(super) fn table_widths(&mut self, id: NodeId) -> Option<(f32, f32)> {
        let grid = self.table_grid(id)?;
        Some((grid.least_width(), grid.wanted_width()))
    }

    /// Starts on the rows of a table, whose own box has been opened.
    pub(super) fn begin_table(&mut self, id: NodeId) {
        if self
            .frames
            .last()
            .is_none_or(|frame: &Frame| frame.node != id)
        {
            return;
        }
        let Some(grid) = self.table_grid(id) else {
            return;
        };
        if self.frames.last().is_some_and(|frame| frame.top.is_none()) {
            self.commit_margins();
        }
        let frame = self.frames.last().expect("checked above");
        let widths = grid.column_widths(frame.content_width);
        let mut xs = Vec::with_capacity(widths.len());
        let mut x = frame.content_x + grid.edge_x;
        for &width in &widths {
            xs.push(x);
            x += width + grid.gap_x;
        }
        self.tables.push(TableState {
            node: id,
            grid,
            xs,
            widths,
            row: None,
            row_top: 0.0,
            row_least_height: 0.0,
            cells_in_row: 0,
            waiting: Vec::new(),
        });
    }

    /// Goes on with what a table is made of. Cells and captions are boxes of their own.
    pub(super) fn enter_table_part(&mut self, id: NodeId, style: &ComputedStyle) {
        match style.display {
            Display::TableCell => self.open_cell(id, style),
            Display::TableCaption => self.open_frame(id, style),
            _ => {}
        }
    }

    /// Ends the current row and starts on one that holds this cell.
    fn start_row(&mut self, cell: NodeId, row: u32) {
        self.finish_row(row);
        let parent = self.document.parent(cell);
        let least = parent
            .map(|parent| self.styles.get(parent))
            .filter(|style| style.display == Display::TableRow)
            .and_then(|style| match style.height {
                Size::Px(px) => Some(px),
                _ => None,
            })
            .unwrap_or(0.0);
        let cursor = self.cursor + self.margin.value();
        self.cursor = cursor;
        self.margin = Margin::default();
        let Some(table) = self.tables.last_mut() else {
            return;
        };
        let top = if table.row.is_some() {
            cursor + table.grid.gap_y
        } else {
            cursor + table.grid.edge_y
        };
        table.row = Some(row);
        table.row_top = top;
        table.row_least_height = least;
        table.cells_in_row = 0;
        self.text_begun = false;
    }

    /// What a cell shows behind it when it has no background itself, from its row or group.
    fn background_around(&self, cell: NodeId) -> Option<Color> {
        let table = self.tables.last()?.node;
        std::iter::successors(self.document.parent(cell), |&node| {
            self.document.parent(node)
        })
        .take_while(|&node| node != table)
        .map(|node| self.styles.get(node).background_color)
        .find(|color| color.alpha != 0)
    }

    fn open_cell(&mut self, id: NodeId, style: &ComputedStyle) {
        let Some(table) = self.tables.last() else {
            return;
        };
        let Some(&place) = table.grid.cells.get(&id.index()) else {
            return;
        };
        if table.row != Some(place.row) {
            self.start_row(id, place.row);
        }
        let Some(table) = self.tables.last_mut() else {
            return;
        };
        let first = place.column as usize;
        let last = first + place.columns as usize - 1;
        let x = table.xs[first];
        let width = table.xs[last] + table.widths[last] - x;
        let top = table.row_top;
        // Copied text keeps the cells of a row apart, and on one line.
        if std::mem::replace(&mut table.cells_in_row, 1) != 0 && !self.out.text.is_empty() {
            self.out.text.push('\t');
            self.text_begun = true;
        }
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
        self.lowest_float = 0.0;
        let around = (style.background_color.alpha == 0
            && style.visibility == ron::css::Visibility::Visible)
            .then(|| self.background_around(id))
            .flatten();
        let content_width = (width - BoxSides::of(style, 0.0).insets()).max(0.0);
        self.open_frame_at(id, style, Some(content_width), Some((x, top)));
        if let Some(color) = around {
            self.push_rect(x, top, width, 0.0, color);
        }
        let around_item = around.map(|_| self.out.items.len() - 1);
        let mut saved = saved;
        // What is drawn behind the content does not move with it.
        saved.items_start = self.out.items.len();
        saved.lines_start = self.out.lines.len();
        saved.link_areas_start = self.out.link_areas.len();
        saved.area_shapes_start = self.out.area_shapes.len();
        if let Some(frame) = self.frames.last_mut() {
            frame.width = width;
            frame.background_item = frame.background_item.or(around_item);
            frame.cell_return = Some(Box::new(saved));
        }
    }

    /// Ends a cell, whose row decides the rest.
    pub(super) fn close_cell(&mut self, frame: &Frame, saved: InlineReturn, top: f32, bottom: f32) {
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
        let cell = LaidCell {
            node: frame.node,
            last_row: 0,
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
        let Some(table) = self.tables.last_mut() else {
            return;
        };
        let Some(place) = table.grid.cells.get(&frame.node.index()) else {
            return;
        };
        let last_row = place.row + place.rows.max(1) - 1;
        table.waiting.push(LaidCell { last_row, ..cell });
    }

    /// Ends the current row, if any, as the next one to get a cell is `next`: all cells that end
    /// before that are done now.
    fn finish_row(&mut self, next: u32) {
        let Some(table) = self.tables.last_mut() else {
            return;
        };
        if table.row.is_none() {
            return;
        }
        let mut bottom = table.row_top + table.row_least_height;
        for cell in &table.waiting {
            // Those that go on below are only counted where they end.
            if cell.last_row < next {
                bottom = bottom.max(cell.bottom);
            }
        }
        let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut table.waiting)
            .into_iter()
            .partition(|cell| cell.last_row < next);
        table.waiting = waiting;
        self.cursor = bottom;
        for cell in done {
            self.finish_cell(cell, bottom);
        }
    }

    /// Now that its row is known, puts the content of a cell in place and draws the rest.
    fn finish_cell(&mut self, cell: LaidCell, bottom: f32) {
        let style = self.styles.get(cell.node);
        let free = bottom - cell.bottom;
        let down = match style.vertical_align {
            VerticalPosition::Keyword(VerticalAlign::Middle) => free / 2.0,
            VerticalPosition::Keyword(VerticalAlign::Bottom | VerticalAlign::TextBottom) => free,
            _ => 0.0,
        };
        if down > 0.0 {
            self.move_ranges(
                [cell.items, cell.lines, cell.link_areas, cell.area_shapes],
                0.0,
                down,
            );
        }
        let height = bottom - cell.top;
        if let Some(index) = cell.clip_marker {
            self.out.items[index].height = height;
        }
        if let Some(index) = cell.background_item {
            self.out.items[index].height = height;
        }
        if let Some(index) = cell.background_tile {
            self.finish_background_tile(cell.node, index, cell.x, cell.top, cell.width, height);
        }
        self.push_borders(style, cell.x, cell.top, cell.width, bottom);
        self.out.width = self.out.width.max(cell.x + cell.width);
    }

    /// Ends the rows of a table, before its own box ends.
    pub(super) fn finish_table(&mut self) {
        self.finish_row(u32::MAX);
        if let Some(table) = self.tables.pop() {
            if table.row.is_some() {
                self.cursor += table.grid.edge_y;
            }
            self.table_grids.remove(&table.node.index());
        }
        self.text_begun = false;
    }
}
