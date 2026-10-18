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

use std::sync::OnceLock;
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

/// How long each phase of the terminal caret's blink lasts — visible, then
/// hidden.
///
/// **Five frames of [`SPINNER_FRAME_INTERVAL`], spelled as a duration.** The
/// multiplication is written out because `Duration`'s `Mul` is not `const`, so
/// a derived `const` does not compile on the toolchain this ships; the
/// relationship is pinned by a test instead, which fails the moment someone
/// retunes the frame interval without rechecking this number. Five frames is
/// half a second per phase and a whole second per cycle — the rhythm a terminal
/// caret already has.
pub const CURSOR_BLINK_PHASE: Duration = Duration::from_millis(500);

/// Whether the Insert caret is painted at a given elapsed duration.
///
/// A pure sibling of [`spinner_frame`], and deliberately **not** a phase counter
/// kept on a view. Three things a stored counter gets wrong and this cannot:
///
/// - two renders at the same instant must agree, or the same frame drawn twice
///   shows two different carets;
/// - a dropped or coalesced tick must not leave the caret in a phase no clock
///   reached, which is how an animation ends up frozen at frame zero and blamed
///   on the terminal;
/// - the phase must not survive an attachment change, and state that outlives
///   its owner has to be reset by someone remembering to.
///
/// Only the terminal's Insert caret calls this. In NOR the cursor is a
/// *selection anchor* — `x`, `v` and `d` all act from it — so it stays solid:
/// a blinking anchor turns "which line am I about to delete" into a guess.
pub fn cursor_blink_visible(elapsed: Duration) -> bool {
    // `CURSOR_BLINK_PHASE` is a non-zero const, so this cannot divide by zero.
    (elapsed.as_millis() / CURSOR_BLINK_PHASE.as_millis()) % 2 == 0
}

/// The process-wide monotonic instant the caret's phase is measured from.
///
/// One `OnceLock` rather than a field per view, so the box and the docked panel
/// agree on the phase without either of them owning it, and so nothing here has
/// to be cleared when a terminal is attached, detached or replaced. It holds an
/// instant and reads it; it never schedules anything, so it adds no cadence of
/// its own.
static CARET_EPOCH: OnceLock<std::time::Instant> = OnceLock::new();

/// The caret blink phase for right now.
///
/// The seam both terminal surfaces call. Everything downstream of it is a pure
/// function of the elapsed time this reads, so a test pins
/// [`cursor_blink_visible`] directly instead of racing this one.
pub fn cursor_blink_visible_now() -> bool {
    let elapsed = CARET_EPOCH.get_or_init(std::time::Instant::now).elapsed();
    cursor_blink_visible(elapsed)
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

    // ── Caret blink ───────────────────────────────────────────────────

    /// Both phases, at values chosen inside each phase rather than at the
    /// boundaries. A test pinned to the boundary passes for a function that is
    /// off by one tick in either direction, which is the failure that reads as
    /// "the blink is erratic".
    #[test]
    fn the_caret_is_visible_for_one_phase_and_hidden_for_the_next() {
        assert!(
            cursor_blink_visible(CURSOR_BLINK_PHASE / 2),
            "the first half-second must be visible"
        );
        assert!(
            !cursor_blink_visible(CURSOR_BLINK_PHASE + CURSOR_BLINK_PHASE / 2),
            "the second half-second must be hidden"
        );
        // A whole cycle later: same phase, so the same answer. A function that
        // drifts instead of repeating shows up here.
        let two_cycles = CURSOR_BLINK_PHASE * 4;
        assert_eq!(
            cursor_blink_visible(CURSOR_BLINK_PHASE / 2),
            cursor_blink_visible(two_cycles + CURSOR_BLINK_PHASE / 2)
        );
        assert_eq!(
            cursor_blink_visible(CURSOR_BLINK_PHASE + CURSOR_BLINK_PHASE / 2),
            cursor_blink_visible(two_cycles + CURSOR_BLINK_PHASE + CURSOR_BLINK_PHASE / 2)
        );
    }

    /// The same reason `spinner_frame` is a function and not a counter: two
    /// renders at the same instant must agree, or a frame drawn twice shows two
    /// different carets.
    #[test]
    fn the_caret_phase_is_pure_so_two_renders_at_one_instant_agree() {
        let at = Duration::from_millis(3_317);
        assert_eq!(cursor_blink_visible(at), cursor_blink_visible(at));
    }

    /// The blink is a reading of the one frame table, not a clock beside it.
    /// A second cadence is an I4 violation and would be invisible in review —
    /// the diff would just show a new duration.
    ///
    /// Deliberately *not* asserted here: that `cursor_blink_visible_now()`
    /// equals `cursor_blink_visible` at some sampled instant. That is two
    /// samples of a live clock, so it fails whenever the boundary falls between
    /// them — a test for the pure function would be gated on the impure one.
    #[test]
    fn the_blink_rides_the_frame_interval_rather_than_arming_its_own() {
        assert_eq!(
            CURSOR_BLINK_PHASE.as_millis() % SPINNER_FRAME_INTERVAL.as_millis(),
            0,
            "a phase that is not a whole number of frames cannot ride the frame clock"
        );
        assert_eq!(
            CURSOR_BLINK_PHASE,
            SPINNER_FRAME_INTERVAL * 5,
            "the phase is written as a literal because `Duration`'s `Mul` is not const, \
             so this is what keeps it derived from the frame interval"
        );
        assert!(
            CURSOR_BLINK_PHASE >= SPINNER_FRAME_INTERVAL,
            "a blink faster than one frame is not a blink"
        );
    }
}
