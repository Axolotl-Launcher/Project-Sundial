// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Superlogical core: domain logic that is independent of any UI toolkit.

pub const BRAND_NAME: &str = "Superlogical";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    value: i64,
}

impl Counter {
    pub fn new(start: i64) -> Self {
        Self { value: start }
    }

    pub fn value(&self) -> i64 {
        self.value
    }

    pub fn increment(&mut self) -> i64 {
        self.value += 1;
        self.value
    }

    pub fn decrement(&mut self) -> i64 {
        self.value -= 1;
        self.value
    }

    pub fn reset(&mut self) {
        self.value = 0;
    }
}

impl From<i64> for Counter {
    fn from(value: i64) -> Self {
        Self { value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increments_and_decrements() {
        let mut counter = Counter::new(1);
        assert_eq!(counter.increment(), 2);
        assert_eq!(counter.decrement(), 1);
    }

    #[test]
    fn reset_returns_to_zero() {
        let mut counter = Counter::from(41);
        counter.reset();
        assert_eq!(counter.value(), 0);
    }
}
