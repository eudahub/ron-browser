//! Positioned boxes and what is cut off. Characters are 8 wide and lines 16 high, as in the layout
//! tests, and the view is 800 by 600.

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

fn show(body: &str) -> Layout {
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

/// What gets drawn, in that order, of the whole page, as text.
fn drawn_text(layout: &Layout) -> Vec<&str> {
    layout
        .visible_items(-1e6, 1e6)
        .into_iter()
        .map(|visible| visible.item)
        .filter(|item| item.kind == ItemKind::Text)
        .map(|item| &layout.text[item.start as usize..item.end as usize])
        .collect()
}

fn filled(layout: &Layout) -> Vec<Item> {
    layout
        .visible_items(-1e6, 1e6)
        .into_iter()
        .map(|visible| visible.item)
        .filter(|item| item.kind == ItemKind::Rect && item.color.alpha != 0 && item.height > 0.0)
        .collect()
}

#[test]
fn a_relative_box_moves_and_keeps_its_place_in_the_flow() {
    let layout = show("<p style='position:relative;left:10px;top:5px'>moved</p><p>next</p>");
    let moved = text(&layout, "moved");
    assert_eq!((moved.x, moved.y), (10.0, 5.0));
    assert_eq!(text(&layout, "next").y, 16.0);
    let other_sides = show("<p style='position:relative;right:10px;bottom:5px'>a</p>");
    assert_eq!(text(&other_sides, "a").x, -10.0);
    assert_eq!(text(&other_sides, "a").y, -5.0);
}

#[test]
fn an_absolute_box_takes_no_room() {
    let layout = show(
        "<p>before</p><div style='position:absolute;left:100px;top:200px'>away</div><p>after</p>",
    );
    assert_eq!(text(&layout, "after").y, 16.0);
    let away = text(&layout, "away");
    assert_eq!((away.x, away.y), (100.0, 200.0));
    assert_eq!(layout.height, 216.0);
}

#[test]
fn it_goes_by_the_nearest_positioned_box() {
    let layout = show(
        "<div style='height:50px'></div>\
         <div style='position:relative;margin-left:30px;border:2px solid black;height:100px'>\
           <div><div style='position:absolute;left:5px;top:6px'>in</div></div>\
           <div style='position:absolute;right:0;bottom:0'>corner</div>\
         </div>",
    );
    let inside = text(&layout, "in");
    assert_eq!((inside.x, inside.y), (37.0, 58.0));
    let corner = text(&layout, "corner");
    // 800 less the border, and 100 of height above the border below.
    assert_eq!(corner.x + corner.width, 798.0);
    assert_eq!(corner.y, 50.0 + 2.0 + 100.0 - 16.0);
}

#[test]
fn without_a_positioned_box_it_goes_by_the_page() {
    let layout = show(
        "<div style='margin:50px'><div style='position:absolute;right:10px;bottom:20px'>corner</div></div>",
    );
    let corner = text(&layout, "corner");
    assert_eq!(corner.x + corner.width, 790.0);
    assert_eq!(corner.y, 600.0 - 20.0 - 16.0);
    let fixed = show(
        "<div style='position:relative;margin:50px'><div style='position:fixed;left:0;top:0'>f</div></div>",
    );
    assert_eq!((text(&fixed, "f").x, text(&fixed, "f").y), (0.0, 0.0));
}

#[test]
fn without_offsets_it_stays_where_it_would_have_been() {
    let layout = show(
        "<p>before</p><div style='margin-left:40px'><div style='position:absolute'>here</div><p>after</p></div>",
    );
    let here = text(&layout, "here");
    assert_eq!((here.x, here.y), (40.0, 16.0));
    assert_eq!(text(&layout, "after").y, 16.0);
}

#[test]
fn its_width_comes_from_what_it_holds_or_from_both_sides() {
    let fitting = show("<div style='position:absolute;background:red'>four</div>");
    assert_eq!(filled(&fitting)[0].width, 32.0);
    let both = show("<div style='position:absolute;left:100px;right:200px;background:red'>x</div>");
    assert_eq!((filled(&both)[0].x, filled(&both)[0].width), (100.0, 500.0));
    let set = show("<div style='position:absolute;width:50%;background:red'>x</div>");
    assert_eq!(filled(&set)[0].width, 400.0);
}

#[test]
fn both_sides_down_stretch_it_and_automatic_margins_center_it() {
    let stretched = show(
        "<div style='position:relative;height:300px'>\
         <div style='position:absolute;top:10px;bottom:20px;left:0;background:red'>x</div></div>",
    );
    let shape = filled(&stretched)[0];
    assert_eq!((shape.y, shape.height), (10.0, 270.0));
    let centered = show(
        "<div style='position:absolute;left:0;right:0;top:0;bottom:0;margin:auto;width:100px;height:100px;background:red'>x</div>",
    );
    let shape = filled(&centered)[0];
    assert_eq!((shape.x, shape.y), (350.0, 250.0));
}

#[test]
fn borders_and_margins_count() {
    let layout = show(
        "<div style='position:absolute;left:10px;top:20px;margin:5px;border:2px solid black;padding:3px'>x</div>",
    );
    let x = text(&layout, "x");
    assert_eq!((x.x, x.y), (20.0, 30.0));
    let stretched = show(
        "<div style='position:absolute;left:0;top:0;bottom:0;width:10px;border:1px solid black'></div>",
    );
    let sides = filled(&stretched);
    assert_eq!(sides.len(), 4);
    assert_eq!(sides[1].y, 599.0);
}

#[test]
fn an_image_can_be_absolute() {
    let layout = show(
        "<p>text</p><img src=a.png style='position:absolute;left:100px;top:200px'><p>after</p>",
    );
    let image = layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Image)
        .unwrap();
    assert_eq!((image.x, image.y), (100.0, 200.0));
    assert_eq!(text(&layout, "after").y, 16.0);
}

#[test]
fn it_does_not_break_a_line_or_a_flexible_box_or_a_table() {
    let line = show("before <span style='position:absolute;left:0;top:100px'>away</span> after");
    let on_first_line: Vec<&str> = line
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Text && item.y == 0.0)
        .map(|item| line.text[item.start as usize..item.end as usize].trim())
        .filter(|words| !words.is_empty())
        .collect();
    assert_eq!(on_first_line, ["before", "after"]);
    assert_eq!(text(&line, "away").y, 100.0);
    let flexible = show(
        "<div style='display:flex'><div>a</div><div style='position:absolute;top:100px;left:0'>away</div><div>b</div></div>",
    );
    assert_eq!(text(&flexible, "b").x, 8.0);
    assert_eq!(text(&flexible, "away").y, 100.0);
    let table = show(
        "<table style='border-spacing:0'><tr><td style='padding:0'>a<td style='padding:0;position:relative'>b\
         <div style='position:absolute;right:0;top:0'>c</div></table>",
    );
    let (b, c) = (text(&table, "b"), text(&table, "c"));
    assert_eq!(c.y, b.y);
    assert_eq!(b.x, 8.0);
}

#[test]
fn it_goes_with_what_moves_it() {
    let centered = show(
        "<table style='border-spacing:0'><tr><td style='padding:0'>a<br>b<br>c<td style='padding:0;vertical-align:bottom'>\
         <div style='position:relative'>d<div style='position:absolute;left:0;top:0'>e</div></div></table>",
    );
    assert_eq!(text(&centered, "e").y, text(&centered, "d").y);
    assert_eq!(text(&centered, "d").y, 32.0);
}

#[test]
fn what_is_positioned_draws_above_the_rest() {
    let layout = show(
        "<div style='position:relative'>first</div><p>second</p><div style='position:absolute;z-index:-1'>below</div>\
         <div style='position:absolute;z-index:5'>top</div><div style='position:absolute;z-index:2'>middle</div>",
    );
    assert_eq!(
        drawn_text(&layout),
        ["below", "second", "first", "middle", "top"]
    );
}

#[test]
fn hidden_overflow_cuts_off_what_goes_beyond() {
    // The usual way to keep text for screen readers only.
    let reader = show(
        "<h1 style='position:absolute;width:1px;height:1px;overflow:hidden;margin:0;padding:0'>Navigation Menu</h1><p>seen</p>",
    );
    assert_eq!(drawn_text(&reader), ["seen"]);
    let shut =
        show("<div style='height:16px;overflow:hidden'>one<br>two<br>three</div><p>after</p>");
    assert_eq!(drawn_text(&shut), ["one", "after"]);
    assert_eq!(text(&shut, "after").y, 16.0);
    // Going part of the way beyond it is cut when drawing.
    let part = show("<div style='height:24px;overflow:hidden'>one<br>two</div>");
    let cut: Vec<_> = part
        .visible_items(-1e6, 1e6)
        .into_iter()
        .filter(|visible| visible.item.kind == ItemKind::Text)
        .collect();
    assert_eq!(cut.len(), 2);
    assert_eq!(cut[1].clip.unwrap().height, 24.0);
    let visible = show("<div style='height:16px'>one<br>two</div>");
    assert_eq!(drawn_text(&visible), ["one", "two"]);
    // A box that scrolls has no way yet to reach the rest, which so stays visible.
    let scrolls = show("<div style='height:16px;overflow:auto'>one<br>two</div>");
    assert_eq!(drawn_text(&scrolls), ["one", "two"]);
}

#[test]
fn a_filled_box_is_cut_to_the_size_of_what_hides_it() {
    let layout = show(
        "<div style='width:50px;height:50px;overflow:hidden'><div style='width:100px;height:100px;background:red'></div></div>",
    );
    let shape = filled(&layout)[0];
    assert_eq!((shape.width, shape.height), (50.0, 50.0));
}

#[test]
fn what_is_cut_off_cannot_be_clicked() {
    let layout = show(
        "<div style='height:16px;overflow:hidden'><a href=one>one</a><br><a href=two>two</a></div>",
    );
    assert_eq!(layout.link_at(4.0, 8.0), Some("one"));
    assert_eq!(layout.link_at(4.0, 24.0), None);
}

#[test]
fn what_is_hidden_draws_nothing_and_keeps_its_place() {
    let layout = show(
        "<div style='visibility:hidden;background:red;border:1px solid blue;height:50px'>a</div><p>after</p>",
    );
    assert!(filled(&layout).is_empty());
    assert_eq!(drawn_text(&layout), ["after"]);
    assert_eq!(text(&layout, "after").y, 52.0);
    // The usual backdrop of a menu, which covers all once it shows.
    let backdrop = show(
        "<div style='position:fixed;inset:0;background:black;visibility:hidden'></div><p>seen</p>",
    );
    assert!(filled(&backdrop).is_empty());
    let image = show("<img src=a.png style='visibility:hidden'><p>after</p>");
    assert!(
        image
            .visible_items(-1e6, 1e6)
            .iter()
            .all(|visible| visible.item.kind != ItemKind::Image)
    );
    assert!(text(&image, "after").y >= 20.0);
    // What it holds can show again.
    let inside = show(
        "<div style='visibility:hidden;background:red'><p style='visibility:visible'>shown</p></div>",
    );
    assert!(filled(&inside).is_empty());
    assert_eq!(drawn_text(&inside), ["shown"]);
}

#[test]
fn opacity_lets_through_what_is_behind() {
    let layout = show("<div style='background:rgb(255,0,0);opacity:0.5'>a</div>");
    let shape = filled(&layout)[0];
    assert_eq!(shape.color.alpha, 128);
    let text_item = layout
        .visible_items(-1e6, 1e6)
        .into_iter()
        .map(|visible| visible.item)
        .find(|item| item.kind == ItemKind::Text)
        .unwrap();
    assert_eq!(text_item.color.alpha, 128);
    // All that is within it counts, also multiplied.
    let nested = show(
        "<div style='opacity:0.5'><div style='opacity:0.5;background:rgb(255,0,0)'>a</div></div>",
    );
    assert_eq!(filled(&nested)[0].color.alpha, 64);
    let none = show(
        "<div style='position:fixed;inset:0;background:black;opacity:0'><p>a</p><img src=a.png></div><p>seen</p>",
    );
    assert!(filled(&none).is_empty());
    assert_eq!(drawn_text(&none), ["seen"]);
    let percent = show("<div style='background:rgb(255,0,0);opacity:50%'>a</div>");
    assert_eq!(filled(&percent)[0].color.alpha, 128);
}

#[test]
fn inset_sets_all_sides() {
    let layout = show(
        "<div style='position:relative;width:200px;height:200px'>\
         <div style='position:absolute;inset:10px 20px;background:red'></div></div>",
    );
    let shape = filled(&layout)[0];
    assert_eq!((shape.x, shape.y), (20.0, 10.0));
    assert_eq!((shape.width, shape.height), (160.0, 180.0));
}

#[test]
fn a_transform_moves_a_box_and_leaves_its_place() {
    let layout = show(
        "<div style='transform:translate(10px, 20px);width:100px;height:50px'>a</div><p>after</p>",
    );
    let a = text(&layout, "a");
    assert_eq!((a.x, a.y), (10.0, 20.0));
    assert_eq!(text(&layout, "after").y, 50.0);
    let across = show("<div style='transform:translateX(30px)'>a</div>");
    assert_eq!(text(&across, "a").x, 30.0);
    let down = show("<div style='transform:translateY(30px)'>a</div>");
    assert_eq!(text(&down, "a").y, 30.0);
    let matrix = show("<div style='transform:matrix(1, 0, 0, 1, 7, 8)'>a</div>");
    assert_eq!((text(&matrix, "a").x, text(&matrix, "a").y), (7.0, 8.0));
    let both = show("<div style='transform:translateX(5px) translateX(6px) rotate(1deg)'>a</div>");
    assert_eq!(text(&both, "a").x, 11.0);
}

#[test]
fn percentages_of_a_transform_are_of_the_box_itself() {
    let centered = show(
        "<div style='position:relative;width:400px;height:400px'>\
         <div style='position:absolute;left:50%;top:50%;width:100px;height:60px;\
         transform:translate(-50%, -50%);background:red'></div></div>",
    );
    let shape = filled(&centered)[0];
    assert_eq!((shape.x, shape.y), (150.0, 170.0));
    let mixed = show(
        "<div style='width:100px;background:red;transform:translateX(calc(50% + 10px))'>x</div>",
    );
    assert_eq!(filled(&mixed)[0].x, 60.0);
}

#[test]
fn a_menu_pushed_off_the_page_covers_nothing() {
    let layout = show(
        "<div style='position:fixed;top:0;left:0;bottom:0;width:320px;background:white;\
         transform:translateX(-100%)'>menu</div><p>seen</p>",
    );
    let shape = filled(&layout)[0];
    assert_eq!(shape.x + shape.width, 0.0);
    assert_eq!(text(&layout, "menu").x, -320.0);
    assert_eq!(layout.width, 800.0);
}

#[test]
fn a_transform_counts_as_positioned_for_what_is_inside_and_on_top() {
    let inside = show(
        "<div style='margin-left:50px;transform:translateX(0)'>\
         <div style='position:absolute;left:5px;top:5px'>a</div></div>",
    );
    assert_eq!(text(&inside, "a").x, 55.0);
    let on_top = show("<div style='transform:scale(2)'>first</div><p>second</p>");
    assert_eq!(drawn_text(&on_top), ["second", "first"]);
    let none = show("<div style='transform:none'>first</div><p>second</p>");
    assert_eq!(drawn_text(&none), ["first", "second"]);
}

#[test]
fn a_transform_moves_items_of_flexible_boxes_and_links_too() {
    let layout = show(
        "<div style='display:flex'><a href=there style='transform:translate(100px,100px);background:red'>x</a><div>b</div></div>",
    );
    let x = text(&layout, "x");
    assert_eq!((x.x, x.y), (100.0, 100.0));
    assert_eq!(filled(&layout)[0].x, 100.0);
    assert_eq!(text(&layout, "b").x, 8.0);
    assert_eq!(layout.link_at(104.0, 108.0), Some("there"));
}

#[test]
fn nonsense_in_a_transform_is_dropped() {
    for value in [
        "rubbish",
        "translate",
        "translate(",
        "translate(1foo)",
        "translate(1px) x",
    ] {
        let layout = show(&format!(
            "<style>div{{transform:translate(9px);transform:{value}}}</style><div>a</div>"
        ));
        assert_eq!(text(&layout, "a").x, 9.0, "{value}");
    }
}

#[test]
fn what_is_closed_shows_only_its_summary() {
    let closed = show("<details><summary>menu</summary><ul><li>inside</ul></details>");
    assert_eq!(drawn_text(&closed), ["menu"]);
    let open = show("<details open><summary>menu</summary><p>inside</p></details>");
    assert_eq!(drawn_text(&open), ["menu", "inside"]);
    let dialog = show("<dialog><p>closed</p></dialog><dialog open><p>shown</p></dialog>");
    assert_eq!(drawn_text(&dialog), ["shown"]);
}

#[test]
fn links_go_with_a_box() {
    let layout =
        show("<div style='position:absolute;left:100px;top:100px'><a href=there>x</a></div>");
    assert_eq!(layout.link_at(104.0, 108.0), Some("there"));
    assert_eq!(layout.link_at(4.0, 8.0), None);
    let relative = show("<div style='position:relative;left:100px'><a href=there>x</a></div>");
    assert_eq!(relative.link_at(104.0, 8.0), Some("there"));
    assert_eq!(relative.link_at(4.0, 8.0), None);
}

#[test]
fn the_page_can_be_scrolled_to_what_lies_low() {
    let layout = show("<p>a</p><div style='position:absolute;top:5000px'>low</div>");
    assert_eq!(layout.height, 5016.0);
}

#[test]
fn a_box_going_off_the_page_takes_no_room_there() {
    let layout = show("<div style='position:absolute;left:-9999px'>away</div><p>a</p>");
    assert_eq!(layout.width, 800.0);
    assert_eq!(text(&layout, "a").y, 0.0);
}

#[test]
fn it_shows_the_same_when_it_arrives_in_pieces() {
    let source = "<!DOCTYPE html><style>body{margin:0}p{margin:0}</style>\
        <div style='position:relative;height:100px'><div style='position:absolute;right:0;bottom:0'>corner</div>\
        <p>in it</p></div><p>after</p><div style='position:absolute;top:300px'>low</div><p>end</p>";
    let viewport = ron_browser_core::Viewport {
        width: 800.0,
        height: 600.0,
    };
    let whole = show_whole(source);
    for size in [1, 7, 40] {
        let mut page = Page::open_arriving(viewport);
        let mut measurer = Measurer::default();
        for piece in source.as_bytes().chunks(size) {
            page.feed(
                std::str::from_utf8(piece).unwrap(),
                &mut NoResources,
                &mut measurer,
            );
        }
        page.finish_arriving(&mut NoResources, &mut measurer);
        assert_eq!(page.layout().text, whole.text);
        for wanted in ["corner", "in it", "after", "low", "end"] {
            assert_eq!(
                text(page.layout(), wanted).y,
                text(&whole, wanted).y,
                "{wanted}"
            );
        }
    }
}

fn show_whole(source: &str) -> Layout {
    let mut page = Page::open_with(source, &mut NoResources, ImageStore::default());
    page.lay_out(800.0, 600.0, &mut Measurer::default());
    page.layout().clone()
}

#[test]
fn hostile_boxes_do_no_harm() {
    let deep = format!(
        "{}deep{}",
        "<div style='position:relative;overflow:hidden;z-index:1'>".repeat(3000),
        "</div>".repeat(3000)
    );
    assert_eq!(drawn_text(&show(&deep)), ["deep"]);
    let absolutes = format!(
        "{}deep{}",
        "<div style='position:absolute;left:1px;top:1px'>".repeat(3000),
        "</div>".repeat(3000)
    );
    assert_eq!(text(&show(&absolutes), "deep").height, 16.0);
    show(
        "<div style='position:absolute;top:1e30px;left:-1e30px;width:1e30px;height:1e30px;z-index:2147483647'>x</div>",
    );
    show(
        "<div style='position:absolute'></div><div style='position:relative;top:50%;left:50%'></div>",
    );
}
