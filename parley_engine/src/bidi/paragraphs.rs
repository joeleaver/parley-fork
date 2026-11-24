// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! RINCH (rinch #2940, #2962): splitting the text into paragraphs (UAX #9 P1)
//! and carrying the explicit controls a caller generated across a paragraph
//! separator.

use core::iter::Peekable;
use core::ops::Range;

use alloc::vec::Vec;
use icu_properties::props::{BidiClass, BidiMirroringGlyph};
use parlance::{BaseDirection, BidiLevel};

use super::BidiResolver;
use super::classes::is_isolate_initiator;

/// A character with its bidi class and its mirroring and bracket data.
pub(crate) type BidiChar = (char, (BidiClass, BidiMirroringGlyph));

impl BidiResolver {
    /// Resolves the text with the specified base direction and precomputed
    /// types. Each item is a character and whether it is a control to reopen
    /// after a paragraph separator ([`ReopenedControls`]).
    ///
    /// RINCH (rinch #2940): each paragraph (UAX #9 P1: the text up to and
    /// including a paragraph separator, class B, with CR LF one separator) is
    /// resolved on its own. Under [`BaseDirection::Auto`] its base level comes
    /// from its own first strong character (P2, P3), and [`Self::paragraphs`]
    /// lists them, as CSS `unicode-bidi: plaintext` asks and as Blink does.
    /// RINCH (rinch #2962): under an explicit direction each is resolved at
    /// that direction's level, so a neutral that starts a paragraph takes it
    /// rather than the strong text's before the separator.
    ///
    /// X8 ends every explicit embedding, override and isolate at a separator.
    /// The ones marked as reopened that are still open there (by X6a/X7
    /// nesting) are opened again at the start of the next paragraph, as Blink
    /// reopens an element's bidi context after a forced break, so an element's
    /// `unicode-bidi` runs across it while the text's own controls end.
    ///
    /// The characters are streamed into each paragraph's resolution: nothing
    /// is collected, and text with no separator is resolved in one pass.
    pub(crate) fn resolve(
        &mut self,
        chars: impl Iterator<Item = (BidiChar, bool)>,
        base_direction: BaseDirection,
    ) {
        let auto = base_direction == BaseDirection::Auto;
        let mut chars = chars.peekable();
        // The explicit controls open so far, innermost last, each with
        // whether it is reopened after a separator.
        let mut open: Vec<(BidiChar, bool)> = Vec::new();
        // The controls reopened at the start of the paragraph being resolved.
        let mut carried: Vec<BidiChar> = Vec::new();
        // Every paragraph's levels, once there is more than one.
        let mut levels: Vec<BidiLevel> = Vec::new();
        self.paragraphs.clear();
        let mut start = 0;
        loop {
            let at_separator = {
                let mut paragraph = Paragraph {
                    chars: &mut chars,
                    open: &mut open,
                    ended: false,
                };
                self.resolve_paragraph(
                    carried.iter().copied().chain(&mut paragraph),
                    base_direction,
                );
                paragraph.ended
            };
            // An empty last paragraph follows a separator at the end.
            let empty_last = auto && at_separator && chars.peek().is_none();
            if self.paragraphs.is_empty() && chars.peek().is_none() && !empty_last {
                // One paragraph: its levels are the text's.
                self.paragraphs.push((0, self.base_level));
                return;
            }
            levels.extend_from_slice(&self.levels[carried.len()..]);
            if auto || self.paragraphs.is_empty() {
                self.paragraphs.push((start, self.base_level));
            }
            start += self.levels.len() - carried.len();
            if empty_last {
                self.paragraphs.push((start, BidiLevel::new(0)));
            }
            if chars.peek().is_none() {
                break;
            }
            // X8: the separator ends every control; those marked are reopened.
            open.retain(|&(_, reopened)| reopened);
            carried.clear();
            carried.extend(open.iter().map(|&(c, _)| c));
        }
        self.base_level = self.paragraphs[0].1;
        self.levels = levels;
    }

    /// RINCH (rinch #2940): each paragraph [`Self::resolve`] found, as the index
    /// of its first character (in the order the characters were given) and its
    /// base level, in order; the first starts at 0. A text that ends in a
    /// paragraph separator has an empty last paragraph at its length. Under an
    /// explicit direction there is one entry, at 0, as every paragraph has
    /// that direction's level.
    pub(crate) fn paragraphs(&self) -> &[(usize, BidiLevel)] {
        &self.paragraphs
    }
}

/// The characters of one paragraph, up to and including its separator, taken
/// from the text's; it tracks the explicit controls open as they pass.
struct Paragraph<'a, I: Iterator<Item = (BidiChar, bool)>> {
    chars: &'a mut Peekable<I>,
    open: &'a mut Vec<(BidiChar, bool)>,
    /// Whether the paragraph ended at a separator, not at the text's end.
    ended: bool,
}

impl<I: Iterator<Item = (BidiChar, bool)>> Iterator for Paragraph<'_, I> {
    type Item = BidiChar;

    fn next(&mut self) -> Option<BidiChar> {
        if self.ended {
            return None;
        }
        let (c, reopened) = self.chars.next()?;
        match c.1.0 {
            BidiClass::ParagraphSeparator => {
                // CR LF is one separator, ending after the LF.
                let crlf = c.0 == '\r' && self.chars.peek().is_some_and(|n| n.0.0 == '\n');
                self.ended = !crlf;
            }
            BidiClass::LeftToRightEmbedding
            | BidiClass::RightToLeftEmbedding
            | BidiClass::LeftToRightOverride
            | BidiClass::RightToLeftOverride
            | BidiClass::LeftToRightIsolate
            | BidiClass::RightToLeftIsolate
            | BidiClass::FirstStrongIsolate => self.open.push((c, reopened)),
            // X7: a PDF closes an embedding or override, not an isolate.
            BidiClass::PopDirectionalFormat => {
                if self
                    .open
                    .last()
                    .is_some_and(|o| !is_isolate_initiator(o.0.1.0))
                {
                    self.open.pop();
                }
            }
            // X6a: a PDI closes its isolate and everything opened in it.
            BidiClass::PopDirectionalIsolate => {
                if let Some(at) = self
                    .open
                    .iter()
                    .rposition(|o| is_isolate_initiator(o.0.1.0))
                {
                    self.open.truncate(at);
                }
            }
            _ => {}
        }
        Some(c)
    }
}

/// RINCH (rinch #2962): which characters of a text are controls to reopen after
/// a paragraph separator, from their byte ranges, asked in ascending order.
pub(crate) struct ReopenedControls<'a> {
    ranges: &'a [Range<usize>],
    next: usize,
}

impl<'a> ReopenedControls<'a> {
    /// `ranges` ascending and non-overlapping.
    pub(crate) fn new(ranges: &'a [Range<usize>]) -> Self {
        Self { ranges, next: 0 }
    }

    /// Whether the character at byte `at` is one; `at` never decreases
    /// between calls.
    pub(crate) fn contains(&mut self, at: usize) -> bool {
        while self.ranges.get(self.next).is_some_and(|r| r.end <= at) {
            self.next += 1;
        }
        self.ranges.get(self.next).is_some_and(|r| r.start <= at)
    }
}
