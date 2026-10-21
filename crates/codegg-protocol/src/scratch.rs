//! Managed scratch measurement: bounded wire DTOs (M006).
//!
//! This module owns only data contracts for the daemon-owned, disposable
//! scratch tree: the read-only usage census and its per-tree report. It owns
//! no runtime services, no I/O, and no filesystem authority: the daemon
//! resolves every locator to the tree it owns before answering, and a
//! request can never name a path.
//!
//! ## Disclosure bounds
//!
//! The report carries counts, sizes, ages, and the opaque one-way digest
//! that names a tree. It never carries an absolute path, a file name, file
//! content, or a session identity — the digest is one-way by design, so a
//! session id cannot be recovered from it.
//!
//! ## Compatibility
//!
//! Additive fields and variants are minor: old clients that ignore unknown
//! variants remain forward-compatible.

use serde::{Deserialize, Serialize};

// ── Protocol version ───────────────────────────────────────────────────

/// Version of the scratch protocol surface.
pub const SCRATCH_PROTOCOL_VERSION: u32 = 1;

// ── Refusal taxonomy ───────────────────────────────────────────────────

/// Why the reclaim validator refuses a tree.
///
/// The wire names match the validator's own labels, so an operator report
/// cannot claim a tree is reclaimable when the validator would refuse it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScratchReclaimRefusalDto {
    /// A symbolic link, or a Windows reparse point.
    SymbolicLink,
    /// An entry that is neither a regular file nor a directory.
    NonRegularEntry,
}

// ── Usage census ───────────────────────────────────────────────────────

/// One tree's measured usage.
///
/// Counts and sizes only: no path, no file name, no content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScratchTreeUsageDto {
    /// The opaque, one-way digest that names the tree.
    pub digest: String,
    /// True when the tree is parked under a reclaim tombstone, which means
    /// the automatic path already refused it at least once.
    pub quarantined: bool,
    /// The total size of the regular files in the tree.
    pub total_bytes: u64,
    pub file_count: u64,
    pub directory_count: u64,
    /// Symbolic links (or Windows reparse points). Never traversed.
    pub symlink_count: u64,
    /// Entries that are neither a regular file nor a directory.
    pub non_regular_count: u64,
    /// Regular files with more than one link. An observation only: a hard
    /// link is a file name, not a recursive edge, so it does not stop the
    /// reclaim path. Always zero on Windows, which exposes no stable
    /// link-count accessor at this crate's MSRV.
    pub multiply_linked_count: u64,
    /// The oldest modification time observed, in milliseconds since the epoch.
    pub oldest_modified_ms: Option<i64>,
    /// The newest modification time observed, in milliseconds since the epoch.
    pub newest_modified_ms: Option<i64>,
    /// The reclaim validator's own verdict for this tree.
    pub reclaimable: bool,
    /// Which rule refused the tree, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<ScratchReclaimRefusalDto>,
    /// True when the daemon's owner record is present and is a regular file.
    pub owner_record_present: bool,
    /// True when the walk hit a bound inside this tree.
    pub truncated: bool,
    /// True when the tree vanished while the walk was reading it.
    pub vanished: bool,
}

/// The whole-root measurement.
///
/// A truncated report is never presented as a complete measurement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScratchUsageReportDto {
    /// The number of trees the walk reached.
    pub tree_count: usize,
    /// The number of trees the walk did not reach because of its budget.
    pub unreported_trees: usize,
    /// The total size of every tree the walk reached.
    pub total_bytes: u64,
    /// True when any bound was hit.
    pub truncated: bool,
    /// When the report was produced, in milliseconds since the epoch.
    pub generated_at_ms: i64,
    /// One entry per tree the walk reached.
    pub trees: Vec<ScratchTreeUsageDto>,
}

/// Ask the daemon for a scratch usage report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScratchUsageRequestDto {
    /// Whether to run a fresh walk instead of returning the last completed
    /// report. A fresh walk is a bounded filesystem measurement; it is never
    /// served on an agent hot path.
    #[serde(default)]
    pub refresh: bool,
}

// ── Operator disposition ───────────────────────────────────────────────

/// Ask the daemon to dispose of one scratch tree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScratchReclaimRequestDto {
    /// The opaque digest of the tree. Never a path, never a session id: the
    /// digest is one-way, so a session id cannot be recovered from it.
    pub tree: String,
    /// Whether to actually remove the tree. A dry run is the default, so an
    /// operator sees the verdict before anything is removed.
    #[serde(default)]
    pub apply: bool,
}

/// The verdict of one reclaim attempt.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScratchReclaimVerdictDto {
    /// The tree exists and the reclaim validator accepts it.
    Reclaimable,
    /// The tree exists, but the reclaim validator refuses it.
    Refused,
    /// The tree is still in use: an accepted turn, a non-terminal run, or a
    /// non-terminal job owns it. The operator action refuses it too.
    Leased,
    /// The tree was removed.
    Removed,
    /// There was no such tree.
    Absent,
}

/// The result of one reclaim attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScratchReclaimResultDto {
    /// The opaque digest that names the tree.
    pub digest: String,
    /// The number of entries the tree held.
    pub entry_count: u64,
    /// The total size of the tree's regular files.
    pub byte_count: u64,
    /// The verdict.
    pub verdict: ScratchReclaimVerdictDto,
    /// Which rule refused the tree, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<ScratchReclaimRefusalDto>,
    /// Whether the tree is parked under a reclaim tombstone, which means the
    /// automatic path already refused it at least once.
    pub quarantined: bool,
    /// Whether this was a dry run. A dry run removes nothing.
    pub dry_run: bool,
    /// Whether the tree was removed.
    pub removed: bool,
}
