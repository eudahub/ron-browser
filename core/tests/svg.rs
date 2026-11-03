//! SVG in pages. Characters are 8 wide and lines 16 high, as in the layout tests.

use ron_browser_core::images::{DecodedImage, ImageStore, NoResources, ResourceLoader, decode};
use ron_browser_core::layout::{FontSpec, Item, ItemKind, Layout, TextMeasurer};
use ron_browser_core::page::Page;
use ron_browser_core::svg::render;

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

fn opened(body: &str, loader: &mut dyn ResourceLoader) -> Page {
    let source = format!("<!DOCTYPE html><style>body{{margin:0}}</style>{body}");
    let mut page = Page::open_with(&source, loader, ImageStore::default());
    page.lay_out(800.0, 600.0, &mut Measurer::default());
    page
}

/// The numbers of the images that a page draws, in order.
fn drawn(page: &Page) -> Vec<u32> {
    page.layout()
        .items
        .iter()
        .filter(|item| item.kind == ItemKind::Image)
        .map(|item| item.start)
        .collect()
}

fn only_image(body: &str) -> DecodedImage {
    let page = opened(body, &mut NoResources);
    let numbers = drawn(&page);
    assert_eq!(numbers.len(), 1, "{body}");
    page.images().get(numbers[0]).unwrap().clone()
}

fn pixel(image: &DecodedImage, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.width + x) * 4) as usize;
    image.rgba[at..at + 4].try_into().unwrap()
}

/// A red pixel, which would show if it got through.
const PNG_PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQV\
                         R42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

#[test]
fn shapes_are_drawn_at_the_size_it_has() {
    let image =
        only_image("<svg width=10 height=10><rect x=5 width=5 height=10 fill=#2060d0 /></svg>");
    assert_eq!((image.width, image.height), (10, 10));
    assert_eq!(pixel(&image, 7, 5), [0x20, 0x60, 0xd0, 255]);
    assert_eq!(pixel(&image, 2, 5)[3], 0);
    let by_css = only_image(
        "<svg viewBox='0 0 24 24' style='width:96px;height:48px'>\
         <rect width=24 height=24 fill=red /></svg>",
    );
    assert_eq!((by_css.width, by_css.height), (96, 48));
}

#[test]
fn current_color_comes_from_the_style() {
    let image = only_image(
        "<p style='color:#c00000'><svg width=4 height=4 fill=currentColor>\
         <rect width=4 height=4 /></svg></p>",
    );
    assert_eq!(pixel(&image, 2, 2), [0xc0, 0, 0, 255]);
}

#[test]
fn fill_comes_from_style_sheets() {
    let shape = "<rect width=4 height=4 /></svg>";
    let on_the_svg = only_image(&format!(
        "<style>.icon{{fill:currentColor;color:#c00000}}</style>\
         <svg class=icon width=4 height=4>{shape}"
    ));
    assert_eq!(pixel(&on_the_svg, 2, 2), [0xc0, 0, 0, 255]);
    let within = only_image(
        "<style>svg rect{fill:#00a000}</style><svg width=4 height=4><rect width=4 height=4 /></svg>",
    );
    assert_eq!(pixel(&within, 2, 2), [0, 0xa0, 0, 255]);
    let none = only_image(&format!(
        "<style>svg{{fill:none}}</style><svg width=4 height=4>{shape}"
    ));
    assert_eq!(pixel(&none, 2, 2)[3], 0);
}

#[test]
fn fill_of_a_style_sheet_beats_only_the_attribute_on_the_same_element() {
    let beaten = only_image(
        "<style>rect{fill:#00a000}</style>\
         <svg width=4 height=4><rect width=4 height=4 fill=red /></svg>",
    );
    assert_eq!(pixel(&beaten, 2, 2), [0, 0xa0, 0, 255]);
    let own_stays = only_image(
        "<style>svg{fill:#00a000}</style>\
         <svg width=4 height=4><rect width=4 height=4 fill=#ff0000 /></svg>",
    );
    assert_eq!(pixel(&own_stays, 2, 2), [255, 0, 0, 255]);
}

#[test]
fn fill_can_show_through() {
    let image = only_image(
        "<style>rect{fill:rgba(0,160,0,.5)}</style>\
         <svg width=4 height=4><rect width=4 height=4 fill-opacity=1 /></svg>",
    );
    let [_, green, _, alpha] = pixel(&image, 2, 2);
    assert!(green.abs_diff(0xa0) <= 2, "{green}");
    assert!(alpha.abs_diff(128) <= 2, "{alpha}");
}

#[test]
fn what_shows_through_keeps_its_color() {
    let image = only_image(
        "<svg width=4 height=4><rect width=4 height=4 fill=#00a000 fill-opacity=0.5 /></svg>",
    );
    let [red, green, blue, alpha] = pixel(&image, 2, 2);
    assert_eq!((red, blue), (0, 0));
    assert!(green.abs_diff(0xa0) <= 2, "{green}");
    assert!(alpha.abs_diff(128) <= 1, "{alpha}");
}

#[test]
fn equal_ones_are_drawn_once() {
    let icon = "<svg width=8 height=8><rect width=8 height=8 /></svg>";
    let same = opened(&format!("{icon}{icon}{icon}"), &mut NoResources);
    let numbers = drawn(&same);
    assert_eq!(numbers.len(), 3);
    assert!(numbers.iter().all(|&number| number == numbers[0]));
    let other = opened(
        &format!("{icon}<svg width=9 height=9><rect width=8 height=8 /></svg>"),
        &mut NoResources,
    );
    let numbers = drawn(&other);
    assert_ne!(numbers[0], numbers[1]);
}

#[test]
fn what_xml_does_not_take_does_not_lose_the_picture() {
    let image = only_image(
        "<svg xmlns='http://www.w3.org/2000/svg' xmlns:x='y' @click=a 1a=b width=4 width=9 height=4 \
         color=blue><x:editor></x:editor><rect x:a=1 width=4 height=4 fill=red fill=blue /></svg>",
    );
    assert_eq!(image.width, 4);
    assert_eq!(pixel(&image, 2, 2), [255, 0, 0, 255]);
}

struct LoadsNothing;

impl ResourceLoader for LoadsNothing {
    fn load(&mut self, url: &str) -> Option<Vec<u8>> {
        panic!("asked for {url}");
    }
}

#[test]
fn it_refers_to_nothing() {
    let image = only_image(&format!(
        "<svg width=4 height=4><image href='{PNG_PIXEL}' width=4 height=4 /></svg>"
    ));
    assert_eq!(pixel(&image, 2, 2)[3], 0);
    let page = opened(
        "<svg width=4 height=4><image href=a.png width=4 height=4 />\
         <image xlink:href=/etc/hostname width=4 height=4 />\
         <rect width=4 height=4 style='fill:url(http://127.0.0.1:9/a)' /></svg>",
        &mut LoadsNothing,
    );
    assert_eq!(drawn(&page).len(), 1);
}

#[test]
fn text_in_it_does_not_show() {
    let image = only_image("<svg width=99 height=99><text y=50 font-size=40>In it</text></svg>");
    assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 0));
}

#[test]
fn a_file_is_drawn_at_the_size_it_says() {
    let bytes = b"<?xml version='1.0'?>\n<svg xmlns='http://www.w3.org/2000/svg' width='6' \
                  height='3'><rect width='6' height='3' fill='#884400'/></svg>";
    let image = decode(bytes).unwrap();
    assert_eq!((image.width, image.height), (6, 3));
    assert_eq!(pixel(&image, 3, 1), [0x88, 0x44, 0, 255]);
}

#[test]
fn what_is_no_picture_gives_nothing() {
    assert_eq!(decode(b"just some text"), None);
    assert_eq!(decode(b"<svg"), None);
    assert_eq!(decode(b"<html><svg></html>"), None);
}

#[test]
fn sizes_stay_within_the_limits() {
    let small = "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'></svg>";
    assert!(render(small, 4.0, 4.0).is_some());
    let huge = render(small, 100_000.0, 50_000.0).unwrap();
    assert_eq!((huge.width, huge.height), (4096, 2048));
    assert_eq!(huge.rgba.len(), 4096 * 2048 * 4);
    for (width, height) in [
        (0.0, 4.0),
        (4.0, -1.0),
        (f32::NAN, 4.0),
        (f32::INFINITY, 4.0),
    ] {
        assert_eq!(render(small, width, height), None);
    }
    let long = format!("{small}{}", " ".repeat(1024 * 1024));
    assert_eq!(render(&long, 4.0, 4.0), None);
    assert_eq!(decode(long.as_bytes()), None);
}
