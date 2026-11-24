// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! RINCH (rinch #2944): `<wbr>`s after content that overflows a line ([`crate::TextStyle::wbr`]).
//!
//! Each layout is pushed the way rinch pushes an HTML inline formatting context: a `<wbr>` is a
//! ZERO WIDTH SPACE in a span of its own that wraps, carrying the `<wbr>`'s own wrap mode as
//! [`StyleProperty::Wbr`], with rinch's two line break overrides these layouts meet
//! ([`chrome_breaks`]). Every layout is broken at 1px, and each expected list of lines is Chrome
//! 158's for the same markup in a 1px box (`^` stands for a `<wbr>`).

use std::string::String;
use std::vec::Vec;

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::{
    FontFamily, InlineBox, InlineBoxKind, Layout, LayoutContext, LineBreakContext, LineHeight,
    OverflowWrap, StyleProperty, TextStyle, TextWrapMode, VerticalAlign, WhiteSpaceCollapse,
};

const WRAP: TextWrapMode = TextWrapMode::Wrap;
const NOWRAP: TextWrapMode = TextWrapMode::NoWrap;

/// One piece of an inline formatting context.
#[derive(Clone, Copy)]
enum Piece {
    /// Text in an element of this wrap mode.
    Text(&'static str, TextWrapMode),
    /// A `<wbr>` of this wrap mode.
    Wbr(TextWrapMode),
    /// A 20px wide in-flow box in an element of this wrap mode.
    Atomic(TextWrapMode),
}
use Piece::{Atomic, Text, Wbr};

/// The opportunities rinch adds to UAX #14's for these layouts, as Chrome has them: after a run
/// of spaces whatever follows (UAX #14 has none before a ZERO WIDTH SPACE, LB7), and between two
/// `<wbr>`s.
fn chrome_breaks(cx: LineBreakContext) -> Option<bool> {
    let after_spaces = cx.before == ' ' && cx.after != ' ';
    let between_wbrs = cx.before == '\u{200B}' && cx.after == '\u{200B}';
    (after_spaces || between_wbrs).then_some(true)
}

/// `pieces` in a root of `root` wrap mode and `overflow_wrap`.
fn build(root: TextWrapMode, overflow_wrap: OverflowWrap, pieces: &[Piece]) -> Layout<ColorBrush> {
    build_styled(
        TextStyle {
            text_wrap_mode: root,
            overflow_wrap,
            ..root_style()
        },
        pieces,
    )
}

/// `pieces` in a root that wraps and preserves white space (`white-space: pre-wrap`).
fn build_preserving(pieces: &[Piece]) -> Layout<ColorBrush> {
    build_styled(
        TextStyle {
            white_space_collapse: WhiteSpaceCollapse::Preserve,
            ..root_style()
        },
        pieces,
    )
}

fn root_style() -> TextStyle<'static, 'static, ColorBrush> {
    TextStyle {
        font_family: FontFamily::from(FONT_FAMILY_LIST),
        font_size: 16.,
        line_height: LineHeight::Absolute(20.),
        ..TextStyle::default()
    }
}

fn build_styled(
    style: TextStyle<'static, 'static, ColorBrush>,
    pieces: &[Piece],
) -> Layout<ColorBrush> {
    let mut fcx = create_font_context();
    let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
    let mut builder = lcx.tree_builder(&mut fcx, 1., false, &style);
    builder.set_line_break_override(Some(&chrome_breaks));
    let mut len = 0;
    for (id, piece) in pieces.iter().enumerate() {
        match *piece {
            Text(text, mode) => {
                builder.push_style_modification_span(&[StyleProperty::TextWrapMode(mode)]);
                builder.push_text(text);
                builder.pop_style_span();
                len += text.len();
            }
            Wbr(own) => {
                builder.push_style_modification_span(&[
                    StyleProperty::TextWrapMode(WRAP),
                    StyleProperty::Wbr(Some(own)),
                ]);
                builder.push_text("\u{200B}");
                builder.pop_style_span();
                len += "\u{200B}".len();
            }
            Atomic(mode) => {
                builder.push_style_modification_span(&[StyleProperty::TextWrapMode(mode)]);
                builder.push_inline_box(InlineBox {
                    id: id as u64,
                    kind: InlineBoxKind::InFlow,
                    index: len,
                    width: 20.,
                    height: 10.,
                    baseline: None,
                    vertical_align: VerticalAlign::BASELINE,
                });
                builder.pop_style_span();
            }
        }
    }
    let (layout, text) = builder.build();
    assert_eq!(text.len(), len);
    layout
}

/// Each line's text, broken at 1px, with `^` for a `<wbr>`.
fn lines(root: TextWrapMode, overflow_wrap: OverflowWrap, pieces: &[Piece]) -> Vec<String> {
    let text: String = pieces
        .iter()
        .map(|piece| match piece {
            Text(text, _) => *text,
            Wbr(_) => "\u{200B}",
            Atomic(_) => "",
        })
        .collect();
    let mut layout = build(root, overflow_wrap, pieces);
    layout.break_all_lines(Some(1.));
    layout
        .lines()
        // A line holding only a box has no text.
        .map(|line| {
            text.get(line.text_range())
                .unwrap_or_default()
                .replace('\u{200B}', "^")
        })
        .collect()
}

fn normal(root: TextWrapMode, pieces: &[Piece]) -> Vec<String> {
    lines(root, OverflowWrap::Normal, pieces)
}

#[test]
fn text_that_does_not_wrap_keeps_only_the_wbrs_that_wrap() {
    // Blink notices the overflow of `aaaa` only at the `<wbr>`s: the line ends at the opportunity
    // after the first, a `<wbr>` that wraps stays on it, and one that does not starts the next
    // line, which then ends after it: an empty line.
    let nowrap_wbrs = [
        Text("aaaa", NOWRAP),
        Wbr(NOWRAP),
        Wbr(NOWRAP),
        Text("bbbb", NOWRAP),
    ];
    assert_eq!(normal(NOWRAP, &nowrap_wbrs), ["aaaa^", "^", "bbbb"]);
    let second_wraps = [
        Text("aaaa", NOWRAP),
        Wbr(NOWRAP),
        Wbr(WRAP),
        Text("bbbb", NOWRAP),
    ];
    assert_eq!(normal(NOWRAP, &second_wraps), ["aaaa^^", "bbbb"]);
    // In a root that wraps, only `aaaa` decides: the same lines.
    let in_normal_root = [
        Text("aaaa", NOWRAP),
        Wbr(WRAP),
        Wbr(NOWRAP),
        Text("bbbb", WRAP),
    ];
    assert_eq!(normal(WRAP, &in_normal_root), ["aaaa^", "^", "bbbb"]);
}

#[test]
fn text_that_wraps_keeps_every_wbr() {
    // `aaaa` wraps: its overflow is noticed in it, and every `<wbr>` after the line's first
    // opportunity stays on the line, whatever its own wrap mode.
    let pieces = [
        Text("aaaa", WRAP),
        Wbr(NOWRAP),
        Wbr(NOWRAP),
        Text("bbbb", WRAP),
    ];
    assert_eq!(normal(WRAP, &pieces), ["aaaa^^", "bbbb"]);
}

#[test]
fn text_that_wraps_after_the_overflow_notices_it() {
    // `aaaa` does not wrap and overflows; `cc`, which wraps, is appended after it (there is no
    // opportunity between them) and makes Blink notice: every `<wbr>` stays.
    let pieces = [
        Text("aaaa", NOWRAP),
        Text("cc", WRAP),
        Wbr(NOWRAP),
        Wbr(NOWRAP),
        Text("bbbb", WRAP),
    ];
    assert_eq!(normal(WRAP, &pieces), ["aaaacc^^", "bbbb"]);
}

#[test]
fn white_space_that_wraps_does_not_notice_the_overflow() {
    // The space after `aaaa` wraps, but white space does not make Blink notice: the line ends at
    // the opportunity after the space, before the first `<wbr>`, which does not wrap and starts
    // the next line.
    let pieces = [
        Text("aaaa", NOWRAP),
        Text(" ", WRAP),
        Wbr(NOWRAP),
        Wbr(NOWRAP),
        Text("bbbb", WRAP),
    ];
    assert_eq!(normal(WRAP, &pieces), ["aaaa ", "^^", "bbbb"]);
    // A first `<wbr>` that wraps stays after the space, and the second starts the next line.
    let pieces = [
        Text("aaaa", NOWRAP),
        Text(" ", WRAP),
        Wbr(WRAP),
        Wbr(NOWRAP),
        Text("bbbb", WRAP),
    ];
    assert_eq!(normal(WRAP, &pieces), ["aaaa ^", "^", "bbbb"]);
}

#[test]
fn a_box_that_overflows_at_the_line_start_keeps_the_wbrs_that_wrap() {
    // A box that overflows a line it starts does not end the line at once: the `<wbr>`s after it
    // that wrap stay on its line, and the first that does not starts the next.
    assert_eq!(
        normal(
            WRAP,
            &[Atomic(WRAP), Wbr(WRAP), Wbr(WRAP), Text("bbbb", WRAP)]
        ),
        ["^^", "bbbb"]
    );
    assert_eq!(
        normal(
            WRAP,
            &[Atomic(WRAP), Wbr(WRAP), Wbr(NOWRAP), Text("bbbb", WRAP)]
        ),
        ["^", "^", "bbbb"]
    );
    assert_eq!(
        normal(
            WRAP,
            &[Atomic(WRAP), Wbr(NOWRAP), Wbr(WRAP), Text("bbbb", WRAP)]
        ),
        ["", "^^", "bbbb"]
    );
    // With no `<wbr>` after it, the line still ends right after the box.
    assert_eq!(
        normal(WRAP, &[Atomic(WRAP), Text("bbbb", WRAP)]),
        ["", "bbbb"]
    );
}

#[test]
fn nothing_but_wbrs_stays_after_a_box_that_overflows_at_the_line_start() {
    // Chrome 125 (rinch #1081): `<inline-block 300px>  \n` under `white-space: pre-wrap` gives
    // the spaces and the newline a line of their own, though the spaces would hang.
    let pieces = [Atomic(WRAP), Text("  \n", WRAP)];
    let text = "  \n";
    let mut layout = build_preserving(&pieces);
    layout.break_all_lines(Some(1.));
    let got: Vec<&str> = layout
        .lines()
        .map(|line| text.get(line.text_range()).unwrap_or_default())
        .collect();
    assert_eq!(got[..2], ["", "  \n"]);
}

#[test]
fn overflow_wrap_gives_text_that_does_not_wrap_no_opportunity_before_a_wbr() {
    // `overflow-wrap` has no effect on text that does not wrap, nor so on the `<wbr>`s after it:
    // Chrome 158 lays `aaaa<wbr>bbbb` out in 1px as two lines, under `white-space: nowrap` and
    // `overflow-wrap: anywhere` alike, and `aaaa<wbr><wbr>bbbb` as three, as without
    // `overflow-wrap`.
    //
    // (Chrome breaks at the end of an element, under `overflow-wrap`, where the element's parent
    // wraps: `<nowrap>aaaa</nowrap><wbr nowrap>bbbb` has an empty line after `aaaa` in a root
    // that wraps. These pieces have no such end; parley sees no element's (rinch #2996).)
    for overflow_wrap in [OverflowWrap::Anywhere, OverflowWrap::BreakWord] {
        let one = [Text("aaaa", NOWRAP), Wbr(NOWRAP), Text("bbbb", NOWRAP)];
        let two = [
            Text("aaaa", NOWRAP),
            Wbr(NOWRAP),
            Wbr(NOWRAP),
            Text("bbbb", NOWRAP),
        ];
        for root in [NOWRAP, WRAP] {
            assert_eq!(lines(root, overflow_wrap, &one), ["aaaa^", "bbbb"]);
        }
        assert_eq!(lines(NOWRAP, overflow_wrap, &two), ["aaaa^", "^", "bbbb"]);
    }
}

#[test]
fn a_box_that_starts_a_root_that_does_not_wrap_does_not_end_the_line() {
    // The opportunity after a box that starts the paragraph is the root's and the box's parent's
    // to give: Chrome 158 lays an inline-block and `bbbb` out on one line in 1px under
    // `white-space: nowrap`, and a `<wbr>` between them gives the line its end.
    assert_eq!(
        normal(NOWRAP, &[Atomic(NOWRAP), Text("bbbb", NOWRAP)]),
        ["bbbb"]
    );
    assert_eq!(
        normal(NOWRAP, &[Atomic(NOWRAP), Wbr(NOWRAP), Text("bbbb", NOWRAP)]),
        ["^", "bbbb"]
    );
    // In a root that wraps, the line ends right after the box.
    assert_eq!(
        normal(WRAP, &[Atomic(NOWRAP), Text("bbbb", NOWRAP)]),
        ["", "bbbb"]
    );
}

#[test]
fn a_line_started_by_an_emergency_break_keeps_every_wbr() {
    // `aaaa` wraps and breaks anywhere: each `a` is a line of its own, the last ended by an
    // emergency break before it. Its overflow is noticed in it (it wraps), so both `<wbr>`s
    // stay on its line, whatever their own wrap mode, and no emergency opportunity is taken
    // before them.
    for (first, second) in [
        (WRAP, WRAP),
        (WRAP, NOWRAP),
        (NOWRAP, WRAP),
        (NOWRAP, NOWRAP),
    ] {
        assert_eq!(
            lines(
                WRAP,
                OverflowWrap::Anywhere,
                &[
                    Text("aaaa", WRAP),
                    Wbr(first),
                    Wbr(second),
                    Text("bbbb", WRAP)
                ],
            ),
            ["a", "a", "a", "a^^", "b", "b", "b", "b"],
            "<wbr>s {first:?} {second:?}"
        );
    }
}
