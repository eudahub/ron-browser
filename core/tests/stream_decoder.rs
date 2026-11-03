//! Turning a page into text as its pieces come, wherever they happen to be cut.

use ron_browser_core::charset::{EncodingSource, StreamDecoder, encoding_for_label};

/// Gives the bytes to a decoder in pieces of this size, and returns all of the text.
fn in_pieces(bytes: &[u8], size: usize, server_charset: &str) -> (String, StreamDecoder) {
    let mut decoder = StreamDecoder::new(server_charset);
    let mut text = String::new();
    for piece in bytes.chunks(size) {
        text += &decoder.push(piece);
    }
    text += &decoder.finish();
    (text, decoder)
}

#[test]
fn a_character_cut_by_a_piece_stays_whole() {
    let page = "<p>zażółć gęślą jaźń 🦀</p>";
    for size in 1..8 {
        let (text, decoder) = in_pieces(page.as_bytes(), size, "utf-8");
        assert_eq!(text, page, "{size}");
        assert!(!decoder.replaced());
    }
}

#[test]
fn utf16_is_cut_only_between_characters() {
    let page = "aż🦀b";
    let bytes: Vec<u8> = page.encode_utf16().flat_map(u16::to_le_bytes).collect();
    for size in 1..6 {
        let (text, _) = in_pieces(&bytes, size, "utf-16le");
        assert_eq!(text, page, "{size}");
    }
}

#[test]
fn what_is_invalid_is_replaced_and_noted() {
    let (text, decoder) = in_pieces(b"a\xFFb", 1, "utf-8");
    assert_eq!(text, "a\u{FFFD}b");
    assert!(decoder.replaced());
}

#[test]
fn a_page_cut_off_inside_a_character_ends_with_a_replacement() {
    let (text, decoder) = in_pieces("ż".as_bytes().split_last().unwrap().1, 1, "utf-8");
    assert_eq!(text, "\u{FFFD}");
    assert!(decoder.replaced());
}

#[test]
fn text_comes_at_once_when_the_server_names_the_encoding() {
    let mut decoder = StreamDecoder::new("windows-1250");
    assert_eq!(decoder.push(b"\xB3"), "ł");
    assert_eq!(decoder.chosen().unwrap().source, EncodingSource::Server);
}

#[test]
fn a_byte_order_mark_decides_and_does_not_show() {
    let mut decoder = StreamDecoder::new("windows-1250");
    assert_eq!(decoder.push(b"\xEF\xBB\xBFa"), "a");
    assert_eq!(
        decoder.chosen().unwrap().source,
        EncodingSource::ByteOrderMark
    );
}

#[test]
fn a_declaration_decides_once_enough_has_come() {
    let mut page = b"<meta charset=iso-8859-2>".to_vec();
    page.resize(2000, b' ');
    page.push(0xB3);
    let mut decoder = StreamDecoder::new("");
    assert_eq!(decoder.push(&page[..100]), "");
    let text = decoder.push(&page[100..]);
    assert!(text.ends_with('ł'), "{text}");
    assert_eq!(decoder.chosen().unwrap().source, EncodingSource::Declared);
}

#[test]
fn a_short_page_that_says_nothing_is_looked_at_when_it_ends() {
    let (text, decoder) = in_pieces("zażółć".as_bytes(), 3, "");
    assert_eq!(text, "zażółć");
    assert_eq!(decoder.chosen().unwrap().source, EncodingSource::ValidUtf8);
}

#[test]
fn labels_of_the_web_map_to_encodings() {
    assert_eq!(encoding_for_label("UTF-8").as_deref(), Some("utf8"));
    assert_eq!(encoding_for_label(" latin1 ").as_deref(), Some("cp1252"));
    assert_eq!(
        encoding_for_label("Windows-1250").as_deref(),
        Some("cp1250")
    );
    assert_eq!(encoding_for_label("no-such-thing"), None);
}
