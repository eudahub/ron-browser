//! Pages that arrive in pieces. Characters are 8 wide and lines 16 high, as in the layout tests.

use std::collections::HashMap;

use ron_browser_core::Viewport;
use ron_browser_core::images::{ImageStore, NoResources, ResourceLoader};
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

const VIEW: Viewport = Viewport {
    width: 500.0,
    height: 400.0,
};

/// Files by name.
#[derive(Default)]
struct Files(HashMap<&'static str, Vec<u8>>);

impl ResourceLoader for Files {
    fn load(&mut self, url: &str) -> Option<Vec<u8>> {
        self.0.get(url).cloned()
    }
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([1, 2, 3, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn files() -> Files {
    let mut files = Files::default();
    files.0.insert("a.css", b"p{color:red;margin:0}".to_vec());
    files.0.insert("a.png", png(40, 20));
    files
}

/// Everything about a layout that shows, as text, to compare two of them.
fn described(layout: &Layout) -> String {
    let mut out = format!("{} {} {:?}\n", layout.width, layout.height, layout.text);
    for item in &layout.items {
        out += &format!(
            "{:?} {} {} {} {} {} {:?} {} {}\n",
            item.kind,
            item.x,
            item.y,
            item.width,
            item.height,
            item.baseline,
            item.color,
            item.start,
            item.end
        );
    }
    out
}

fn whole(source: &str) -> String {
    let mut page = Page::open_sized(source, &mut files(), ImageStore::default(), VIEW);
    page.lay_out(VIEW.width, VIEW.height, &mut Measurer::default());
    described(page.layout())
}

/// Feeds a page in pieces of this many characters, and checks it after each.
fn in_pieces(source: &str, size: usize) -> Page {
    let mut measurer = Measurer::default();
    let mut loader = files();
    let mut page = Page::open_arriving(VIEW);
    let characters: Vec<char> = source.chars().collect();
    let mut height = 0.0;
    for piece in characters.chunks(size) {
        let piece: String = piece.iter().collect();
        page.feed(&piece, &mut loader, &mut measurer);
        // It can be shown at any moment, and the same as it is.
        let layout = page.layout();
        assert!(
            layout.height >= height || source.contains("stylesheet") || source.contains("<style")
        );
        height = layout.height;
        assert!(layout.height.is_finite());
    }
    page.finish_arriving(&mut loader, &mut measurer);
    assert!(!page.is_arriving());
    page
}

const PAGES: [&str; 9] = [
    "<!DOCTYPE html><title>t</title><p>one</p><p>two <b>three</b> four</p><ul><li>a<li>b</ul>",
    "<!DOCTYPE html><style>p{color:red}p:last-child{color:blue}</style><div><p>a</p><p>b</p></div>",
    "<!DOCTYPE html><link rel=stylesheet href=a.css><p>x</p><p>y</p>",
    "<!DOCTYPE html><img src=a.png><p>text</p><img src=a.png style='float:left'>around it",
    "<!DOCTYPE html><span style='display:inline-block;background:red'>a<div>b</div></span>c<p>d",
    "<!DOCTYPE html><div style='background:red;padding:5px'><div style='float:right'>f</div>x</div>",
    "<!DOCTYPE html><p>unclosed <b>bold <i>both</b> italic</i> rest<p>next<table><tr><td>cell</table>",
    "plain text with no tags at all, in quirks mode<br>and a line",
    "<!DOCTYPE html><body background=a.png><a href=x.htm name=n id=i>link</a><pre>a\n  b\n</pre>end",
];

#[test]
fn in_the_end_a_page_is_the_same_however_it_came() {
    for source in PAGES {
        let expected = whole(source);
        for size in [1, 2, 7, 64, 100_000] {
            let page = in_pieces(source, size);
            assert_eq!(
                described(page.layout()),
                expected,
                "pieces of {size}: {source}"
            );
        }
    }
}

#[test]
fn a_page_shows_as_it_comes_and_only_grows() {
    let source: String = std::iter::once("<!DOCTYPE html>".to_owned())
        .chain((0..100).map(|number| format!("<p>line {number}</p>\n")))
        .collect();
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    let mut heights = Vec::new();
    let mut first_lines = None;
    for piece in source.as_bytes().chunks(300) {
        page.feed(
            std::str::from_utf8(piece).unwrap(),
            &mut NoResources,
            &mut measurer,
        );
        let layout = page.layout();
        heights.push(layout.height);
        if layout.lines.len() > 5 {
            let lines: Vec<f32> = layout.lines[..5].iter().map(|line| line.top).collect();
            // What has been laid out does not move.
            assert_eq!(*first_lines.get_or_insert_with(|| lines.clone()), lines);
        }
    }
    assert!(
        heights.windows(2).all(|pair| pair[0] <= pair[1]),
        "{heights:?}"
    );
    assert!(heights.first() < heights.last());
    assert!(heights[0] > 0.0, "shows from the first piece");
    let before = *heights.last().unwrap();
    page.finish_arriving(&mut NoResources, &mut measurer);
    assert!(page.layout().height >= before);
}

#[test]
fn what_is_not_known_yet_waits() {
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    page.feed(
        "<!DOCTYPE html><p>first</p><p>part of",
        &mut NoResources,
        &mut measurer,
    );
    // The text at the end can still grow, and its line could still wrap.
    assert_eq!(page.layout().text, "first");
    page.feed(" it</p><p>more", &mut NoResources, &mut measurer);
    assert_eq!(page.layout().text, "first\n\npart of it");
    page.finish_arriving(&mut NoResources, &mut measurer);
    assert_eq!(page.layout().text, "first\n\npart of it\n\nmore");
}

#[test]
fn an_open_block_paints_as_far_as_has_come() {
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    page.feed(
        "<!DOCTYPE html><style>body{margin:0}p{margin:0}</style><div style='background:red'><p>a</p><p>b</p>",
        &mut NoResources,
        &mut measurer,
    );
    let height_of_block = |page: &Page| {
        page.layout()
            .items
            .iter()
            .find(|item| item.kind == ItemKind::Rect)
            .map(|item| item.height)
    };
    assert_eq!(height_of_block(&page), Some(32.0));
    page.feed("<p>c</p>", &mut NoResources, &mut measurer);
    assert_eq!(height_of_block(&page), Some(48.0));
    page.feed("</div><p>after</p>", &mut NoResources, &mut measurer);
    assert_eq!(height_of_block(&page), Some(48.0));
}

#[test]
fn an_inline_block_waiting_for_its_line_is_not_drawn_yet() {
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    page.feed(
        "<!DOCTYPE html><style>body{margin:0}</style><p style='margin:0'>a<a href=x style='display:inline-block'>box</a>",
        &mut NoResources,
        &mut measurer,
    );
    let layout = page.layout();
    assert!(layout.unplaced_items.is_some());
    assert_eq!(layout.link_at(5.0, 5.0), None);
    page.feed("</p><p>b</p>", &mut NoResources, &mut measurer);
    let layout = page.layout();
    assert_eq!(layout.unplaced_items, None);
    assert_eq!(layout.link_at(10.0, 8.0), Some("x"));
}

#[test]
fn style_sheets_that_come_late_count_for_all() {
    let mut measurer = Measurer::default();
    let mut loader = files();
    let mut page = Page::open_arriving(VIEW);
    page.feed(
        "<!DOCTYPE html><p>a</p><p>b</p>",
        &mut loader,
        &mut measurer,
    );
    let black = page
        .layout()
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Text)
        .unwrap()
        .color;
    page.feed(
        "<link rel=stylesheet href=a.css><p>c</p><p>d</p>",
        &mut loader,
        &mut measurer,
    );
    let red = page
        .layout()
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Text)
        .unwrap()
        .color;
    assert_ne!(black, red);
    assert_eq!(red.red, 255);
    page.finish_arriving(&mut loader, &mut measurer);
}

#[test]
fn a_style_sheet_being_written_waits_until_it_is_whole() {
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    page.feed(
        "<!DOCTYPE html><style>p{col",
        &mut NoResources,
        &mut measurer,
    );
    page.feed(
        "or:red}</style><p>x</p><p>y",
        &mut NoResources,
        &mut measurer,
    );
    let text = page
        .layout()
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Text)
        .unwrap();
    assert_eq!(text.color.red, 255);
}

#[test]
fn a_new_size_while_arriving_lays_out_again() {
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    page.feed(
        "<!DOCTYPE html><p>aaaa bbbb cccc dddd</p><p>x",
        &mut NoResources,
        &mut measurer,
    );
    let lines = page.layout().lines.len();
    page.lay_out(50.0, 400.0, &mut measurer);
    assert!(page.layout().lines.len() > lines);
    page.feed("</p><p>y</p>", &mut NoResources, &mut measurer);
    assert!(page.layout().text.contains('x'));
    page.finish_arriving(&mut NoResources, &mut measurer);
    assert!(page.layout().text.ends_with('y'));
}

#[test]
fn a_page_can_be_asked_about_while_arriving() {
    let mut measurer = Measurer::default();
    let mut page = Page::open_arriving(VIEW);
    assert_eq!(page.title(), "");
    assert_eq!(page.plain_text(), "");
    page.feed(
        "<!DOCTYPE html><title>Name</title><p id=here>one</p><p>tw",
        &mut NoResources,
        &mut measurer,
    );
    assert_eq!(page.title(), "Name");
    assert_eq!(page.fragment_y("here"), Some(16.0));
    assert!(page.plain_text().contains("one"));
}

#[test]
fn ending_at_any_moment_does_no_harm() {
    let source = PAGES.join("");
    for end in (0..source.len()).step_by(11) {
        if source.is_char_boundary(end) {
            let mut measurer = Measurer::default();
            let mut page = Page::open_arriving(VIEW);
            page.feed(&source[..end], &mut NoResources, &mut measurer);
            page.finish_arriving(&mut NoResources, &mut measurer);
        }
    }
}
