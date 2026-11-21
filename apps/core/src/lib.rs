//! Superlogical core: domain logic that is independent of any UI toolkit.

/// Product name shown by the platform shells.
pub const BRAND_NAME: &str = "Superlogical";

/// A minimal piece of shared state, kept here so the UI layer stays thin.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    value: i64,
}

impl Counter {
    /// Creates a counter starting at `start`.
    pub fn new(start: i64) -> Self {
        Self { value: start }
    }

    /// Returns the current value.
    pub fn value(&self) -> i64 {
        self.value
    }

    /// Adds one and returns the new value.
    pub fn increment(&mut self) -> i64 {
        self.value += 1;
        self.value
    }

    /// Subtracts one and returns the new value.
    pub fn decrement(&mut self) -> i64 {
        self.value -= 1;
        self.value
    }

    /// Resets the value to zero.
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
