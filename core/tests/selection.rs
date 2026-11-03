//! Selecting text. Characters are 8 wide and lines 16 high, as in the layout tests, and the
//! tests drag as a mouse would. The main concern is selecting up to the end of a block and
//! not one character further.

use ron_browser_core::layout::{FontSpec, Layout, TextMeasurer};
use ron_browser_core::page::Page;
use ron_browser_core::selection::{Selection, Unit};

#[derive(Default)]
struct Measurer {
    sizes: Vec<f32>,
}

impl TextMeasurer for Measurer {
    fn font(&mut self, spec: &FontSpec) -> u32 {
        self.sizes.push(spec.size);
        u32::try_from(self.sizes.len() - 1).unwrap()
    }
    fn width(&mut self, font: u32, text: &str) -> f32 {
        self.sizes[font as usize] * 0.5 * text.chars().count() as f32
    }
    fn ascent(&mut self, font: u32) -> f32 {
        self.sizes[font as usize] * 0.8
    }
    fn descent(&mut self, font: u32) -> f32 {
        self.sizes[font as usize] * 0.2
    }
}

struct Test {
    layout: Layout,
    measurer: Measurer,
}

impl Test {
    fn new(body: &str, width: f32) -> Self {
        let source = format!("<!DOCTYPE html><style>body{{margin:0}}</style>{body}");
        let mut measurer = Measurer::default();
        let mut page = Page::open(&source);
        page.lay_out(width, 600.0, &mut measurer);
        Self {
            layout: page.layout().clone(),
            measurer,
        }
    }

    fn press(&mut self, x: f32, y: f32, unit: Unit) -> Selection {
        Selection::start(&self.layout, x, y, unit, &mut self.measurer).expect("a page with text")
    }

    /// Selects by pressing at one point and dragging to another.
    fn drag(&mut self, from: (f32, f32), to: (f32, f32)) -> String {
        let mut selection = self.press(from.0, from.1, Unit::Character);
        selection.drag_to(&self.layout, to.0, to.1, &mut self.measurer);
        selection.text(&self.layout).to_owned()
    }

    fn click(&mut self, x: f32, y: f32, unit: Unit) -> String {
        self.press(x, y, unit).text(&self.layout).to_owned()
    }
}

/// Two paragraphs: the first line is at y 16 to 32 and the second at y 48 to 64, with the
/// margin between them from 32 to 48.
fn two_paragraphs() -> Test {
    Test::new("<p>first</p><p>second</p>", 500.0)
}

#[test]
fn nothing_is_selected_by_a_plain_click() {
    let mut test = two_paragraphs();
    assert_eq!(test.click(12.0, 20.0, Unit::Character), "");
}

#[test]
fn dragging_within_a_line_takes_the_nearest_characters() {
    let mut test = two_paragraphs();
    assert_eq!(test.drag((0.0, 20.0), (16.0, 20.0)), "fi");
    assert_eq!(test.drag((0.0, 20.0), (19.0, 20.0)), "fi");
    assert_eq!(test.drag((0.0, 20.0), (21.0, 20.0)), "fir");
    assert_eq!(test.drag((16.0, 20.0), (0.0, 20.0)), "fi");
}

/// The case that is so often wrong: up to the end of the block and not one character further.
#[test]
fn past_the_end_of_a_line_stops_at_the_end_of_the_block() {
    let mut test = two_paragraphs();
    assert_eq!(test.drag((8.0, 20.0), (40.0, 20.0)), "irst");
    assert_eq!(test.drag((8.0, 20.0), (499.0, 20.0)), "irst");
    assert_eq!(test.drag((8.0, 20.0), (100_000.0, 31.9)), "irst");
}

#[test]
fn the_space_below_a_block_stops_at_its_end() {
    let mut test = two_paragraphs();
    for x in [0.0, 8.0, 100.0, 499.0] {
        for y in [32.0, 40.0, 47.9] {
            assert_eq!(test.drag((8.0, 20.0), (x, y)), "irst", "at {x}, {y}");
        }
    }
}

#[test]
fn the_start_of_the_next_block_does_not_count_until_the_text_is_reached() {
    let mut test = two_paragraphs();
    // Left of the first character, and before the middle of it, nothing of it is taken.
    assert_eq!(test.drag((8.0, 20.0), (0.0, 50.0)), "irst");
    assert_eq!(test.drag((8.0, 20.0), (3.9, 50.0)), "irst");
    // Past the middle of it, it is.
    assert_eq!(test.drag((8.0, 20.0), (4.1, 50.0)), "irst\n\ns");
}

#[test]
fn going_backward_is_exact_too() {
    let mut test = two_paragraphs();
    // From the middle of the second block up to the end of the first: nothing of the first.
    assert_eq!(test.drag((24.0, 50.0), (499.0, 20.0)), "sec");
    // Up into the space between them: still nothing of the first.
    assert_eq!(test.drag((24.0, 50.0), (0.0, 40.0)), "sec");
    // Above the start of the page: everything back to its start.
    assert_eq!(test.drag((24.0, 50.0), (0.0, 0.0)), "first\n\nsec");
    assert_eq!(test.drag((24.0, 50.0), (8.0, 20.0)), "irst\n\nsec");
}

#[test]
fn below_the_last_line_stops_at_the_end_of_the_page() {
    let mut test = two_paragraphs();
    assert_eq!(test.drag((8.0, 50.0), (0.0, 5000.0)), "econd");
}

#[test]
fn a_selection_across_blocks_keeps_what_separates_them() {
    let mut test = Test::new("<p>one</p><p>two</p><div>x</div><div>y</div>", 500.0);
    let text = test.drag((0.0, 20.0), (499.0, 5000.0));
    assert_eq!(text, "one\n\ntwo\n\nx\ny");
}

#[test]
fn triple_click_takes_exactly_the_block() {
    let mut test = Test::new("<p>aaa bbb ccc ddd</p><p>next</p>", 64.0);
    assert_eq!(test.layout.lines.len(), 3);
    // On any of the wrapped lines of the first paragraph.
    assert_eq!(test.click(8.0, 20.0, Unit::Block), "aaa bbb ccc ddd");
    assert_eq!(test.click(8.0, 36.0, Unit::Block), "aaa bbb ccc ddd");
    // Past the end of its last line too.
    assert_eq!(test.click(60.0, 36.0, Unit::Block), "aaa bbb ccc ddd");
    assert_eq!(test.click(8.0, 70.0, Unit::Block), "next");
}

#[test]
fn triple_click_on_list_items_and_lines_stays_within_them() {
    let mut test = Test::new(
        "<ul style='margin:0;padding:0'><li>one<li>two</ul>a<br>b",
        500.0,
    );
    assert_eq!(test.click(8.0, 4.0, Unit::Block), "one");
    assert_eq!(test.click(8.0, 20.0, Unit::Block), "two");
    assert_eq!(test.click(4.0, 36.0, Unit::Block), "a");
    assert_eq!(test.click(4.0, 52.0, Unit::Block), "b");
}

#[test]
fn dragging_after_a_triple_click_takes_whole_blocks() {
    let mut test = Test::new("<div>one</div><div>two</div><div>three</div>", 500.0);
    let mut selection = test.press(8.0, 4.0, Unit::Block);
    selection.drag_to(&test.layout, 0.0, 20.0, &mut test.measurer);
    assert_eq!(selection.text(&test.layout), "one\ntwo");
}

#[test]
fn double_click_takes_the_word() {
    let mut test = Test::new("aaa bbb, ccc", 500.0);
    assert_eq!(test.click(4.0, 4.0, Unit::Word), "aaa");
    assert_eq!(test.click(36.0, 4.0, Unit::Word), "bbb");
    assert_eq!(test.click(84.0, 4.0, Unit::Word), "ccc");
    assert_eq!(test.click(60.0, 4.0, Unit::Word), ",");
}

#[test]
fn dragging_after_a_double_click_takes_whole_words() {
    let mut test = Test::new("aaa bbb ccc", 500.0);
    let mut selection = test.press(36.0, 4.0, Unit::Word);
    selection.drag_to(&test.layout, 68.0, 4.0, &mut test.measurer);
    assert_eq!(selection.text(&test.layout), "bbb ccc");
    selection.drag_to(&test.layout, 4.0, 4.0, &mut test.measurer);
    assert_eq!(selection.text(&test.layout), "aaa bbb");
}

#[test]
fn a_wrapped_line_ends_without_the_space_it_hides() {
    let mut test = Test::new("aaa bbb", 32.0);
    assert_eq!(test.drag((0.0, 4.0), (499.0, 4.0)), "aaa");
    assert_eq!(test.drag((0.0, 4.0), (8.0, 20.0)), "aaa b");
    assert_eq!(test.drag((0.0, 4.0), (499.0, 20.0)), "aaa bbb");
}

#[test]
fn lists_copy_their_text_without_markers() {
    let mut test = Test::new("<ol><li>one<li>two</ol>", 500.0);
    let text = test.drag((0.0, 0.0), (499.0, 5000.0));
    assert_eq!(text, "one\ntwo");
}

#[test]
fn preformatted_text_copies_as_written() {
    let mut test = Test::new("<pre style='margin:0'>a  b\n\n  c</pre>", 500.0);
    assert_eq!(test.drag((0.0, 0.0), (499.0, 5000.0)), "a  b\n\n  c");
    assert_eq!(test.click(0.0, 4.0, Unit::Block), "a  b");
}

#[test]
fn characters_are_never_cut() {
    let mut test = Test::new("\u{17c}\u{f3}\u{142}w e\u{301}a \u{1f600}x", 500.0);
    assert_eq!(test.drag((0.0, 4.0), (21.0, 4.0)), "\u{17c}\u{f3}\u{142}");
    // A letter with a combining mark, 16 wide here, is one character to select.
    assert_eq!(test.drag((40.0, 4.0), (47.0, 4.0)), "");
    assert_eq!(test.drag((40.0, 4.0), (49.0, 4.0)), "e\u{301}");
}

#[test]
fn all_takes_the_whole_text() {
    let test = Test::new("<p>a</p><p>b</p>", 500.0);
    assert_eq!(Selection::all(&test.layout).text(&test.layout), "a\n\nb");
}

#[test]
fn highlights_cover_exactly_the_selected_text() {
    let mut test = Test::new("<p>aaaa bbbb</p><p>cccc</p>", 40.0);
    assert_eq!(test.layout.lines.len(), 3);
    let highlights = |test: &mut Test, from: (f32, f32), to: (f32, f32)| {
        let mut selection = test.press(from.0, from.1, Unit::Character);
        selection.drag_to(&test.layout, to.0, to.1, &mut test.measurer);
        selection
            .highlights(&test.layout, 0.0, 1000.0, &mut test.measurer)
            .iter()
            .map(|h| (h.x, h.y, h.width, h.height))
            .collect::<Vec<_>>()
    };
    // Part of a line.
    assert_eq!(
        highlights(&mut test, (8.0, 20.0), (24.0, 20.0)),
        [(8.0, 16.0, 16.0, 16.0)]
    );
    // Past the end of the line: only as far as the text, not to the edge.
    assert_eq!(
        highlights(&mut test, (8.0, 20.0), (499.0, 20.0)),
        [(8.0, 16.0, 24.0, 16.0)]
    );
    // Across lines of a block.
    assert_eq!(
        highlights(&mut test, (8.0, 20.0), (16.0, 36.0)),
        [(8.0, 16.0, 24.0, 16.0), (0.0, 32.0, 16.0, 16.0)]
    );
    // Into the space below a block: nothing of the next one.
    assert_eq!(
        highlights(&mut test, (24.0, 36.0), (0.0, 55.0)),
        [(24.0, 32.0, 8.0, 16.0)]
    );
    // Nothing selected, nothing highlighted.
    assert!(highlights(&mut test, (8.0, 20.0), (8.0, 20.0)).is_empty());
}

#[test]
fn highlights_stay_within_what_is_asked_for() {
    let mut test = Test::new(&"<div>aaaa</div>".repeat(100), 500.0);
    let mut selection = test.press(0.0, 0.0, Unit::Character);
    selection.drag_to(&test.layout, 499.0, 5000.0, &mut test.measurer);
    let shown = selection.highlights(&test.layout, 160.0, 320.0, &mut test.measurer);
    assert_eq!(shown.len(), 10);
    assert_eq!(shown[0].y, 160.0);
}

#[test]
fn selecting_on_a_page_without_text_does_no_harm() {
    let mut test = Test::new("<div></div>", 500.0);
    assert!(Selection::start(&test.layout, 0.0, 0.0, Unit::Block, &mut test.measurer).is_none());
    assert_eq!(Selection::all(&test.layout).text(&test.layout), "");
}
