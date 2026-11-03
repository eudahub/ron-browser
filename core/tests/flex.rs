//! Flexible boxes. Characters are 8 wide and lines 16 high, as in the layout tests.

use ron_browser_core::images::{DecodedImage, ImageStore, NoResources};
use ron_browser_core::layout::{FontSpec, Item, ItemKind, Layout, TextMeasurer};
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

fn show(body: &str, width: f32) -> Layout {
    let source = format!("<!DOCTYPE html><style>body{{margin:0}}p{{margin:0}}</style>{body}");
    let mut store = ImageStore::default();
    store.insert(
        "a.png",
        DecodedImage {
            width: 40,
            height: 20,
            rgba: vec![128; 40 * 20 * 4],
        },
    );
    let mut page = Page::open_with(&source, &mut NoResources, store);
    page.lay_out(width, 600.0, &mut Measurer::default());
    page.layout().clone()
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
fn items_stand_side_by_side_and_what_follows_goes_below() {
    let layout = show(
        "<div style='display:flex'><div>aa</div><span>bbb</span></div><p>after</p>",
        800.0,
    );
    assert_eq!((text(&layout, "aa").x, text(&layout, "aa").y), (0.0, 0.0));
    assert_eq!(
        (text(&layout, "bbb").x, text(&layout, "bbb").y),
        (16.0, 0.0)
    );
    assert_eq!(text(&layout, "after").y, 16.0);
}

#[test]
fn a_flexible_box_is_as_wide_as_a_block() {
    let layout = show(
        "<div style='display:flex;background:red'><div>a</div></div>",
        800.0,
    );
    assert_eq!(rectangles(&layout)[0].width, 800.0);
}

#[test]
fn what_is_free_goes_to_those_that_grow() {
    let layout = show(
        "<div style='display:flex'><div style='flex-grow:1'>a</div><div style='flex-grow:3'>b</div><div>cc</div></div>",
        416.0,
    );
    // Taken are 8, 8 and 16, which leaves 384 to share.
    assert_eq!(text(&layout, "b").x, 8.0 + 96.0);
    assert_eq!(text(&layout, "cc").x, 400.0);
}

#[test]
fn flex_one_shares_all_of_it_equally() {
    let layout = show(
        "<div style='display:flex'><div style='flex:1'>a</div><div style='flex:1'>bbbbbbbb</div></div>",
        400.0,
    );
    assert_eq!(text(&layout, "bbbbbbbb").x, 200.0);
}

#[test]
fn a_basis_counts_and_sizes_stay_within_limits() {
    let basis = show(
        "<div style='display:flex'><div style='flex:0 0 100px'>a</div><div>b</div></div>",
        400.0,
    );
    assert_eq!(text(&basis, "b").x, 100.0);
    let width = show(
        "<div style='display:flex'><div style='width:150px'>a</div><div>b</div></div>",
        400.0,
    );
    assert_eq!(text(&width, "b").x, 150.0);
    let limited = show(
        "<div style='display:flex'><div style='flex:1;max-width:50px'>a</div><div style='flex:1'>b</div></div>",
        400.0,
    );
    assert_eq!(text(&limited, "b").x, 50.0);
}

#[test]
fn too_much_shrinks_but_not_below_a_word() {
    let layout = show(
        "<div style='display:flex'><div>aaaa aaaa aaaa</div><div>bbbbbbbb</div></div>",
        100.0,
    );
    // The second cannot be narrower than its word.
    let b = text(&layout, "bbbbbbbb");
    assert!(b.x + b.width <= 101.0, "{} {}", b.x, b.width);
    assert!(layout.height >= 32.0);
    let nothing_shrinks = show(
        "<div style='display:flex'><div style='flex:none'>aaaaaaaaaaaaaaaa</div><div>b</div></div>",
        100.0,
    );
    assert_eq!(text(&nothing_shrinks, "b").x, 128.0);
    let may_get_narrower = show(
        "<div style='display:flex'><div style='min-width:0;flex:1 1 200px;overflow:hidden'>aaaaaaaaaaaaaaaaaaaaaaaaaa</div><div style='flex:0 0 50px'>b</div></div>",
        100.0,
    );
    assert_eq!(text(&may_get_narrower, "b").x, 50.0);
}

#[test]
fn justify_content_spreads_what_is_free() {
    let at = |justify: &str| {
        let layout = show(
            &format!(
                "<div style='display:flex;justify-content:{justify}'><div>a</div><div>b</div></div>"
            ),
            116.0,
        );
        (text(&layout, "a").x, text(&layout, "b").x)
    };
    assert_eq!(at("flex-start"), (0.0, 8.0));
    assert_eq!(at("flex-end"), (100.0, 108.0));
    assert_eq!(at("center"), (50.0, 58.0));
    assert_eq!(at("space-between"), (0.0, 108.0));
    assert_eq!(at("space-around"), (25.0, 83.0));
    assert!((at("space-evenly").0 - 100.0 / 3.0).abs() < 0.01);
}

#[test]
fn an_auto_margin_pushes_to_the_side() {
    let layout = show(
        "<div style='display:flex'><div>a</div><div style='margin-left:auto'>b</div></div>",
        100.0,
    );
    assert_eq!(text(&layout, "b").x, 92.0);
}

#[test]
fn items_stretch_down_their_line_unless_told_otherwise() {
    let tall = "<div>a<br>b<br>c</div>";
    let stretched = show(
        &format!("<div style='display:flex'>{tall}<div style='background:red'>m</div></div>"),
        800.0,
    );
    assert_eq!(rectangles(&stretched)[0].height, 48.0);
    let aligned = |align: &str| {
        let layout = show(
            &format!(
                "<div style='display:flex;align-items:{align}'>{tall}<div style='background:red'>m</div></div>"
            ),
            800.0,
        );
        (
            text(&layout, "m").y,
            rectangles(&layout)[0].y,
            rectangles(&layout)[0].height,
        )
    };
    assert_eq!(aligned("flex-start"), (0.0, 0.0, 16.0));
    assert_eq!(aligned("center"), (16.0, 16.0, 16.0));
    assert_eq!(aligned("flex-end"), (32.0, 32.0, 16.0));
    let own = show(
        &format!(
            "<div style='display:flex;align-items:flex-end'>{tall}<div style='align-self:flex-start'>m</div></div>"
        ),
        800.0,
    );
    assert_eq!(text(&own, "m").y, 0.0);
}

#[test]
fn a_row_of_a_least_height_passes_it_on_to_what_stretches() {
    let layout = show(
        "<div style='display:flex;min-height:200px'><div style='background:red;flex:0 0 50px'>s</div>\
         <div style='flex:1;display:flex;flex-direction:column'><div>a</div><div style='margin-top:auto'>end</div></div></div>\
         <p>after</p>",
        200.0,
    );
    assert_eq!(rectangles(&layout)[0].height, 200.0);
    assert_eq!(text(&layout, "end").y, 184.0);
    assert_eq!(text(&layout, "after").y, 200.0);
}

#[test]
fn a_box_of_a_set_height_centers_within_it() {
    let layout = show(
        "<div style='display:flex;height:100px;align-items:center;justify-content:center'><div>m</div></div><p>after</p>",
        108.0,
    );
    assert_eq!(text(&layout, "m").x, 50.0);
    assert_eq!(text(&layout, "m").y, 42.0);
    assert_eq!(text(&layout, "after").y, 100.0);
}

#[test]
fn a_row_can_wrap() {
    let layout = show(
        "<div style='display:flex;flex-wrap:wrap'><div style='width:60px'>a</div><div style='width:60px'>b</div><div style='width:60px'>c</div></div>",
        130.0,
    );
    assert_eq!(text(&layout, "b").x, 60.0);
    assert_eq!(text(&layout, "c").x, 0.0);
    assert_eq!(text(&layout, "c").y, 16.0);
    assert_eq!(layout.height, 32.0);
    let flexible = show(
        "<div style='display:flex;flex-wrap:wrap'><div style='flex:1 1 60px'>a</div><div style='flex:1 1 60px'>b</div><div style='flex:1 1 60px'>c</div></div>",
        130.0,
    );
    assert_eq!(text(&flexible, "b").x, 65.0);
}

#[test]
fn gaps_go_between() {
    let row = show(
        "<div style='display:flex;gap:10px'><div>a</div><div>b</div></div>",
        800.0,
    );
    assert_eq!(text(&row, "b").x, 18.0);
    let lines = show(
        "<div style='display:flex;flex-wrap:wrap;gap:5px 10px'><div style='width:60px'>a</div><div style='width:60px'>b</div></div>",
        100.0,
    );
    assert_eq!(text(&lines, "b").y, 21.0);
    let column = show(
        "<div style='display:flex;flex-direction:column;gap:10px'><div>a</div><div>b</div></div>",
        800.0,
    );
    assert_eq!(text(&column, "b").y, 26.0);
}

#[test]
fn order_and_reversing_change_where_items_go() {
    let order = show(
        "<div style='display:flex'><div style='order:2'>a</div><div style='order:1'>b</div></div>",
        800.0,
    );
    assert_eq!(text(&order, "b").x, 0.0);
    assert_eq!(text(&order, "a").x, 8.0);
    // Copied text stays as it was written.
    assert!(order.text.find('a') < order.text.find('b'));
    let reversed = show(
        "<div style='display:flex;flex-direction:row-reverse'><div>a</div><div>b</div></div>",
        100.0,
    );
    assert_eq!(text(&reversed, "a").x, 92.0);
    assert_eq!(text(&reversed, "b").x, 84.0);
    let up = show(
        "<div style='display:flex;flex-direction:column-reverse'><div>a</div><div>b</div></div>",
        100.0,
    );
    assert_eq!(text(&up, "b").y, 0.0);
    assert_eq!(text(&up, "a").y, 16.0);
}

#[test]
fn a_column_stacks_and_stretches_across() {
    let layout = show(
        "<div style='display:flex;flex-direction:column'><div style='background:red'>a</div><div style='align-self:center;background:blue'>bb</div><div style='margin:5px'>c</div></div>",
        100.0,
    );
    let shapes = rectangles(&layout);
    assert_eq!(shapes[0].width, 100.0);
    assert_eq!(shapes[1].width, 16.0);
    assert_eq!(shapes[1].x, 42.0);
    assert_eq!(text(&layout, "c").x, 5.0);
    // Margins do not collapse in a flexible box.
    assert_eq!(text(&layout, "c").y, 37.0);
    assert_eq!(layout.height, 58.0);
}

#[test]
fn in_a_column_of_a_set_height_items_grow_and_get_pushed() {
    let grown = show(
        "<div style='display:flex;flex-direction:column;height:200px'><div>a</div><div style='flex:1;background:red'>b</div><div>c</div></div><p>after</p>",
        100.0,
    );
    assert_eq!(rectangles(&grown)[0].height, 168.0);
    assert_eq!(text(&grown, "c").y, 184.0);
    assert_eq!(text(&grown, "after").y, 200.0);
    let footer = show(
        "<div style='display:flex;flex-direction:column;min-height:200px'><div>a</div><div style='margin-top:auto'>c</div></div>",
        100.0,
    );
    assert_eq!(text(&footer, "c").y, 184.0);
    let centered = show(
        "<div style='display:flex;flex-direction:column;height:116px;justify-content:center'><div>a</div></div>",
        100.0,
    );
    assert_eq!(text(&centered, "a").y, 50.0);
}

#[test]
fn text_alone_in_the_box_is_an_item() {
    let layout = show(
        "<div style='display:flex;justify-content:space-between'>left <span>middle</span> right</div>",
        200.0,
    );
    assert_eq!(text(&layout, "left").x, 0.0);
    let right = text(&layout, "right");
    assert_eq!(right.x + right.width, 200.0);
    let middle = text(&layout, "middle");
    assert!(middle.x > 32.0 && middle.x + middle.width < right.x);
}

#[test]
fn an_image_is_an_item() {
    let layout = show(
        "<div style='display:flex;align-items:center'><img src=a.png style='margin:5px'><div>a<br>b<br>c<br>d</div></div>",
        800.0,
    );
    let image = layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Image)
        .unwrap();
    assert_eq!((image.x, image.width, image.height), (5.0, 40.0, 20.0));
    // Centered in a line of 64.
    assert_eq!(image.y, 22.0);
    assert_eq!(text(&layout, "a").x, 50.0);
}

#[test]
fn a_flexible_box_nests_and_gives_its_width() {
    let layout = show(
        "<div style='display:flex'><div style='display:flex'><div>aa</div><div>bb</div></div><div>c</div></div>",
        800.0,
    );
    assert_eq!(text(&layout, "bb").x, 16.0);
    assert_eq!(text(&layout, "c").x, 32.0);
    let in_a_table = show(
        "<table style='border-spacing:0'><tr><td style='padding:0'><div style='display:flex'><div>aa</div><div>bb</div></div><td style='padding:0'>c</table>",
        800.0,
    );
    assert_eq!(text(&in_a_table, "c").x, 32.0);
    let table_in_it = show(
        "<div style='display:flex'><table style='border-spacing:0'><tr><td style='padding:0'>aa<td style='padding:0'>bb</table><div>c</div></div>",
        800.0,
    );
    assert_eq!(text(&table_in_it, "bb").x, 16.0);
    assert_eq!(text(&table_in_it, "c").x, 32.0);
    let inline = show(
        "x <div style='display:inline-flex'><div>aa</div><div>bb</div></div> y",
        800.0,
    );
    assert_eq!(text(&inline, "bb").x - text(&inline, "aa").x, 16.0);
    assert_eq!(text(&inline, "aa").y, text(&inline, "bb").y);
}

#[test]
fn borders_and_padding_of_an_item_count() {
    let layout = show(
        "<div style='display:flex'><div style='padding:4px;border:1px solid black'>a</div><div>b</div></div>",
        800.0,
    );
    assert_eq!(text(&layout, "a").x, 5.0);
    assert_eq!(text(&layout, "b").x, 18.0);
    // Top, bottom, left and right, stretched to what the line has.
    assert_eq!(rectangles(&layout).len(), 4);
    assert_eq!(layout.height, 26.0);
}

#[test]
fn a_link_in_an_item_moves_with_it() {
    let layout = show(
        "<div style='display:flex;align-items:flex-end'><div>a<br>b<br>c</div><a href=there>x</a></div>",
        800.0,
    );
    assert_eq!(layout.link_at(9.0, 36.0), Some("there"));
    assert_eq!(layout.link_at(9.0, 4.0), None);
}

#[test]
fn hostile_boxes_do_no_harm() {
    let deep = format!(
        "{}deep{}",
        "<div style='display:flex'>".repeat(3000),
        "</div>".repeat(3000)
    );
    assert_eq!(text(&show(&deep, 800.0), "deep").height, 16.0);
    let many = "<div style='flex:1e30 1e30 1e30px'>x</div>".repeat(2000);
    show(&format!("<div style='display:flex'>{many}</div>"), 800.0);
    show(
        "<div style='display:flex'></div><div style='display:flex;flex-direction:column'></div>\
         <div style='display:flex;gap:1e30px;flex-wrap:wrap'><div>a</div><div>b</div></div>",
        800.0,
    );
}
