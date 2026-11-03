//! An opened page: its tree and styles.

use ron::css::{
    Display, Styles, Viewport, Visibility, WhiteSpace, compute_styles, document_stylesheets,
};
use ron::html::{Document, NodeId, NodeKind, Tag, parse};

pub struct Page {
    document: Document,
    styles: Styles,
}

impl Page {
    /// Parses a page and computes its styles.
    #[must_use]
    pub fn open(utf8: &str) -> Self {
        let document = parse(utf8);
        let viewport = Viewport::default();
        let sheets = document_stylesheets(&document, viewport);
        let styles = compute_styles(&document, &sheets, viewport);
        Self { document, styles }
    }

    /// The title, with its white space collapsed.
    #[must_use]
    pub fn title(&self) -> String {
        let document = &self.document;
        let title = document
            .descendants(document.root())
            .find(|&id| document.tag(id) == Tag::Title)
            .and_then(|title| document.first_child(title));
        title.map_or_else(String::new, |text| {
            collapse_spaces(&document.decoded_text(text))
                .trim()
                .to_owned()
        })
    }

    /// The visible text in document order, with a line for each block. This stands in
    /// until there is layout. Walks the tree without recursion.
    #[must_use]
    pub fn plain_text(&self) -> String {
        let document = &self.document;
        let mut out = String::new();
        let mut current = document.first_child(document.root());
        while let Some(id) = current {
            let mut descend = false;
            match document.kind(id) {
                NodeKind::Element => {
                    let display = self.styles.get(id).display;
                    if display != Display::None {
                        if is_block(display) {
                            end_line(&mut out);
                        }
                        if document.tag(id) == Tag::Br {
                            out.push('\n');
                        }
                        descend = true;
                    }
                }
                NodeKind::Text => self.add_text(id, &mut out),
                _ => {}
            }
            current = if descend {
                document.first_child(id)
            } else {
                None
            }
            .or_else(|| self.next_after(id, &mut out));
        }
        out.trim_end().to_owned()
    }

    /// Finds the next node once `id` and everything below it are done, ending the lines
    /// of the blocks it leaves on the way.
    fn next_after(&self, id: NodeId, out: &mut String) -> Option<NodeId> {
        let document = &self.document;
        let mut node = id;
        loop {
            if let Some(sibling) = document.next_sibling(node) {
                return Some(sibling);
            }
            node = document.parent(node)?;
            if document.kind(node) == NodeKind::Element && is_block(self.styles.get(node).display) {
                end_line(out);
            }
        }
    }

    fn add_text(&self, id: NodeId, out: &mut String) {
        let style = self.styles.get(id);
        if style.visibility != Visibility::Visible {
            return;
        }
        let text = self.document.decoded_text(id);
        if matches!(style.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap) {
            out.push_str(&text);
            return;
        }
        let text = collapse_spaces(&text);
        // White space at the start of a line, or after other white space, shows nothing.
        let at_line_start = out.is_empty() || out.ends_with(['\n', ' ']);
        out.push_str(if at_line_start {
            text.trim_start_matches(' ')
        } else {
            &text
        });
    }
}

fn is_block(display: Display) -> bool {
    !matches!(
        display,
        Display::Inline | Display::InlineBlock | Display::Contents
    )
}

fn end_line(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        let kept = out.trim_end_matches(' ').len();
        out.truncate(kept);
        out.push('\n');
    }
}

fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for character in text.chars() {
        if matches!(character, ' ' | '\t' | '\n' | '\r' | '\x0c') {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(character);
            in_space = false;
        }
    }
    out
}
