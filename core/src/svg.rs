//! Drawing SVG, in pages and as files, into plain images by `resvg`, which is all in Rust.
//!
//! An SVG comes from a page, so it can do little: it reaches neither the network nor the disk, as
//! references to other images are refused, and text in it does not show, as no fonts are given to
//! it. The size of what comes in and of what comes out is limited. Styles of the page reach it only
//! through the color of the `<svg>` itself and the fill of it and of what it holds.

use std::collections::HashSet;
use std::fmt::Write;

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{ImageHrefResolver, Options, Tree};
use ron::css::{Color, Styles};
use ron::html::{Document, NodeId, NodeKind};

use crate::images::DecodedImage;

/// Larger ones are not drawn: an SVG can take any amount of work in its size.
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_SIDE: f32 = 4096.0;

/// Reads an SVG, with nothing that it refers to.
fn parse(text: &str) -> Option<Tree> {
    if text.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let options = Options {
        image_href_resolver: ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..Options::default()
    };
    Tree::from_str(text, &options).ok()
}

fn draw(tree: &Tree, width: f32, height: f32) -> Option<DecodedImage> {
    let ok = |side: f32| side.is_finite() && side >= 1.0;
    if !ok(width) || !ok(height) {
        return None;
    }
    let shrink = (MAX_SIDE / width.max(height)).min(1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (out_width, out_height) = (
        ((width * shrink).ceil() as u32).max(1),
        ((height * shrink).ceil() as u32).max(1),
    );
    let mut pixmap = Pixmap::new(out_width, out_height)?;
    let size = tree.size();
    #[allow(clippy::cast_precision_loss)]
    let transform = Transform::from_scale(
        out_width as f32 / size.width(),
        out_height as f32 / size.height(),
    );
    resvg::render(tree, transform, &mut pixmap.as_mut());
    // What comes out is multiplied by its alpha, and what gets drawn is not.
    let mut rgba = pixmap.take();
    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha != 0 && alpha != 255 {
            for channel in &mut pixel[..3] {
                let value = (u32::from(*channel) * 255 + alpha / 2) / alpha;
                *channel = u8::try_from(value.min(255)).unwrap_or(255);
            }
        }
    }
    Some(DecodedImage {
        width: out_width,
        height: out_height,
        rgba,
    })
}

/// Draws a file that is an SVG at the size it says it has. Anything else gives nothing.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<DecodedImage> {
    if bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let start = &bytes[..bytes.len().min(1024)];
    if !start.windows(4).any(|window| window == b"<svg") {
        return None;
    }
    let tree = parse(std::str::from_utf8(bytes).ok()?)?;
    let size = tree.size();
    draw(&tree, size.width(), size.height())
}

fn hex(color: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

fn escape_into(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}

/// Whether XML takes this for a name, and has what it needs for its prefix. HTML takes far more, and
/// one bad name would lose the whole picture.
fn is_plain_name(name: &str) -> bool {
    let local = name
        .strip_prefix("xlink:")
        .or_else(|| name.strip_prefix("xml:"))
        .unwrap_or(name);
    local.starts_with(|first: char| first.is_ascii_alphabetic() || first == '_')
        && local
            .chars()
            .all(|part| part.is_ascii_alphanumeric() || matches!(part, '-' | '_' | '.'))
}

/// Writes an `<svg>` of a page out as a file of its own, with the color it gets from its style and
/// the fills that style sheets give to it and to what it holds. Gives nothing when it gets too long.
#[must_use]
pub fn serialize(document: &Document, styles: &Styles, id: NodeId) -> Option<String> {
    let color = styles.get(id).color;
    let mut out = String::new();
    // Each node, and whether it is being left.
    let mut waiting = vec![(id, false)];
    while let Some((node, leaving)) = waiting.pop() {
        if out.len() > MAX_SOURCE_BYTES {
            return None;
        }
        if leaving {
            out.push_str("</");
            out.push_str(&document.local_name(node));
            out.push('>');
            continue;
        }
        match document.kind(node) {
            NodeKind::Text => escape_into(&mut out, &document.decoded_text(node)),
            NodeKind::Element => {
                let name = document.local_name(node);
                // What only an editor of the file reads, such as `sodipodi:namedview`.
                if !is_plain_name(&name) || name.contains(':') {
                    continue;
                }
                out.push('<');
                out.push_str(&name);
                // HTML lets a name repeat, and XML does not.
                let mut seen = HashSet::new();
                // A style sheet goes before the attribute that it would meet in the SVG.
                if let Some(fill) = styles.get(node).fill {
                    seen.insert("fill");
                    out.push_str(" fill=\"");
                    if fill.alpha == 0 {
                        out.push_str("none");
                    } else {
                        out.push_str(&hex(fill));
                    }
                    out.push('"');
                    if fill.alpha != 0 && fill.alpha != 255 {
                        seen.insert("fill-opacity");
                        let opacity = f32::from(fill.alpha) / 255.0;
                        let _ = write!(out, " fill-opacity=\"{opacity}\"");
                    }
                }
                for attribute in document.attributes(node) {
                    let skipped = attribute.name == "xmlns"
                        || (node == id && attribute.name.eq_ignore_ascii_case("color"))
                        || !is_plain_name(attribute.name)
                        || !seen.insert(attribute.name);
                    if skipped {
                        continue;
                    }
                    out.push(' ');
                    out.push_str(attribute.name);
                    out.push_str("=\"");
                    escape_into(&mut out, &attribute.decoded_value());
                    out.push('"');
                }
                if node == id {
                    out.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
                    out.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
                    out.push_str(" color=\"");
                    out.push_str(&hex(color));
                    out.push('"');
                }
                out.push('>');
                waiting.push((node, true));
                let children: Vec<NodeId> = document.children(node).collect();
                waiting.extend(children.into_iter().rev().map(|child| (child, false)));
            }
            _ => {}
        }
    }
    Some(out)
}

/// Draws what [`serialize`] gave at this size.
#[must_use]
pub fn render(text: &str, width: f32, height: f32) -> Option<DecodedImage> {
    draw(&parse(text)?, width, height)
}
