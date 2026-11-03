//! Reading the title and visible text of a page.

use ron_browser_core::page::Page;

fn text(source: &str) -> String {
    Page::open(source).plain_text()
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
    assert_eq!(text("<p>a</p><p>b</p>"), "a\nb");
    assert_eq!(text("a<b>b</b><i>c</i> d"), "abc d");
    assert_eq!(text("a<div>b</div>c"), "a\nb\nc");
    assert_eq!(text("a<br>b"), "a\nb");
    assert_eq!(text("<ul><li>a<li>b</ul>"), "a\nb");
}

#[test]
fn white_space_collapses_except_where_it_counts() {
    assert_eq!(text("  a \n\t b  "), "a b");
    assert_eq!(text("<p> a </p> <p> b </p>"), "a\nb");
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
