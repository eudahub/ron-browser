//! SVG in pages. Characters are 8 wide and lines 16 high, as in the layout tests.

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

#[test]
fn what_it_holds_does_not_show_as_text() {
    let layout = show(
        "<p>before</p><svg width=20 height=20><title>A title</title><desc>About it</desc>\
         <text>In it</text></svg><p>after</p>",
    );
    assert!(!layout.text.contains("title"), "{:?}", layout.text);
    assert!(!layout.text.contains("About"));
    assert!(!layout.text.contains("In it"));
}

#[test]
fn it_takes_the_room_it_says() {
    let attributes = show("<svg width=100 height=40 style='display:block'></svg><p>after</p>");
    assert_eq!(text(&attributes, "after").y, 40.0);
    let in_line = show("<svg width=100 height=10></svg>after");
    assert_eq!(text(&in_line, "after").x, 100.0);
    let css = show(
        "<svg width=100 height=10 style='display:block;width:30px;height:70px'></svg><p>after</p>",
    );
    assert_eq!(text(&css, "after").y, 70.0);
    let by_shape =
        show("<svg viewBox='0 0 40 20' width=80 style='display:block'></svg><p>after</p>");
    assert_eq!(text(&by_shape, "after").y, 40.0);
    let by_css_and_shape =
        show("<svg viewBox='0 0 40 20' style='display:block;height:30px'></svg><p>after</p>");
    assert_eq!(text(&by_css_and_shape, "after").y, 30.0);
    let with_nothing = show("<svg style='display:block'></svg><p>after</p>");
    assert_eq!(text(&with_nothing, "after").y, 150.0);
    let fills = show(
        "<div style='width:200px'><svg viewBox='0 0 2 1' style='display:block'></svg></div><p>after</p>",
    );
    assert_eq!(text(&fills, "after").y, 100.0);
    let px = show("<svg width=100px height=40px style='display:block'></svg><p>after</p>");
    assert_eq!(text(&px, "after").y, 40.0);
}

#[test]
fn it_can_be_hidden() {
    let layout = show("<svg width=100 height=40 style='display:none'></svg><p>after</p>");
    assert_eq!(text(&layout, "after").y, 0.0);
}

#[test]
fn it_stands_in_a_row_of_a_flexible_box_and_can_be_positioned() {
    let row = show("<div style='display:flex'><svg width=50 height=20></svg><div>a</div></div>");
    assert_eq!(text(&row, "a").x, 50.0);
    let aside =
        show("<svg width=500 height=500 style='position:absolute;left:0;top:0'></svg><p>after</p>");
    assert_eq!(text(&aside, "after").y, 0.0);
}

#[test]
fn hostile_sizes_do_no_harm() {
    show(
        "<svg width=1e30 height=1e30></svg><svg viewBox='0 0 0 0'></svg><svg viewBox='a b c'></svg>",
    );
    show("<svg width=-5 height=-5 viewBox='0 0 -1 -1'></svg><svg width=NaN height=inf></svg>");
}
