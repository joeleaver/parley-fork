// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The explicit levels and directions (UAX #9 X1–X8).

use super::BidiResolver;
use super::classes::*;
use icu_properties::props::BidiClass;
use parlance::BidiLevel;

impl BidiResolver {
    pub(super) fn resolve_levels(&mut self) {
        let base = self.base_level;
        let len = self.types.len();
        self.levels.clear();
        self.levels.resize(len, BidiLevel::new(0));
        let mut stack = Stack::new();
        let mut overflow_isolates = 0;
        let mut overflow_embedding = 0;
        let mut valid_isolates = 0;
        stack.push(base, BidiClass::OtherNeutral, false);
        for i in 0..len {
            let t = self.types[i];
            let tmask = mask(t);
            if tmask & EXPLICIT_MASK != 0 {
                let is_isolate = tmask & ISOLATE_MASK != 0;
                let is_rtl = if t == BidiClass::FirstStrongIsolate && i + 1 < len {
                    Self::default_level_until_pdi(&self.types[i + 1..]) == 1
                } else {
                    tmask & RTL_MASK != 0
                };
                if is_isolate {
                    self.levels[i] = stack.embedding_level();
                    let os = stack.override_status();
                    if os != BidiClass::OtherNeutral {
                        self.types[i] = os;
                    }
                }
                let new_level = if is_rtl {
                    stack.embedding_level().next_odd()
                } else {
                    stack.embedding_level().next_even()
                };
                if new_level <= BidiLevel::MAX && overflow_isolates == 0 && overflow_embedding == 0
                {
                    if is_isolate {
                        valid_isolates += 1;
                    }
                    stack.push(
                        new_level,
                        if t == BidiClass::LeftToRightOverride {
                            BidiClass::LeftToRight
                        } else if t == BidiClass::RightToLeftOverride {
                            BidiClass::RightToLeft
                        } else {
                            BidiClass::OtherNeutral
                        },
                        is_isolate,
                    );
                } else if is_isolate {
                    overflow_isolates += 1;
                } else if overflow_isolates == 0 {
                    overflow_embedding += 1;
                }
            } else if t == BidiClass::PopDirectionalIsolate {
                if overflow_isolates > 0 {
                    overflow_isolates -= 1;
                } else if valid_isolates == 0 {
                    // empty
                } else {
                    overflow_embedding = 0;
                    while !stack.isolate_status() {
                        stack.pop();
                    }
                    stack.pop();
                    valid_isolates -= 1;
                }
                self.levels[i] = stack.embedding_level();
                if stack.override_status() != BidiClass::OtherNeutral {
                    self.types[i] = stack.override_status();
                }
            } else if t == BidiClass::PopDirectionalFormat {
                self.levels[i] = stack.embedding_level();
                if overflow_isolates > 0 {
                    // empty
                } else if overflow_embedding > 0 {
                    overflow_embedding -= 1;
                } else if !stack.isolate_status() && stack.depth >= 2 {
                    stack.pop();
                }
            } else if t == BidiClass::ParagraphSeparator {
                stack.depth = 1;
                overflow_isolates = 0;
                overflow_embedding = 0;
                valid_isolates = 0;
                self.levels[i] = base;
            } else if t != BidiClass::BoundaryNeutral {
                self.levels[i] = stack.embedding_level();
                if stack.override_status() != BidiClass::OtherNeutral {
                    self.types[i] = stack.override_status();
                }
            }
        }
    }
}

const MAX_STACK: usize = BidiLevel::MAX.to_u8() as usize;

struct Stack {
    embedding_level: [BidiLevel; MAX_STACK + 1],
    override_status: [BidiClass; MAX_STACK + 1],
    isolate_status: [bool; MAX_STACK + 1],
    depth: usize,
}

impl Stack {
    fn new() -> Self {
        Self {
            depth: 0,
            embedding_level: [BidiLevel::new(0); MAX_STACK + 1],
            override_status: [BidiClass::OtherNeutral; MAX_STACK + 1],
            isolate_status: [false; MAX_STACK + 1],
        }
    }

    fn push(&mut self, level: BidiLevel, override_status: BidiClass, isolate_status: bool) {
        let d = self.depth;
        self.embedding_level[d] = level;
        self.override_status[d] = override_status;
        self.isolate_status[d] = isolate_status;
        self.depth += 1;
    }

    fn pop(&mut self) {
        if self.depth > 1 {
            self.depth -= 1;
        }
    }

    fn embedding_level(&self) -> BidiLevel {
        self.embedding_level[self.depth - 1]
    }

    fn override_status(&self) -> BidiClass {
        self.override_status[self.depth - 1]
    }

    fn isolate_status(&self) -> bool {
        self.isolate_status[self.depth - 1]
    }
}
