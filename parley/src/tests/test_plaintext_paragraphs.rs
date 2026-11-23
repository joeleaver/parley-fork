// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! rinch-patch (rinch #2940): under `BaseDirection::Auto` each paragraph (UAX #9 P1, the text up
//! to and including a paragraph separator) takes its base direction from its own first strong
//! character, as CSS `unicode-bidi: plaintext` asks and as Blink does; its lines are aligned and
//! hang their whitespace by it. An explicit direction keeps one for every paragraph.

use alloc::vec;
use alloc::vec::Vec;

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
