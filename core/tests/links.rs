//! Links: where they lead, where they can be clicked, and the history. Characters are 8 wide and
//! lines 16 high, as in the layout tests.

use std::fs;
use std::path::{Path, PathBuf};

use ron_browser_core::history::History;
use ron_browser_core::images::{DecodedImage, ImageStore, NoResources};
use ron_browser_core::layout::{FontSpec, TextMeasurer};
use ron_browser_core::links::{Target, resolve};
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

fn page(body: &str, width: f32) -> Page {
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
    page
}

/// A directory for one test, removed afterwards.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ron-links-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("site/sub")).unwrap();
        fs::create_dir_all(path.join("site/empty")).unwrap();
        for file in [
            "other.htm",
            "site/page.html",
            "site/next.HTM",
            "site/with space.html",
            "site/photo.jpg",
            "site/sub/index.html",
        ] {
            fs::write(path.join(file), "x").unwrap();
        }
        Self(path.canonicalize().unwrap())
    }
    fn page(&self) -> PathBuf {
        self.0.join("site/page.html")
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn opens(path: PathBuf, fragment: &str) -> Target {
    Target::Page {
        path,
        fragment: fragment.to_owned(),
    }
}

#[test]
fn links_lead_to_local_pages() {
    let dir = TempDir::new("pages");
    let page = dir.page();
    for (href, file, fragment) in [
        ("next.HTM", "site/next.HTM", ""),
        ("./next.HTM", "site/next.HTM", ""),
        (" next.HTM\n", "site/next.HTM", ""),
        ("next.HTM#part", "site/next.HTM", "part"),
        ("next.HTM?a=1#part", "site/next.HTM", "part"),
        ("with%20space.html", "site/with space.html", ""),
        ("next.HTM#z%20y", "site/next.HTM", "z y"),
        ("../other.htm", "other.htm", ""),
        ("..\\other.htm", "other.htm", ""),
        ("sub/", "site/sub/index.html", ""),
        ("sub", "site/sub/index.html", ""),
    ] {
        assert_eq!(
            resolve(&page, href),
            opens(dir.file(file), fragment),
            "{href}"
        );
    }
    let absolute = format!("file://{}", dir.file("other.htm").display());
    assert_eq!(resolve(&page, &absolute), opens(dir.file("other.htm"), ""));
}

#[test]
fn a_link_within_the_page_stays_there() {
    let page = Path::new("/nowhere/page.html");
    for (href, fragment) in [("#part", "part"), ("#", ""), ("", ""), ("?a=1", "")] {
        assert_eq!(
            resolve(page, href),
            Target::SamePage {
                fragment: fragment.to_owned()
            },
            "{href}"
        );
    }
}

#[test]
fn only_pages_open() {
    let dir = TempDir::new("others");
    let page = dir.page();
    assert_eq!(
        resolve(&page, "photo.jpg"),
        Target::NotAPage(dir.file("site/photo.jpg"))
    );
    assert_eq!(
        resolve(&page, "empty"),
        Target::NotAPage(dir.file("site/empty"))
    );
    assert_eq!(
        resolve(&page, "/etc/passwd"),
        Target::NotAPage("/etc/passwd".into())
    );
    assert_eq!(
        resolve(&page, "file:///etc/passwd"),
        Target::NotAPage("/etc/passwd".into())
    );
    assert_eq!(resolve(&page, "missing.html"), Target::Missing);
    assert_eq!(resolve(&page, "next.HTM%00.txt"), Target::Missing);
}

#[test]
fn what_is_elsewhere_is_only_named() {
    let page = Path::new("/nowhere/page.html");
    for href in [
        "http://example.com/a",
        "HTTPS://example.com/",
        "ftp://example.com/",
        "mailto:a@example.com",
        "//example.com/a",
    ] {
        assert_eq!(resolve(page, href), Target::External(href.to_owned()));
    }
}

#[test]
fn dangerous_schemes_are_not_followed() {
    let page = Path::new("/nowhere/page.html");
    for href in [
        "javascript:alert(1)",
        " JaVaScRiPt:alert(1)",
        "java\nscript:alert(1)",
        "data:text/html,<b>x</b>",
        "vbscript:x",
        "about:blank",
        "chrome://settings",
        "file://example.com/etc/passwd",
        "c:/windows/a.htm",
    ] {
        assert_eq!(resolve(page, href), Target::Refused, "{href}");
    }
}

#[test]
fn text_and_images_in_a_link_can_be_clicked() {
    let layout_page = page(
        "aa <a href=x.htm>bbb</a> cc<br><a href=y.htm><img src=a.png></a>",
        500.0,
    );
    let layout = layout_page.layout();
    assert_eq!(layout.link_at(10.0, 8.0), None);
    assert_eq!(layout.link_at(30.0, 8.0), Some("x.htm"));
    assert_eq!(layout.link_at(47.0, 8.0), Some("x.htm"));
    assert_eq!(layout.link_at(60.0, 8.0), None);
    assert_eq!(layout.link_at(20.0, 26.0), Some("y.htm"));
    assert_eq!(layout.link_at(60.0, 26.0), None);
}

#[test]
fn images_outside_the_line_can_be_clicked_too() {
    let floating = page(
        "<a href=f.htm><img src=a.png style='float:right'></a>",
        200.0,
    );
    assert_eq!(floating.layout().link_at(170.0, 10.0), Some("f.htm"));
    assert_eq!(floating.layout().link_at(10.0, 10.0), None);
    let block = page(
        "<a href=b.htm><img src=a.png style='display:block'></a>",
        200.0,
    );
    assert_eq!(block.layout().link_at(10.0, 10.0), Some("b.htm"));
    assert_eq!(block.layout().link_at(50.0, 10.0), None);
}

#[test]
fn a_link_ends_with_its_element_and_covers_what_wraps() {
    let wrapped = page("<a href=x.htm>aaaa bbbb</a> cccc", 50.0);
    let layout = wrapped.layout();
    assert_eq!(layout.link_at(10.0, 8.0), Some("x.htm"));
    assert_eq!(layout.link_at(10.0, 24.0), Some("x.htm"));
    assert_eq!(layout.link_at(10.0, 40.0), None);
    let without_address = page("<a name=x>aaaa</a>", 100.0);
    assert_eq!(without_address.layout().link_at(10.0, 8.0), None);
}

#[test]
fn parts_of_an_image_map_lead_where_they_go() {
    let shown = page(
        "<a href=around.htm><img src=a.png usemap=#m></a>\
         <map name=m>\
         <area shape=rect coords='0,0,10,10' href=first.htm>\
         <area shape=rect coords='0,0,20,10' href=second.htm>\
         <area shape=circle coords='30,10,5' href=round.htm>\
         <area shape=poly coords='10,12 20,12 20,20' href=three.htm>\
         <area shape=rect coords='0,12,4,20' nohref>\
         </map>",
        500.0,
    );
    let layout = shown.layout();
    // The first that covers a point counts.
    assert_eq!(layout.link_at(5.0, 5.0), Some("first.htm"));
    assert_eq!(layout.link_at(15.0, 5.0), Some("second.htm"));
    assert_eq!(layout.link_at(30.0, 10.0), Some("round.htm"));
    // In the corner of the circle's box, but not in the circle.
    assert_eq!(layout.link_at(26.0, 6.0), Some("around.htm"));
    assert_eq!(layout.link_at(18.0, 15.0), Some("three.htm"));
    assert_eq!(layout.link_at(12.0, 18.0), Some("around.htm"));
    // A part with no address hides what is below it.
    assert_eq!(layout.link_at(2.0, 15.0), None);
}

#[test]
fn a_map_needs_its_name_and_stays_on_the_image() {
    let other_name = page(
        "<img src=a.png usemap=#other><map name=m><area shape=default href=x.htm></map>",
        500.0,
    );
    assert_eq!(other_name.layout().link_at(5.0, 5.0), None);
    let whole = page(
        "<map name=m><area shape=default href=x.htm><area coords='0,0,500,500' href=y.htm></map>\
         <img src=a.png usemap=#m>",
        500.0,
    );
    assert_eq!(whole.layout().link_at(39.0, 19.0), Some("x.htm"));
    assert_eq!(whole.layout().link_at(41.0, 5.0), None);
    let cut_off = page(
        "<map name=m><area coords='0,0,500,500' href=y.htm></map><img src=a.png usemap=#m>",
        500.0,
    );
    assert_eq!(cut_off.layout().link_at(41.0, 5.0), None);
}

#[test]
fn a_map_follows_its_image_past_borders_and_floats() {
    let framed = page(
        "<map name=m><area coords='0,0,10,10' href=x.htm></map>\
         <img src=a.png usemap=#m border=5 style='float:right'>",
        200.0,
    );
    // The image starts at 200 - 40 - 10 + 5 across and 5 down.
    assert_eq!(framed.layout().link_at(156.0, 6.0), Some("x.htm"));
    assert_eq!(framed.layout().link_at(153.0, 6.0), None);
}

#[test]
fn broken_and_huge_maps_do_no_harm() {
    let broken = page(
        "<img src=a.png usemap=#m><map name=m>\
         <area shape=circle coords='1'><area shape=poly coords='1,2'><area coords='x,y,z,w' href=a>\
         <area shape=star coords='1,2,3,4' href=b><area shape=poly coords='5,5,5,5,5,5' href=c></map>",
        500.0,
    );
    assert_eq!(broken.layout().link_at(30.0, 15.0), None);
    let start = std::time::Instant::now();
    let many = page(
        &format!(
            "<map name=m>{}</map>{}",
            "<area coords='0,0,1,1' href=a>".repeat(5000),
            "<img src=a.png usemap=#m>".repeat(500)
        ),
        500.0,
    );
    assert!(many.layout().link_areas.len() <= 100_000);
    assert!(start.elapsed().as_secs() < 20);
}

#[test]
fn a_fragment_leads_to_its_place() {
    let source = "<p>one</p><p id=second>two</p><p>three <a name=third>word</a></p>\
                  <div id=hidden style='display:none'>x</div><p id=last></p><p>four</p>\
                  <a name=picture></a><img src=a.png><p>five</p>";
    let shown = page(source, 500.0);
    assert_eq!(shown.fragment_y("second"), Some(16.0));
    assert_eq!(shown.fragment_y("third"), Some(32.0));
    assert_eq!(shown.fragment_y("last"), Some(48.0));
    assert_eq!(shown.fragment_y("picture"), Some(64.0));
    assert_eq!(shown.fragment_y("missing"), None);
    assert_eq!(shown.fragment_y("hidden"), None);
    assert_eq!(shown.fragment_y(""), Some(0.0));
    assert_eq!(shown.fragment_y("TOP"), Some(0.0));
    let named_top = page("<p>one</p><p id=top>two</p>", 500.0);
    assert_eq!(named_top.fragment_y("top"), Some(16.0));
}

#[test]
fn an_id_comes_before_a_name() {
    let both = page("<p><a name=x>one</a></p><p id=x>two</p>", 500.0);
    assert_eq!(both.fragment_y("x"), Some(16.0));
}

#[test]
fn history_goes_back_and_forward() {
    let mut history = History::default();
    assert!(!history.go_back());
    assert!(!history.go_forward());
    history.visit("a", "");
    history.save_scroll(100);
    history.visit("b", "");
    history.visit("b", "part");
    assert!(history.can_go_back());
    assert!(!history.can_go_forward());
    assert!(history.go_back());
    assert_eq!(history.current().unwrap().fragment, "");
    assert!(history.go_back());
    assert_eq!(history.current().unwrap().path, "a");
    assert_eq!(history.current().unwrap().scroll_y, 100);
    assert!(!history.go_back());
    assert!(history.go_forward());
    assert_eq!(history.current().unwrap().path, "b");
}

#[test]
fn going_somewhere_new_forgets_what_was_ahead() {
    let mut history = History::default();
    history.visit("a", "");
    history.visit("b", "");
    history.go_back();
    history.visit("c", "");
    assert!(!history.can_go_forward());
    history.go_back();
    assert_eq!(history.current().unwrap().path, "a");
}

#[test]
fn going_where_the_user_is_adds_nothing() {
    let mut history = History::default();
    history.visit("a", "");
    history.visit("a", "");
    assert!(!history.can_go_back());
}

#[test]
fn history_does_not_grow_without_end() {
    let mut history = History::default();
    for number in 0..5000 {
        history.visit(&number.to_string(), "");
    }
    let mut steps = 0;
    while history.go_back() {
        steps += 1;
    }
    assert_eq!(steps, 999);
    assert_eq!(history.current().unwrap().path, "4000");
}
