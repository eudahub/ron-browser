//! What draws above what. Characters are 8 wide and lines 16 high, as in the layout tests.

use ron_browser_core::layout::{FontSpec, ItemKind, Layout, TextMeasurer};
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

fn show(body: &str) -> Layout {
    let source = format!("<!DOCTYPE html><style>body{{margin:0}}</style>{body}");
    let mut page = Page::open(&source);
    page.lay_out(800.0, 600.0, &mut Measurer::default());
    page.layout().clone()
}

/// The text of the page in the order of drawing.
fn drawn_text(layout: &Layout) -> Vec<&str> {
    layout
        .visible_items(0.0, 100_000.0)
        .iter()
        .filter(|visible| visible.item.kind == ItemKind::Text)
        .map(|visible| &layout.text[visible.item.start as usize..visible.item.end as usize])
        .collect()
}

/// Where the text comes in the order of drawing, against the background with a red of this much.
fn text_and_background(layout: &Layout, red: u8) -> (usize, usize) {
    let drawn = layout.visible_items(0.0, 100_000.0);
    let text = drawn
        .iter()
        .position(|visible| visible.item.kind == ItemKind::Text)
        .unwrap();
    let background = drawn
        .iter()
        .position(|visible| {
            visible.item.kind == ItemKind::Rect
                && visible.item.color.alpha == 255
                && visible.item.color.red == red
        })
        .unwrap();
    (text, background)
}

#[test]
fn a_relative_box_stays_above_the_background_of_a_box_with_a_z_index() {
    let layout = show(
        "<div style='position:relative;z-index:99;background:#110000'>\
         <div style='position:relative'>a</div></div>",
    );
    let (text, background) = text_and_background(&layout, 0x11);
    assert!(text > background);
}

#[test]
fn what_is_within_a_z_index_stays_below_a_higher_one_outside() {
    let layout = show(
        "<div style='position:absolute;z-index:1'>\
         <div style='position:absolute;z-index:9'>inner</div></div>\
         <div style='position:absolute;z-index:5'>outside</div>",
    );
    assert_eq!(drawn_text(&layout), ["inner", "outside"]);
}

#[test]
fn a_z_index_decides_among_boxes_at_the_same_level() {
    let layout = show(
        "<div style='position:absolute;z-index:3'>c</div>\
         <div style='position:absolute;z-index:1'>a</div>\
         <div style='position:absolute;z-index:2'>b</div>",
    );
    assert_eq!(drawn_text(&layout), ["a", "b", "c"]);
}

#[test]
fn a_box_without_a_z_index_lets_those_within_it_compare_with_those_outside() {
    let layout = show(
        "<div style='position:absolute'>\
         <div style='position:absolute;z-index:9'>inner</div></div>\
         <div style='position:absolute;z-index:5'>outside</div>",
    );
    assert_eq!(drawn_text(&layout), ["outside", "inner"]);
}

#[test]
fn opacity_holds_what_is_within_together() {
    let layout = show(
        "<div style='opacity:.5'><div style='position:relative;z-index:9'>inner</div></div>\
         <div style='position:absolute;z-index:1'>outside</div>",
    );
    assert_eq!(drawn_text(&layout), ["inner", "outside"]);
}

#[test]
fn a_transform_holds_what_is_within_together() {
    let layout = show(
        "<div style='transform:translateX(1px)'>\
         <div style='position:relative;z-index:9'>inner</div></div>\
         <div style='position:absolute;z-index:1'>outside</div>",
    );
    assert_eq!(drawn_text(&layout), ["inner", "outside"]);
}

#[test]
fn positioned_boxes_come_above_the_flow_and_negative_ones_below_it() {
    let layout = show(
        "<div style='position:absolute;z-index:-1'>below</div>\
         <div style='position:relative'>above</div><div>flow</div>",
    );
    assert_eq!(drawn_text(&layout), ["below", "flow", "above"]);
}

#[test]
fn boxes_without_a_z_index_keep_the_order_of_the_page() {
    let layout = show(
        "<div style='position:absolute'>a</div><div style='position:relative'>b</div>\
         <div style='position:absolute'>c</div>",
    );
    assert_eq!(drawn_text(&layout), ["a", "b", "c"]);
}

#[test]
fn deep_nesting_does_no_harm() {
    let open = "<div style='position:relative;z-index:1'>".repeat(5000);
    let layout = show(&format!("{open}deep"));
    assert_eq!(drawn_text(&layout), ["deep"]);
}
