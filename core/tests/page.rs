//! Reading the title and visible text of a page.

use ron_browser_core::images::NoResources;
use ron_browser_core::layout::{FontSpec, TextMeasurer};
use ron_browser_core::page::Page;

fn text(source: &str) -> String {
    Page::open(source).plain_text()
}

struct Measurer;

impl TextMeasurer for Measurer {
    fn font(&mut self, _spec: &FontSpec) -> u32 {
        0
    }
    fn width(&mut self, _font: u32, text: &str) -> f32 {
        8.0 * text.chars().count() as f32
    }
    fn ascent(&mut self, _font: u32) -> f32 {
        13.0
    }
    fn descent(&mut self, _font: u32) -> f32 {
        3.0
    }
}

/// Over the size from which a source gets packed.
fn large_page() -> String {
    let mut source =
        "<!DOCTYPE html><title>  The   title </title><body style='margin:0'><p id=first>first"
            .to_owned();
    for number in 0..10_000 {
        source.push_str(&format!(
            "<p id=place{number} style='margin:0'><a href=to{number}>line {number}</a>"
        ));
    }
    source
}

#[test]
fn a_packed_source_comes_back_whole_when_needed() {
    let mut page = Page::open(&large_page());
    page.lay_out(800.0, 600.0, &mut Measurer);
    let before = page.layout().clone();
    let place = page.fragment_y("place5000");
    assert!(place.is_some());
    let all_text = page.plain_text();
    page.pack_source();
    assert!(page.is_source_packed());
    // What is on the screen does not need it.
    assert_eq!(page.title(), "The title");
    assert_eq!(*page.layout(), before);
    assert!(page.layout().link_at(5.0, 60.0).is_some());
    // Anything looking at the tree brings it back, with the same result.
    page.lay_out(800.0, 600.0, &mut Measurer);
    assert!(!page.is_source_packed());
    assert_eq!(*page.layout(), before);
    assert_eq!(page.title(), "The title");
    assert_eq!(page.fragment_y("place5000"), place);
    assert_eq!(page.plain_text(), all_text);
}

#[test]
fn styles_can_be_worked_out_again_from_a_packed_source() {
    let mut page = Page::open(&large_page());
    page.lay_out(800.0, 600.0, &mut Measurer);
    page.pack_source();
    // Another size asks for styles again too.
    page.lay_out(400.0, 600.0, &mut Measurer);
    assert!(!page.is_source_packed());
    let mut same = Page::open(&large_page());
    same.lay_out(400.0, 600.0, &mut Measurer);
    assert_eq!(*page.layout(), *same.layout());
}

#[test]
fn only_a_large_page_that_has_arrived_gets_packed() {
    let mut small = Page::open("<title>small</title><p>text");
    small.pack_source();
    assert!(!small.is_source_packed());
    let viewport = ron_browser_core::Viewport {
        width: 800.0,
        height: 600.0,
    };
    let mut arriving = Page::open_arriving(viewport);
    arriving.feed(&large_page(), &mut NoResources, &mut Measurer);
    arriving.pack_source();
    assert!(!arriving.is_source_packed());
    arriving.finish_arriving(&mut NoResources, &mut Measurer);
    arriving.pack_source();
    assert!(arriving.is_source_packed());
    assert_eq!(arriving.title(), "The title");
}

#[test]
fn reads_the_title() {
    assert_eq!(
        Page::open("<title>  a\n b &amp; c </title>").title(),
        "a b & c"
    );
    assert_eq!(Page::open("<title>a</title><title>b</title>").title(), "a");
    assert_eq!(Page::open("<p>x").title(), "");
}

#[test]
fn blocks_take_their_own_lines_and_inline_text_stays_together() {
    assert_eq!(text("<p>a</p><p>b</p>"), "a\n\nb");
    assert_eq!(text("a<b>b</b><i>c</i> d"), "abc d");
    assert_eq!(text("a<div>b</div>c"), "a\nb\nc");
    assert_eq!(text("a<br>b"), "a\nb");
    assert_eq!(text("<ul><li>a<li>b</ul>"), "a\nb");
}

#[test]
fn white_space_collapses_except_where_it_counts() {
    assert_eq!(text("  a \n\t b  "), "a b");
    assert_eq!(text("<p> a </p> <p> b </p>"), "a\n\nb");
    assert_eq!(text("a <b> b</b>"), "a b");
    assert_eq!(text("<pre>a  b\n c</pre>"), "a  b\n c");
}

#[test]
fn what_is_hidden_does_not_show() {
    assert_eq!(
        text("<title>t</title><style>p{}</style><script>s</script>x"),
        "x"
    );
    assert_eq!(
        text("<!DOCTYPE html><style>.h{display:none}</style><p class=h>a<p>b"),
        "b"
    );
    assert_eq!(text("<p hidden>a<b>c</b></p>b"), "b");
    assert_eq!(text("<!-- c -->a"), "a");
    assert_eq!(text("<p style='visibility:hidden'>a</p>b"), "b");
}

#[test]
fn character_references_are_decoded() {
    assert_eq!(text("&#169; &amp; &lt;b&gt;"), "\u{a9} & <b>");
}

#[test]
fn deep_and_empty_pages_do_no_harm() {
    assert_eq!(text(""), "");
    assert_eq!(text(&format!("{}x", "<div>".repeat(5000))), "x");
}
