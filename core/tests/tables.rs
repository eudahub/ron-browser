//! Tables as grids. Characters are 8 wide and lines 16 high, as in the layout tests.

use ron_browser_core::images::{ImageStore, NoResources};
use ron_browser_core::layout::{FontSpec, Item, ItemKind, Layout, TextMeasurer};
use ron_browser_core::page::Page;
use ron_browser_core::selection::Selection;

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

/// Without any room around cells, which makes positions easy to tell.
const PLAIN: &str = "<!DOCTYPE html><style>body{margin:0}p{margin:0}\
    table{border-spacing:0}td,th{padding:0;vertical-align:top}</style>";
/// The same with `valign` left to count, as style sheets go before what attributes ask for.
const ALIGNED_BY_ATTRIBUTES: &str = "<!DOCTYPE html><style>body{margin:0}\
    table{border-spacing:0}td,th{padding:0}</style>";

fn show_as(source: &str, width: f32) -> Layout {
    let mut page = Page::open_with(source, &mut NoResources, ImageStore::default());
    page.lay_out(width, 600.0, &mut Measurer::default());
    page.layout().clone()
}

fn show(body: &str, width: f32) -> Layout {
    show_as(&format!("{PLAIN}{body}"), width)
}

fn text(layout: &Layout, wanted: &str) -> Item {
    *layout
        .items
        .iter()
        .find(|item| {
            item.kind == ItemKind::Text
                && &layout.text[item.start as usize..item.end as usize] == wanted
        })
        .unwrap_or_else(|| panic!("no text {wanted} in {:?}", layout.text))
}

fn rectangles(layout: &Layout) -> Vec<Item> {
    layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Rect && item.width > 0.0 && item.height > 0.0)
        .copied()
        .collect()
}

#[test]
fn cells_stand_side_by_side_and_rows_below_each_other() {
    let layout = show("<table><tr><td>aa<td>bbb<tr><td>cccc<td>d</table>", 800.0);
    assert_eq!((text(&layout, "aa").x, text(&layout, "aa").y), (0.0, 0.0));
    assert_eq!(
        (text(&layout, "bbb").x, text(&layout, "bbb").y),
        (32.0, 0.0)
    );
    assert_eq!(
        (text(&layout, "cccc").x, text(&layout, "cccc").y),
        (0.0, 16.0)
    );
    assert_eq!((text(&layout, "d").x, text(&layout, "d").y), (32.0, 16.0));
    assert_eq!(layout.height, 32.0);
}

#[test]
fn a_table_is_only_as_wide_as_it_needs_and_what_follows_goes_below() {
    let layout = show(
        "<table style='background:red'><tr><td>aa<td>bb</table><p>after</p>",
        800.0,
    );
    let background = rectangles(&layout)[0];
    assert_eq!(
        (background.x, background.width, background.height),
        (0.0, 32.0, 16.0)
    );
    assert_eq!(text(&layout, "after").y, 16.0);
}

#[test]
fn spacing_and_padding_count() {
    let layout = show_as(
        "<!DOCTYPE html><style>body{margin:0}</style><table cellspacing=2 cellpadding=3><tr><td>a<td>b</table>",
        800.0,
    );
    assert_eq!(text(&layout, "a").x, 5.0);
    assert_eq!(text(&layout, "a").y, 5.0);
    // 2 + 3 + 8 + 3 + 2 + 3
    assert_eq!(text(&layout, "b").x, 21.0);
    assert_eq!(layout.height, 26.0);
}

#[test]
fn a_cell_can_span_columns() {
    let layout = show(
        "<table><tr><td colspan=2>wide wide<td>c<tr><td>a<td>b<td>d</table>",
        800.0,
    );
    let (a, b, c) = (text(&layout, "a"), text(&layout, "b"), text(&layout, "c"));
    assert_eq!(a.x, 0.0);
    assert!(b.x > a.x);
    assert_eq!(c.x, text(&layout, "d").x);
    assert_eq!(c.x, 72.0);
}

#[test]
fn a_cell_can_span_rows_and_keeps_its_columns_from_others() {
    let layout = show(
        "<table><tr><td rowspan=2>tall<td>a<tr><td>b<tr><td>c<td>d</table>",
        800.0,
    );
    assert_eq!(text(&layout, "b").x, 32.0);
    assert_eq!(text(&layout, "b").y, 16.0);
    assert_eq!(text(&layout, "c").x, 0.0);
    assert_eq!(text(&layout, "c").y, 32.0);
    assert_eq!(text(&layout, "d").x, 32.0);
}

#[test]
fn a_tall_cell_over_two_rows_makes_room_in_the_last() {
    let layout = show(
        "<table><tr><td rowspan=2 style='height:100px'>tall<td>a<tr><td>b<tr><td>c</table>",
        800.0,
    );
    assert_eq!(text(&layout, "b").y, 16.0);
    assert_eq!(text(&layout, "c").y, 100.0);
}

#[test]
fn widths_are_kept() {
    let layout = show(
        "<table style='width:400px'><tr><td style='width:100px'>a<td>b</table>",
        800.0,
    );
    assert_eq!(text(&layout, "b").x, 100.0);
    let percent = show("<table width=400><tr><td width=25%>a<td>b</table>", 800.0);
    assert_eq!(text(&percent, "b").x, 100.0);
    let of_page = show("<table width=50%><tr><td width=50%>a<td>b</table>", 800.0);
    assert_eq!(text(&of_page, "b").x, 200.0);
    let columns = show("<table><col width=70><col><tr><td>a<td>b</table>", 800.0);
    assert_eq!(text(&columns, "b").x, 70.0);
}

#[test]
fn room_left_goes_to_those_that_ask_for_none() {
    let layout = show(
        "<table width=300><tr><td width=50>a<td>bb<td>bb</table>",
        800.0,
    );
    let (second, third) = (
        text(&layout, "bb"),
        layout
            .items
            .iter()
            .rev()
            .find(|item| item.kind == ItemKind::Text)
            .unwrap()
            .x,
    );
    assert_eq!(second.x, 50.0);
    assert_eq!(third, 175.0);
}

#[test]
fn text_wraps_when_there_is_too_little_room_but_words_stay_whole() {
    let layout = show(
        "<table><tr><td>aaaa bbbb cccc<td>dddddddd eeee</table>",
        100.0,
    );
    // Neither column can be narrower than its longest word, 32 and 64 wide.
    assert_eq!(text(&layout, "aaaa").x, 0.0);
    let d = text(&layout, "dddddddd");
    assert!(d.x >= 32.0, "{}", d.x);
    assert!(d.x + d.width <= 101.0, "{} {}", d.x, d.width);
    assert!(layout.height >= 32.0);
    // With less room than the words take, the page gets wider, and they do not break.
    let narrow = show("<table><tr><td>aaaaaaaa<td>bbbbbbbb</table>", 50.0);
    assert_eq!(text(&narrow, "bbbbbbbb").x, 64.0);
    assert!(narrow.width >= 128.0);
}

#[test]
fn what_does_not_wrap_only_counts_for_itself() {
    let layout = show(
        "<table><tr><td>aa bb cc dd ee ff <span style='white-space:nowrap'>gggg gggg</span><td>h</table>",
        100.0,
    );
    // The first column can be as narrow as 72 for the part that stays together, of the 100 there is.
    assert!(text(&layout, "h").x <= 92.0, "{}", text(&layout, "h").x);
    let new_line = show(
        "<table style='width:10px'><tr><td>aa bb cc dd<br><span style='white-space:nowrap'>ee</span><td>h</table>",
        800.0,
    );
    assert_eq!(text(&new_line, "h").x, 16.0);
}

#[test]
fn nowrap_keeps_a_cell_on_one_line() {
    let layout = show("<table><tr><td nowrap>a b c d<td>e</table>", 40.0);
    assert_eq!(text(&layout, "e").x, 56.0);
}

#[test]
fn shorter_cells_are_placed_as_they_ask() {
    let tall = "<td>a<br>b<br>c";
    let middle = show(
        &format!("<table><tr>{tall}<td style='vertical-align:middle'>m</table>"),
        800.0,
    );
    assert_eq!(text(&middle, "m").y, 16.0);
    let bottom = show_as(
        &format!("{ALIGNED_BY_ATTRIBUTES}<table><tr>{tall}<td valign=bottom>m</table>"),
        800.0,
    );
    assert_eq!(text(&bottom, "m").y, 32.0);
    let top = show_as(
        &format!("{ALIGNED_BY_ATTRIBUTES}<table><tr>{tall}<td valign=top>m</table>"),
        800.0,
    );
    assert_eq!(text(&top, "m").y, 0.0);
    // Without anything said, as the standard's own style has it.
    let plain = show_as(
        "<!DOCTYPE html><table style='border-spacing:0'><tr><td style='padding:0'>a<br>b<br>c<td style='padding:0'>m</table>",
        800.0,
    );
    assert!(text(&plain, "m").y > text(&plain, "a").y + 8.0);
}

#[test]
fn backgrounds_and_borders_of_a_cell_fill_its_row() {
    let layout = show(
        "<table><tr><td>a<br>b<td style='background:red;border:1px solid blue'>c</table>",
        800.0,
    );
    let shapes = rectangles(&layout);
    let background = shapes[0];
    assert_eq!(background.height, 32.0);
    assert_eq!(background.x, 8.0);
    // Top, bottom, left and right.
    assert_eq!(shapes.len(), 5);
    assert_eq!(shapes[2].y, 31.0);
    assert_eq!(shapes[3].height, 32.0);
}

#[test]
fn a_row_shows_its_background_behind_its_cells() {
    let layout = show(
        "<table><tr bgcolor=red><td>a<td>b</table><table><thead style='background:blue'><tr><td>c</table>",
        800.0,
    );
    let shapes = rectangles(&layout);
    assert_eq!(shapes.len(), 3);
    assert_eq!(shapes[0].width, 8.0);
    assert_eq!(shapes[1].x, 8.0);
    assert_eq!(shapes[2].y, 16.0);
}

#[test]
fn collapsed_borders_overlap() {
    let separate = show(
        "<style>td{border:1px solid black}</style><table><tr><td>a<td>b</table>",
        800.0,
    );
    // A cell is 10 wide with its borders.
    assert_eq!(text(&separate, "b").x, 11.0);
    let collapsed = show(
        "<style>td{border:1px solid black}</style><table style='border-collapse:collapse'><tr><td>a<td>b<tr><td>c</table>",
        800.0,
    );
    assert_eq!(text(&collapsed, "b").x, 10.0);
    // A cell is 18 high with its borders, and the next one starts on the last line of it.
    assert_eq!(text(&collapsed, "c").y, 18.0);
}

#[test]
fn tables_nest_and_the_inner_one_gives_its_width() {
    let layout = show(
        "<table><tr><td><table><tr><td>aaaa<td>bbbb</table><td>c</table>",
        800.0,
    );
    assert_eq!(text(&layout, "bbbb").x, 32.0);
    assert_eq!(text(&layout, "c").x, 64.0);
}

#[test]
fn a_box_of_a_set_width_in_a_cell_counts() {
    let layout = show(
        "<table><tr><td><div style='width:200px'>a</div><td>b</table>",
        800.0,
    );
    assert_eq!(text(&layout, "b").x, 200.0);
}

#[test]
fn margins_inside_a_cell_stay_inside() {
    let layout = show(
        "<table><tr><td><p style='margin:10px'>a</p><td>b<tr><td>c</table>",
        800.0,
    );
    assert_eq!(text(&layout, "a").x, 10.0);
    assert_eq!(text(&layout, "a").y, 10.0);
    assert_eq!(text(&layout, "c").y, 36.0);
}

#[test]
fn a_table_can_be_centered_and_can_float() {
    let centered = show("<table align=center><tr><td>aa</table>", 100.0);
    assert_eq!(text(&centered, "aa").x, 42.0);
    let floated = show("<table align=left><tr><td>aa</table><p>beside</p>", 100.0);
    assert_eq!(text(&floated, "beside").x, 16.0);
    assert_eq!(text(&floated, "beside").y, 0.0);
    let inline = show(
        "before <table style='display:inline-table'><tr><td>aa<td>bb</table> after",
        800.0,
    );
    assert!(text(&inline, "bb").x > text(&inline, "aa").x);
    assert_eq!(text(&inline, "aa").y, text(&inline, "bb").y);
}

#[test]
fn a_float_is_not_narrower_than_its_columns() {
    let layout = show(
        "<table style='float:right;width:50px;background:red'><tr><td>aaaaaaaaaaaaaaaa</table>",
        800.0,
    );
    assert_eq!(rectangles(&layout)[0].width, 128.0);
}

#[test]
fn a_caption_goes_above() {
    let layout = show(
        "<table><caption>long caption</caption><tr><td>a<td>b</table>",
        800.0,
    );
    // The table is as wide as its longest word, on which the caption wraps.
    assert_eq!(text(&layout, "caption").width, 56.0);
    assert_eq!(text(&layout, "a").y, 32.0);
    assert!(text(&layout, "b").x >= 8.0);
}

#[test]
fn css_can_make_a_table_of_anything() {
    let layout = show(
        "<div style='display:table'><div style='display:table-row'><div style='display:table-cell'>a</div>\
         <div style='display:table-cell'>b</div></div></div>",
        800.0,
    );
    assert_eq!(text(&layout, "b").x, 8.0);
    assert_eq!(text(&layout, "b").y, 0.0);
    // Cells without a row around them make one.
    let no_row = show(
        "<div style='display:table'><span style='display:table-cell'>a</span><span style='display:table-cell'>b</span></div>",
        800.0,
    );
    assert_eq!(text(&no_row, "b").x, 8.0);
}

#[test]
fn what_is_not_made_up_as_a_table_stacks() {
    let layout = show(
        "<div style='display:table'><div>a</div><div>b</div></div>",
        800.0,
    );
    assert_eq!(text(&layout, "b").y, 16.0);
    let lone = show(
        "<div style='display:table-cell'>a</div><div style='display:table-cell'>b</div>",
        800.0,
    );
    assert_eq!(text(&lone, "b").y, 16.0);
}

#[test]
fn a_form_or_white_space_in_the_structure_makes_no_difference() {
    let layout = show(
        "<table>  <form></form> <tr> <td>a</td> <td>b</td> </tr>  </table>",
        800.0,
    );
    assert_eq!(text(&layout, "b").x, 8.0);
    assert_eq!(layout.height, 16.0);
}

#[test]
fn copied_text_has_tabs_between_cells_and_a_line_for_each_row() {
    let layout = show("<table><tr><td>a<td>b<tr><td>c<td>d</table><p>e</p>", 800.0);
    assert_eq!(layout.text, "a\tb\nc\td\ne");
    assert_eq!(Selection::all(&layout).text(&layout), "a\tb\nc\td\ne");
}

#[test]
fn a_link_in_a_cell_moves_with_it() {
    let layout = show_as(
        &format!(
            "{ALIGNED_BY_ATTRIBUTES}<table><tr><td>a<br>b<br>c<td valign=bottom><a href=there>x</a></table>"
        ),
        800.0,
    );
    assert_eq!(layout.link_at(9.0, 36.0), Some("there"));
    assert_eq!(layout.link_at(9.0, 4.0), None);
}

#[test]
fn hostile_tables_do_no_harm() {
    let huge = show(
        "<table><tr><td colspan=999999999 rowspan=999999999>a<td>b</table>",
        800.0,
    );
    assert_eq!(text(&huge, "a").y, 0.0);
    let cells = "<td colspan=1000>x".repeat(50);
    let wide = show(&format!("<table><tr>{cells}</table>"), 800.0);
    assert!(wide.height >= 16.0);
    let deep = format!(
        "{}deep{}",
        "<table><tr><td>".repeat(3000),
        "</table>".repeat(3000)
    );
    assert_eq!(text(&show(&deep, 800.0), "deep").height, 16.0);
    show(
        "<table></table><table><tr></table><table><tr><td></table>",
        800.0,
    );
}
