// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Unicode bidirectional algorithm.

mod classes;
mod explicit;
mod runs;
mod sequence;

use alloc::vec::Vec;
use icu_properties::props::{BidiClass, BidiMirroringGlyph, BidiPairedBracketType};
use parlance::{BaseDirection, BidiLevel};

pub(crate) use classes::needs_bidi_resolution;
use classes::{BIDI_MASK, is_isolate_initiator, is_removed_by_x9, mask};
use runs::Run;

/// Resolver for the Unicode bidirectional algorithm.
#[derive(Clone, Default)]
pub(crate) struct BidiResolver {
    base_level: BidiLevel,
    pub(crate) levels: Vec<BidiLevel>,
    initial_types: Vec<BidiClass>,
    types: Vec<BidiClass>,
    brackets: Vec<(usize, char, BidiMirroringGlyph)>,
    bracket_pairs: Vec<(usize, usize)>,
    runs: Vec<Run>,
    indices: Vec<usize>,
    flags: u16,
    /// RINCH (rinch #2940): see [`Self::paragraphs`].
    paragraphs: Vec<(usize, BidiLevel)>,
}

impl core::fmt::Debug for BidiResolver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BidiResolver")
            .field("base_level", &self.base_level)
            .field("levels", &self.levels)
            .finish_non_exhaustive()
    }
}

impl BidiResolver {
    /// Returns the base level of the text.
    pub(crate) fn base_level(&self) -> BidiLevel {
        self.base_level
    }

    /// Clears the resolver state.
    pub(crate) fn clear(&mut self) {
        self.initial_types.clear();
        self.levels.clear();
        self.types.clear();
        self.brackets.clear();
        self.bracket_pairs.clear();
        self.flags = 0;
        self.base_level = BidiLevel::new(0);
    }

    /// Resolves the text with the specified base direction and precomputed
    /// types.
    ///
    /// RINCH (rinch #2940): under [`BaseDirection::Auto`] each paragraph (UAX #9
    /// P1: the text up to and including a paragraph separator, class B, with
    /// CR LF one separator) is resolved on its own, its base level from its own
    /// first strong character (P2, P3), and [`Self::paragraphs`] lists them. As
    /// CSS `unicode-bidi: plaintext` asks, and as Blink does. RINCH (rinch #2962):
    /// under an explicit direction the paragraphs are resolved on their own too,
    /// each at that direction's level. Either way the explicit controls open at
    /// a separator are reopened after it (Blink reopens an element's bidi
    /// context after a forced break), so an element's embedding or isolate
    /// runs across it.
    pub(crate) fn resolve(
        &mut self,
        chars: impl Iterator<Item = (char, (BidiClass, BidiMirroringGlyph))>,
        base_direction: BaseDirection,
    ) {
        let chars: Vec<_> = chars.collect();
        // The end of each paragraph, one past its separator.
        let mut ends = Vec::new();
        for (i, &(ch, (t, _))) in chars.iter().enumerate() {
            let crlf = ch == '\r' && chars.get(i + 1).is_some_and(|&(next, _)| next == '\n');
            if t == BidiClass::ParagraphSeparator && !crlf {
                ends.push(i + 1);
            }
        }
        if ends.last() != Some(&chars.len()) {
            ends.push(chars.len());
        }
        let auto = base_direction == BaseDirection::Auto;
        // A separator at the end leaves an empty last paragraph, which has no
        // strong character: left to right (P3) under `Auto`.
        let empty_last = auto
            && chars
                .last()
                .is_some_and(|&(_, (t, _))| t == BidiClass::ParagraphSeparator);
        if ends.len() == 1 && !empty_last {
            self.resolve_paragraph(chars.into_iter(), base_direction);
            self.paragraphs.clear();
            self.paragraphs.push((0, self.base_level));
            return;
        }
        let mut levels = Vec::with_capacity(chars.len());
        let mut paragraphs = Vec::with_capacity(ends.len() + 1);
        // RINCH (rinch #2962): the explicit controls open at the end of the
        // paragraph before, reopened at the start of the next, as Blink
        // reopens an element's bidi context after a forced break (X8 would
        // end them at the separator).
        let mut open: Vec<(char, (BidiClass, BidiMirroringGlyph))> = Vec::new();
        let mut start = 0;
        for end in ends {
            let carried = open.len();
            self.resolve_paragraph(
                open.iter()
                    .copied()
                    .chain(chars[start..end].iter().copied()),
                base_direction,
            );
            paragraphs.push((start, self.base_level));
            levels.extend_from_slice(&self.levels[carried..]);
            for &c in &chars[start..end] {
                match c.1.0 {
                    BidiClass::LeftToRightEmbedding
                    | BidiClass::RightToLeftEmbedding
                    | BidiClass::LeftToRightOverride
                    | BidiClass::RightToLeftOverride
                    | BidiClass::LeftToRightIsolate
                    | BidiClass::RightToLeftIsolate
                    | BidiClass::FirstStrongIsolate => open.push(c),
                    // X7: a PDF closes an embedding or override, not an isolate.
                    BidiClass::PopDirectionalFormat => {
                        if open.last().is_some_and(|o| !is_isolate_initiator(o.1.0)) {
                            open.pop();
                        }
                    }
                    // X6a: a PDI closes its isolate and everything opened in it.
                    BidiClass::PopDirectionalIsolate => {
                        if let Some(at) = open.iter().rposition(|o| is_isolate_initiator(o.1.0)) {
                            open.truncate(at);
                        }
                    }
                    _ => {}
                }
            }
            start = end;
        }
        if empty_last {
            paragraphs.push((chars.len(), BidiLevel::new(0)));
        }
        self.base_level = paragraphs[0].1;
        self.levels = levels;
        // Under an explicit direction every paragraph has the base level.
        if auto {
            self.paragraphs = paragraphs;
        } else {
            self.paragraphs.clear();
            self.paragraphs.push((0, self.base_level));
        }
    }

    /// RINCH (rinch #2940): each paragraph [`Self::resolve`] found, as the index
    /// of its first character (in the order the characters were given) and its
    /// base level, in order; the first starts at 0. A text that ends in a
    /// paragraph separator has an empty last paragraph at its length.
    pub(crate) fn paragraphs(&self) -> &[(usize, BidiLevel)] {
        &self.paragraphs
    }

    /// Resolves one paragraph with the specified base direction and
    /// precomputed types.
    fn resolve_paragraph(
        &mut self,
        chars: impl Iterator<Item = (char, (BidiClass, BidiMirroringGlyph))>,
        base_direction: BaseDirection,
    ) {
        self.clear();
        let mut needs_bidi = false;
        let mut len = 0;
        for (i, (ch, (t, bracket))) in chars.enumerate() {
            self.initial_types.push(t);

            if bracket.paired_bracket_type != BidiPairedBracketType::None {
                self.brackets.push((i, ch, bracket));
            }

            needs_bidi = needs_bidi || mask(t) & BIDI_MASK != 0;
            len += 1;
        }
        self.base_level = match base_direction {
            BaseDirection::Auto => Self::default_level(&self.initial_types),
            BaseDirection::Ltr => BidiLevel::new(0),
            BaseDirection::Rtl => BidiLevel::new(1),
        };
        if !needs_bidi && self.base_level == BidiLevel::new(0) {
            self.flags |= 1;
            self.levels.resize(len, self.base_level);
            return;
        }
        self.types.extend_from_slice(&self.initial_types);
        self.resolve_levels();
        self.resolve_runs();
        //self.dump_sequences();
        for i in 0..self.runs.len() {
            if self.runs[i].in_sequence {
                continue;
            }
            self.types.truncate(len);
            self.indices.clear();
            let mut cur = i;
            let level = self.runs[i].level;
            let sos = self.runs[i].sos;
            let mut eos;
            loop {
                let run = &self.runs[cur];
                for i in run.start..run.end {
                    let ty = self.types[i];
                    if !is_removed_by_x9(ty) {
                        self.types.push(ty);
                        self.indices.push(i);
                    }
                }
                eos = run.eos;
                cur = match run.next {
                    Some(i) => i,
                    None => break,
                };
            }
            self.resolve_sequence(level.to_u8(), sos, eos, self.indices.len());
        }
        for i in 0..len {
            let t = self.initial_types[i];
            if t == BidiClass::SegmentSeparator || t == BidiClass::ParagraphSeparator {
                self.levels[i] = self.base_level;
                for j in (0..i).rev() {
                    let t = self.initial_types[j];
                    if is_removed_by_x9(t) {
                        continue;
                    } else if t == BidiClass::WhiteSpace
                        || is_isolate_initiator(t)
                        || t == BidiClass::PopDirectionalIsolate
                    {
                        self.levels[j] = self.base_level;
                    } else {
                        break;
                    }
                }
            } else if is_removed_by_x9(t) {
                if i == 0 {
                    self.levels[i] = self.base_level;
                } else {
                    self.levels[i] = self.levels[i - 1];
                }
                //self.levels[i] = 0xFF;
            }
        }
        for i in (0..len).rev() {
            let t = self.initial_types[i];
            if is_removed_by_x9(t) {
                continue;
            } else if t == BidiClass::WhiteSpace
                || is_isolate_initiator(t)
                || t == BidiClass::PopDirectionalIsolate
            {
                //self.levels[i] = self.base_level;
            } else {
                break;
            }
        }
    }

    fn default_level(types: &[BidiClass]) -> BidiLevel {
        let mut isolates = 0;
        for ty in types {
            let ty = *ty;
            match ty {
                BidiClass::RightToLeftIsolate
                | BidiClass::LeftToRightIsolate
                | BidiClass::FirstStrongIsolate => isolates += 1,
                BidiClass::PopDirectionalIsolate if isolates > 0 => isolates -= 1,
                BidiClass::LeftToRight | BidiClass::RightToLeft | BidiClass::ArabicLetter
                    if isolates == 0 =>
                {
                    return if ty == BidiClass::LeftToRight {
                        BidiLevel::new(0)
                    } else {
                        BidiLevel::new(1)
                    };
                }
                _ => {}
            }
        }
        BidiLevel::new(0)
    }

    fn default_level_until_pdi(types: &[BidiClass]) -> u8 {
        let mut isolates = 0;
        for ty in types {
            let ty = *ty;
            match ty {
                BidiClass::RightToLeftIsolate
                | BidiClass::LeftToRightIsolate
                | BidiClass::FirstStrongIsolate => isolates += 1,
                BidiClass::PopDirectionalIsolate => {
                    if isolates > 0 {
                        isolates -= 1;
                    } else {
                        return 0;
                    }
                }
                BidiClass::LeftToRight | BidiClass::RightToLeft | BidiClass::ArabicLetter
                    if isolates == 0 =>
                {
                    return if ty == BidiClass::LeftToRight { 0 } else { 1 };
                }
                _ => {}
            }
        }
        0
    }
}
