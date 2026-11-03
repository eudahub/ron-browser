//! Selecting text with the mouse. The aim is exactness: it must be easy to select up to the
//! end of a block and not one character further.
//!
//! A selection is a range of [`Layout::text`], so it follows the text and not the geometry:
//!
//! - Past the end of a line means the end of that line, not the start of what follows.
//! - In the empty space between lines, going forward stays at the end of the line above, and
//!   going backward at the start of the line below. Only the text itself moves it across.
//! - A selection never starts or ends on the line ends between blocks.
//! - Only text gets highlighted, and exactly as wide as it is.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::layout::{Item, ItemKind, Layout, Line, TextMeasurer};

/// What a drag extends by, after a single, double or triple click.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    Character,
    Word,
    Block,
}

/// What is selected: the range clicked on first, and the range reached since.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selection {
    anchor: Range<usize>,
    focus: Range<usize>,
    unit: Unit,
}

/// A rectangle to highlight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Highlight {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Selection {
    /// Starts a selection where the mouse was pressed. `None` on a page without text.
    pub fn start(
        layout: &Layout,
        x: f32,
        y: f32,
        unit: Unit,
        measurer: &mut dyn TextMeasurer,
    ) -> Option<Self> {
        let at = position_at(layout, x, y, Going::Nowhere, measurer)?;
        let anchor = unit_around(layout, at, unit);
        Some(Self {
            focus: anchor.clone(),
            anchor,
            unit,
        })
    }

    /// Everything on the page.
    #[must_use]
    pub fn all(layout: &Layout) -> Self {
        let whole = 0..layout.text.len();
        Self {
            anchor: 0..0,
            focus: whole,
            unit: Unit::Character,
        }
    }

    /// Moves the end of the selection to where the mouse is now.
    pub fn drag_to(&mut self, layout: &Layout, x: f32, y: f32, measurer: &mut dyn TextMeasurer) {
        let going = direction_from(layout, self.anchor.start, y);
        if let Some(at) = position_at(layout, x, y, going, measurer) {
            self.focus = unit_around(layout, at, self.unit);
        }
    }

    /// Moves the end of the selection, by characters, as a click with Shift does.
    pub fn extend_to(&mut self, layout: &Layout, x: f32, y: f32, measurer: &mut dyn TextMeasurer) {
        self.unit = Unit::Character;
        self.drag_to(layout, x, y, measurer);
    }

    /// The selected range of the text. It is empty when nothing is selected.
    #[must_use]
    pub fn range(&self, text: &str) -> Range<usize> {
        let start = self.anchor.start.min(self.focus.start).min(text.len());
        let end = self.anchor.end.max(self.focus.end).min(text.len());
        trim_line_ends(text, start, end)
    }

    /// The selected text, as it goes to the clipboard.
    #[must_use]
    pub fn text<'a>(&self, layout: &'a Layout) -> &'a str {
        &layout.text[self.range(&layout.text)]
    }

    /// What to highlight in this vertical range of the page.
    pub fn highlights(
        &self,
        layout: &Layout,
        top: f32,
        bottom: f32,
        measurer: &mut dyn TextMeasurer,
    ) -> Vec<Highlight> {
        let range = self.range(&layout.text);
        if range.is_empty() {
            return Vec::new();
        }
        let first = if layout.lines_in_order {
            layout.lines.partition_point(|line| line.bottom <= top)
        } else {
            0
        };
        let mut highlights = Vec::new();
        for line in &layout.lines[first..] {
            if line.start as usize >= range.end || (layout.lines_in_order && line.top >= bottom) {
                break;
            }
            if (line.end as usize) <= range.start || line.top >= bottom || line.bottom <= top {
                continue;
            }
            for item in line_items(layout, line) {
                let (start, end) = (item.start as usize, item.end as usize);
                if end <= range.start || start >= range.end {
                    continue;
                }
                let left = text_width(layout, item, range.start.max(start), measurer);
                let right = text_width(layout, item, range.end.min(end), measurer);
                highlights.push(Highlight {
                    x: item.x + left,
                    y: line.top,
                    width: right - left,
                    height: line.bottom - line.top,
                });
            }
        }
        highlights
    }
}

/// Takes line ends off both sides, which are between blocks and not part of either.
fn trim_line_ends(text: &str, mut start: usize, mut end: usize) -> Range<usize> {
    let bytes = text.as_bytes();
    while start < end && bytes[start] == b'\n' {
        start += 1;
    }
    while end > start && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    start..end
}

/// Which way a drag goes, which decides where the space between lines counts.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Going {
    Nowhere,
    Forward,
    Backward,
}

fn direction_from(layout: &Layout, anchor: usize, y: f32) -> Going {
    let anchor_line = layout
        .lines
        .partition_point(|line| (line.end as usize) < anchor)
        .min(layout.lines.len().saturating_sub(1));
    let Some(line) = layout.lines.get(anchor_line) else {
        return Going::Nowhere;
    };
    if y >= line.top {
        Going::Forward
    } else {
        Going::Backward
    }
}

fn position_at(
    layout: &Layout,
    x: f32,
    y: f32,
    going: Going,
    measurer: &mut dyn TextMeasurer,
) -> Option<usize> {
    let lines = &layout.lines;
    let last = lines.last()?;
    if !layout.lines_in_order {
        return Some(position_among_columns(layout, x, y, measurer));
    }
    let index = lines.partition_point(|line| line.bottom <= y);
    let Some(line) = lines.get(index) else {
        return Some(last.end as usize);
    };
    if y >= line.top {
        return Some(position_in_line(layout, line, x, measurer));
    }
    // In the space above this line.
    let Some(previous) = index.checked_sub(1).map(|before| &lines[before]) else {
        return Some(line.start as usize);
    };
    Some(match going {
        Going::Forward => previous.end as usize,
        Going::Backward => line.start as usize,
        Going::Nowhere => {
            if y - previous.bottom < line.top - y {
                position_in_line(layout, previous, x, measurer)
            } else {
                position_in_line(layout, line, x, measurer)
            }
        }
    })
}

/// With text in floats, lines do not simply go down the page, so all are looked through for
/// the one under the point, or else the nearest.
fn position_among_columns(
    layout: &Layout,
    x: f32,
    y: f32,
    measurer: &mut dyn TextMeasurer,
) -> usize {
    let distance = |line: &Line| {
        let across = (line.left - x).max(x - line.right).max(0.0);
        let down = (line.top - y).max(y - line.bottom).max(0.0);
        // Being on the level of a line counts for more than being near it sideways.
        down * 1000.0 + across
    };
    let nearest = layout
        .lines
        .iter()
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))
        .expect("the caller checked that there are lines");
    position_in_line(layout, nearest, x, measurer)
}

fn line_items<'a>(layout: &'a Layout, line: &Line) -> &'a [Item] {
    let first = line.first_item as usize;
    &layout.items[first..first + line.item_count as usize]
}

fn position_in_line(
    layout: &Layout,
    line: &Line,
    x: f32,
    measurer: &mut dyn TextMeasurer,
) -> usize {
    let items = line_items(layout, line);
    for item in items {
        debug_assert_eq!(item.kind, ItemKind::Text);
        if x < item.x {
            return item.start as usize;
        }
        if x < item.x + item.width {
            return position_in_item(layout, item, x - item.x, measurer);
        }
    }
    line.end as usize
}

/// Finds the character boundary nearest to a distance from the start of an item.
fn position_in_item(
    layout: &Layout,
    item: &Item,
    distance: f32,
    measurer: &mut dyn TextMeasurer,
) -> usize {
    let start = item.start as usize;
    let text = &layout.text[start..item.end as usize];
    let mut boundaries: Vec<usize> = text
        .grapheme_indices(true)
        .map(|(index, _)| start + index)
        .collect();
    boundaries.push(start + text.len());
    // The first boundary whose text reaches the distance.
    let mut low = 0;
    let mut high = boundaries.len() - 1;
    while low < high {
        let middle = usize::midpoint(low, high);
        if text_width(layout, item, boundaries[middle], measurer) < distance {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    if low == 0 {
        return boundaries[0];
    }
    let after = text_width(layout, item, boundaries[low], measurer);
    let before = text_width(layout, item, boundaries[low - 1], measurer);
    if distance - before < after - distance {
        boundaries[low - 1]
    } else {
        boundaries[low]
    }
}

/// The width of an item's text up to a position in it.
fn text_width(
    layout: &Layout,
    item: &Item,
    position: usize,
    measurer: &mut dyn TextMeasurer,
) -> f32 {
    let start = item.start as usize;
    if position >= item.end as usize {
        return item.width;
    }
    if position <= start {
        return 0.0;
    }
    measurer.width(item.font, &layout.text[start..position])
}

fn unit_around(layout: &Layout, at: usize, unit: Unit) -> Range<usize> {
    match unit {
        Unit::Character => at..at,
        Unit::Word => word_around(layout, at),
        Unit::Block => block_around(&layout.text, at),
    }
}

/// The word or other stretch at a position. A position at the end of a word counts as in it.
fn word_around(layout: &Layout, at: usize) -> Range<usize> {
    let index = layout
        .lines
        .partition_point(|line| (line.end as usize) < at)
        .min(layout.lines.len().saturating_sub(1));
    let Some(line) = layout.lines.get(index) else {
        return at..at;
    };
    let start = line.start as usize;
    let text = &layout.text[start..line.end as usize];
    let mut previous = at..at;
    for (offset, piece) in text.split_word_bound_indices() {
        let range = start + offset..start + offset + piece.len();
        if range.contains(&at) {
            // Clicking just past a word, on what follows it, still means that word
            // unless it is one itself.
            let is_blank = piece.chars().all(char::is_whitespace);
            return if is_blank && previous.end == at && range.start == at {
                previous
            } else {
                range
            };
        }
        previous = range;
    }
    previous
}

/// The block at a position: everything between the line ends around it, and neither of them.
fn block_around(text: &str, at: usize) -> Range<usize> {
    let at = at.min(text.len());
    let start = text[..at].rfind('\n').map_or(0, |index| index + 1);
    let end = text[at..].find('\n').map_or(text.len(), |index| at + index);
    start..end
}
