//! Bounded payload and collection limits for the session projection
//! contract.
//!
//! All projection fields that hold strings, arrays, maps, or
//! collected history MUST apply the limits declared here. Reducers
//! and adapters truncate, summarise, or replace by handle when a
//! payload exceeds a bound rather than panicking or embedding the
//! unbounded value into a projection DTO.

/// Maximum number of distinct sessions represented in a single
/// [`crate::projection::SessionProjectionSnapshot`].
///
/// `SnapshotSession` plus replay updates should never inflate a
/// projection past this bound; older sessions are evicted into a
/// bounded [`crate::projection::SessionSummaryProjection::RecentSummary`]
/// record.
pub const MAX_PROJECTION_SESSIONS: usize = 16;

/// Maximum number of message projections retained per session turn.
///
/// Assistant, user, tool, system, and reasoning projections all count
/// against this bound. Older messages are dropped once the limit is
/// reached; the snapshot tracks the count of evicted messages.
pub const MAX_PROJECTION_MESSAGES: usize = 256;

/// Maximum number of in-flight or recently completed tool
/// projections retained per turn.
pub const MAX_PROJECTION_RECENT_TOOLS: usize = 32;

/// Maximum number of pending permission projections retained at any
/// given time.
pub const MAX_PROJECTION_PENDING_PERMISSIONS: usize = 16;

/// Maximum number of pending question projections retained at any
/// given time.
pub const MAX_PROJECTION_PENDING_QUESTIONS: usize = 16;

/// Maximum number of run projections retained per session (active or
/// completed).
pub const MAX_PROJECTION_RUNS: usize = 32;

/// Maximum number of artifact handle projections retained per session.
pub const MAX_PROJECTION_ARTIFACTS: usize = 32;

/// Maximum number of job projections retained per workspace.
pub const MAX_PROJECTION_JOBS: usize = 32;

/// Maximum number of subagent (task_id) projections retained per
/// session.
pub const MAX_PROJECTION_SUBAGENTS: usize = 16;

/// Maximum number of diagnostic / projection error records retained
/// per snapshot.
pub const MAX_PROJECTION_DIAGNOSTICS: usize = 32;

/// Maximum number of diff / file-change lines preserved when the
/// [`crate::projection::dto::FileChangeProjection::UnifiedDiff`]
/// variant is used.
pub const MAX_PROJECTION_DIFF_LINES: usize = 64;

/// Maximum number of bytes for any single string field inside a
/// projection DTO. Strings longer than this are truncated with the
/// [`TRUNCATION_MARKER`] suffix.
pub const MAX_PROJECTION_STRING_BYTES: usize = 4_096;

/// Maximum number of bytes for tool argument payloads. Larger
/// arguments are replaced by
/// [`crate::projection::dto::ToolProjection::TruncatedArguments`].
pub const MAX_PROJECTION_TOOL_ARGS_BYTES: usize = 8_192;

/// Maximum number of bytes for tool output payloads. Larger outputs
/// are replaced by
/// [`crate::projection::dto::ToolProjection::TruncatedOutput`].
pub const MAX_PROJECTION_TOOL_OUTPUT_BYTES: usize = 8_192;

/// Maximum number of bytes for run summary text. Larger summaries are
/// truncated with the [`TRUNCATION_MARKER`] suffix.
pub const MAX_PROJECTION_RUN_SUMMARY_BYTES: usize = 2_048;

/// Maximum number of bytes retained from the truncation marker itself
/// before it is itself truncated. Acts as a guard rail so a truncated
/// payload cannot grow unboundedly through repeated truncation.
pub const MAX_PROJECTION_TRUNCATION_MARKER_BYTES: usize = 64;

/// Marker appended (or prepended) to truncated projection strings so
/// consumers can detect that truncation occurred.
pub const TRUNCATION_MARKER: &str = "\u{2026}[truncated]";

/// Return `s` truncated to at most `max_bytes` bytes, appending the
/// [`TRUNCATION_MARKER`] when truncation occurred.
///
/// Truncation is byte-based and never splits a multi-byte UTF-8
/// codepoint: the function walks back from the cutoff to the nearest
/// valid char boundary.
pub fn truncate_str(s: &str, max_bytes: usize) -> std::borrow::Cow<'_, str> {
    if s.len() <= max_bytes {
        return std::borrow::Cow::Borrowed(s);
    }
    if max_bytes <= TRUNCATION_MARKER.len() + 1 {
        return std::borrow::Cow::Owned(TRUNCATION_MARKER[..max_bytes].to_string());
    }
    let cut = max_bytes - TRUNCATION_MARKER.len();
    let cut = floor_char_boundary(s, cut);
    let mut out = String::with_capacity(cut + TRUNCATION_MARKER.len());
    out.push_str(&s[..cut]);
    out.push_str(TRUNCATION_MARKER);
    std::borrow::Cow::Owned(out)
}

/// Truncate `s` to at most `max_bytes` bytes. The cut is rounded down
/// to the nearest UTF-8 char boundary. The [`TRUNCATION_MARKER`] is
/// **not** appended; use [`truncate_str`] when the marker is required.
pub fn clip_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let cut = floor_char_boundary(s, max_bytes);
    &s[..cut]
}

fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_under_limit_returns_input() {
        let s = "hello";
        assert_eq!(truncate_str(s, MAX_PROJECTION_STRING_BYTES), "hello");
    }

    #[test]
    fn truncate_over_limit_appends_marker() {
        let s = "a".repeat(MAX_PROJECTION_STRING_BYTES + 32);
        let out = truncate_str(&s, MAX_PROJECTION_STRING_BYTES);
        assert!(out.ends_with(TRUNCATION_MARKER));
        assert!(out.len() <= MAX_PROJECTION_STRING_BYTES);
    }

    #[test]
    fn truncate_does_not_split_codepoint() {
        // 'é' is two bytes in UTF-8
        let s = "é".repeat(64);
        let out = truncate_str(&s, 32);
        assert!(out.ends_with(TRUNCATION_MARKER));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }

    #[test]
    fn clip_does_not_append_marker() {
        let s = "abcdef";
        assert_eq!(clip_str(s, 3), "abc");
    }
}
