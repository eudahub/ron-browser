//! Inline blocks: boxes of their own that stand among the text of a line. Characters are 8 wide
//! and lines 16 high, as in the layout tests.

use ron_browser_core::images::{DecodedImage, ImageStore, NoResources};
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

struct Shown {
    layout: Layout,
    measurer: Measurer,
}

fn show(body: &str, width: f32) -> Shown {
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
    let mut measurer = Measurer::default();
    let mut page = Page::open_with(&source, &mut NoResources, store);
    page.lay_out(width, 600.0, &mut measurer);
    Shown {
        layout: page.layout().clone(),
        measurer,
    }
}

/// The text item with this text.
fn text(shown: &Shown, wanted: &str) -> Item {
    *shown
        .layout
        .items
        .iter()
        .find(|item| {
            item.kind == ItemKind::Text
                && &shown.layout.text[item.start as usize..item.end as usize] == wanted
        })
        .unwrap_or_else(|| panic!("no text {wanted}"))
}

fn rectangles(shown: &Shown) -> Vec<Item> {
    shown
        .layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Rect)
        .copied()
        .collect()
}

const BOX: &str = "display:inline-block;background:red";

#[test]
fn it_stands_in_the_line_as_wide_as_what_it_holds() {
    let shown = show(&format!("aa<span style='{BOX}'>bbb</span>cc"), 500.0);
    let rectangle = rectangles(&shown)[0];
    assert_eq!(
        (rectangle.x, rectangle.y, rectangle.width, rectangle.height),
        (16.0, 0.0, 24.0, 16.0)
    );
    assert_eq!(text(&shown, "aa").x, 0.0);
    assert_eq!(text(&shown, "bbb").x, 16.0);
    assert_eq!(text(&shown, "cc").x, 40.0);
    assert_eq!(text(&shown, "bbb").baseline, text(&shown, "aa").baseline);
    assert_eq!(shown.layout.text, "aabbbcc");
}

#[test]
fn padding_borders_and_margins_go_around_what_it_holds() {
    let shown = show(
        &format!("a<span style='{BOX};padding:2px 3px;border:1px solid blue;margin:5px'>b</span>c"),
        500.0,
    );
    // 8 for the a, then a margin of 5.
    let rectangle = rectangles(&shown)[0];
    assert_eq!((rectangle.x, rectangle.width), (13.0, 16.0));
    assert_eq!(rectangle.height, 22.0);
    assert_eq!(text(&shown, "b").x, 17.0);
    // After the box and another margin.
    assert_eq!(text(&shown, "c").x, 34.0);
    // On the same line of text as the rest.
    assert_eq!(text(&shown, "a").baseline, text(&shown, "b").baseline);
    assert_eq!(text(&shown, "a").baseline, text(&shown, "c").baseline);
    // The box makes the line higher, with its margins: 5 + 1 + 2 above the text.
    assert!(rectangle.y >= 5.0);
    assert!(shown.layout.lines.last().unwrap().bottom >= rectangle.y + 22.0 + 5.0 - 0.01);
}

#[test]
fn a_set_size_counts() {
    let shown = show(
        &format!("<span style='{BOX};width:100px;height:50px'>x</span>y"),
        500.0,
    );
    let rectangle = rectangles(&shown)[0];
    assert_eq!((rectangle.width, rectangle.height), (100.0, 50.0));
    assert_eq!(text(&shown, "y").x, 100.0);
}

#[test]
fn lines_wrap_around_it_as_around_a_word() {
    let shown = show(
        &format!(
            "<span style='{BOX}'>aaaa</span> <span style='{BOX}'>bbbb</span> <span style='{BOX}'>cccc</span>"
        ),
        80.0,
    );
    let places: Vec<(f32, f32)> = rectangles(&shown)
        .iter()
        .map(|item| (item.x, item.y.round()))
        .collect();
    assert_eq!(places, [(0.0, 0.0), (40.0, 0.0), (0.0, 16.0)]);
}

#[test]
fn its_white_space_rules_inside_and_that_around_it_rules_outside() {
    let places = |body: &str| -> Vec<f32> {
        let shown = show(body, 80.0);
        rectangles(&shown)
            .iter()
            .map(|item| item.y.round())
            .collect()
    };
    let nowrap = format!("{BOX};white-space:nowrap");
    assert_eq!(
        places(&format!(
            "<span style='{nowrap}'>aaaa</span> <span style='{nowrap}'>bbbb</span> <span style='{nowrap}'>cccc</span>"
        )),
        [0.0, 0.0, 16.0]
    );
    assert_eq!(
        places(&format!(
            "<div style='white-space:nowrap'><span style='{BOX}'>aaaa</span> <span style='{BOX}'>bbbb</span> <span style='{BOX}'>cccc</span></div>"
        )),
        [0.0, 0.0, 0.0]
    );
}

#[test]
fn it_follows_the_alignment_of_its_line() {
    let shown = show(
        &format!("<div style='text-align:center'><span style='{BOX}'>aaaa</span></div>"),
        100.0,
    );
    assert_eq!(rectangles(&shown)[0].x, 34.0);
    assert_eq!(text(&shown, "aaaa").x, 34.0);
}

#[test]
fn blocks_inside_it_stack_and_set_its_width() {
    let shown = show(
        &format!(
            "<a href=x style='{BOX};margin:1px'><b style='display:block;padding:0 10px'>long text</b>\
             <i style='display:block'>bit</i></a><span>after</span>"
        ),
        500.0,
    );
    let rectangle = rectangles(&shown)[0];
    // 9 characters, with the padding of the block they are in.
    assert_eq!(rectangle.width, 92.0);
    assert_eq!(rectangle.height, 32.0);
    assert_eq!(text(&shown, "long text").x, 11.0);
    assert_eq!(text(&shown, "bit").x, 1.0);
    assert_eq!(text(&shown, "after").x, 94.0);
}

#[test]
fn those_inside_each_other_all_find_their_places() {
    let inner = format!("<span style='{BOX}'>aa</span> <span style='{BOX}'>bb</span>");
    let shown = show(
        &format!("x<div style='{BOX};padding:4px'>{inner}</div>y"),
        500.0,
    );
    // The two with a space between them fit on one line of the outer box.
    let boxes = rectangles(&shown);
    assert_eq!(boxes[0].width, 16.0 + 8.0 + 16.0 + 8.0);
    assert_eq!(text(&shown, "aa").x, 12.0);
    assert_eq!(text(&shown, "bb").x, 36.0);
    assert_eq!(text(&shown, "y").x, 56.0);
    assert_eq!(text(&shown, "aa").baseline, text(&shown, "x").baseline);
}

#[test]
fn a_box_too_wide_for_the_room_wraps_inside() {
    let shown = show(
        &format!("<span style='{BOX}'>aaaa bbbb cccc dddd</span>"),
        80.0,
    );
    let rectangle = rectangles(&shown)[0];
    assert_eq!(rectangle.width, 80.0);
    assert_eq!(rectangle.height, 32.0);
}

#[test]
fn it_can_be_clicked_as_a_whole_when_in_a_link() {
    let shown = show(
        &format!("<a href=x style='{BOX};padding:10px'>a</a>b"),
        500.0,
    );
    // In its padding, where there is no text.
    assert_eq!(shown.layout.link_at(2.0, 2.0), Some("x"));
    assert_eq!(shown.layout.link_at(14.0, 14.0), Some("x"));
    assert_eq!(shown.layout.link_at(30.0, 14.0), None);
}

#[test]
fn links_and_images_inside_it_move_with_it() {
    let shown = show(
        &format!(
            "aaaa<span style='{BOX}'><a href=x>b</a><img src=a.png usemap=#m></span>\
             <map name=m><area shape=circle coords='5,5,4' href=round></map>"
        ),
        500.0,
    );
    let image = shown
        .layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Image)
        .unwrap();
    assert_eq!(image.x, 40.0);
    let line_y = text(&shown, "b").y;
    assert_eq!(shown.layout.link_at(34.0, line_y + 8.0), Some("x"));
    assert_eq!(shown.layout.link_at(45.0, image.y + 5.0), Some("round"));
    assert_eq!(shown.layout.link_at(2.0, 1000.0), None);
}

#[test]
fn a_background_image_moves_with_it() {
    let shown = show(
        "aaaa<span style='display:inline-block;background:url(a.png);width:80px;height:30px'></span>",
        500.0,
    );
    let tile = shown
        .layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Tile)
        .unwrap();
    assert_eq!(tile.x, 32.0);
    let origin = shown.layout.tiles[tile.start as usize];
    assert_eq!((origin.origin_x, origin.origin_y), (32.0, tile.y));
}

#[test]
fn floats_inside_it_stay_inside_and_floats_outside_stay_out() {
    let shown = show(
        "<img src=a.png style='float:left'>\
         <span style='display:inline-block'>\
         <img src=a.png style='float:right'>tt</span>after",
        500.0,
    );
    let images: Vec<(f32, f32)> = shown
        .layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Image)
        .map(|item| (item.x, item.y))
        .collect();
    assert_eq!(images[0], (0.0, 0.0));
    // Beside the outer float, as wide as both things in it.
    assert_eq!(text(&shown, "tt").x, 40.0);
    assert_eq!(images[1].0, 56.0);
    assert_eq!(text(&shown, "after").x, 96.0);
}

#[test]
fn decorations_stay_out_of_it() {
    let shown = show(&format!("<u>a<span style='{BOX}'>b</span></u>"), 500.0);
    assert_ne!(text(&shown, "a").decoration, 0);
    assert_eq!(text(&shown, "b").decoration, 0);
}

#[test]
fn a_list_marker_stays_with_the_line_of_its_item() {
    let shown = show(
        &format!("<ul style='padding-left:40px'><li><span style='{BOX}'>a</span>b</ul>"),
        500.0,
    );
    let marker = shown
        .layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Marker)
        .unwrap();
    assert_eq!(marker.baseline, text(&shown, "b").baseline);
}

#[test]
fn text_in_it_can_be_selected() {
    let mut shown = show(&format!("aaaa<span style='{BOX}'>bbbb</span>cccc"), 500.0);
    let mut selection = Selection::start(
        &shown.layout,
        2.0,
        8.0,
        ron_browser_core::selection::Unit::Character,
        &mut shown.measurer,
    )
    .unwrap();
    selection.drag_to(&shown.layout, 94.0, 8.0, &mut shown.measurer);
    assert_eq!(selection.text(&shown.layout), "aaaabbbbcccc");
    let highlights = selection.highlights(&shown.layout, 0.0, 100.0, &mut shown.measurer);
    let covered: f32 = highlights.iter().map(|highlight| highlight.width).sum();
    assert_eq!(covered, 96.0);
}

#[test]
fn deep_nesting_stays_fast_and_safe() {
    let start = std::time::Instant::now();
    let deep = format!(
        "{}x{}",
        "<span style='display:inline-block'>".repeat(5000),
        "</span>".repeat(5000)
    );
    assert_eq!(show(&deep, 500.0).layout.text, "x");
    let wide = format!("<span style='{BOX}'>x</span> ").repeat(20_000);
    let shown = show(&wide, 500.0);
    assert_eq!(rectangles(&shown).len(), 20_000);
    assert!(start.elapsed().as_secs() < 20);
}
