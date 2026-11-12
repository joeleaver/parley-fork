// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! rinch-patch (rinch #2542): an inline box's bidi level is the one UAX #9 gives the object
//! replacement character it stands for, a neutral (N1/N2), and the line is reordered by it.

use alloc::vec;
use alloc::vec::Vec;

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::{
    BaseDirection, FontFamily, InlineBox, InlineBoxKind, LayoutContext, LineHeight,
    PositionedLayoutItem, TextStyle, VerticalAlign,
};

/// A paragraph of `parts` — text, or `None` for a 10px box — with base direction `base`: each
/// box's resolved level, and the boxes' ids in visual order (left to right).
fn boxes(base: BaseDirection, parts: &[Option<&str>]) -> (Vec<u8>, Vec<u64>) {
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
    layout.break_all_lines(None);
    let mut found: Vec<(u64, u8, f32)> = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::InlineBox(b) = item {
                assert_eq!(b.is_rtl(), b.bidi_level.is_rtl());
                found.push((b.id, b.bidi_level.to_u8(), b.x));
            }
        }
    }
    let mut levels = found.clone();
    levels.sort_by_key(|f| f.0);
    found.sort_by(|a, b| a.2.total_cmp(&b.2));
    (
        levels.iter().map(|f| f.1).collect(),
        found.iter().map(|f| f.0).collect(),
    )
}

#[test]
fn boxes_alone_in_an_rtl_paragraph_run_right_to_left() {
    assert_eq!(
        boxes(BaseDirection::Rtl, &[None, None, None]),
        (vec![1, 1, 1], vec![2, 1, 0])
    );
    assert_eq!(
        boxes(BaseDirection::Ltr, &[None, None]),
        (vec![0, 0], vec![0, 1])
    );
}

#[test]
fn numbers_beside_a_box_count_as_right_to_left() {
    // `x א<A>1234567890`: between a Hebrew letter and European digits (level 2), right to left.
    let ltr = BaseDirection::Ltr;
    assert_eq!(
        boxes(ltr, &[Some("x \u{5d0}"), None, Some("1234567890")]).0,
        vec![1]
    );
    // Arabic-Indic digits are Arabic numbers.
    assert_eq!(
        boxes(
            ltr,
            &[Some("x \u{5d0}"), None, Some("\u{661}\u{662}\u{663}")]
        )
        .0,
        vec![1]
    );
    // A European number after a Latin letter is left to right (W7).
    assert_eq!(boxes(ltr, &[Some("x"), None, Some("123")]).0, vec![0]);
    assert_eq!(
        boxes(BaseDirection::Rtl, &[Some("abc"), None, Some("123")]).0,
        vec![2]
    );
    assert_eq!(
        boxes(
            BaseDirection::Rtl,
            &[Some("\u{5d0}\u{5d1}"), None, Some("123")]
        )
        .0,
        vec![1]
    );
}

#[test]
fn a_box_between_directions_takes_the_paragraphs() {
    assert_eq!(
        boxes(
            BaseDirection::Ltr,
            &[Some("abc"), None, Some("\u{5d0}\u{5d1}")]
        )
        .0,
        vec![0]
    );
    assert_eq!(
        boxes(
            BaseDirection::Rtl,
            &[Some("abc"), None, Some("\u{5d0}\u{5d1}")]
        )
        .0,
        vec![1]
    );
    // The paragraph's start and end run in its own direction.
    assert_eq!(boxes(BaseDirection::Rtl, &[None, Some("abc")]).0, vec![1]);
    assert_eq!(boxes(BaseDirection::Rtl, &[Some("abc"), None]).0, vec![1]);
    assert_eq!(
        boxes(BaseDirection::Ltr, &[None, Some("\u{5d0}\u{5d1}")]).0,
        vec![0]
    );
    // Between two runs of one direction, that direction.
    assert_eq!(
        boxes(BaseDirection::Rtl, &[Some("ab"), None, Some("cd")]).0,
        vec![2]
    );
    assert_eq!(
        boxes(
            BaseDirection::Ltr,
            &[Some("\u{5d0}"), None, Some("\u{5d1}")]
        )
        .0,
        vec![1]
    );
}

#[test]
fn boxes_take_the_embedding_controls_around_them() {
    // rinch #2588: a box is resolved with the explicit embeddings, isolates and overrides around
    // it, as the object replacement character it stands for. Chrome 157, `direction: rtl` with
    // `a <span direction:ltr unicode-bidi:embed>b e</span>`: `a` at the right, then `b e` left
    // to right (LRE … PDF).
    assert_eq!(
        boxes(
            BaseDirection::Rtl,
            &[None, Some("\u{202a}"), None, None, Some("\u{202c}")]
        ),
        (vec![1, 2, 2], vec![1, 2, 0])
    );
    // A `ltr` paragraph, `a <span direction:rtl unicode-bidi:isolate>b e</span>`: `b e` right to
    // left after `a` (RLI … PDI).
    assert_eq!(
        boxes(
            BaseDirection::Ltr,
            &[None, Some("\u{2067}"), None, None, Some("\u{2069}")]
        ),
        (vec![0, 1, 1], vec![0, 2, 1])
    );
    // An override makes a box strong: RLO … PDF in a `ltr` paragraph, between Latin letters.
    assert_eq!(
        boxes(
            BaseDirection::Ltr,
            &[Some("ab\u{202e}"), None, None, Some("\u{202c}cd")]
        ),
        (vec![1, 1], vec![1, 0])
    );
    // The isolate's content does not reach the box beside it: `abc RLI א PDI <A> def` keeps the
    // box left to right, between `abc` and `def`.
    assert_eq!(
        boxes(
            BaseDirection::Ltr,
            &[Some("abc \u{2067}\u{5d0}\u{2069}"), None, Some("def")]
        )
        .0,
        vec![0]
    );
}

#[test]
fn a_box_beside_text_changes_how_the_neutrals_between_resolve() {
    // `א <A> 1`: the space and the box are one run of neutrals between R and EN (as R), so both
    // are right to left in a `ltr` paragraph.
    assert_eq!(
        boxes(BaseDirection::Ltr, &[Some("\u{5d0} "), None, Some(" 1")]).0,
        vec![1]
    );
}
