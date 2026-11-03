//! Layout with a text measurer whose sizes are easy to work out: each character is half as
//! wide as the font size, ascent is 0.8 of it and descent 0.2.

use ron_browser_core::layout::{FontSpec, Item, ItemKind, Layout, Line, TextMeasurer};
use ron_browser_core::page::Page;

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

/// Lays out a page without a body margin, so that positions are round. Characters are 8 wide
/// and lines 16 high at the default font size.
fn lay_out(body: &str, width: f32) -> Layout {
    let source = format!("<!DOCTYPE html><style>body{{margin:0}}</style>{body}");
    let mut page = Page::open(&source);
    page.lay_out(width, 600.0, &mut Measurer::default());
    page.layout().clone()
}

fn texts(layout: &Layout) -> Vec<&Item> {
    layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Text)
        .collect()
}

fn rects(layout: &Layout) -> Vec<&Item> {
    layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Rect)
        .collect()
}

fn line_text<'a>(layout: &'a Layout, line: &Line) -> &'a str {
    &layout.text[line.start as usize..line.end as usize]
}

fn lines(layout: &Layout) -> Vec<&str> {
    layout
        .lines
        .iter()
        .map(|line| line_text(layout, line))
        .collect()
}

fn item_text<'a>(layout: &'a Layout, item: &Item) -> &'a str {
    &layout.text[item.start as usize..item.end as usize]
}

#[test]
fn text_wraps_at_spaces_to_the_width() {
    // Each character is 8 wide, so 40 fits five.
    let layout = lay_out("aaa bb cc dddd", 48.0);
    assert_eq!(lines(&layout), ["aaa bb", "cc", "dddd"]);
    assert_eq!(layout.lines[0].top, 0.0);
    assert_eq!(layout.lines[1].top, 16.0);
    assert_eq!(layout.lines[2].top, 32.0);
    assert_eq!(layout.height, 48.0);
    assert_eq!(
        layout.text, "aaa bb cc dddd",
        "wrapping leaves the text as it is"
    );
}

#[test]
fn a_word_too_long_for_the_line_overflows_instead_of_breaking() {
    let layout = lay_out("aa bbbbbbbbbb cc", 40.0);
    assert_eq!(lines(&layout), ["aa", "bbbbbbbbbb", "cc"]);
    assert_eq!(layout.width, 80.0);
}

#[test]
fn a_word_across_elements_stays_together() {
    let layout = lay_out("aaa b<b>bb</b>b", 48.0);
    assert_eq!(lines(&layout), ["aaa", "bbbb"]);
    let last = layout.lines[1];
    assert_eq!(last.item_count, 3);
}

#[test]
fn white_space_collapses_across_elements() {
    let layout = lay_out("  a  <b> b </b>  c  ", 500.0);
    assert_eq!(layout.text, "a b c");
}

#[test]
fn a_space_takes_the_look_of_where_it_was_written() {
    let layout = lay_out("<a href=x>aa</a> <a href=x>bb </a>cc", 500.0);
    let parts: Vec<(&str, u8)> = texts(&layout)
        .iter()
        .map(|item| (item_text(&layout, item), item.decoration))
        .collect();
    assert_eq!(parts, [("aa", 1), (" ", 0), ("bb ", 1), ("cc", 0)]);
}

#[test]
fn line_breaks_and_preformatted_text_keep_their_lines() {
    assert_eq!(lines(&lay_out("a<br>b", 500.0)), ["a", "b"]);
    assert_eq!(lay_out("a<br>b", 500.0).text, "a\nb");
    assert_eq!(
        lines(&lay_out("<pre>a  b\n\n c</pre>", 500.0)),
        ["a  b", "", " c"]
    );
    let blank = lay_out("a<br><br>b", 500.0);
    assert_eq!(lines(&blank), ["a", "", "b"]);
    assert_eq!(blank.lines[2].top, 32.0);
    assert_eq!(lines(&lay_out("<pre>aaaa bbbb</pre>", 40.0)), ["aaaa bbbb"]);
    let nowrap = lay_out("<p style='white-space:nowrap'>aaaa bbbb cccc</p>", 40.0);
    assert_eq!(nowrap.lines.len(), 1);
}

#[test]
fn blocks_stack_and_margins_collapse() {
    let layout = lay_out(
        "<div style='margin:10px 0'>a</div><div style='margin:30px 0'>b</div>",
        500.0,
    );
    assert_eq!(layout.lines[0].top, 10.0);
    assert_eq!(layout.lines[1].top, 10.0 + 16.0 + 30.0);
    assert_eq!(layout.height, 10.0 + 16.0 + 30.0 + 16.0 + 30.0);
}

#[test]
fn margins_collapse_through_parents_without_padding() {
    let through = lay_out(
        "<div style='margin-top:10px'><div style='margin-top:30px'>a</div></div>",
        500.0,
    );
    assert_eq!(through.lines[0].top, 30.0);
    let stopped = lay_out(
        "<div style='margin-top:10px;padding-top:1px'><div style='margin-top:30px'>a</div></div>",
        500.0,
    );
    assert_eq!(stopped.lines[0].top, 10.0 + 1.0 + 30.0);
    let negative = lay_out(
        "<div style='margin-bottom:20px'>a</div><div style='margin-top:-5px'>b</div>",
        500.0,
    );
    assert_eq!(negative.lines[1].top, 16.0 + 15.0);
    let empty = lay_out(
        "<div style='margin:10px 0'></div><div style='margin:5px 0'>a</div>",
        500.0,
    );
    assert_eq!(empty.lines[0].top, 10.0);
}

#[test]
fn padding_and_borders_move_the_content_and_paint_the_box() {
    let layout = lay_out(
        "<div style='padding:5px;border:2px solid red;background:blue;margin:3px'>a</div>",
        100.0,
    );
    let text = texts(&layout)[0];
    assert_eq!(text.x, 3.0 + 2.0 + 5.0);
    assert_eq!(text.y, 3.0 + 2.0 + 5.0);
    let boxes = rects(&layout);
    assert_eq!((boxes[0].x, boxes[0].y), (3.0, 3.0));
    assert_eq!((boxes[0].width, boxes[0].height), (94.0, 30.0));
    assert_eq!(boxes.len(), 5, "the background and four borders");
    assert_eq!(boxes[0].color, ron_browser_core::Color::rgb(0, 0, 255));
    assert_eq!(layout.height, 36.0);
}

#[test]
fn sunken_and_raised_borders_are_darker_on_one_side() {
    let colors = |line: &str| {
        let page = format!("<div style='border:1px {line} rgb(120,120,120)'>a</div>");
        rects(&lay_out(&page, 100.0))
            .iter()
            .map(|rect| rect.color.red)
            .collect::<Vec<_>>()
    };
    // Top, bottom, left and right.
    assert_eq!(colors("inset"), [80, 120, 80, 120]);
    assert_eq!(colors("outset"), [120, 80, 120, 80]);
    assert_eq!(colors("solid"), [120, 120, 120, 120]);
}

#[test]
fn widths_and_centering_resolve_against_the_parent() {
    let percent = lay_out("<div style='width:50%;background:red'>a</div>", 200.0);
    assert_eq!(rects(&percent)[0].width, 100.0);
    let centered = lay_out(
        "<div style='width:100px;margin:0 auto;background:red'>a</div>",
        300.0,
    );
    assert_eq!(rects(&centered)[0].x, 100.0);
    let right = lay_out(
        "<div style='width:100px;margin-left:auto;background:red'>a</div>",
        300.0,
    );
    assert_eq!(rects(&right)[0].x, 200.0);
    let border_box = lay_out(
        "<div style='width:100px;padding:10px;box-sizing:border-box;background:red'>a</div>",
        300.0,
    );
    assert_eq!(rects(&border_box)[0].width, 100.0);
    let limited = lay_out("<div style='max-width:50px;background:red'>a</div>", 300.0);
    assert_eq!(rects(&limited)[0].width, 50.0);
    let high = lay_out("<div style='height:100px;background:red'>a</div>b", 300.0);
    assert_eq!(rects(&high)[0].height, 100.0);
    assert_eq!(high.lines[1].top, 100.0);
}

#[test]
fn text_aligns_within_the_block() {
    let x = |align: &str| {
        let page = format!("<p style='margin:0;text-align:{align}'>aa</p>");
        texts(&lay_out(&page, 100.0))[0].x
    };
    assert_eq!(x("left"), 0.0);
    assert_eq!(x("center"), 42.0);
    assert_eq!(x("right"), 84.0);
    let indented = lay_out(
        "<p style='margin:0;text-indent:16px'>aaaa bbbb cc</p>",
        80.0,
    );
    assert_eq!(lines(&indented), ["aaaa", "bbbb cc"]);
    assert_eq!(indented.items[0].x, 16.0);
    assert_eq!(texts(&indented)[1].x, 0.0);
}

#[test]
fn line_height_spreads_around_the_text() {
    let layout = lay_out("<p style='margin:0;line-height:2'>a<br>b</p>", 100.0);
    assert_eq!(layout.lines[1].top, 32.0);
    // Half of the extra 16 goes above: 8, then the ascent of 12.8.
    assert!((texts(&layout)[0].baseline - 20.8).abs() < 0.01);
    let mixed = lay_out("a<span style='font-size:32px'>b</span>", 100.0);
    assert_eq!(mixed.lines[0].bottom, 32.0);
    let items = texts(&mixed);
    assert_eq!(
        items[0].baseline, items[1].baseline,
        "text shares a baseline"
    );
}

#[test]
fn raised_and_lowered_text_shifts_from_the_baseline() {
    let layout = lay_out("a<sup>b</sup><sub>c</sub>", 100.0);
    let items = texts(&layout);
    assert!(items[1].baseline < items[0].baseline);
    assert!(items[2].baseline > items[0].baseline);
}

#[test]
fn decoration_reaches_the_text_inside() {
    let layout = lay_out("<u>a<b>b</b></u><s>c</s>", 100.0);
    let bits: Vec<u8> = texts(&layout).iter().map(|item| item.decoration).collect();
    assert_eq!(bits, [1, 1, 4]);
}

#[test]
fn hidden_things_take_no_room_and_replaced_ones_show_nothing_yet() {
    let hidden = lay_out("<div style='display:none'>a</div>b", 100.0);
    assert_eq!(hidden.text, "b");
    assert_eq!(hidden.lines[0].top, 0.0);
    let image = lay_out("<img src=x border=2 align=right>a", 100.0);
    assert_eq!(rects(&image).len(), 0);
    assert_eq!(image.text, "a");
    assert_eq!(lay_out("<iframe>a</iframe>b", 100.0).text, "b");
}

#[test]
fn list_markers_are_drawn_but_stay_out_of_the_text() {
    let layout = lay_out(
        "<ul><li>a</ul><ol start=3><li>b<li>c<li value=9>d<li>e</ol><ul style='list-style:none'><li>f</ul>",
        200.0,
    );
    assert_eq!(layout.text, "a\n\nb\nc\nd\ne\n\nf");
    let markers: Vec<&str> = layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Marker)
        .map(|item| &layout.marker_text[item.start as usize..item.end as usize])
        .collect();
    assert_eq!(markers, ["\u{2022}", "3.", "4.", "9.", "10."]);
    let marker = layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Marker)
        .unwrap();
    assert!(marker.x + marker.width <= 40.0, "left of the text");
}

#[test]
fn paragraphs_are_separated_more_than_plain_blocks_in_the_text() {
    assert_eq!(lay_out("<div>a</div><div>b</div>", 100.0).text, "a\nb");
    assert_eq!(lay_out("<p>a</p><p>b</p>", 100.0).text, "a\n\nb");
    assert_eq!(lay_out("<div>a</div>b<div>c</div>", 100.0).text, "a\nb\nc");
}

#[test]
fn line_breaks_count_towards_what_separates_blocks() {
    // As Chrome copies it: the end of the line, one empty line, and another after the rule.
    assert_eq!(
        lay_out("x<br><br><hr width=60%><br>y", 100.0).text,
        "x\n\n\ny"
    );
    assert_eq!(lay_out("a<br><div>b</div>", 100.0).text, "a\nb");
    assert_eq!(lay_out("a<br><p>b</p>", 100.0).text, "a\n\nb");
    assert_eq!(lay_out("a<br><br><br><p>b</p>", 100.0).text, "a\n\n\nb");
    assert_eq!(lay_out("<p>a</p><br>b", 100.0).text, "a\n\n\nb");
}

#[test]
fn the_canvas_takes_the_background_of_the_root_or_body() {
    let color = |page: &str| lay_out(page, 100.0).canvas;
    let red = Some(ron_browser_core::Color::rgb(255, 0, 0));
    assert_eq!(color("<body style='background:red'>"), red);
    assert_eq!(
        color("<html style='background:red'><body style='background:blue'>"),
        red
    );
    assert_eq!(color("a"), None);
}

#[test]
fn deep_wide_and_empty_pages_do_no_harm() {
    assert_eq!(lay_out("", 100.0).lines.len(), 0);
    let deep = lay_out(
        &format!("{}a", "<div style='margin:1px'>".repeat(5000)),
        100.0,
    );
    assert_eq!(deep.text, "a");
    let narrow = lay_out("aaa bbb", 0.0);
    assert_eq!(lines(&narrow), ["aaa", "bbb"]);
    let long = lay_out(&"word ".repeat(20_000), 400.0);
    assert!(long.lines.len() > 900);
}
