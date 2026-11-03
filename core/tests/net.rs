//! Reading from the network, through a small server on this computer. What only concerns addresses
//! needs no server at all.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::GzEncoder;
use ron_browser_core::links::{Target, resolve};
use ron_browser_core::net::{
    Asker, FetchError, ImageFetch, ImageRequest, MAX_PAGE_BYTES, PageEvent, PageFetch, fetch_page,
    image_requests, is_private_ip, may_redirect,
};
use url::Url;

/// Answers each path with a canned answer, and notes the requests it got.
struct Server {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
}

fn serve(answer: fn(&str, &str) -> Vec<u8>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let origin = address.clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&requests);
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
            let path = text.split(' ').nth(1).unwrap_or("/").to_owned();
            noted.lock().unwrap().push(text);
            let _ = stream.write_all(&answer(&path, &origin));
        }
    });
    Server { address, requests }
}

fn http(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut answer = format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    answer.extend_from_slice(body);
    answer
}

fn site(path: &str, origin: &str) -> Vec<u8> {
    match path {
        "/page" => http(
            "200 OK",
            "Content-Type: Text/HTML; charset=ISO-8859-2\r\n",
            b"<p>hello",
        ),
        "/bare" => http("200 OK", "", b"plain"),
        "/moved" => http("302 Found", "Location: /page#ignored\r\n", b""),
        "/moved-far" => http("301 Moved", &format!("Location: {origin}/moved\r\n"), b""),
        "/loop" => http("302 Found", "Location: /loop\r\n", b""),
        "/to-script" => http("302 Found", "Location: javascript:alert(1)\r\n", b""),
        "/to-file" => http("302 Found", "Location: file:///etc/passwd\r\n", b""),
        "/moved-nowhere" => http("302 Found", "", b""),
        "/image" => {
            let mut png = Vec::new();
            let image = image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
            image::DynamicImage::ImageRgba8(image)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            http("200 OK", "Content-Type: image/png\r\n", &png)
        }
        "/huge" => {
            let mut answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                MAX_PAGE_BYTES + 1
            )
            .into_bytes();
            answer.extend(std::iter::repeat_n(b'x', 1024));
            answer
        }
        "/unpacks-huge" => {
            let mut packed = GzEncoder::new(Vec::new(), Compression::fast());
            let zeros = vec![0; 1024 * 1024];
            for _ in 0..=(MAX_PAGE_BYTES / 1024 / 1024) {
                packed.write_all(&zeros).unwrap();
            }
            http(
                "200 OK",
                "Content-Encoding: gzip\r\n",
                &packed.finish().unwrap(),
            )
        }
        "/packed" => {
            let mut packed = GzEncoder::new(Vec::new(), Compression::fast());
            packed.write_all(b"<p>hello").unwrap();
            http(
                "200 OK",
                "Content-Encoding: gzip\r\n",
                &packed.finish().unwrap(),
            )
        }
        "/endless" => {
            let mut answer = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
            answer.extend(std::iter::repeat_n(b'x', MAX_PAGE_BYTES as usize + 1024));
            answer
        }
        _ => http("404 Not Found", "", b"gone"),
    }
}

#[test]
fn a_page_arrives_with_what_the_server_says_of_it() {
    let server = serve(site);
    let page = fetch_page(&format!("{}/page", server.address), Asker::User).unwrap();
    assert_eq!(page.bytes, b"<p>hello");
    assert_eq!(page.mime, "text/html");
    assert_eq!(page.charset.to_ascii_lowercase(), "iso-8859-2");
    assert_eq!(page.address, format!("{}/page", server.address));
    let bare = fetch_page(&format!("{}/bare", server.address), Asker::User).unwrap();
    assert_eq!(bare.mime, "");
    assert_eq!(bare.charset, "");
}

#[test]
fn a_request_tells_nothing_about_the_user() {
    let server = serve(site);
    fetch_page(&format!("{}/page", server.address), Asker::User).unwrap();
    let requests = server.requests.lock().unwrap();
    let request = requests[0].to_ascii_lowercase();
    assert!(request.contains("user-agent: ron-browser/"), "{request}");
    for header in ["cookie:", "referer:", "origin:", "accept-language:"] {
        assert!(!request.contains(header), "{request}");
    }
}

#[test]
fn redirects_are_followed_to_where_the_page_is() {
    let server = serve(site);
    let page = fetch_page(&format!("{}/moved-far", server.address), Asker::User).unwrap();
    assert_eq!(page.bytes, b"<p>hello");
    assert_eq!(page.address, format!("{}/page", server.address));
}

#[test]
fn bad_redirects_end_there() {
    let server = serve(site);
    let get = |path: &str| fetch_page(&format!("{}{path}", server.address), Asker::User);
    assert!(matches!(get("/loop"), Err(FetchError::Failed(_))));
    assert!(matches!(get("/to-script"), Err(FetchError::Refused(_))));
    assert!(matches!(get("/to-file"), Err(FetchError::Refused(_))));
    assert_eq!(get("/moved-nowhere"), Err(FetchError::Status(302)));
}

#[test]
fn a_redirect_never_leaves_https_for_http() {
    let url = |text: &str| Url::parse(text).unwrap();
    let secure = url("https://example.com/");
    let plain = url("http://example.com/");
    assert!(may_redirect(&secure, &secure));
    assert!(!may_redirect(&secure, &plain));
    assert!(may_redirect(&plain, &plain));
    assert!(may_redirect(&plain, &secure));
    assert!(!may_redirect(&plain, &url("file:///etc/passwd")));
    assert!(!may_redirect(&secure, &url("ftp://example.com/")));
}

#[test]
fn errors_and_limits_come_back_as_such() {
    let server = serve(site);
    let get = |path: &str| fetch_page(&format!("{}{path}", server.address), Asker::User);
    assert_eq!(get("/nothing"), Err(FetchError::Status(404)));
    assert_eq!(get("/packed").unwrap().bytes, b"<p>hello");
    // Not compared directly, as a failure would print many megabytes.
    for path in ["/huge", "/unpacks-huge", "/endless"] {
        assert!(matches!(get(path), Err(FetchError::TooLarge)), "{path}");
    }
    assert!(matches!(
        fetch_page("http://127.0.0.1:1/", Asker::User),
        Err(FetchError::Failed(_))
    ));
    for address in ["file:///etc/passwd", "ftp://example.com/", "nonsense"] {
        assert!(fetch_page(address, Asker::User).is_err(), "{address}");
    }
}

#[test]
fn a_page_on_the_network_stays_out_of_the_local_network() {
    let server = serve(site);
    let target = format!("{}/page", server.address);
    // Asked for by the user, by a local page, or by a page on the same server, it is fine.
    assert!(fetch_page(&target, Asker::User).is_ok());
    assert!(fetch_page(&target, Asker::Page("file:///a/page.html")).is_ok());
    assert!(fetch_page(&target, Asker::Page(&format!("{}/other", server.address))).is_ok());
    // Not from a page elsewhere, as an address or through a name.
    let elsewhere = Asker::Page("https://example.com/");
    assert!(matches!(
        fetch_page(&target, elsewhere),
        Err(FetchError::Refused(_))
    ));
    let port = server.address.rsplit(':').next().unwrap();
    assert!(matches!(
        fetch_page(&format!("http://localhost:{port}/page"), elsewhere),
        Err(FetchError::Refused(_))
    ));
    assert!(fetch_page("http://[::1]/", elsewhere).is_err());
}

#[test]
fn private_addresses_are_known() {
    for address in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.1.1",
        "100.64.0.1",
        "0.0.0.0",
        "255.255.255.255",
        "224.0.0.1",
        "::1",
        "::",
        "fe80::1",
        "fd00::1",
        "ff02::1",
        "::ffff:192.168.1.1",
    ] {
        assert!(
            is_private_ip(address.parse::<IpAddr>().unwrap()),
            "{address}"
        );
    }
    for address in [
        "8.8.8.8",
        "172.32.0.1",
        "100.128.0.1",
        "2606:4700::1111",
        "::ffff:8.8.8.8",
    ] {
        assert!(
            !is_private_ip(address.parse::<IpAddr>().unwrap()),
            "{address}"
        );
    }
}

#[test]
fn a_page_is_read_apart_from_the_window() {
    let server = serve(site);
    let mut fetch = PageFetch::start(&format!("{}/page", server.address), None);
    let events = all_events(&mut fetch);
    assert!(matches!(
        &events[0],
        PageEvent::Started { mime, .. } if mime == "text/html"
    ));
    assert_eq!(events[1], PageEvent::Data(b"<p>hello".to_vec()));
    assert_eq!(events[2], PageEvent::End);
    assert_eq!(fetch.poll(), None);
}

fn all_events(fetch: &mut PageFetch) -> Vec<PageEvent> {
    let start = Instant::now();
    let mut events = Vec::new();
    loop {
        match fetch.poll() {
            Some(event) => {
                let last = matches!(event, PageEvent::End | PageEvent::Failed(_));
                events.push(event);
                if last {
                    return events;
                }
            }
            None => thread::sleep(Duration::from_millis(5)),
        }
        assert!(start.elapsed() < Duration::from_secs(20));
    }
}

#[test]
fn a_page_comes_as_the_server_sends_it() {
    // Sends a part, and the rest only once the first part has been seen.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    let (seen_sender, seen) = std::sync::mpsc::channel::<()>();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0; 4096];
        let _ = stream.read(&mut buffer);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nfirst")
            .unwrap();
        stream.flush().unwrap();
        seen.recv_timeout(Duration::from_secs(10)).unwrap();
        stream.write_all(b"second").unwrap();
    });
    let mut fetch = PageFetch::start(&address, None);
    let start = Instant::now();
    let mut came = Vec::new();
    while came.len() < 5 {
        match fetch.poll() {
            Some(PageEvent::Data(piece)) => came.extend(piece),
            Some(event) => assert!(matches!(event, PageEvent::Started { .. }), "{event:?}"),
            None => thread::sleep(Duration::from_millis(5)),
        }
        assert!(start.elapsed() < Duration::from_secs(10));
    }
    assert_eq!(came, b"first");
    seen_sender.send(()).unwrap();
    for event in all_events(&mut fetch) {
        if let PageEvent::Data(piece) = event {
            came.extend(piece);
        }
    }
    assert_eq!(came, b"firstsecond");
}

#[test]
fn what_goes_wrong_while_reading_is_told() {
    let server = serve(site);
    for (path, wanted) in [
        ("/nothing", FetchError::Status(404)),
        ("/huge", FetchError::TooLarge),
    ] {
        let mut fetch = PageFetch::start(&format!("{}{path}", server.address), None);
        assert_eq!(
            all_events(&mut fetch),
            [PageEvent::Failed(wanted)],
            "{path}"
        );
    }
    // Too large after unpacking comes out only while reading.
    let mut fetch = PageFetch::start(&format!("{}/unpacks-huge", server.address), None);
    let events = all_events(&mut fetch);
    assert_eq!(
        events.last(),
        Some(&PageEvent::Failed(FetchError::TooLarge))
    );
}

#[test]
fn images_arrive_one_by_one() {
    let server = serve(site);
    let page = format!("{}/page", server.address);
    let requests = vec![
        ImageRequest {
            written: "a".to_owned(),
            url: format!("{}/image", server.address),
        },
        ImageRequest {
            written: "missing".to_owned(),
            url: format!("{}/nothing", server.address),
        },
        ImageRequest {
            written: "not-an-image".to_owned(),
            url: format!("{}/page", server.address),
        },
    ];
    let mut fetch = ImageFetch::start(&page, requests);
    let mut arrived = Vec::new();
    let start = Instant::now();
    while !fetch.is_done() {
        arrived.extend(fetch.poll());
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
    arrived.extend(fetch.poll());
    assert_eq!(arrived.len(), 1);
    assert_eq!(arrived[0].0, "a");
    assert_eq!((arrived[0].1.width, arrived[0].1.height), (3, 2));
}

#[test]
fn which_images_of_a_page_are_read() {
    let sources: Vec<String> = [
        "a.png",
        "/b.png#x",
        "//other.org/c.png",
        "http://example.com/d.png",
        "https://example.com/e.png",
        "data:image/png;base64,AAAA",
        "file:///etc/passwd",
        "javascript:alert(1)",
    ]
    .map(str::to_owned)
    .to_vec();
    let urls = |page: &str| -> Vec<String> {
        image_requests(page, &sources)
            .into_iter()
            .map(|request| request.url)
            .collect()
    };
    assert_eq!(
        urls("https://example.com/dir/page.html"),
        [
            "https://example.com/dir/a.png",
            "https://example.com/b.png",
            "https://other.org/c.png",
            "https://example.com/e.png",
        ]
    );
    assert_eq!(
        urls("http://example.com/page.html"),
        [
            "http://example.com/a.png",
            "http://example.com/b.png",
            "http://other.org/c.png",
            "http://example.com/d.png",
            "https://example.com/e.png",
        ]
    );
    assert!(image_requests("not an address", &sources).is_empty());
}

#[test]
fn a_redirect_target_cannot_be_a_local_file_through_a_link_either() {
    assert!(matches!(
        resolve("https://example.com/", "file:///etc/passwd"),
        Target::Refused(_)
    ));
}
