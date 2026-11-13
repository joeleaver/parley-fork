// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! RINCH (rinch #2614, #2621): a line that ends at a soft hyphen draws a hyphen, which counts
//! when the line is fitted, and `hyphens: none` takes the soft hyphen's opportunity away.

use std::string::{String, ToString};
use std::vec::Vec;

use super::utils::ColorBrush;
use super::utils::fonts::{FONT_FAMILY_LIST, create_font_context};
use crate::{
    BaseDirection, FontFamily, Hyphens, Layout, LayoutContext, LineHeight, PositionedLayoutItem,
    StyleProperty, TextStyle, TextWrapMode,
};

fn root_style() -> TextStyle<'static, 'static, ColorBrush> {
    TextStyle {
        font_family: FontFamily::from(FONT_FAMILY_LIST),
        font_size: 16.,
        line_height: LineHeight::Absolute(20.),
        ..TextStyle::default()
    }
}

/// `text`, built with `root` and the base direction `dir`.
fn build_with(
    text: &str,
    root: TextStyle<'static, 'static, ColorBrush>,
    dir: BaseDirection,
) -> Layout<ColorBrush> {
    let mut fcx = create_font_context();
    let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
    let mut builder = lcx.tree_builder(&mut fcx, 1., false, &root);
    builder.set_base_direction(dir);
    builder.push_text(text);
    let (layout, _) = builder.build();
    layout
}

fn build(text: &str) -> Layout<ColorBrush> {
    build_with(text, root_style(), BaseDirection::Auto)
}

/// The advance `text` has on one line.
fn width(text: &str) -> f32 {
    let mut layout = build(text);
    layout.break_all_lines(None);
    layout.width()
}

/// The hyphen's glyph and advance in the layout's first run.
fn hyphen(layout: &Layout<ColorBrush>) -> (u32, f32) {
    layout.data.shaped_text.hyphen_glyph(0).expect("a hyphen")
}

/// Each line's text and advance.
fn lines(layout: &Layout<ColorBrush>, text: &str) -> Vec<(String, f32)> {
    layout
        .lines()
        .map(|l| (text[l.text_range()].to_string(), l.metrics().advance))
        .collect()
}

/// The glyph ids each line draws, in visual order.
fn glyphs(layout: &Layout<ColorBrush>) -> Vec<Vec<u32>> {
    layout
        .lines()
        .map(|l| {
            l.items()
                .flat_map(|item| match item {
                    PositionedLayoutItem::GlyphRun(run) => {
                        run.positioned_glyphs().map(|g| g.id).collect::<Vec<_>>()
                    }
                    PositionedLayoutItem::InlineBox(_) => Vec::new(),
                })
                .collect()
        })
        .collect()
}

#[track_caller]
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.01, "{a} != {b}");
}

#[test]
fn a_line_ending_at_a_soft_hyphen_draws_a_hyphen() {
    let text = "Extra\u{ad}ordinary";
    let mut layout = build(text);
    let (glyph, h) = hyphen(&layout);
    assert!(h > 1.0, "the hyphen has an advance");
    let extra = width("Extra");
    layout.break_all_lines(Some(extra + h + 1.));
    let got = lines(&layout, text);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].0, "Extra\u{ad}");
    close(got[0].1, extra + h);
    close(got[1].1, width("ordinary"));
    let drawn = glyphs(&layout);
    assert_eq!(
        drawn[0].last(),
        Some(&glyph),
        "the first line ends in the hyphen"
    );
    assert!(!drawn[1].contains(&glyph), "the last line draws none");
    // On one line again, the soft hyphen draws nothing.
    layout.break_all_lines(None);
    let got = lines(&layout, text);
    assert_eq!(got.len(), 1);
    close(got[0].1, width("Extraordinary"));
    assert!(!glyphs(&layout)[0].contains(&glyph));
}

#[test]
fn the_line_must_fit_with_its_hyphen() {
    let text = "123 Ex\u{ad}traordinary";
    let layout = build(text);
    let (_, h) = hyphen(&layout);
    let piece = width("123 Ex");
    // Room for `123 Ex` but not for its hyphen: the break before `Ex` is taken.
    let mut layout = build(text);
    layout.break_all_lines(Some(piece + h / 2.));
    assert_eq!(lines(&layout, text)[0].0, "123 ");
    // With room for the hyphen, the soft hyphen's.
    layout.break_all_lines(Some(piece + h + 0.5));
    let got = lines(&layout, text);
    assert_eq!(got[0].0, "123 Ex\u{ad}");
    close(got[0].1, piece + h);
}

#[test]
fn an_overflowing_line_takes_its_first_soft_hyphen() {
    // Chrome 157: `Extra&shy;ordinary` in 1px is `Extra-` / `ordinary`.
    let text = "Extra\u{ad}ordinary";
    let mut layout = build(text);
    let (_, h) = hyphen(&layout);
    layout.break_all_lines(Some(1.));
    let got = lines(&layout, text);
    assert_eq!(got.len(), 2);
    close(got[0].1, width("Extra") + h);
}

#[test]
fn hyphens_none_has_no_opportunity_at_a_soft_hyphen() {
    let text = "Extra\u{ad}ordinary";
    let mut layout = build_with(
        text,
        TextStyle {
            hyphens: Hyphens::None,
            ..root_style()
        },
        BaseDirection::Auto,
    );
    layout.break_all_lines(Some(1.));
    assert_eq!(lines(&layout, text).len(), 1);
    // `auto` has no dictionary: a soft hyphen is still an opportunity.
    let mut layout = build_with(
        text,
        TextStyle {
            hyphens: Hyphens::Auto,
            ..root_style()
        },
        BaseDirection::Auto,
    );
    layout.break_all_lines(Some(1.));
    assert_eq!(lines(&layout, text).len(), 2);
}

#[test]
fn only_the_soft_hyphens_own_style_takes_it_away() {
    // Chrome 157: `hyphens: none` on the soft hyphen's span keeps the word whole, on the text
    // before it does not.
    let split = |none_before: bool| {
        let mut fcx = create_font_context();
        let mut lcx: LayoutContext<ColorBrush> = LayoutContext::new();
        let root = root_style();
        let mut builder = lcx.tree_builder(&mut fcx, 1., false, &root);
        let none = [StyleProperty::Hyphens(Hyphens::None)];
        if none_before {
            builder.push_style_modification_span(&none);
            builder.push_text("Extra");
            builder.pop_style_span();
            builder.push_text("\u{ad}ordinary");
        } else {
            builder.push_text("Extra");
            builder.push_style_modification_span(&none);
            builder.push_text("\u{ad}");
            builder.pop_style_span();
            builder.push_text("ordinary");
        }
        let (mut layout, _) = builder.build();
        layout.break_all_lines(Some(1.));
        layout.len()
    };
    assert_eq!(split(true), 2);
    assert_eq!(split(false), 1);
}

#[test]
fn a_soft_hyphen_in_text_that_does_not_wrap_draws_none() {
    // Chrome 157: `<span nowrap>Extra&shy;</span>ordinary` breaks after the soft hyphen with no
    // hyphen. Here the line is ended there by length.
    let text = "Extra\u{ad}ordinary";
    let mut layout = build_with(
        text,
        TextStyle {
            text_wrap_mode: TextWrapMode::NoWrap,
            ..root_style()
        },
        BaseDirection::Auto,
    );
    let mut breaker = layout.break_lines();
    breaker.break_next_with_length(6).unwrap();
    breaker.break_next_with_length(8).unwrap();
    breaker.finish();
    let got = lines(&layout, text);
    assert_eq!(got[0].0, "Extra\u{ad}");
    close(got[0].1, width("Extra"));
}

#[test]
fn a_line_committed_by_length_at_a_soft_hyphen_draws_one() {
    let text = "Extra\u{ad}ordinary";
    let mut layout = build(text);
    let (_, h) = hyphen(&layout);
    let mut breaker = layout.break_lines();
    breaker.break_next_with_length(6).unwrap();
    breaker.break_next_with_length(8).unwrap();
    breaker.finish();
    let got = lines(&layout, text);
    close(got[0].1, width("Extra") + h);
}

#[test]
fn a_reverted_line_puts_its_soft_hyphen_back() {
    let text = "Extra\u{ad}ordinary";
    let mut layout = build(text);
    let cluster = layout.data.soft_hyphens[0].cluster as usize;
    let mut breaker = layout.break_lines();
    let start = breaker.state().clone();
    breaker.state_mut().set_layout_max_advance(1.);
    breaker.state_mut().set_line_max_advance(1.);
    breaker.break_next().unwrap();
    breaker.revert_to(start);
    // One line of it all: no hyphen anywhere.
    breaker.state_mut().set_layout_max_advance(1000.);
    breaker.state_mut().set_line_max_advance(1000.);
    breaker.break_next().unwrap();
    breaker.finish();
    assert_eq!(layout.len(), 1);
    assert_eq!(
        layout.data.shaped_text.shaped_clusters()[cluster].advance,
        0.
    );
    close(
        layout.lines().next().unwrap().metrics().advance,
        width("Extraordinary"),
    );
}

#[test]
fn the_min_content_width_counts_the_hyphen() {
    let text = "Extraordinary\u{ad}ab";
    // The hyphenated piece, as the line breaker lays it out at 1px.
    let mut broken = build(text);
    broken.break_all_lines(Some(1.));
    let piece = broken.lines().next().unwrap().metrics().advance;
    let (_, h) = hyphen(&broken);
    assert!(
        piece > width("Extraordinary") + h / 2.,
        "the piece counts its hyphen"
    );
    let mut layout = build(text);
    let widths = layout.calculate_content_widths();
    close(widths.min, piece);
    close(widths.max, width(text));
    // Broken first, the same: the hyphen drawn then is not counted mid-line.
    layout.break_all_lines(Some(1.));
    let widths = layout.calculate_content_widths();
    close(widths.min, piece);
    close(widths.max, width(text));
}

#[test]
fn a_right_to_left_line_draws_its_hyphen_at_its_left() {
    // Chrome 157: `שלום&shy;עולם` broken at the soft hyphen draws the hyphen left of `שלום`,
    // the end of the right-to-left run. Here in Arabic, which the test fonts have.
    let text = "\u{633}\u{644}\u{627}\u{645}\u{ad}\u{639}\u{627}\u{644}\u{645}";
    let mut layout = build_with(text, root_style(), BaseDirection::Rtl);
    layout.break_all_lines(Some(1.));
    assert_eq!(layout.len(), 2);
    let k = layout.data.soft_hyphens[0].clone();
    let glyph = k.hyphen.glyph_offset;
    let first = &glyphs(&layout)[0];
    assert_eq!(
        first.first(),
        Some(&glyph),
        "the hyphen is the line's leftmost glyph"
    );
    let line = layout.lines().next().unwrap();
    let run_advance: f32 = line
        .items()
        .map(|i| match i {
            PositionedLayoutItem::GlyphRun(r) => r.advance(),
            PositionedLayoutItem::InlineBox(b) => b.width,
        })
        .sum();
    close(line.metrics().advance, run_advance);
    close(
        k.hyphen.advance,
        layout.data.shaped_text.shaped_clusters()[k.cluster as usize].advance,
    );
}

#[test]
fn clusters_and_the_breaker_report_the_hyphen() {
    let text = "Extra\u{ad}ordinary";
    let mut layout = build(text);
    let (_, h) = hyphen(&layout);
    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(1.);
    breaker.state_mut().set_line_max_advance(1.);
    breaker.break_next().unwrap();
    close(breaker.prior_line_hyphen(), h);
    breaker.break_next().unwrap();
    assert_eq!(breaker.prior_line_hyphen(), 0., "the last line draws none");
    breaker.finish();
    let shy = crate::Cluster::from_byte_index(&layout, 5).unwrap();
    let info = shy.soft_hyphen().expect("a soft hyphen");
    assert!(info.drawn);
    close(info.advance, h);
    close(shy.advance(), h);
    assert_eq!(
        crate::Cluster::from_byte_index(&layout, 0)
            .unwrap()
            .soft_hyphen(),
        None
    );
    layout.break_all_lines(None);
    let shy = crate::Cluster::from_byte_index(&layout, 5).unwrap();
    assert!(!shy.soft_hyphen().unwrap().drawn);
    assert_eq!(shy.advance(), 0.);
}
