//! Grids. Characters are 8 wide and lines 16 high, as in the layout tests, in a view 800 wide.

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

fn show_at(body: &str, width: f32) -> Layout {
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

fn show(body: &str) -> Layout {
    show_at(body, 800.0)
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

fn place(layout: &Layout, wanted: &str) -> (f32, f32) {
    let item = text(layout, wanted);
    (item.x, item.y)
}

fn filled(layout: &Layout) -> Vec<Item> {
    layout
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Rect && item.color.alpha != 0 && item.height > 0.0)
        .copied()
        .collect()
}

fn cells(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("<div>{name}</div>"))
        .collect()
}

#[test]
fn items_fill_columns_and_go_on_in_new_rows() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-columns:100px 200px'>{}</div><p>after</p>",
        cells(&["a", "b", "c", "d", "e"])
    ));
    assert_eq!(place(&layout, "a"), (0.0, 0.0));
    assert_eq!(place(&layout, "b"), (100.0, 0.0));
    assert_eq!(place(&layout, "c"), (0.0, 16.0));
    assert_eq!(place(&layout, "d"), (100.0, 16.0));
    assert_eq!(place(&layout, "e"), (0.0, 32.0));
    assert_eq!(place(&layout, "after").1, 48.0);
}

#[test]
fn without_columns_each_item_has_a_row() {
    let layout = show(&format!(
        "<div style='display:grid'>{}</div>",
        cells(&["a", "b"])
    ));
    assert_eq!(place(&layout, "b"), (0.0, 16.0));
    let wide = show("<div style='display:grid'><div style='background:red'>a</div></div>");
    assert_eq!(filled(&wide)[0].width, 800.0);
}

#[test]
fn shares_of_what_is_left() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-columns:100px 1fr 3fr'>{}</div>",
        cells(&["a", "b", "c"])
    ));
    assert_eq!(place(&layout, "b").0, 100.0);
    assert_eq!(place(&layout, "c").0, 100.0 + 175.0);
    let equal = show(&format!(
        "<div style='display:grid;grid-template-columns:repeat(4,1fr)'>{}</div>",
        cells(&["a", "b", "c", "d"])
    ));
    assert_eq!(place(&equal, "d").0, 600.0);
}

#[test]
fn a_share_does_not_get_smaller_than_what_it_holds() {
    let layout = show_at(
        "<div style='display:grid;grid-template-columns:1fr 1fr'><div>aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</div><div>b</div></div>",
        300.0,
    );
    // The first holds a word of 256, and leaves 44 of the 300.
    assert_eq!(place(&layout, "b").0, 256.0);
    let may = show_at(
        "<div style='display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr)'><div>aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</div><div>b</div></div>",
        300.0,
    );
    assert_eq!(place(&may, "b").0, 150.0);
}

#[test]
fn auto_columns_take_what_they_hold_and_the_rest() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-columns:auto 100px auto'>{}</div>",
        cells(&["aa", "b", "cccc"])
    ));
    // Of the 652 left after 16, 100 and 32, each gets half more.
    assert_eq!(place(&layout, "b").0, 16.0 + 326.0);
    let content = show(&format!(
        "<div style='display:grid;grid-template-columns:max-content 1fr'>{}</div>",
        cells(&["aaaa", "b"])
    ));
    assert_eq!(place(&content, "b").0, 32.0);
    let least = show(&format!(
        "<div style='display:grid;grid-template-columns:min-content 1fr'>{}</div>",
        cells(&["aa bbb", "c"])
    ));
    assert_eq!(place(&least, "c").0, 24.0);
    assert_eq!(place(&least, "bbb").1, 16.0);
}

#[test]
fn percentages_and_minmax() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-columns:25% minmax(100px,200px) 1fr'>{}</div>",
        cells(&["a", "b", "c"])
    ));
    assert_eq!(place(&layout, "b").0, 200.0);
    assert_eq!(place(&layout, "c").0, 400.0);
}

#[test]
fn as_many_columns_as_fit() {
    let layout = show_at(
        &format!(
            "<div style='display:grid;grid-template-columns:repeat(auto-fill,minmax(100px,1fr))'>{}</div>",
            cells(&["a", "b", "c", "d"])
        ),
        310.0,
    );
    assert_eq!(place(&layout, "c").1, 0.0);
    assert_eq!(place(&layout, "d").1, 16.0);
    assert!((place(&layout, "b").0 - 103.33).abs() < 0.1);
    let fit = show_at(
        &format!(
            "<div style='display:grid;grid-template-columns:repeat(auto-fit,minmax(100px,1fr))'>{}</div>",
            cells(&["a", "b"])
        ),
        800.0,
    );
    assert_eq!(place(&fit, "b").0, 400.0);
    let fill = show_at(
        &format!(
            "<div style='display:grid;grid-template-columns:repeat(auto-fill,minmax(100px,1fr))'>{}</div>",
            cells(&["a", "b"])
        ),
        800.0,
    );
    assert_eq!(place(&fill, "b").0, 100.0);
}

#[test]
fn gaps_go_between() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-columns:100px 100px;gap:10px 20px'>{}</div><p>after</p>",
        cells(&["a", "b", "c"])
    ));
    assert_eq!(place(&layout, "b").0, 120.0);
    assert_eq!(place(&layout, "c").1, 26.0);
    assert_eq!(place(&layout, "after").1, 42.0);
}

#[test]
fn items_say_where_they_go() {
    let layout = show(
        "<div style='display:grid;grid-template-columns:100px 100px 100px'>\
           <div style='grid-column:2;grid-row:2'>a</div>\
           <div style='grid-column:1 / 3'>b</div>\
           <div style='grid-column:span 2'>c</div>\
           <div style='grid-column:-2 / -1;grid-row:3'>d</div>\
           <div>e</div>\
         </div>",
    );
    assert_eq!(place(&layout, "a"), (100.0, 16.0));
    assert_eq!(place(&layout, "b"), (0.0, 0.0));
    // Takes the next free place for two columns, as the cell at the end of the row is too small.
    assert_eq!(place(&layout, "c").0, 0.0);
    assert_eq!(place(&layout, "c").1, 32.0);
    assert_eq!(place(&layout, "d"), (200.0, 32.0));
    assert_eq!(place(&layout, "e").1, 48.0);
}

#[test]
fn a_row_holds_what_spans_it() {
    let layout = show(
        "<div style='display:grid;grid-template-columns:100px 100px'>\
           <div style='grid-row:span 2;height:100px;background:red'>tall</div><div>a</div><div>b</div><div>c</div>\
         </div>",
    );
    let total = place(&layout, "c").1;
    assert_eq!(place(&layout, "a").0, 100.0);
    assert_eq!(place(&layout, "b").0, 100.0);
    assert_eq!(total, 100.0);
}

#[test]
fn rows_can_have_sizes() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-rows:50px 100px;grid-auto-rows:30px'>{}</div><p>after</p>",
        cells(&["a", "b", "c", "d"])
    ));
    assert_eq!(place(&layout, "b").1, 50.0);
    assert_eq!(place(&layout, "c").1, 150.0);
    assert_eq!(place(&layout, "d").1, 180.0);
    assert_eq!(place(&layout, "after").1, 210.0);
    let shares = show(&format!(
        "<div style='display:grid;height:300px;grid-template-rows:auto 1fr auto'>{}</div>",
        cells(&["head", "main", "foot"])
    ));
    assert_eq!(place(&shares, "main").1, 16.0);
    assert_eq!(place(&shares, "foot").1, 284.0);
}

#[test]
fn areas_are_named_by_a_picture_of_the_page() {
    let layout = show(
        "<style>.page{display:grid;grid-template-columns:100px 1fr;grid-template-areas:'head head' 'side main' 'foot foot'}</style>\
         <div class=page>\
           <div style='grid-area:main'>main</div>\
           <div style='grid-area:foot'>foot</div>\
           <div style='grid-area:head;background:red'>head</div>\
           <div style='grid-area:side'>side</div>\
         </div>",
    );
    assert_eq!(place(&layout, "head"), (0.0, 0.0));
    assert_eq!(filled(&layout)[0].width, 800.0);
    assert_eq!(place(&layout, "side"), (0.0, 16.0));
    assert_eq!(place(&layout, "main"), (100.0, 16.0));
    assert_eq!(place(&layout, "foot"), (0.0, 32.0));
}

#[test]
fn items_stretch_unless_told_otherwise() {
    let tall = "<div>a<br>b<br>c</div>";
    let stretched = show(&format!(
        "<div style='display:grid;grid-template-columns:100px 100px'>{tall}<div style='background:red'>m</div></div>"
    ));
    assert_eq!(filled(&stretched)[0].height, 48.0);
    assert_eq!(filled(&stretched)[0].width, 100.0);
    let centered = show(&format!(
        "<div style='display:grid;grid-template-columns:100px 100px;place-items:center'>{tall}<div style='background:red'>m</div></div>"
    ));
    let shape = filled(&centered)[0];
    assert_eq!((shape.x, shape.y), (146.0, 16.0));
    assert_eq!((shape.width, shape.height), (8.0, 16.0));
    let own = show(&format!(
        "<div style='display:grid;grid-template-columns:100px 100px'>{tall}<div style='background:red;justify-self:end;align-self:end'>m</div></div>"
    ));
    let shape = filled(&own)[0];
    assert_eq!((shape.x, shape.y), (192.0, 32.0));
}

#[test]
fn text_and_images_are_items() {
    let layout = show(
        "<div style='display:grid;grid-template-columns:100px 100px'>text <img src=a.png></div>",
    );
    assert_eq!(place(&layout, "text"), (0.0, 0.0));
    let image = layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Image)
        .unwrap();
    assert_eq!(image.x, 100.0);
}

#[test]
fn a_grid_nests_and_gives_its_width() {
    let layout = show(
        "<div style='display:flex'><div style='display:grid;grid-template-columns:50px 60px'>\
         <div>a</div><div>b</div></div><div>c</div></div>",
    );
    assert_eq!(place(&layout, "c").0, 110.0);
    let in_a_float = show(
        "<div style='float:left;display:grid;grid-template-columns:auto auto'><div>aa</div><div>bb</div></div><p>c</p>",
    );
    assert_eq!(place(&in_a_float, "bb").0, 16.0);
    assert_eq!(place(&in_a_float, "c").0, 32.0);
    let inline = show(
        "x <div style='display:inline-grid;grid-template-columns:auto auto'><div>aa</div><div>bb</div></div> y",
    );
    assert_eq!(text(&inline, "bb").x - text(&inline, "aa").x, 16.0);
}

#[test]
fn what_is_not_understood_is_left_out() {
    let layout = show(&format!(
        "<div style='display:grid;grid-template-columns:what is this;grid-column:/ /'>{}</div>",
        cells(&["a", "b"])
    ));
    assert_eq!(place(&layout, "b"), (0.0, 16.0));
}

#[test]
fn hostile_grids_do_no_harm() {
    show(
        "<div style='display:grid;grid-template-columns:repeat(99999999,1fr);grid-template-rows:repeat(99999999,1px)'>\
         <div style='grid-column:99999;grid-row:-99999 / span 99999'>a</div></div>",
    );
    show(
        "<div style='display:grid;grid-template-columns:repeat(auto-fill,0px)'><div>a</div></div>\
         <div style='display:grid;grid-template-columns:repeat(auto-fill,minmax(1e30px,1e30fr))'><div>a</div></div>\
         <div style='display:grid;grid-template-columns:1e30fr 1e30fr;gap:1e30px'><div>a</div><div>b</div></div>\
         <div style='display:grid'></div>",
    );
    let spans = "<div style='grid-column:span 1000;grid-row:span 1000'>x</div>".repeat(50);
    show(&format!(
        "<div style='display:grid;grid-template-columns:repeat(3,1fr)'>{spans}</div>"
    ));
    let deep = format!(
        "{}deep{}",
        "<div style='display:grid'>".repeat(3000),
        "</div>".repeat(3000)
    );
    assert_eq!(text(&show(&deep), "deep").height, 16.0);
    let many = "<div>x</div>".repeat(20000);
    show(&format!(
        "<div style='display:grid;grid-template-columns:repeat(3,1fr)'>{many}</div>"
    ));
}
