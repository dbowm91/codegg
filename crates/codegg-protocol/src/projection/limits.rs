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

/// Maximum number of background tool program projections retained
/// per session.
pub const MAX_PROJECTION_TOOL_PROGRAMS: usize = 32;

/// Maximum number of human-shell command run projections retained per
/// session.
pub const MAX_PROJECTION_SHELL_RUNS: usize = 32;

/// Maximum number of tool contract projections retained per session.
pub const MAX_PROJECTION_TOOL_CONTRACTS: usize = 64;

/// Maximum number of plugin catalogue entries retained per session.
pub const MAX_PROJECTION_PLUGIN_CATALOGUE: usize = 64;

/// Maximum number of call entries returned in a single
/// [`crate::projection::dto::ToolProgramCallPage`].
pub const MAX_PROJECTION_CALL_PAGE_SIZE: usize = 32;

/// Maximum number of call summaries retained in a
/// [`crate::projection::dto::ToolProgramDetail`] before older entries
/// are evicted.
pub const MAX_PROJECTION_TOOL_PROGRAM_CALLS: usize = 128;

/// Maximum number of background tool program notifications retained
/// per session before the oldest pending notification is suppressed.
pub const MAX_PROJECTION_NOTIFICATION_BOUND: usize = 16;

/// Maximum number of subagent (task_id) projections retained per
/// session.
pub const MAX_PROJECTION_SUBAGENTS: usize = 16;

/// Maximum session-owned child instances, independent of per-turn history.
pub const MAX_PROJECTION_SESSION_SUBAGENTS: usize = 32;

/// Maximum byte length of a subagent display name in the projection.
///
/// Display names are drawn from a validated pool whose entries are capped at
/// 24 characters, so this only ever trims a name arriving from a producer that
/// did not go through the registry. The pool's own bound is the contract; this
/// is the projection's defence in depth.
pub const MAX_PROJECTION_DISPLAY_NAME_BYTES: usize = 32;

/// Maximum byte length of any one subagent descriptor (`profile` or
/// `usefulness`) in the projection.
///
/// **Not** [`MAX_PROJECTION_STRING_BYTES`]: a node already carries up to
/// 4 KB of `description`, so reusing it would leave the per-descriptor
/// bound unenforced.
///
/// Sized above the curated pool's own maximum: the longest `profile` is
/// 223 bytes, so 256 leaves authoring headroom.
pub const MAX_PROJECTION_DESCRIPTOR_BYTES: usize = 256;

/// Total descriptor bytes one turn's agent tree may carry, across every node.
///
/// Derived rather than measured, so the ceiling is enforced rather than
/// observed: [`MAX_PROJECTION_SUBAGENTS`] nodes × two descriptors ×
/// [`MAX_PROJECTION_DESCRIPTOR_BYTES`] = 8,192 B.
///
/// The *real* figure is well under it — the pool's worst single node carries
/// 291 B, so a full tree of the largest entries grows a snapshot by 4,656 B —
/// but a bound that is only as large as today's data is a bound that is
/// violated by the first longer entry someone authors.
pub const MAX_PROJECTION_SUBAGENT_DESCRIPTOR_BYTES: usize =
    MAX_PROJECTION_SUBAGENTS * MAX_PROJECTION_DESCRIPTOR_BYTES * 2;

/// Maximum number of durable delegated runs retained in one session
/// projection. Detached/background runs remain inspectable without making
/// the projection an unbounded history store.
pub const MAX_PROJECTION_AGENT_RUNS: usize = 64;

/// Maximum number of durable worktree summaries retained in one session
/// projection.
pub const MAX_PROJECTION_WORKTREES: usize = 32;

/// Maximum body bytes one delegated run's transcript payload may carry.
///
/// The bound is on the **body** bytes, not on the serialised envelope: the
/// envelope overhead is the transport's business, the body is the
/// conversation the user reads. A run whose recorded transcript is longer
/// than this is carried as a **truncated** payload that states where it
/// stops, never as a silently shortened one.
///
/// Sized so that a session snapshot holding the maximum number of durable
/// runs still carries a bounded transcript total: 64 runs x 8 KiB = 512 KiB.
pub const MAX_PROJECTION_TRANSCRIPT_BYTES: usize = 8 * 1024;

/// Maximum rows one delegated run's transcript payload may carry.
///
/// The byte bound above is what caps the payload; this one caps the row
/// count so a payload cannot be thousands of empty rows.
pub const MAX_PROJECTION_TRANSCRIPT_ROWS: usize = 64;

/// Maximum body bytes one delegated run transcript **page** may carry.
///
/// The tail-read page is bounded by the same ceiling as the initial
/// preview: the preview *is* the page at ordinal 0, and a client that
/// walks past it with the cursor must not be able to widen the wire
/// payload by asking for a page instead of a preview.
///
/// The ceiling is applied to the **redacted** body bytes, after the
/// store's own redaction and before serialization.
pub const MAX_PROJECTION_TRANSCRIPT_PAGE_BYTES: usize = 8 * 1024;

/// Maximum rows one delegated run transcript **page** may carry.
///
/// Bounded separately from the byte ceiling so a page cannot be thousands
/// of empty rows. The store's own read limit
/// (`MAX_TRANSCRIPT_READ_LIMIT`, 256) is a third, independent bound: the
/// request's `limit` is clamped to this value before the store is asked.
pub const MAX_PROJECTION_TRANSCRIPT_PAGE_ROWS: usize = 64;

/// Maximum number of per-run transcript watermarks retained in one session
/// projection.
///
/// The watermark is advisory metadata, not a transcript: it says how far a
/// run's recording reached so a reader knows to fetch a page. It is bounded
/// so a session with many delegated runs cannot grow the snapshot without
/// limit.
pub const MAX_PROJECTION_TRANSCRIPT_WATERMARKS: usize = 64;

/// Maximum number of durable run-group summaries retained in one session
/// projection.
pub const MAX_PROJECTION_RUN_GROUPS: usize = 32;

/// Maximum number of convergence summaries retained in one session.
pub const MAX_PROJECTION_CONVERGENCES: usize = 32;

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
        let cut = floor_char_boundary(TRUNCATION_MARKER, max_bytes);
        return std::borrow::Cow::Owned(TRUNCATION_MARKER[..cut].to_string());
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

    #[test]
    fn truncate_small_limits_do_not_split_marker() {
        for max_bytes in 0..=15 {
            let out = truncate_str("abcdef", max_bytes);
            assert!(out.len() <= max_bytes);
            assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        }
    }
}
