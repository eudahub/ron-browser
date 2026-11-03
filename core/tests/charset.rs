//! Finding the charset a page declares.

use ron_browser_core::charset::declared;

fn found(page: &str) -> Option<String> {
    declared(page.as_bytes())
}

#[test]
fn reads_each_way_of_declaring() {
    assert_eq!(found("<meta charset=utf-8>").as_deref(), Some("utf-8"));
    assert_eq!(found("<meta charset=\"utf-8\">").as_deref(), Some("utf-8"));
    assert_eq!(
        found("<meta charset='iso-8859-2'/>").as_deref(),
        Some("iso-8859-2")
    );
    assert_eq!(found("<META CHARSET=UTF-8>").as_deref(), Some("utf-8"));
    assert_eq!(found("<meta charset = utf-8>").as_deref(), Some("utf-8"));
    assert_eq!(
        found("<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1250\">")
            .as_deref(),
        Some("windows-1250")
    );
    assert_eq!(
        found("<html><head><title>x</title><meta name=a><meta charset=utf-8>").as_deref(),
        Some("utf-8")
    );
}

#[test]
fn only_a_meta_tag_counts() {
    assert_eq!(found("<p>charset=utf-8</p>"), None);
    assert_eq!(found("<meta name=description content=x>"), None);
    assert_eq!(found("<meta charset>"), None);
    assert_eq!(found("<meta charset=>"), None);
    assert_eq!(found(""), None);
}

#[test]
fn only_the_start_of_the_page_counts() {
    let late = format!("{}<meta charset=utf-8>", " ".repeat(2000));
    assert_eq!(found(&late), None);
    let early = format!("{}<meta charset=utf-8>", " ".repeat(900));
    assert_eq!(found(&early).as_deref(), Some("utf-8"));
}

#[test]
fn bytes_that_are_not_text_do_no_harm() {
    assert_eq!(declared(b"\xff\xfe<meta charset=\xff\xfe>\x00"), None);
    assert_eq!(declared(b"<meta charset=utf-8\xc5"), None);
    assert_eq!(declared(&[0xff; 4096]), None);
}
