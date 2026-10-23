//! The [`Range`] type: a closed interval of integers.

/// A closed interval `[lower, upper]` of integers.
///
/// Both endpoints belong to the range: [`Range::contains`] is inclusive on
/// both sides. The bounds are stored exactly as given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    lower: i64,
    upper: i64,
}

impl Range {
    /// Builds a range with the given inclusive bounds.
    pub fn new(lower: i64, upper: i64) -> Self {
        Range { lower, upper }
    }

    /// The inclusive lower bound.
    pub fn lower(&self) -> i64 {
        self.lower
    }

    /// The inclusive upper bound.
    pub fn upper(&self) -> i64 {
        self.upper
    }

    /// Whether `value` lies inside the range, bounds included.
    pub fn contains(&self, value: i64) -> bool {
        value >= self.lower && value <= self.upper
    }

    /// Builds the range that spans the first and the last of `samples`.
    ///
    /// Returns `None` when `samples` is empty.
    pub fn from_samples(samples: &[i64]) -> Option<Self> {
        let first = *samples.first()?;
        let last = *samples.last()?;
        Some(Range { low: first, high: last })
    }
}
