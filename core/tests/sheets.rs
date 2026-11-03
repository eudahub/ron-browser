//! Style sheets in files of their own, from the disk and from a small server on this computer.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use ron::css::{Color, Viewport};
use ron_browser_core::images::{ImageStore, ResourceLoader};
use ron_browser_core::layout::{FontSpec, ItemKind, TextMeasurer};
use ron_browser_core::net::SheetFetch;
use ron_browser_core::page::Page;
use ron_browser_core::sheets::decode_css;

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

/// Files by name, noting what is asked for.
#[derive(Default)]
struct Files {
    files: HashMap<&'static str, &'static str>,
    asked: Vec<String>,
}

impl ResourceLoader for Files {
    fn load(&mut self, url: &str) -> Option<Vec<u8>> {
        self.asked.push(url.to_owned());
        self.files.get(url).map(|text| text.as_bytes().to_vec())
    }
}

fn open(body: &str, files: &[(&'static str, &'static str)]) -> (Page, Vec<String>) {
    let mut loader = Files {
        files: files.iter().copied().collect(),
        asked: Vec::new(),
    };
    let source = format!("<!DOCTYPE html>{body}");
    let mut page = Page::open_with(&source, &mut loader, ImageStore::default());
    page.lay_out(500.0, 400.0, &mut Measurer::default());
    (page, loader.asked)
}

/// The color of the first text.
fn text_color(page: &Page) -> Color {
    let layout = page.layout();
    layout
        .items
        .iter()
        .find(|item| item.kind == ItemKind::Text)
        .expect("text")
        .color
}

const RED: Color = Color {
    red: 255,
    green: 0,
    blue: 0,
    alpha: 255,
};
const LIME: Color = Color {
    red: 0,
    green: 255,
    blue: 0,
    alpha: 255,
};
const BLUE: Color = Color {
    red: 0,
    green: 0,
    blue: 255,
    alpha: 255,
};

#[test]
fn a_linked_sheet_counts() {
    let (page, asked) = open(
        "<link rel=stylesheet href=a.css><p>x",
        &[("a.css", "p{color:red}")],
    );
    assert_eq!(text_color(&page), RED);
    assert_eq!(asked[0], "a.css");
}

#[test]
fn sheets_count_in_the_order_of_the_page() {
    let files = [("a.css", "p{color:red}"), ("b.css", "p{color:lime}")];
    let (first, _) = open(
        "<link rel=stylesheet href=a.css><link rel=stylesheet href=b.css><p>x",
        &files,
    );
    assert_eq!(text_color(&first), LIME);
    let (around_style, _) = open(
        "<link rel=stylesheet href=a.css><style>p{color:blue}</style><p>x",
        &files,
    );
    assert_eq!(text_color(&around_style), BLUE);
    let (style_first, _) = open(
        "<style>p{color:blue}</style><link rel=stylesheet href=a.css><p>x",
        &files,
    );
    assert_eq!(text_color(&style_first), RED);
}

#[test]
fn only_true_sheets_are_read() {
    let files = [("a.css", "p{color:red}")];
    for link in [
        "<link rel=icon href=a.css>",
        "<link rel='alternate stylesheet' href=a.css>",
        "<link rel=stylesheet type=text/x-other href=a.css>",
        "<link rel=stylesheet>",
    ] {
        let (page, asked) = open(&format!("{link}<p>x"), &files);
        assert_eq!(
            text_color(&page),
            Color {
                red: 0,
                green: 0,
                blue: 0,
                alpha: 255
            },
            "{link}"
        );
        assert!(asked.is_empty(), "{link}");
    }
    let (upper, _) = open("<LINK REL=StyleSheet HREF=a.css><p>x", &files);
    assert_eq!(text_color(&upper), RED);
}

#[test]
fn media_decides_and_follows_the_view() {
    let files = [("a.css", "p{color:red}")];
    let (screen, _) = open("<link rel=stylesheet media=screen href=a.css><p>x", &files);
    assert_eq!(text_color(&screen), RED);
    let (print, _) = open("<link rel=stylesheet media=print href=a.css><p>x", &files);
    assert_ne!(text_color(&print), RED);
    let (mut narrow, _) = open(
        "<link rel=stylesheet media='(max-width: 300px)' href=a.css><p>x",
        &files,
    );
    assert_ne!(text_color(&narrow), RED);
    narrow.lay_out(200.0, 400.0, &mut Measurer::default());
    assert_eq!(text_color(&narrow), RED);
}

#[test]
fn imports_count_before_the_sheet_and_from_where_it_is() {
    let (page, asked) = open(
        "<link rel=stylesheet href=css/a.css><p>x",
        &[
            ("css/a.css", "@import url(deep/b.css); p{background:red}"),
            ("css/deep/b.css", "@import 'c.css'; p{color:lime}"),
            ("css/deep/c.css", "p{color:red}"),
        ],
    );
    assert_eq!(text_color(&page), LIME);
    assert_eq!(
        asked[..3],
        ["css/a.css", "css/deep/b.css", "css/deep/c.css"]
    );
}

#[test]
fn an_import_in_the_page_itself_counts() {
    let (page, _) = open(
        "<style>@import 'a.css';</style><p>x",
        &[("a.css", "p{color:red}")],
    );
    assert_eq!(text_color(&page), RED);
}

#[test]
fn imports_going_round_end() {
    let (page, asked) = open(
        "<link rel=stylesheet href=a.css><p>x",
        &[
            ("a.css", "@import 'b.css';p{color:red}"),
            ("b.css", "@import 'a.css';"),
        ],
    );
    assert_eq!(text_color(&page), RED);
    assert!(asked.len() <= 10, "{asked:?}");
}

#[test]
fn a_flood_of_imports_ends() {
    let many: String = (0..5000).map(|n| format!("@import 'x{n}.css';")).collect();
    let many: &'static str = Box::leak(many.into_boxed_str());
    let (_, asked) = open("<link rel=stylesheet href=a.css><p>x", &[("a.css", many)]);
    assert!(asked.len() <= 200, "{}", asked.len());
}

#[test]
fn images_in_a_sheet_count_from_the_sheet() {
    let (page, asked) = open(
        "<link rel=stylesheet href=css/a.css><p>x",
        &[(
            "css/a.css",
            "p{background:url(img/a.png)} body{background-image:url('../b.png')}\
             div{background:url(\"/c.png\")} h1{background:url(data:image/gif;base64,AA)}\
             h2{background:url(http://example.com/d.png)}",
        )],
    );
    drop(page);
    assert!(asked.contains(&"css/img/a.png".to_owned()), "{asked:?}");
    assert!(asked.contains(&"css/../b.png".to_owned()), "{asked:?}");
}

#[test]
fn what_is_wrong_with_a_sheet_does_no_harm() {
    for css in [
        "",
        "}{",
        "@import",
        "@import url(",
        "url(",
        "p{background:url(",
        "\u{0}",
    ] {
        let (page, _) = open("<link rel=stylesheet href=a.css><p>x", &[("a.css", css)]);
        assert_eq!(page.layout().text, "x", "{css}");
    }
    let (page, _) = open("<link rel=stylesheet href=missing.css><p>x", &[]);
    assert_eq!(page.layout().text, "x");
}

#[test]
fn the_text_of_a_sheet_is_decoded() {
    assert_eq!(decode_css(b"\xEF\xBB\xBFp{}", ""), "p{}");
    assert_eq!(decode_css(b"a\xB1", "iso-8859-2"), "aą");
    assert_eq!(decode_css(b"a\xB9", "Windows-1250"), "aą");
    assert_eq!(decode_css("aą".as_bytes(), ""), "aą");
    assert_eq!(decode_css("aą".as_bytes(), "no-such-charset"), "aą");
}

fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => request.extend_from_slice(&buffer[..count]),
                }
            }
            let text = String::from_utf8_lossy(&request).into_owned();
            let path = text.split(' ').nth(1).unwrap_or("/");
            let (kind, body) = match path {
                "/css/a.css" => ("text/css", "@import 'b.css'; p{color:red}"),
                "/css/b.css" => ("text/css", "p{background:url(p.png)}"),
                "/not-css" => ("text/html", "p{color:red}"),
                _ => ("text/plain", ""),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    address
}

/// Reads all a page asks for, round by round, as the window does.
fn read_sheets(page: &mut Page, address: &str) {
    let start = Instant::now();
    loop {
        let requests = page.sheet_requests();
        if requests.is_empty() {
            break;
        }
        let mut fetch = SheetFetch::start(address, requests);
        while !fetch.is_done() {
            for (slot, css) in fetch.poll() {
                page.add_sheet(slot, &css);
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
        for (slot, css) in fetch.poll() {
            page.add_sheet(slot, &css);
        }
    }
    page.apply_sheets();
}

#[test]
fn sheets_come_from_the_network_with_their_addresses() {
    let server = serve();
    let address = format!("{server}/dir/page.html");
    let mut page = Page::open_fetched(
        "<!DOCTYPE html><link rel=stylesheet href=/css/a.css><p>x",
        &address,
        Viewport::default(),
    );
    read_sheets(&mut page, &address);
    page.lay_out(500.0, 400.0, &mut Measurer::default());
    assert_eq!(text_color(&page), RED);
    // What the imported sheet names counts from that sheet.
    let sources = page.image_sources();
    assert_eq!(sources, [format!("{server}/css/p.png")]);
}

#[test]
fn what_is_not_called_css_does_not_count() {
    let server = serve();
    let address = format!("{server}/page.html");
    let mut page = Page::open_fetched(
        "<!DOCTYPE html><link rel=stylesheet href=/not-css><p>x",
        &address,
        Viewport::default(),
    );
    read_sheets(&mut page, &address);
    page.lay_out(500.0, 400.0, &mut Measurer::default());
    assert_ne!(text_color(&page), RED);
}

#[test]
fn a_secure_page_reads_no_plain_sheets() {
    let server = serve();
    let address = "https://example.com/page.html";
    let mut page = Page::open_fetched(
        &format!("<!DOCTYPE html><link rel=stylesheet href={server}/css/a.css><p>x"),
        address,
        Viewport::default(),
    );
    read_sheets(&mut page, address);
    page.lay_out(500.0, 400.0, &mut Measurer::default());
    assert_ne!(text_color(&page), RED);
}

#[test]
fn a_page_on_the_network_reads_no_local_sheets() {
    let address = "http://example.com/page.html";
    let mut page = Page::open_fetched(
        "<!DOCTYPE html><link rel=stylesheet href=file:///etc/passwd><p>x",
        address,
        Viewport::default(),
    );
    read_sheets(&mut page, address);
    assert_eq!(page.sheet_requests(), []);
}
