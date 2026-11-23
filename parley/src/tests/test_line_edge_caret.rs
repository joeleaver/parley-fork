// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! rinch-patch (rinch #2950): a caret at a line's logical start or end, beside text whose bidi
//! level is not the paragraph's, is drawn at the line's edge in the paragraph's direction, and a
//! point at that edge maps to it, as in Blink: `abc` in a right-to-left paragraph has its offset
//! 0 at the line's right and its offset 3 at its left.

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::editing::Cursor;
use crate::{
    Affinity, Alignment, AlignmentOptions, BaseDirection, FontFamily, Layout, LayoutContext,
    LineHeight, TextStyle,
};

const WIDTH: f32 = 400.;

fn layout(base: BaseDirection, text: &str) -> Layout<ColorBrush> {
    let mut fcx = create_font_context();
    let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
    let root = TextStyle {
        font_family: FontFamily::from(FONT_FAMILY_LIST),
        font_size: 20.,
        line_height: LineHeight::Absolute(30.),
        ..TextStyle::default()
    };
    let mut builder = lcx.tree_builder(&mut fcx, 1., false, &root);
    builder.set_base_direction(base);
    builder.push_text(text);
    let (mut layout, _) = builder.build();
    layout.break_all_lines(Some(WIDTH));
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

fn caret_x(layout: &Layout<ColorBrush>, index: usize, affinity: Affinity) -> f32 {
    Cursor::from_byte_index(layout, index, affinity)
        .geometry(layout, 0.)
        .x0 as f32
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn latin_in_a_right_to_left_paragraph_has_its_ends_at_the_lines_edges() {
    let layout = layout(BaseDirection::Rtl, "abc");
    let width = layout.width();
    // The run is right-aligned: its left edge is at 400 less its width.
    let left = WIDTH - width;
    assert!(close(caret_x(&layout, 0, Affinity::Downstream), WIDTH));
    assert!(close(caret_x(&layout, 3, Affinity::Upstream), left));
    // Inside the run nothing changes: after the `a`.
    let a = caret_x(&layout, 1, Affinity::Downstream);
    assert!(a > left + 1. && a < WIDTH - 1., "{a}");

    // A point left of the text is its end, and one on the `c`'s right half its start.
    assert_eq!(Cursor::from_point(&layout, 5., 10.).index(), 3);
    assert_eq!(Cursor::from_point(&layout, WIDTH - 0.5, 10.).index(), 0);
    // On the `a`'s left half, the line's left edge: the end.
    assert_eq!(Cursor::from_point(&layout, left + 0.5, 10.).index(), 3);
}

#[test]
fn hebrew_in_a_left_to_right_paragraph_has_its_ends_at_the_lines_edges() {
    let layout = layout(BaseDirection::Ltr, "\u{5d0}\u{5d1}\u{5d2}");
    let width = layout.width();
    assert!(close(caret_x(&layout, 0, Affinity::Downstream), 0.));
    assert!(close(caret_x(&layout, 6, Affinity::Upstream), width));
    assert_eq!(Cursor::from_point(&layout, WIDTH - 5., 10.).index(), 6);
    assert_eq!(Cursor::from_point(&layout, 0.5, 10.).index(), 0);
}

#[test]
fn text_in_the_paragraphs_own_direction_is_unchanged() {
    // `abc` in a left-to-right paragraph: offset 0 at the left, 3 at the right, as ever.
    let layout = layout(BaseDirection::Ltr, "abc");
    assert!(close(caret_x(&layout, 0, Affinity::Downstream), 0.));
    assert!(close(
        caret_x(&layout, 3, Affinity::Upstream),
        layout.width()
    ));
    assert_eq!(Cursor::from_point(&layout, WIDTH - 5., 10.).index(), 3);
}

#[test]
fn a_mixed_line_takes_the_paragraphs_edges() {
    // `abc אב` in a right-to-left paragraph: `abc` is at the right, so offset 0 is the line's
    // right, not the `a`'s left (nor the right of the space before it, visually).
    let l = layout(BaseDirection::Rtl, "abc \u{5d0}\u{5d1}");
    assert!(close(caret_x(&l, 0, Affinity::Downstream), WIDTH));
    // `אב abc`: the end is the line's left.
    let l = layout(BaseDirection::Rtl, "\u{5d0}\u{5d1} abc");
    assert!(close(caret_x(&l, 8, Affinity::Upstream), WIDTH - l.width()));
    // A hard break: the caret before it, downstream, is still on its line's end.
    let l = layout(BaseDirection::Rtl, "abc\ndef");
    let line = l.lines().next().unwrap();
    let left = line.metrics().offset;
    assert!(close(caret_x(&l, 3, Affinity::Downstream), left));
    assert!(close(caret_x(&l, 4, Affinity::Downstream), WIDTH));
}
