use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedProviderUsage {
    pub input_tokens: usize,
    pub cached_input_tokens: Option<usize>,
    pub output_tokens: usize,
}

impl NormalizedProviderUsage {
    pub fn total_tokens(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
}

impl fmt::Display for NormalizedProviderUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "input={}, cached={:?}, output={}",
            self.input_tokens, self.cached_input_tokens, self.output_tokens
        )
    }
}

pub fn normalize_from_finish(
    input_tokens: usize,
    output_tokens: usize,
    cached_tokens: Option<usize>,
) -> NormalizedProviderUsage {
    let cached_input_tokens = cached_tokens.map(|cached| {
        if cached > input_tokens {
            tracing::warn!(
                cached,
                input_tokens,
                "cached_tokens exceeds input_tokens, clamping"
            );
            input_tokens
        } else {
            cached
        }
    });

    NormalizedProviderUsage {
        input_tokens,
        cached_input_tokens,
        output_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cached_tokens_normalizes_to_none() {
        let usage = normalize_from_finish(1000, 500, None);
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.cached_input_tokens, None);
        assert_eq!(usage.output_tokens, 500);
    }

    #[test]
    fn cached_exceeding_input_gets_clamped() {
        let usage = normalize_from_finish(500, 100, Some(999));
        assert_eq!(usage.cached_input_tokens, Some(500));
    }

    #[test]
    fn normal_values_pass_through() {
        let usage = normalize_from_finish(1000, 200, Some(400));
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.cached_input_tokens, Some(400));
        assert_eq!(usage.output_tokens, 200);
    }

    #[test]
    fn cached_equal_to_input_passes_through() {
        let usage = normalize_from_finish(800, 300, Some(800));
        assert_eq!(usage.cached_input_tokens, Some(800));
    }

    #[test]
    fn zero_cached_passes_through() {
        let usage = normalize_from_finish(1000, 500, Some(0));
        assert_eq!(usage.cached_input_tokens, Some(0));
    }

    #[test]
    fn total_tokens_computed_correctly() {
        let usage = normalize_from_finish(1000, 500, Some(200));
        assert_eq!(usage.total_tokens(), 1500);
    }
}
