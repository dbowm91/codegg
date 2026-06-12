use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::report::SecurityReviewCommandArgs;
use super::types::*;

// ---------------------------------------------------------------------------
// Security review receipt — TUI-facing structured output
// ---------------------------------------------------------------------------

/// TUI-facing receipt for a single completed security review run.
///
/// Carries the structured [`SecurityReviewOutput`] plus the rendered text
/// used by the message timeline. Cloning is required because the
/// receipt lives in `App` (not in a borrowed view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityReviewReceipt {
    pub id: String,
    pub root: PathBuf,
    pub args: SecurityReviewCommandArgs,
    pub output: SecurityReviewOutput,
    pub rendered_report: String,
    pub completed_at_ms: i64,
    pub enriched: bool,
    pub lsp_available: bool,
}

impl SecurityReviewReceipt {
    /// Build a fresh receipt with a completed_at_ms timestamp derived
    /// from the wall clock. The caller fills `output`, `rendered_report`,
    /// `enriched`, and `lsp_available`.
    pub fn now(
        id: String,
        root: PathBuf,
        args: SecurityReviewCommandArgs,
        output: SecurityReviewOutput,
        rendered_report: String,
        enriched: bool,
        lsp_available: bool,
    ) -> Self {
        Self {
            id,
            root,
            args,
            output,
            rendered_report,
            completed_at_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            enriched,
            lsp_available,
        }
    }
}

// ---------------------------------------------------------------------------
// Panel projection — TUI-side view model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityReviewPanelItemKind {
    Finding,
    Prompt,
    Note,
    Preflight,
}

/// Flat view of a single item in the security review result panel.
///
/// Findings, prompts, notes, and preflight results are all projected
/// into this uniform shape so the panel can render them with a single
/// list view. Severity/confidence are only populated for findings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewPanelItem {
    pub kind: SecurityReviewPanelItemKind,
    pub file_path: Option<PathBuf>,
    pub line: Option<u32>,
    pub title: String,
    pub severity: Option<SecuritySeverity>,
    pub confidence: Option<SecurityConfidence>,
    pub summary: String,
    pub detail: Vec<String>,
    pub hunk: Option<SecurityReviewHunkRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityReviewFilter {
    All,
    Findings,
    Prompts,
    Notes,
    HighConfidence,
    MediumOrHigherSeverity,
    HunkBacked,
}

impl SecurityReviewFilter {
    pub const ALL: [SecurityReviewFilter; 7] = [
        SecurityReviewFilter::All,
        SecurityReviewFilter::Findings,
        SecurityReviewFilter::Prompts,
        SecurityReviewFilter::Notes,
        SecurityReviewFilter::HighConfidence,
        SecurityReviewFilter::MediumOrHigherSeverity,
        SecurityReviewFilter::HunkBacked,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            SecurityReviewFilter::All => "All",
            SecurityReviewFilter::Findings => "Findings",
            SecurityReviewFilter::Prompts => "Prompts",
            SecurityReviewFilter::Notes => "Notes",
            SecurityReviewFilter::HighConfidence => "High confidence",
            SecurityReviewFilter::MediumOrHigherSeverity => "Medium+ severity",
            SecurityReviewFilter::HunkBacked => "Hunk-backed",
        }
    }

    pub fn next(&self) -> Self {
        let idx = Self::ALL.iter().position(|f| f == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

/// Project a structured [`SecurityReviewReceipt`] into a flat list of
/// [`SecurityReviewPanelItem`]s the panel can render. Findings come
/// first, then prompts, then preflight summaries, then notes. The
/// caller can filter the resulting list with [`filter_panel_items`].
pub fn project_receipt_to_panel_items(
    receipt: &SecurityReviewReceipt,
) -> Vec<SecurityReviewPanelItem> {
    let mut items: Vec<SecurityReviewPanelItem> = Vec::new();

    // Build a hunk index: (file_path, line range) -> SecurityReviewHunkRef
    // for matching findings/prompts to their source hunk context.
    let hunk_index: Vec<(&SecurityReviewHunkRef, PathBuf, u32, u32)> = receipt
        .output
        .hunks
        .iter()
        .filter_map(|h| {
            let new_start = h.new_start?;
            let new_lines = h.new_lines.unwrap_or(1);
            Some((h, h.file_path.clone(), new_start, new_start + new_lines))
        })
        .collect();

    for finding in &receipt.output.findings {
        let location = finding_location(&finding.file_path, finding.line);
        let mut detail: Vec<String> = Vec::new();
        detail.push(format!("Severity: {}", finding.severity));
        detail.push(format!("Confidence: {}", finding.confidence));
        if let Some(cat) = &finding.category {
            detail.push(format!("Category: {}", cat));
        }
        if !finding.reasoning.is_empty() {
            detail.push(format!("Reasoning: {}", finding.reasoning));
        }
        if !finding.recommendation.is_empty() {
            detail.push(format!("Recommendation: {}", finding.recommendation));
        }
        if !finding.tests.is_empty() {
            detail.push(format!("Suggested tests: {}", finding.tests.join(", ")));
        }
        if !finding.evidence.is_empty() {
            detail.push(format!("Evidence ({} items):", finding.evidence.len()));
            for ev in &finding.evidence {
                let loc = ev
                    .file_path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let line = ev.line.map(|l| format!(":{l}")).unwrap_or_default();
                detail.push(format!("  - [{:?}] {}{}{}", ev.kind, loc, line, ev.summary));
                if let Some(d) = &ev.detail {
                    if !d.is_empty() {
                        detail.push(format!("    {d}"));
                    }
                }
            }
        }

        // Try to find a matching hunk for this finding
        let hunk = finding.line.and_then(|line| {
            hunk_index
                .iter()
                .find(|(_, fp, start, end)| {
                    *fp == finding.file_path && line >= *start && line < *end
                })
                .map(|(h, _, _, _)| (*h).clone())
        });

        items.push(SecurityReviewPanelItem {
            kind: SecurityReviewPanelItemKind::Finding,
            file_path: Some(finding.file_path.clone()),
            line: finding.line,
            title: format!(
                "[{}/{}] {}{}",
                finding.severity, finding.confidence, location, finding.title
            ),
            severity: Some(finding.severity),
            confidence: Some(finding.confidence),
            summary: finding.recommendation.clone(),
            detail,
            hunk,
        });
    }

    for prompt in &receipt.output.review_prompts {
        let location = finding_location(&prompt.file_path, prompt.line);
        let mut detail: Vec<String> = Vec::new();
        detail.push("Not a confirmed finding — review prompt only.".to_string());
        if let Some(cat) = &prompt.category {
            detail.push(format!("Category: {cat}"));
        }
        detail.push(format!("Preset: {}", prompt.preset));
        if !prompt.rationale.is_empty() {
            detail.push(format!("Rationale: {}", prompt.rationale));
        }
        if !prompt.evidence.is_empty() {
            detail.push(format!("Evidence: {}", prompt.evidence.join("; ")));
        }

        // Try to find a matching hunk for this prompt
        let hunk = prompt.line.and_then(|line| {
            hunk_index
                .iter()
                .find(|(_, fp, start, end)| {
                    *fp == prompt.file_path && line >= *start && line < *end
                })
                .map(|(h, _, _, _)| (*h).clone())
        });

        items.push(SecurityReviewPanelItem {
            kind: SecurityReviewPanelItemKind::Prompt,
            file_path: Some(prompt.file_path.clone()),
            line: prompt.line,
            title: format!("[PROMPT] {}{}", location, prompt.title),
            severity: None,
            confidence: None,
            summary: prompt.rationale.clone(),
            detail,
            hunk,
        });
    }

    for preflight in &receipt.output.preflight_results {
        let status = format!("{:?}", preflight.status);
        let mut detail: Vec<String> = Vec::new();
        detail.push(format!("Status: {status}"));
        if !preflight.evidence.is_empty() {
            detail.push(format!("Evidence: {}", preflight.evidence.join("; ")));
        }
        if !preflight.notes.is_empty() {
            detail.push(format!("Notes: {}", preflight.notes.join("; ")));
        }
        items.push(SecurityReviewPanelItem {
            kind: SecurityReviewPanelItemKind::Preflight,
            file_path: None,
            line: None,
            title: format!("[PREFLIGHT] {} — {}", preflight.check_name, status),
            severity: None,
            confidence: None,
            summary: preflight.notes.join("; "),
            detail,
            hunk: None,
        });
    }

    for note in &receipt.output.notes {
        items.push(SecurityReviewPanelItem {
            kind: SecurityReviewPanelItemKind::Note,
            file_path: None,
            line: None,
            title: format!("[NOTE] {note}"),
            severity: None,
            confidence: None,
            summary: note.clone(),
            detail: Vec::new(),
            hunk: None,
        });
    }

    items
}

/// Filter a list of panel items through a [`SecurityReviewFilter`]. The
/// predicate is pure: callers are responsible for clamping the
/// selection index after a filter change.
pub fn filter_panel_items(
    items: &[SecurityReviewPanelItem],
    filter: SecurityReviewFilter,
) -> Vec<SecurityReviewPanelItem> {
    items
        .iter()
        .filter(|it| matches_filter(it, filter))
        .cloned()
        .collect()
}

fn matches_filter(item: &SecurityReviewPanelItem, filter: SecurityReviewFilter) -> bool {
    match filter {
        SecurityReviewFilter::All => true,
        SecurityReviewFilter::Findings => item.kind == SecurityReviewPanelItemKind::Finding,
        SecurityReviewFilter::Prompts => item.kind == SecurityReviewPanelItemKind::Prompt,
        SecurityReviewFilter::Notes => {
            matches!(
                item.kind,
                SecurityReviewPanelItemKind::Note | SecurityReviewPanelItemKind::Preflight
            )
        }
        SecurityReviewFilter::HighConfidence => {
            matches!(item.confidence, Some(SecurityConfidence::High))
                && item.kind == SecurityReviewPanelItemKind::Finding
        }
        SecurityReviewFilter::MediumOrHigherSeverity => {
            matches!(
                item.severity,
                Some(SecuritySeverity::Medium)
                    | Some(SecuritySeverity::High)
                    | Some(SecuritySeverity::Critical)
            ) && item.kind == SecurityReviewPanelItemKind::Finding
        }
        SecurityReviewFilter::HunkBacked => item.hunk.is_some(),
    }
}

fn finding_location(file_path: &Path, line: Option<u32>) -> String {
    match line {
        Some(line) => format!("{}:{} ", file_path.display(), line),
        None => format!("{} ", file_path.display()),
    }
}

// ---------------------------------------------------------------------------
// Task state — used by the TUI to track the running background review
// ---------------------------------------------------------------------------

/// State for a single in-flight `/security-review` invocation. The
/// `App` stores one of these while a background review is running so
/// the TUI can abort it on user request and guard against reentrancy.
#[derive(Debug)]
pub struct SecurityReviewTaskState {
    pub id: String,
    pub abort_handle: tokio::task::AbortHandle,
}

// `tokio::task::AbortHandle` does not implement `Clone`, but it is
// `Send` and cheap to keep as a single field. `Debug` is fine because
// `AbortHandle` itself implements `Debug`.

// ---------------------------------------------------------------------------
// Root-aware path resolution
// ---------------------------------------------------------------------------

/// Resolve a panel item's file path against the receipt root with
/// traversal guards. Returns the canonicalized absolute path on
/// success, or a human-readable error on failure.
pub fn resolve_security_review_item_path(
    receipt: &SecurityReviewReceipt,
    item: &SecurityReviewPanelItem,
) -> Result<PathBuf, String> {
    let raw = item
        .file_path
        .as_ref()
        .ok_or_else(|| "Item has no file path".to_string())?;

    let resolved = if raw.is_absolute() {
        raw.clone()
    } else {
        receipt.root.join(raw)
    };

    let canonical = if resolved.exists() {
        std::fs::canonicalize(&resolved)
            .map_err(|e| format!("Cannot canonicalize {}: {e}", resolved.display()))?
    } else {
        let parent = resolved.parent().unwrap_or(&resolved);
        let canon_parent = std::fs::canonicalize(parent)
            .map_err(|e| format!("Cannot canonicalize parent of {}: {e}", resolved.display()))?;
        canon_parent.join(resolved.file_name().unwrap_or_default())
    };

    let canon_root = std::fs::canonicalize(&receipt.root).map_err(|e| {
        format!(
            "Cannot canonicalize review root {}: {e}",
            receipt.root.display()
        )
    })?;

    if !canonical.starts_with(&canon_root) {
        return Err(format!(
            "Path {} escapes review root {}",
            canonical.display(),
            canon_root.display()
        ));
    }

    Ok(canonical)
}
