// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Bidi class sets and small helpers over them.

use icu_properties::props::BidiClass;
use parlance::BidiLevel;

/// Returns a default bidi type for a level.
pub(crate) fn type_from_level(level: BidiLevel) -> BidiClass {
    if level.is_ltr() {
        BidiClass::LeftToRight
    } else {
        BidiClass::RightToLeft
    }
}

/// Computes an ordering for a sequence of bidi runs based on levels.
pub(crate) fn _reorder<F>(order: &mut [usize], levels: F)
where
    F: Fn(usize) -> u8,
{
    let mut max_level = 0;
    let mut lowest_odd_level = 255;
    for (i, o) in order.iter_mut().enumerate() {
        *o = i;
        let level = levels(i);
        if level > max_level {
            max_level = level;
        }
        if level & 1 != 0 && level < lowest_odd_level {
            lowest_odd_level = level;
        }
    }
    let len = order.len();
    for level in (lowest_odd_level..=max_level).rev() {
        let mut i = 0;
        while i < len {
            if levels(i) >= level {
                let mut end = i + 1;
                while end < len && levels(end) >= level {
                    end += 1;
                }
                let mut j = i;
                let mut k = end - 1;
                while j < k {
                    order.swap(j, k);
                    j += 1;
                    k -= 1;
                }
                i = end;
            }
            i += 1;
        }
    }
}

/// Returns whether the character needs bidirectional resolution.
#[inline(always)]
pub(crate) fn needs_bidi_resolution(bidi_class: BidiClass) -> bool {
    mask(bidi_class) & BIDI_MASK != 0
}

pub(super) const OVERRIDE_MASK: u32 = mask(BidiClass::RightToLeftEmbedding)
    | mask(BidiClass::LeftToRightEmbedding)
    | mask(BidiClass::RightToLeftOverride)
    | mask(BidiClass::LeftToRightOverride);
pub(super) const ISOLATE_MASK: u32 = mask(BidiClass::RightToLeftIsolate)
    | mask(BidiClass::LeftToRightIsolate)
    | mask(BidiClass::FirstStrongIsolate);
pub(super) const EXPLICIT_MASK: u32 = OVERRIDE_MASK | ISOLATE_MASK;
pub(super) const RTL_MASK: u32 = mask(BidiClass::RightToLeftEmbedding)
    | mask(BidiClass::RightToLeftOverride)
    | mask(BidiClass::RightToLeftIsolate);
pub(super) const REMOVED_BY_X9_MASK: u32 =
    OVERRIDE_MASK | mask(BidiClass::PopDirectionalFormat) | mask(BidiClass::BoundaryNeutral);
pub(super) const BIDI_MASK: u32 = EXPLICIT_MASK
    | mask(BidiClass::RightToLeft)
    | mask(BidiClass::ArabicLetter)
    | mask(BidiClass::ArabicNumber);
pub(super) const _RESET_MASK: u32 =
    ISOLATE_MASK | mask(BidiClass::PopDirectionalIsolate) | mask(BidiClass::WhiteSpace);

pub(super) fn is_isolate_initiator(ty: BidiClass) -> bool {
    mask(ty) & ISOLATE_MASK != 0
}

pub(crate) fn is_removed_by_x9(ty: BidiClass) -> bool {
    mask(ty) & REMOVED_BY_X9_MASK != 0
}

pub(crate) fn _is_reset(ty: BidiClass) -> bool {
    mask(ty) & _RESET_MASK != 0
}

pub(super) fn find_limit(types: &[BidiClass], offset: usize, ty: BidiClass) -> usize {
    let mut len = offset;
    for &t in &types[offset..] {
        if t != ty {
            break;
        }
        len += 1;
    }
    len
}

pub(super) fn find_limit_by_mask(types: &[BidiClass], offset: usize, mask: u32) -> usize {
    let mut len = offset;
    for &t in &types[offset..] {
        if self::mask(t) & mask == 0 {
            break;
        }
        len += 1;
    }
    len
}

/// Turns a bidi class into a single bit, for cheap set membership testing.
pub(super) const fn mask(t: BidiClass) -> u32 {
    let bit = match t {
        BidiClass::LeftToRight => 0,
        BidiClass::RightToLeft => 1,
        BidiClass::EuropeanNumber => 2,
        BidiClass::EuropeanSeparator => 3,
        BidiClass::EuropeanTerminator => 4,
        BidiClass::ArabicNumber => 5,
        BidiClass::CommonSeparator => 6,
        BidiClass::ParagraphSeparator => 7,
        BidiClass::SegmentSeparator => 8,
        BidiClass::WhiteSpace => 9,
        BidiClass::OtherNeutral => 10,
        BidiClass::LeftToRightEmbedding => 11,
        BidiClass::LeftToRightOverride => 12,
        BidiClass::ArabicLetter => 13,
        BidiClass::RightToLeftEmbedding => 14,
        BidiClass::RightToLeftOverride => 15,
        BidiClass::PopDirectionalFormat => 16,
        BidiClass::NonspacingMark => 17,
        BidiClass::BoundaryNeutral => 18,
        BidiClass::FirstStrongIsolate => 19,
        BidiClass::LeftToRightIsolate => 20,
        BidiClass::RightToLeftIsolate => 21,
        BidiClass::PopDirectionalIsolate => 22,
        _ => {
            debug_assert!(false, "unhandled Bidi_Class");
            return 0;
        }
    };
    1 << bit
}
