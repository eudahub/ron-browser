//! A page in a window that changes its size, where styles stay when nothing in them can change.
//! Characters are 8 wide and lines 16 high, as in the layout tests.

use ron_browser_core::layout::{FontSpec, ItemKind, TextMeasurer};
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

/// Where the text of a page with this sheet starts, at each size in turn.
fn x_at_sizes(css: &str, sizes: &[(f32, f32)]) -> Vec<f32> {
    let source = format!("<!DOCTYPE html><style>body,p{{margin:0}}{css}</style><p>x");
    let mut page = Page::open(&source);
    let mut measurer = Measurer::default();
    sizes
        .iter()
        .map(|&(width, height)| {
            page.lay_out(width, height, &mut measurer);
            page.layout()
                .items
                .iter()
                .find(|item| item.kind == ItemKind::Text)
                .unwrap()
                .x
        })
        .collect()
}

fn x_at_widths(css: &str, widths: &[f32]) -> Vec<f32> {
    let sizes: Vec<_> = widths.iter().map(|&width| (width, 500.0)).collect();
    x_at_sizes(css, &sizes)
}

#[test]
fn media_conditions_follow_the_width_both_ways() {
    let css = "@media (width>=1000px){p{margin-left:100px}}";
    assert_eq!(
        x_at_widths(css, &[800.0, 900.0, 1100.0, 1200.0, 900.0]),
        [0.0, 0.0, 100.0, 100.0, 0.0]
    );
}

#[test]
fn media_conditions_within_each_other_follow_the_width() {
    let css = "@media (min-width:500px){@media (max-width:700px){p{margin-left:5px}}}";
    assert_eq!(x_at_widths(css, &[400.0, 600.0, 800.0]), [0.0, 5.0, 0.0]);
}

#[test]
fn media_conditions_of_the_height_follow_the_height() {
    let css = "@media (height>=600px){p{margin-left:7px}}";
    assert_eq!(
        x_at_sizes(css, &[(800.0, 500.0), (800.0, 700.0), (800.0, 500.0)]),
        [0.0, 7.0, 0.0]
    );
}

#[test]
fn lengths_of_the_view_follow_the_width() {
    assert_eq!(
        x_at_widths("p{margin-left:10vw}", &[800.0, 900.0, 1000.0]),
        [80.0, 90.0, 100.0]
    );
    assert_eq!(
        x_at_widths("p{margin-left:calc(10vw + 1px)}", &[800.0, 1000.0]),
        [81.0, 101.0]
    );
    assert_eq!(
        x_at_widths("html{font-size:2vw}p{margin-left:1rem}", &[800.0, 1000.0]),
        [16.0, 20.0]
    );
}

#[test]
fn lengths_of_the_height_stay_with_the_width_and_follow_the_height() {
    assert_eq!(
        x_at_widths("p{margin-left:10vh}", &[800.0, 1000.0]),
        [50.0, 50.0]
    );
    assert_eq!(
        x_at_sizes("p{margin-left:10vh}", &[(800.0, 500.0), (800.0, 700.0)]),
        [50.0, 70.0]
    );
}

#[test]
fn the_smaller_and_larger_side_follow_both() {
    assert_eq!(
        x_at_widths("p{margin-left:10vmin}", &[300.0, 400.0, 800.0]),
        [30.0, 40.0, 50.0]
    );
    assert_eq!(
        x_at_widths("p{margin-left:10vmax}", &[300.0, 800.0, 1000.0]),
        [50.0, 80.0, 100.0]
    );
    assert_eq!(
        x_at_sizes(
            "p{margin-left:10vmin}",
            &[(300.0, 500.0), (900.0, 500.0), (900.0, 700.0)]
        ),
        [30.0, 50.0, 70.0]
    );
}

#[test]
fn what_inherits_a_length_of_the_view_follows_it() {
    let css = "html{font-size:2vw}p{margin-left:1em}";
    assert_eq!(x_at_widths(css, &[800.0, 1000.0]), [16.0, 20.0]);
}
