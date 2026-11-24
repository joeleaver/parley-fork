// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Unicode bidirectional algorithm.

mod classes;
mod explicit;
mod paragraphs;
mod runs;
mod sequence;

use alloc::vec::Vec;
use icu_properties::props::{BidiClass, BidiMirroringGlyph, BidiPairedBracketType};
use parlance::{BaseDirection, BidiLevel};

pub(crate) use classes::needs_bidi_resolution;
use classes::{BIDI_MASK, is_isolate_initiator, is_removed_by_x9, mask};
pub(crate) use paragraphs::{BidiChar, ReopenedControls};
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

    /// Resolves one paragraph with the specified base direction and
    /// precomputed types.
    fn resolve_paragraph(
        &mut self,
        chars: impl Iterator<Item = BidiChar>,
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
            BaseDirection::Auto => Self::default_level(&self.initial_types, true),
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

    /// The level of the first strong character (P2, P3): left to right with
    /// none. P2 skips the characters inside an isolate; RINCH (rinch #2962):
    /// without `skip_isolates` they count, as Blink takes a `unicode-bidi:
    /// plaintext` line's direction after a forced break ([`Self::paragraphs`]).
    pub(super) fn default_level(types: &[BidiClass], skip_isolates: bool) -> BidiLevel {
        let mut isolates = 0;
        for ty in types {
            let ty = *ty;
            match ty {
                BidiClass::RightToLeftIsolate
                | BidiClass::LeftToRightIsolate
                | BidiClass::FirstStrongIsolate
                    if skip_isolates =>
                {
                    isolates += 1;
                }
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
