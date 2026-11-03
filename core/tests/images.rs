//! Images, backgrounds and floats. Characters are 8 wide and lines 16 high, as in the layout
//! tests, `a.png` is 40 by 20 and `b.png` 10 by 10.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::{ImageFormat, Rgba, RgbaImage};
use ron_browser_core::images::{
    DecodedImage, ImageStore, LocalFiles, NoResources, ResourceLoader, decode,
};
use ron_browser_core::layout::{FontSpec, Item, ItemKind, Layout, TextMeasurer};
use ron_browser_core::page::Page;
use ron_browser_core::selection::{Selection, Unit};

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

fn blank(width: u32, height: u32) -> DecodedImage {
    DecodedImage {
        width,
        height,
        rgba: vec![128; (width * height * 4) as usize],
    }
}

fn lay_out_source(source: &str, width: f32) -> Layout {
    let mut store = ImageStore::default();
    store.insert("a.png", blank(40, 20));
    store.insert("b.png", blank(10, 10));
    let mut measurer = Measurer::default();
    let mut page = Page::open_with(source, &mut NoResources, store);
    page.lay_out(width, 600.0, &mut measurer);
    page.layout().clone()
}

fn lay_out(body: &str, width: f32) -> Layout {
    lay_out_source(
        &format!("<!DOCTYPE html><style>body{{margin:0}}</style>{body}"),
        width,
    )
}

fn of_kind(layout: &Layout, kind: ItemKind) -> Vec<Item> {
    layout
        .items
        .iter()
        .filter(|item| item.kind == kind)
        .copied()
        .collect()
}

fn image_box(layout: &Layout) -> (f32, f32, f32, f32) {
    let images = of_kind(layout, ItemKind::Image);
    assert_eq!(images.len(), 1, "expected one image");
    (images[0].x, images[0].y, images[0].width, images[0].height)
}

fn tile_box(layout: &Layout) -> (f32, f32, f32, f32) {
    let tiles = of_kind(layout, ItemKind::Tile);
    assert_eq!(tiles.len(), 1, "expected one background image");
    (tiles[0].x, tiles[0].y, tiles[0].width, tiles[0].height)
}

/// Where each line of text starts.
fn line_starts(layout: &Layout) -> Vec<(f32, f32)> {
    layout
        .lines
        .iter()
        .map(|line| {
            let item = &layout.items[line.first_item as usize];
            (item.x, line.top)
        })
        .collect()
}

fn encoded(format: ImageFormat) -> Vec<u8> {
    let image = RgbaImage::from_pixel(3, 2, Rgba([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .to_rgb8()
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

/// A directory for one test, removed afterwards.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ron-images-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_format_decodes_to_the_same_pixels_size() {
    for format in [
        ImageFormat::Png,
        ImageFormat::Gif,
        ImageFormat::Jpeg,
        ImageFormat::Bmp,
    ] {
        let image = decode(&encoded(format)).unwrap_or_else(|| panic!("{format:?}"));
        assert_eq!((image.width, image.height), (3, 2), "{format:?}");
        assert_eq!(image.rgba.len(), 3 * 2 * 4, "{format:?}");
    }
}

#[test]
fn what_is_not_an_image_does_no_harm() {
    let png = encoded(ImageFormat::Png);
    let mut noise_gif = b"GIF89a".to_vec();
    noise_gif.extend((0..200_u8).map(|byte| byte.wrapping_mul(37)));
    let cases: [&[u8]; 4] = [
        b"",
        b"<html>not an image</html>",
        &png[..png.len() / 2],
        &noise_gif,
    ];
    for case in cases {
        assert_eq!(decode(case), None);
    }
}

#[test]
fn images_claiming_a_huge_size_are_refused_before_allocating() {
    let mut header = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    header.extend(60_000_u32.to_be_bytes());
    header.extend(60_000_u32.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    let checksum = crc_of(&header[12..]);
    header.extend(checksum.to_be_bytes());
    assert_eq!(decode(&header), None);
}

/// The checksum of a PNG chunk, so that only the size can be the reason for refusal.
fn crc_of(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[test]
fn files_load_only_from_the_directory_of_the_page() {
    let directory = TempDir::new("policy");
    let page = directory.path().join("page");
    fs::create_dir_all(page.join("sub")).unwrap();
    fs::write(directory.path().join("secret.txt"), b"secret").unwrap();
    fs::write(page.join("a.png"), b"a").unwrap();
    fs::write(page.join("sub/b.png"), b"b").unwrap();
    fs::write(page.join("with space.png"), b"c").unwrap();
    let files = LocalFiles::new(&page);
    for url in [
        "a.png",
        "sub/b.png",
        "./a.png",
        "sub/../a.png",
        "a.png?v=1#x",
        "with%20space.png",
    ] {
        assert!(files.resolve(url).is_some(), "should load {url}");
    }
    for url in [
        "../secret.txt",
        "sub/../../secret.txt",
        "%2e%2e/secret.txt",
        "..%2fsecret.txt",
        "/etc/passwd",
        "file:///etc/passwd",
        "http://example.com/a.png",
        "https://example.com/a.png",
        "//example.com/a.png",
        "data:image/png;base64,AAAA",
        "javascript:alert(1)",
        "",
        "missing.png",
        "a.png%00.txt",
    ] {
        assert_eq!(files.resolve(url), None, "should refuse {url}");
    }
    // A directory is inside, but there is nothing in it to read.
    assert_eq!(LocalFiles::new(&page).load("sub"), None);
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_cannot_lead_out() {
    let directory = TempDir::new("link");
    let page = directory.path().join("page");
    fs::create_dir_all(&page).unwrap();
    fs::write(directory.path().join("secret.txt"), b"secret").unwrap();
    std::os::unix::fs::symlink(directory.path().join("secret.txt"), page.join("link.png")).unwrap();
    assert_eq!(LocalFiles::new(&page).resolve("link.png"), None);
}

#[test]
fn a_page_without_a_directory_loads_nothing() {
    let directory = TempDir::new("gone");
    let gone = directory.path().join("no-such-directory");
    let mut files = LocalFiles::new(&gone);
    assert_eq!(files.resolve("a.png"), None);
    assert_eq!(files.load("a.png"), None);
}

/// Notes what a page asks for.
#[derive(Default)]
struct Recorder {
    urls: Vec<String>,
}

impl ResourceLoader for Recorder {
    fn load(&mut self, url: &str) -> Option<Vec<u8>> {
        self.urls.push(url.to_owned());
        None
    }
}

#[test]
fn a_page_loads_its_images_and_backgrounds_through_the_loader() {
    let mut recorder = Recorder::default();
    let source = "<!DOCTYPE html><body background=paper.png><img src=a.png>\
                  <img src=missing.txt><img src=a.png><p style='background:url(b.png)'>x";
    let page = Page::open_with(source, &mut recorder, ImageStore::default());
    assert_eq!(
        recorder.urls,
        ["paper.png", "a.png", "missing.txt", "b.png"]
    );
    assert_eq!(page.images().number("a.png"), None);
}

#[test]
fn an_image_sits_on_the_baseline() {
    let layout = lay_out("aa<img src=a.png>bb", 500.0);
    assert_eq!(image_box(&layout), (16.0, 0.0, 40.0, 20.0));
    let texts = of_kind(&layout, ItemKind::Text);
    assert_eq!(texts.len(), 2);
    assert_eq!(texts[0].baseline, 20.0);
    assert_eq!(texts[1].x, 56.0);
    assert!((layout.lines[0].bottom - 23.2).abs() < 0.01);
    assert_eq!(layout.text, "aabb");
}

#[test]
fn an_image_is_a_word_that_lines_can_break_around() {
    let layout = lay_out("aaaa <img src=a.png> <img src=a.png>", 90.0);
    let images = of_kind(&layout, ItemKind::Image);
    assert_eq!(images.len(), 2);
    // 32 of text and a space, then the first image; the second would pass the width.
    assert_eq!((images[0].x, images[0].y), (40.0, 0.0));
    assert_eq!(images[1].x, 0.0);
    assert!(images[1].y > 0.0);
}

#[test]
fn set_sizes_keep_the_shape() {
    for (attributes, width, height) in [
        ("width=80", 80.0, 40.0),
        ("height=10", 20.0, 10.0),
        ("width=80 height=80", 80.0, 80.0),
        ("style='width:60px'", 60.0, 30.0),
    ] {
        let layout = lay_out(&format!("<img src=a.png {attributes}>"), 500.0);
        let found = image_box(&layout);
        assert_eq!((found.2, found.3), (width, height), "{attributes}");
    }
}

#[test]
fn a_border_and_margins_go_around() {
    let layout = lay_out("<img src=a.png border=2 hspace=5>x", 500.0);
    let (x, y, width, height) = image_box(&layout);
    assert_eq!((x, y, width, height), (7.0, 2.0, 40.0, 20.0));
    // 5 of margin, 2 of border, the image, 2 and 5 again.
    assert_eq!(of_kind(&layout, ItemKind::Text)[0].x, 54.0);
}

#[test]
fn a_block_image_takes_a_line() {
    let layout = lay_out("a<img src=a.png style='display:block'>b", 500.0);
    let (x, y, _, _) = image_box(&layout);
    assert_eq!((x, y), (0.0, 16.0));
    assert_eq!(line_starts(&layout), [(0.0, 0.0), (0.0, 36.0)]);
}

#[test]
fn a_missing_image_shows_alt_or_room() {
    let with_alt = lay_out("<img src=missing.png alt=hello>", 500.0);
    assert!(of_kind(&with_alt, ItemKind::Image).is_empty());
    assert_eq!(with_alt.text, "hello");

    let with_size = lay_out("<img src=missing.png width=30 height=30>x", 500.0);
    assert!(of_kind(&with_size, ItemKind::Image).is_empty());
    assert_eq!(of_kind(&with_size, ItemKind::Text)[0].x, 30.0);
}

#[test]
fn a_background_color_and_image_fill_their_box() {
    let layout = lay_out(
        "<div style='height:30px;width:100px;background:red url(b.png)'></div>",
        500.0,
    );
    assert_eq!(tile_box(&layout), (0.0, 0.0, 100.0, 30.0));
    let rectangles = of_kind(&layout, ItemKind::Rect);
    assert_eq!(rectangles.len(), 1);
    assert_eq!(rectangles[0].height, 30.0);
    // The color lies below the image.
    let rectangle = layout.items.iter().position(|i| i.kind == ItemKind::Rect);
    let tile = layout.items.iter().position(|i| i.kind == ItemKind::Tile);
    assert!(rectangle < tile);
}

#[test]
fn a_background_repeats_as_asked() {
    // A box of 200 by 100, and an image of 10 by 10.
    for (rule, expected) in [
        ("no-repeat", (0.0, 0.0, 10.0, 10.0)),
        ("repeat-x", (0.0, 0.0, 200.0, 10.0)),
        ("repeat-y", (0.0, 0.0, 10.0, 100.0)),
        ("no-repeat 10px 5px", (10.0, 5.0, 10.0, 10.0)),
        ("no-repeat center", (95.0, 45.0, 10.0, 10.0)),
        ("no-repeat right bottom", (190.0, 90.0, 10.0, 10.0)),
        ("no-repeat 195px 0", (195.0, 0.0, 5.0, 10.0)),
    ] {
        let layout = lay_out(
            &format!("<div style='width:200px;height:100px;background:url(b.png) {rule}'></div>"),
            500.0,
        );
        assert_eq!(tile_box(&layout), expected, "{rule}");
    }
}

#[test]
fn the_background_of_body_or_root_covers_the_whole_page() {
    for source in [
        "<!DOCTYPE html><body background=b.png style='margin:50px'>x",
        "<!DOCTYPE html><html style='background:url(b.png)'><body style='margin:50px'>x",
    ] {
        let layout = lay_out_source(source, 500.0);
        assert!(layout.canvas_tile.is_some(), "{source}");
        assert!(of_kind(&layout, ItemKind::Tile).is_empty(), "{source}");
    }
}

#[test]
fn text_goes_around_a_floating_image() {
    let words = "aaaa bbbb cccc";
    let left = lay_out(&format!("<img src=a.png style='float:left'>{words}"), 80.0);
    assert_eq!(image_box(&left).0, 0.0);
    assert_eq!(line_starts(&left), [(40.0, 0.0), (40.0, 16.0), (0.0, 32.0)]);

    let right = lay_out(&format!("<img align=right src=a.png>{words}"), 100.0);
    assert_eq!(image_box(&right).0, 60.0);
}

#[test]
fn a_float_does_not_add_to_lines_or_height() {
    let layout = lay_out("<img src=a.png style='float:left'>", 100.0);
    assert!(layout.lines.is_empty());
    let parent = lay_out(
        "<div style='background:red'><img src=a.png style='float:left'>x</div>",
        100.0,
    );
    assert_eq!(of_kind(&parent, ItemKind::Rect)[0].height, 16.0);
}

#[test]
fn floats_stand_side_by_side_and_wrap() {
    let float = "<img src=a.png style='float:left'>";
    let layout = lay_out(&float.repeat(3), 100.0);
    let places: Vec<(f32, f32)> = of_kind(&layout, ItemKind::Image)
        .iter()
        .map(|item| (item.x, item.y))
        .collect();
    assert_eq!(places, [(0.0, 0.0), (40.0, 0.0), (0.0, 20.0)]);
}

#[test]
fn a_word_with_no_room_beside_a_float_goes_below_it() {
    let layout = lay_out(
        "<img src=a.png style='float:left'>wwwwwwwwwwwwwwwwwwww",
        100.0,
    );
    assert_eq!(line_starts(&layout), [(0.0, 20.0)]);
}

#[test]
fn clear_moves_below_floats() {
    let float = "<img src=a.png style='float:left'>";
    for clear in [
        "<p style='clear:left;margin:0'>x</p>",
        "<p style='clear:both;margin:0'>x</p>",
        "<br clear=all>x",
    ] {
        let layout = lay_out(&format!("{float}{clear}"), 100.0);
        let last = layout.lines.last().unwrap();
        assert_eq!(last.top, 20.0, "{clear}");
        assert_eq!(layout.items[last.first_item as usize].x, 0.0, "{clear}");
    }
    let not_cleared = lay_out(
        &format!("{float}<p style='clear:right;margin:0'>x</p>"),
        100.0,
    );
    assert_eq!(line_starts(&not_cleared), [(40.0, 0.0)]);
}

#[test]
fn a_block_can_float_as_wide_as_what_it_holds() {
    let layout = lay_out("<div style='float:left;background:red'>aaaa</div>bb", 200.0);
    assert_eq!(of_kind(&layout, ItemKind::Rect)[0].width, 32.0);
    assert_eq!(line_starts(&layout), [(0.0, 0.0), (32.0, 0.0)]);

    let set = lay_out("<div style='float:left;width:50px'>aa</div>bb", 200.0);
    assert_eq!(line_starts(&set), [(0.0, 0.0), (50.0, 0.0)]);
}

#[test]
fn only_some_blocks_keep_their_floats_inside() {
    let float = "<img src=a.png style='float:left'>";
    let height_of = |style: &str| {
        let layout = lay_out(
            &format!("<div style='background:red;{style}'>{float}x</div>"),
            100.0,
        );
        of_kind(&layout, ItemKind::Rect)[0].height
    };
    assert_eq!(height_of(""), 16.0);
    assert_eq!(height_of("overflow:hidden"), 20.0);
    assert_eq!(height_of("float:left"), 20.0);
}

#[test]
fn selecting_works_across_text_in_floats() {
    let layout = lay_out("<div style='float:right'>right</div>left", 200.0);
    assert_eq!(layout.text.replace('\n', " "), "right left");
    // Selecting looks up fonts by number, which all have the same size here.
    let mut measurer = Measurer::default();
    let last_font = layout.items.iter().map(|item| item.font).max().unwrap();
    measurer.sizes.resize(last_font as usize + 1, 16.0);
    // A word of each, picked by where it shows on the screen.
    let word = |x: f32, measurer: &mut Measurer| {
        Selection::start(&layout, x, 8.0, Unit::Word, measurer)
            .unwrap()
            .text(&layout)
            .to_owned()
    };
    assert_eq!(word(180.0, &mut measurer), "right");
    assert_eq!(word(10.0, &mut measurer), "left");
    // Dragging takes the text between, in the order of the text.
    let mut selection =
        Selection::start(&layout, 161.0, 8.0, Unit::Character, &mut measurer).unwrap();
    selection.drag_to(&layout, 10.0, 8.0, &mut measurer);
    assert_eq!(selection.text(&layout).replace('\n', " "), "right l");
}

#[test]
fn hostile_floats_stay_fast() {
    let many = "<div style='float:left;width:1px;height:1px'>x</div>".repeat(20_000);
    let start = Instant::now();
    let layout = lay_out(&many, 800.0);
    assert!(!layout.items.is_empty());
    let nested = format!("{}x", "<div style='float:left'>".repeat(5_000));
    lay_out(&nested, 800.0);
    assert!(start.elapsed() < Duration::from_secs(20));
}
