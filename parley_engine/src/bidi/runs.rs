// Copyright 2021 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Level runs and isolating run sequences (UAX #9 X9–X10).

use super::BidiResolver;
use super::classes::*;
use icu_properties::props::BidiClass;
use parlance::BidiLevel;

impl BidiResolver {
    pub(super) fn resolve_runs(&mut self) {
        let len = self.types.len();
        self.runs.clear();
        let mut start = 0;
        while start < len {
            if !is_removed_by_x9(self.types[start]) {
                break;
            }
            start += 1;
        }
        if start == len {
            return;
        }
        let mut level = self.levels[start];
        let mut offset = 0;
        for i in start + 1..len {
            if is_removed_by_x9(self.types[i]) {
                continue;
            }
            if self.levels[i] != level {
                self.runs.push(Run::new(level, offset, i));
                offset = i;
                level = self.levels[i];
            }
        }
        if offset < len {
            self.runs.push(Run::new(level, offset, len));
        }
        for run in &mut self.runs {
            while run.start < run.end {
                if is_removed_by_x9(self.types[run.start]) {
                    run.start += 1;
                } else {
                    break;
                }
            }
            while run.end > run.start {
                if is_removed_by_x9(self.types[run.end - 1]) {
                    run.end -= 1;
                } else {
                    break;
                }
            }
            if run.start == run.end {
                continue;
            }
            if self.types[run.start] == BidiClass::PopDirectionalIsolate {
                run.starts_with_pdi = true;
            }
            let mut prev_level = self.base_level;
            for i in (0..run.start).rev() {
                if !is_removed_by_x9(self.types[i]) {
                    prev_level = self.levels[i];
                    break;
                }
            }
            run.sos = type_from_level(prev_level.max(run.level));
            if is_isolate_initiator(self.initial_types[run.end - 1]) {
                run.ends_with_isolate = true;
                run.eos = type_from_level(self.base_level.max(run.level));
            } else {
                let mut next_level = self.base_level;
                for i in run.end..len {
                    if !is_removed_by_x9(self.types[i]) {
                        next_level = self.levels[i];
                        break;
                    }
                }
                run.eos = type_from_level(next_level.max(run.level));
            }
        }
        for i in 0..self.runs.len() {
            if self.runs[i].ends_with_isolate {
                let level = self.runs[i].level;
                for j in i + 1..self.runs.len() {
                    if self.runs[j].starts_with_pdi && self.runs[j].level == level {
                        self.runs[i].next = Some(j);
                        self.runs[j].in_sequence = true;
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct Run {
    pub(super) level: BidiLevel,
    pub(super) ends_with_isolate: bool,
    pub(super) starts_with_pdi: bool,
    pub(super) sos: BidiClass,
    pub(super) eos: BidiClass,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) in_sequence: bool,
    pub(super) next: Option<usize>,
}

impl Run {
    pub(super) fn new(level: BidiLevel, start: usize, end: usize) -> Self {
        Self {
            level,
            ends_with_isolate: false,
            starts_with_pdi: false,
            sos: BidiClass::OtherNeutral,
            eos: BidiClass::OtherNeutral,
            start,
            end,
            in_sequence: false,
            next: None,
        }
    }
}
