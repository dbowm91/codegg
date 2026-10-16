//! The one frame table and the one cadence every spinner in the TUI uses.
//!
//! There used to be two independent animations: a block-progress `SpinnerWidget`
//! living on `App` as `busy_spinner`, and the braille `ActivityIndicator` that
//! actually draws the working strip. They disagreed on glyph family *and* on
//! speed — the widget stepped every 80ms while the indicator stepped once per
//! whole elapsed second, so the throbber a user actually watches during a turn
//! took ten seconds to complete one rotation. Two sources of truth for "is
//! anything happening" is how the faster one stopped being the visible one.
//!
//! This module is now the only place a frame is chosen from a duration, so a
//! fix to the cadence lands everywhere at once.

use std::time::Duration;

/// Braille dot cycle. Reversed braille reads as a single smooth rotation
/// rather than the stutter of a mirrored cycle.
pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// How long one frame is held.
///
/// 100ms puts a full rotation at one second — fast enough to read as alive
/// without flickering, and slower than the 80ms render pump so a frame is
/// drawn several times rather than being skipped between paints.
pub const SPINNER_FRAME_INTERVAL: Duration = Duration::from_millis(100);

/// The frame to draw for a given elapsed duration.
///
/// A pure function of elapsed time rather than of a mutable frame counter:
/// two renders at the same instant produce the same glyph, which is what keeps
/// the strip reproducible under a `TestBackend` no matter when the frame is
/// captured. It also means a dropped or coalesced render tick cannot desync
/// the animation — there is no counter to drift.
pub fn spinner_frame(elapsed: Duration) -> &'static str {
    let interval_ms = SPINNER_FRAME_INTERVAL.as_millis();
    // `interval_ms` is a non-zero const, so this division cannot fail.
    let ticks = (elapsed.as_millis() / interval_ms) as usize;
    SPINNER_FRAMES[ticks % SPINNER_FRAMES.len()]
}

/// The frame to draw right now for a start instant.
pub fn spinner_frame_since(started: std::time::Instant) -> &'static str {
    spinner_frame(started.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cadence_is_faster_than_one_frame_per_second() {
        // The regression this module exists to prevent: a frame table indexed
        // by whole seconds makes a ten-frame rotation take ten seconds.
        let interval_ms = SPINNER_FRAME_INTERVAL.as_millis();
        assert!(
            interval_ms < 1000,
            "a spinner that steps once per second reads as frozen"
        );
        let rotation_ms = interval_ms * SPINNER_FRAMES.len() as u128;
        assert!(
            rotation_ms <= 2000,
            "a full rotation should be around a second, got {rotation_ms}ms"
        );
    }

    #[test]
    fn frames_advance_within_one_second_and_wrap() {
        // Distinct frames across a single second proves the animation actually
        // moves; the wrap proves it never indexes out of bounds.
        let mut seen = std::collections::BTreeSet::new();
        for step in 0..10u64 {
            let at = Duration::from_millis(step * 100);
            seen.insert(spinner_frame(at));
        }
        assert_eq!(seen.len(), SPINNER_FRAMES.len(), "frames must differ");

        let wrapped = spinner_frame(Duration::from_secs(1_000));
        assert!(SPINNER_FRAMES.contains(&wrapped));
    }

    #[test]
    fn the_function_is_pure_so_renders_are_reproducible() {
        let at = Duration::from_millis(1_234);
        assert_eq!(spinner_frame(at), spinner_frame(at));
    }
}
