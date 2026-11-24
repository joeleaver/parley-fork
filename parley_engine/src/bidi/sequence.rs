// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Resolving an isolating run sequence: weak types, brackets and neutrals
//! (UAX #9 W1–W7, N0–N2) and implicit levels (I1–I2).

use super::BidiResolver;
use super::classes::*;
use icu_properties::props::{BidiClass, BidiPairedBracketType};
use parlance::BidiLevel;

impl BidiResolver {
    #[expect(clippy::needless_range_loop, reason = "Deferred")]
    pub(super) fn resolve_sequence(
        &mut self,
        level: u8,
        sos: BidiClass,
        eos: BidiClass,
        len: usize,
    ) {
        if len == 0 {
            return;
        }
        const W1_MASK: u32 = mask(BidiClass::LeftToRightIsolate)
            | mask(BidiClass::RightToLeftIsolate)
            | mask(BidiClass::FirstStrongIsolate)
            | mask(BidiClass::PopDirectionalIsolate);
        const W2_MASK: u32 = mask(BidiClass::LeftToRight)
            | mask(BidiClass::RightToLeft)
            | mask(BidiClass::ArabicLetter);
        const W4_MASK: u32 = mask(BidiClass::EuropeanSeparator) | mask(BidiClass::CommonSeparator);
        let mut prev = sos;
        let mut prev_strong = prev;
        let types = &mut self.types[self.initial_types.len()..];
        for i in 0..len {
            let mut t = types[i];
            let tmask = mask(t);
            if t == BidiClass::NonspacingMark {
                // W1
                types[i] = prev;
            } else {
                if tmask & W1_MASK != 0 {
                    prev = BidiClass::OtherNeutral;
                    continue;
                }
                if t == BidiClass::EuropeanNumber {
                    // W2
                    if prev_strong == BidiClass::ArabicLetter {
                        t = BidiClass::ArabicNumber;
                        types[i] = t;
                    }
                } else if tmask & W2_MASK != 0 {
                    prev_strong = t;
                    // W3
                    if t == BidiClass::ArabicLetter {
                        t = BidiClass::RightToLeft;
                        types[i] = t;
                    }
                } else if tmask & W4_MASK != 0 && i < (len - 1) {
                    // W4
                    let mut next = types[i + 1];
                    if next == BidiClass::EuropeanNumber && prev_strong == BidiClass::ArabicLetter {
                        next = BidiClass::ArabicNumber;
                    }
                    if prev == BidiClass::EuropeanNumber && next == BidiClass::EuropeanNumber {
                        t = BidiClass::EuropeanNumber;
                        types[i] = t;
                    } else if t == BidiClass::CommonSeparator
                        && prev == BidiClass::ArabicNumber
                        && next == BidiClass::ArabicNumber
                    {
                        t = BidiClass::ArabicNumber;
                        types[i] = t;
                    }
                }
                prev = t;
            }
        }
        // W5
        let mut i = 0;
        while i < len {
            if types[i] == BidiClass::EuropeanTerminator {
                let limit = find_limit(types, i, BidiClass::EuropeanTerminator);
                let mut t = if i == 0 { sos } else { types[i - 1] };
                if t != BidiClass::EuropeanNumber {
                    t = if limit == len { eos } else { types[limit] };
                }
                if t == BidiClass::EuropeanNumber {
                    for j in i..limit {
                        types[j] = BidiClass::EuropeanNumber;
                    }
                }
                i = limit;
            }
            i += 1;
        }
        // W6, W7
        const W6_MASK: u32 = mask(BidiClass::EuropeanSeparator)
            | mask(BidiClass::EuropeanTerminator)
            | mask(BidiClass::CommonSeparator);
        prev_strong = sos;
        for i in 0..len {
            let t = types[i];
            if mask(t) & W6_MASK != 0 {
                // W6
                types[i] = BidiClass::OtherNeutral;
            } else if t == BidiClass::EuropeanNumber {
                // W7
                if prev_strong == BidiClass::LeftToRight {
                    types[i] = BidiClass::LeftToRight;
                }
            } else if t == BidiClass::LeftToRight || t == BidiClass::RightToLeft {
                prev_strong = t;
            }
        }
        // N0
        if !self.brackets.is_empty() {
            let base_brackets = self.bracket_pairs.len();
            let mut bracket_stack = BracketStack::new();
            for i in 0..len {
                if types[i] != BidiClass::OtherNeutral {
                    continue;
                }
                let index = self.indices[i];
                if let Ok(index) = self.brackets.binary_search_by(|x| x.0.cmp(&index)) {
                    let (_, ch, bracket) = self.brackets[index];
                    match bracket.paired_bracket_type {
                        BidiPairedBracketType::Open => {
                            if bracket_stack.depth == MAX_BRACKET_STACK {
                                break;
                            }
                            bracket_stack.push(i, bracket.mirroring_glyph.unwrap());
                        }
                        BidiPairedBracketType::Close => {
                            if let Some(open) = bracket_stack.find_and_pop(ch) {
                                self.bracket_pairs.push((open, i));
                            }
                        }
                        _ => {}
                    }
                }
            }
            if self.bracket_pairs.len() > base_brackets {
                let embed_dir = if level & 1 != 0 {
                    BidiClass::RightToLeft
                } else {
                    BidiClass::LeftToRight
                };
                let bracket_pairs = &mut self.bracket_pairs[base_brackets..];
                bracket_pairs.sort_unstable_by_key(|a| a.0);
                for pair in bracket_pairs {
                    let mut pair_dir = BidiClass::OtherNeutral;
                    for i in pair.0 + 1..pair.1 {
                        let dir = match types[i] {
                            BidiClass::EuropeanNumber
                            | BidiClass::ArabicNumber
                            | BidiClass::ArabicLetter
                            | BidiClass::RightToLeft => BidiClass::RightToLeft,
                            BidiClass::LeftToRight => BidiClass::LeftToRight,
                            _ => BidiClass::OtherNeutral,
                        };
                        if dir == BidiClass::OtherNeutral {
                            continue;
                        }
                        pair_dir = dir;
                        if dir == embed_dir {
                            break;
                        }
                    }
                    if pair_dir == BidiClass::OtherNeutral {
                        pair.0 = self.indices[pair.0];
                        pair.1 = self.indices[pair.1];
                        continue;
                    }
                    if pair_dir != embed_dir {
                        pair_dir = sos;
                        for i in (0..pair.0).rev() {
                            let dir = match types[i] {
                                BidiClass::EuropeanNumber
                                | BidiClass::ArabicNumber
                                | BidiClass::ArabicLetter
                                | BidiClass::RightToLeft => BidiClass::RightToLeft,
                                BidiClass::LeftToRight => BidiClass::LeftToRight,
                                _ => BidiClass::OtherNeutral,
                            };
                            if dir != BidiClass::OtherNeutral {
                                pair_dir = dir;
                                break;
                            }
                        }
                        if pair_dir == embed_dir || pair_dir == BidiClass::OtherNeutral {
                            pair_dir = embed_dir;
                        }
                    }
                    types[pair.0] = pair_dir;
                    types[pair.1] = pair_dir;
                    for i in pair.0 + 1..pair.1 {
                        let index = self.indices[i];
                        if self.initial_types[index] == BidiClass::NonspacingMark {
                            types[i] = pair_dir;
                        } else {
                            break;
                        }
                    }
                    for i in pair.1 + 1..len {
                        let index = self.indices[i];
                        if self.initial_types[index] == BidiClass::NonspacingMark {
                            types[i] = pair_dir;
                        } else {
                            break;
                        }
                    }
                    pair.0 = self.indices[pair.0];
                    pair.1 = self.indices[pair.1];
                }
            }
        }
        // N1, N2
        const N_MASK: u32 = mask(BidiClass::ParagraphSeparator)
            | mask(BidiClass::SegmentSeparator)
            | mask(BidiClass::WhiteSpace)
            | mask(BidiClass::OtherNeutral)
            | mask(BidiClass::RightToLeftIsolate)
            | mask(BidiClass::LeftToRightIsolate)
            | mask(BidiClass::FirstStrongIsolate)
            | mask(BidiClass::PopDirectionalIsolate);
        let mut i = 0;
        while i < len {
            let t = types[i];
            if mask(t) & N_MASK != 0 {
                let offset = i;
                let limit = find_limit_by_mask(types, offset, N_MASK);
                let mut leading;
                let mut trailing;
                if offset == 0 {
                    leading = sos;
                } else {
                    leading = types[offset - 1];
                    if leading == BidiClass::ArabicNumber || leading == BidiClass::EuropeanNumber {
                        leading = BidiClass::RightToLeft;
                    }
                }
                if limit == len {
                    trailing = eos;
                } else {
                    trailing = types[limit];
                    if trailing == BidiClass::ArabicNumber || trailing == BidiClass::EuropeanNumber
                    {
                        trailing = BidiClass::RightToLeft;
                    }
                }
                let resolved = if leading == trailing {
                    // N1
                    leading
                } else {
                    // N2
                    if level & 1 != 0 {
                        BidiClass::RightToLeft
                    } else {
                        BidiClass::LeftToRight
                    }
                };
                for j in offset..limit {
                    types[j] = resolved;
                }
                i = limit - 1;
            }
            i += 1;
        }
        // Implicit levels
        if level & 1 == 0 {
            // I1
            for i in 0..len {
                let index = self.indices[i];
                let t = types[i];
                if t == BidiClass::RightToLeft {
                    self.levels[index] = BidiLevel::new(level + 1);
                } else if t != BidiClass::LeftToRight {
                    self.levels[index] = BidiLevel::new(level + 2);
                } else {
                    self.levels[index] = BidiLevel::new(level);
                }
            }
        } else {
            // I2
            for i in 0..len {
                let index = self.indices[i];
                let t = types[i];
                if t != BidiClass::RightToLeft {
                    self.levels[index] = BidiLevel::new(level + 1);
                } else {
                    self.levels[index] = BidiLevel::new(level);
                }
            }
        }
    }
}

const MAX_BRACKET_STACK: usize = 63;

struct BracketStack {
    openers: [(usize, char); MAX_BRACKET_STACK],
    depth: usize,
}

impl BracketStack {
    fn new() -> Self {
        Self {
            openers: [(0, '\0'); MAX_BRACKET_STACK],
            depth: 0,
        }
    }

    fn push(&mut self, offset: usize, closer: char) {
        self.openers[self.depth] = (offset, closer);
        self.depth += 1;
    }

    fn find_and_pop(&mut self, closer: char) -> Option<usize> {
        if self.depth == 0 {
            return None;
        }
        for i in (0..self.depth).rev() {
            let c = self.openers[i].1;
            if c == closer
                || (c == '\u{232A}' && closer == '\u{3009}')
                || (c == '\u{3009}' && closer == '\u{232A}')
            {
                self.depth = i;
                return Some(self.openers[i].0);
            }
        }
        None
    }
}
