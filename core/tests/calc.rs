//! Expressions in layout, where percentages come to something. Characters are 8 wide and lines 16
//! high, as in the layout tests, in a view of 800 by 600.

use ron_browser_core::images::{ImageStore, NoResources};
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

fn show(body: &str) -> Layout {
    let source = format!("<!DOCTYPE html><style>body{{margin:0}}p{{margin:0}}</style>{body}");
    let mut page = Page::open_with(&source, &mut NoResources, ImageStore::default());
    page.lay_out(800.0, 600.0, &mut Measurer::default());
    page.layout().clone()
}

fn box_of(layout: &Layout) -> Item {
    *layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Rect && item.color.alpha != 0 && item.width > 0.0)
        .expect("a filled box")
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

fn width_of(value: &str) -> f32 {
    box_of(&show(&format!(
        "<div style='width:{value};background:red'>x</div>"
    )))
    .width
}

#[test]
fn widths_go_by_what_is_around_them() {
    assert_eq!(width_of("calc(100% - 100px)"), 700.0);
    assert_eq!(width_of("calc(50% + 10px)"), 410.0);
    assert_eq!(width_of("min(100%, 300px)"), 300.0);
    assert_eq!(width_of("min(50%, 900px)"), 400.0);
    assert_eq!(width_of("max(50%, 500px)"), 500.0);
    assert_eq!(width_of("clamp(100px, 50%, 200px)"), 200.0);
    // Relative to the box around it, and not to the page.
    let nested = show(
        "<div style='width:200px'><div style='width:calc(100% - 20px);background:red'>x</div></div>",
    );
    assert_eq!(box_of(&nested).width, 180.0);
}

#[test]
fn it_centers_with_a_width_that_has_a_limit() {
    let layout =
        show("<div style='width:min(100% - 100px, 400px);margin:0 auto;background:red'>x</div>");
    let shape = box_of(&layout);
    assert_eq!((shape.x, shape.width), (200.0, 400.0));
    // The usual way to go as wide as the view, whatever is around.
    let view = show("<div style='width:calc(100vw - 40px);background:red'>x</div>");
    assert_eq!(box_of(&view).width, 760.0);
}

#[test]
fn margins_and_padding_take_expressions() {
    let layout =
        show("<div style='margin-left:calc(10% + 5px);padding-left:calc(5% - 10px)'>x</div>");
    assert_eq!(text(&layout, "x").x, 85.0 + 30.0);
}

#[test]
fn positioned_boxes_take_expressions() {
    let layout =
        show("<div style='position:absolute;left:calc(50% - 50px);top:calc(50% - 8px)'>x</div>");
    let x = text(&layout, "x");
    assert_eq!((x.x, x.y), (350.0, 292.0));
    let high = show(
        "<div style='position:relative;height:200px'><div style='position:absolute;top:0;left:0;\
         height:calc(100% - 50px);width:10px;background:red'></div></div>",
    );
    assert_eq!(box_of(&high).height, 150.0);
}

#[test]
fn flexible_boxes_and_grids_take_expressions() {
    let flexible = show(
        "<div style='display:flex'><div style='flex:0 0 calc(25% + 10px)'>a</div><div>b</div></div>",
    );
    assert_eq!(text(&flexible, "b").x, 210.0);
    let grid = show(
        "<div style='display:grid;grid-template-columns:calc(10px * 5) 1fr;gap:calc(5px + 5px)'>\
         <div>a</div><div>b</div></div>",
    );
    assert_eq!(text(&grid, "b").x, 60.0);
}

#[test]
fn a_size_from_the_view_gets_worked_out_for_a_font() {
    let layout = show("<p style='font-size:clamp(10px, 2vw, 14px)'>x</p>");
    // 2vw of 800 would be 16, and so stops at 14.
    assert_eq!(text(&layout, "x").width, 7.0);
}

#[test]
fn nonsense_leaves_what_came_before() {
    let layout =
        show("<style>div{width:100px;width:calc(1px + 1);background:red}</style><div>x</div>");
    assert_eq!(box_of(&layout).width, 100.0);
}

#[test]
fn hostile_expressions_do_no_harm() {
    // More sizes than are kept: those beyond that are dropped, and no more than that.
    let many: String = (0..70_000)
        .map(|index| format!("<p style='width:calc(10% + {index}.5px)'>"))
        .collect();
    show(&many);
    show("<div style='width:calc(1e30px * 1e30);margin:calc(100% * 1e30)'>x</div>");
    let deep = format!("{}1px{}", "calc(".repeat(100_000), ")".repeat(100_000));
    show(&format!("<div style='width:{deep}'>x</div>"));
}
