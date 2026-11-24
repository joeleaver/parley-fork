// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! rinch-patch (rinch #2940): under `BaseDirection::Auto` each paragraph (UAX #9 P1, the text up
//! to and including a paragraph separator) takes its base direction from its own first strong
//! character, as CSS `unicode-bidi: plaintext` asks and as Blink does; its lines are aligned and
//! hang their whitespace by it. An explicit direction keeps one for every paragraph.

use alloc::vec;
use alloc::vec::Vec;
use core::ops::Range;

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::{
    Alignment, AlignmentOptions, BaseDirection, FontFamily, InlineBox, InlineBoxKind,
    LayoutContext, LineHeight, PositionedLayoutItem, TextStyle, VerticalAlign,
};

const WIDTH: f32 = 400.;

/// A paragraph of `parts` — text, or `None` for a 10px box — with base direction `base`, broken
/// in 400px and start-aligned: each line's direction, and each box's x.
fn lay_out(base: BaseDirection, parts: &[Option<&str>]) -> (Vec<bool>, Vec<f32>) {
    lay_out_reopening(base, parts, &[])
}

/// [`lay_out`], with the bidi controls at `reopened` marked as an element's
/// (`TreeBuilder::set_reopened_bidi_controls`).
fn lay_out_reopening(
    base: BaseDirection,
    parts: &[Option<&str>],
    reopened: &[Range<usize>],
) -> (Vec<bool>, Vec<f32>) {
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
    builder.set_reopened_bidi_controls(reopened);
    let mut id = 0;
    for part in parts {
        match part {
            Some(text) => builder.push_text(text),
            None => {
                builder.push_inline_box(InlineBox {
                    id,
                    kind: InlineBoxKind::InFlow,
                    index: 0,
                    width: 10.,
                    height: 10.,
                    baseline: None,
                    vertical_align: VerticalAlign::BASELINE,
                });
                id += 1;
            }
        }
    }
    let (mut layout, _) = builder.build();
    layout.break_all_lines(Some(WIDTH));
    layout.align(Alignment::Start, AlignmentOptions::default());
    let mut rtl = Vec::new();
    let mut xs = Vec::new();
    for line in layout.lines() {
        assert_eq!(line.is_rtl(), line.base_level().is_rtl());
        rtl.push(line.is_rtl());
        for item in line.items() {
            if let PositionedLayoutItem::InlineBox(b) = item {
                xs.push(b.x);
            }
        }
    }
    (rtl, xs)
}

#[test]
fn each_paragraph_takes_its_own_direction() {
    // A Latin paragraph after a Hebrew one starts at the left.
    assert_eq!(
        lay_out(BaseDirection::Auto, &[Some("א\n"), None, Some("abc")]),
        (vec![true, false], vec![0.])
    );
    // A Hebrew paragraph after a Latin one: its box, before the `א`, is at the right.
    assert_eq!(
        lay_out(
            BaseDirection::Auto,
            &[None, Some("abc\n"), None, Some(" א")]
        ),
        (vec![false, true], vec![0., WIDTH - 10.])
    );
    // The same without boxes, the other path through the analysis.
    assert_eq!(
        lay_out(BaseDirection::Auto, &[Some("abc\nא\nabc")]),
        (vec![false, true, false], vec![])
    );
}

#[test]
fn a_paragraph_without_a_strong_character_is_left_to_right() {
    assert_eq!(
        lay_out(BaseDirection::Auto, &[Some("א\n"), None, Some("12")]),
        (vec![true, false], vec![0.])
    );
    // The empty last paragraph after a separator at the end, too.
    assert_eq!(
        lay_out(BaseDirection::Auto, &[Some("א\n")]),
        (vec![true, false], vec![])
    );
}

#[test]
fn cr_lf_is_one_separator() {
    // CR LF ends one paragraph, so no empty one lies between them: the `abc` line is the second.
    assert_eq!(
        lay_out(BaseDirection::Auto, &[Some("א\r\n"), None, Some("abc")]),
        (vec![true, false], vec![0.])
    );
}

#[test]
fn an_explicit_direction_is_every_paragraphs() {
    assert_eq!(
        lay_out(BaseDirection::Rtl, &[Some("abc\n"), None, Some(" א")]),
        (vec![true, true], vec![WIDTH - 10.])
    );
    assert_eq!(
        lay_out(BaseDirection::Ltr, &[Some("א\n"), None, Some("abc")]),
        (vec![false, false], vec![0.])
    );
}

// RINCH (rinch #2962): under an explicit direction the paragraphs are resolved on their own too,
// each at that direction: a neutral that starts one takes the block's direction, not the strong
// text's before the separator.
#[test]
fn an_explicit_directions_paragraphs_are_resolved_on_their_own() {
    assert_eq!(
        lay_out(BaseDirection::Rtl, &[Some("abc\n"), None, Some("abc")]),
        (vec![true, true], vec![WIDTH - 10.])
    );
    assert_eq!(
        lay_out(BaseDirection::Ltr, &[Some("א\n"), None, Some("א")]),
        (vec![false, false], vec![0.])
    );
}

// RINCH (rinch #2962): an element's embedding open at a separator is reopened after it, so it runs
// across: the second line's box and ` d` stay left to right inside the right-to-left line, the
// box first.
#[test]
fn an_elements_embedding_runs_across_a_separator() {
    // LRE `abc` LF, the box, ` d` PDF: the controls at 0..3 and 9..12.
    let marked = [0..3, 9..12];
    let (_, xs) = lay_out_reopening(
        BaseDirection::Rtl,
        &[Some("\u{202a}abc\n"), None, Some(" d\u{202c}")],
        &marked,
    );
    assert!(xs[0] < WIDTH - 10. - 1., "{xs:?}");
    // An isolate too.
    let (_, xs) = lay_out_reopening(
        BaseDirection::Rtl,
        &[Some("\u{2066}abc\n"), None, Some(" d\u{2069}")],
        &marked,
    );
    assert!(xs[0] < WIDTH - 10. - 1., "{xs:?}");
    // Closed before the separator, it is not reopened: the box is at the right.
    let (_, xs) = lay_out_reopening(
        BaseDirection::Rtl,
        &[Some("\u{202a}abc\u{202c}\n"), None, Some(" d")],
        &[0..3, 6..9],
    );
    assert_eq!(xs, vec![WIDTH - 10.]);
    // Carried across two separators.
    let (_, xs) = lay_out_reopening(
        BaseDirection::Rtl,
        &[Some("\u{202a}abc\ne\n"), None, Some(" d\u{202c}")],
        &[0..3, 11..14],
    );
    assert!(xs[0] < WIDTH - 10. - 1., "{xs:?}");
}

// RINCH (rinch #2962): the text's own controls end at a separator (UAX #9 X8), as in a browser: the
// box after it starts a right-to-left paragraph of its own, at the right.
#[test]
fn the_texts_own_controls_end_at_a_separator() {
    for (open, close) in [("\u{202a}", "\u{202c}"), ("\u{2066}", "\u{2069}")] {
        let first = alloc::format!("{open}abc\n");
        let last = alloc::format!(" d{close}");
        let (_, xs) = lay_out(BaseDirection::Rtl, &[Some(&first), None, Some(&last)]);
        assert_eq!(xs, vec![WIDTH - 10.], "{open:?}");
    }
    // Only the marked one of two is reopened: an author's LRE inside an element's RLE.
    let (_, xs) = lay_out_reopening(
        BaseDirection::Ltr,
        &[
            Some("\u{202b}\u{202a}abc\n"),
            None,
            Some(" d\u{202c}\u{202c}"),
        ],
        &[0..3, 15..18],
    );
    // The box opens a right-to-left embedding's line: at the line's right.
    assert!(xs[0] > 1., "{xs:?}");
}
