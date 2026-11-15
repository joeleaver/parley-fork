// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! RINCH (rinch #2381): a line can end inside a ligature at a soft wrap opportunity, and both
//! sides then draw their characters shaped on their own, as Blink's line breaker reshapes them.
//!
//! Roboto ligates `fi`; under `word-break: break-all` there is an opportunity between `f` and
//! `i`.

use std::string::{String, ToString};
use std::vec::Vec;

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::{
    FontFamily, Layout, LayoutContext, LineHeight, PositionedLayoutItem, TextStyle, WordBreak,
};

fn build(text: &str, break_all: bool) -> Layout<ColorBrush> {
    let root = TextStyle {
        font_family: FontFamily::from(FONT_FAMILY_LIST),
        font_size: 16.,
        line_height: LineHeight::Absolute(20.),
        word_break: if break_all {
            WordBreak::BreakAll
        } else {
            WordBreak::Normal
        },
        ..TextStyle::default()
    };
    let mut fcx = create_font_context();
    let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
    let mut builder = lcx.tree_builder(&mut fcx, 1., false, &root);
    builder.push_text(text);
    let (layout, _) = builder.build();
    layout
}

/// The advance `text` has on one line, shaped as one run (no opportunity inside it).
fn width(text: &str) -> f32 {
    let mut layout = build(text, false);
    layout.break_all_lines(None);
    layout.width()
}

/// Each line's text and advance.
fn lines(layout: &Layout<ColorBrush>, text: &str) -> Vec<(String, f32)> {
    layout
        .lines()
        .map(|l| (text[l.text_range()].to_string(), l.metrics().advance))
        .collect()
}

/// The glyph ids each line draws, and the sum of their advances.
fn glyphs(layout: &Layout<ColorBrush>) -> Vec<(Vec<u32>, f32)> {
    layout
        .lines()
        .map(|l| {
            let mut ids = Vec::new();
            let mut advance = 0.;
            for item in l.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    for g in run.positioned_glyphs() {
                        ids.push(g.id);
                        advance += g.advance;
                    }
                }
            }
            (ids, advance)
        })
        .collect()
}

#[track_caller]
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

/// The control: Roboto does ligate `fi`, narrower than `f` and `i` apart.
#[test]
fn roboto_ligates_fi() {
    let layout = build("fi", false);
    let clusters = layout.data.shaped_text.shaped_clusters();
    assert_eq!(clusters.len(), 1, "one cluster for the ligature");
    assert_eq!(clusters[0].chars_range(), 0..2);
    assert!(layout.data.shaped_text.ligature_splits().is_empty());
}

#[test]
fn a_ligature_over_an_opportunity_is_split_and_draws_joined_on_one_line() {
    let text = "afib";
    let mut layout = build(text, true);
    assert_eq!(layout.data.shaped_text.ligature_splits().len(), 1);
    layout.break_all_lines(None);
    let got = lines(&layout, text);
    assert_eq!(got.len(), 1);
    // As wide as the ligature, and drawing the ligature's one glyph.
    close(got[0].1, width(text));
    let drawn = glyphs(&layout);
    assert_eq!(drawn[0].0.len(), 3, "a, the fi ligature, b");
    close(drawn[0].1, got[0].1);
}

#[test]
fn a_line_ends_inside_a_ligature_and_both_sides_reshape() {
    let text = "afib";
    let mut layout = build(text, true);
    // Every opportunity is taken.
    layout.break_all_lines(Some(1.));
    let got = lines(&layout, text);
    let texts: Vec<&str> = got.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(texts, ["a", "f", "i", "b"]);
    close(got[1].1, width("f"));
    close(got[2].1, width("i"));
    let drawn = glyphs(&layout);
    // Each line draws its own glyph, as wide as the line.
    for (line, (ids, advance)) in got.iter().zip(&drawn) {
        assert_eq!(ids.len(), 1, "{line:?}");
        close(*advance, line.1);
    }
    // On one line again, the ligature is drawn joined.
    layout.break_all_lines(None);
    assert_eq!(glyphs(&layout)[0].0.len(), 3);
    close(lines(&layout, text)[0].1, width(text));
}

/// A line that fits with the ligature broken ends inside it, though the joined ligature does not
/// fit (Blink measures the line's end reshaped).
#[test]
fn a_line_fits_with_its_end_reshaped() {
    let text = "afib";
    let mut layout = build(text, true);
    let af = width("af");
    let afi_joined = width("afi");
    assert!(af < afi_joined, "{af} {afi_joined}");
    layout.break_all_lines(Some(af + 0.1));
    let got = lines(&layout, text);
    assert_eq!(got[0].0, "af");
    close(got[0].1, af);
}

#[test]
fn min_content_breaks_inside_the_ligature_and_counts_it_after() {
    // Chrome 157 does not reshape at min-content: the ligature counts whole with the text after
    // the break (Inter's `a->b` is the arrow and `b` wide). Under `break-all` nothing follows
    // it unbroken, so the widest piece is the ligature, not `f` or `i` reshaped.
    let text = "afib";
    let layout = build(text, true);
    let widths = layout.calculate_content_widths();
    let fi = width("fi");
    assert!(fi > width("f").max(width("i")) + 1.);
    close(widths.min, fi.max(width("a")).max(width("b")));
    close(widths.max, width(text));
    // A layout broken inside the ligature measures the same.
    let mut broken = build(text, true);
    broken.break_all_lines(Some(1.));
    let again = broken.calculate_content_widths();
    close(again.min, widths.min);
    close(again.max, widths.max);
}

/// Joined, the ligature's advance is shared over its pieces, so a caret between them stands
/// inside it (Chrome 157 puts the caret inside Inter's `->` at its middle).
#[test]
fn a_caret_between_the_pieces_stands_inside_the_ligature() {
    let text = "afib";
    let mut layout = build(text, true);
    layout.break_all_lines(None);
    let a = width("a");
    let fi = width("fi");
    let at = |byte: usize| {
        crate::Cursor::from_byte_index(&layout, byte, crate::Affinity::Downstream)
            .geometry(&layout, 1.)
            .x0 as f32
    };
    close(at(1), a);
    close(at(2), a + fi / 2.);
    close(at(3), a + fi);
}

/// A glyph run after a joined ligature starts past all of it: the pieces without glyphs move it
/// by their share of the ligature's advance.
#[test]
fn a_glyph_run_after_the_ligature_starts_past_it() {
    let root = TextStyle {
        font_family: FontFamily::from(FONT_FAMILY_LIST),
        font_size: 16.,
        line_height: LineHeight::Absolute(20.),
        word_break: WordBreak::BreakAll,
        ..TextStyle::default()
    };
    let mut fcx = create_font_context();
    let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
    let mut builder = lcx.tree_builder(&mut fcx, 1., false, &root);
    builder.push_text("afi");
    builder.push_style_span(TextStyle {
        brush: ColorBrush::new(peniko::color::palette::css::RED),
        ..root.clone()
    });
    builder.push_text("b");
    builder.pop_style_span();
    let (mut layout, _) = builder.build();
    assert_eq!(layout.data.shaped_text.ligature_splits().len(), 1);
    layout.break_all_lines(None);
    let line = layout.lines().next().unwrap();
    let runs: Vec<f32> = line
        .items()
        .filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(run) => Some(run.offset()),
            PositionedLayoutItem::InlineBox(_) => None,
        })
        .collect();
    assert_eq!(runs.len(), 2, "the red `b` is a glyph run of its own");
    close(runs[1], width("afi"));
}
