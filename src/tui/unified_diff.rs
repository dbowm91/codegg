//! M006-E: bounded unified-diff parsing for the agent change-review surface.
//!
//! The LSP preview registry stores each candidate's change as a standard
//! unified diff (`crates/egglsp/src/edit.rs::generate_unified_patch`). The
//! existing `DiffViewer` in `src/tui/components/diff.rs` renders
//! `DiffHunk`/`DiffLine`, but it only *computes* a diff from old and new text
//! it is handed — it cannot consume a patch that already exists.
//!
//! This module is the missing half: it parses patch text into those same
//! types, so the review surface reuses the whole existing renderer instead of
//! forking it.
//!
//! ## Bounded on every axis
//!
//! Patch count, bytes per patch, hunks per patch, and lines per hunk are all
//! capped. Hitting a cap is a *reported* outcome, not a silent partial result:
//! a truncated diff presented as complete would be worse than showing nothing,
//! because the user would accept a change they did not fully see.
//!
//! ## Inert
//!
//! Patch text is rendered, never evaluated. Nothing here interprets an escape
//! sequence, expands a path, or touches the filesystem. A patch whose content
//! looks like shell input is displayed as the literal characters it is.

use crate::tui::components::diff::{DiffHunk, DiffLine};
use similar::ChangeTag;

/// Maximum patches parsed from one candidate.
pub const MAX_REVIEW_PATCHES: usize = 64;

/// Maximum bytes read from any single patch.
pub const MAX_REVIEW_PATCH_BYTES: usize = 512 * 1024;

/// Maximum hunks parsed from any single patch.
pub const MAX_REVIEW_HUNKS_PER_PATCH: usize = 256;

/// Maximum lines retained in any single hunk.
pub const MAX_REVIEW_LINES_PER_HUNK: usize = 2_000;

/// Why parsing stopped, when it stopped early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Truncation {
    pub reason: String,
}

/// A parsed patch: its target path and hunks.
#[derive(Debug, Clone)]
pub struct ParsedPatch {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
    /// Set when this patch was cut short by a bound.
    pub truncated: Option<Truncation>,
}

/// The result of parsing a whole candidate.
#[derive(Debug, Clone, Default)]
pub struct ParsedReview {
    pub patches: Vec<ParsedPatch>,
    /// Set when the candidate as a whole was cut short.
    pub truncated: Option<Truncation>,
    /// Patches whose header could not be understood, so the surface can say
    /// something was skipped instead of pretending the diff is complete.
    pub malformed_patches: Vec<String>,
}

impl ParsedReview {
    /// Total hunks across every patch.
    pub fn hunk_count(&self) -> usize {
        self.patches.iter().map(|patch| patch.hunks.len()).sum()
    }

    /// Every truncation that applies, candidate-level first.
    pub fn truncations(&self) -> Vec<&Truncation> {
        let mut out: Vec<&Truncation> = self
            .patches
            .iter()
            .filter_map(|p| p.truncated.as_ref())
            .collect();
        if let Some(candidate) = self.truncated.as_ref() {
            out.insert(0, candidate);
        }
        out
    }
}

/// Parse a candidate's `(path, unified diff)` pairs.
///
/// A patch whose header is unreadable is recorded in `malformed_patches` and
/// skipped; it never aborts the parse and never panics.
pub fn parse_review(patches: &[(String, String)]) -> ParsedReview {
    let mut review = ParsedReview::default();
    for (path, patch) in patches.iter().take(MAX_REVIEW_PATCHES) {
        if patches.len() > MAX_REVIEW_PATCHES && review.truncated.is_none() {
            review.truncated = Some(Truncation {
                reason: format!(
                    "showing {MAX_REVIEW_PATCHES} of {} files in this change",
                    patches.len()
                ),
            });
        }
        let parsed = parse_patch(path, patch);
        if parsed.hunks.is_empty() && parsed.truncated.is_none() && !is_known_empty(patch) {
            // A patch with no hunk header is either empty or malformed. An
            // empty patch is legitimate ("no changes"); a malformed one is
            // worth reporting. Distinguish by whether anything was consumed.
            if !has_hunk_header(patch) {
                review.malformed_patches.push(path.clone());
            }
        }
        review.patches.push(parsed);
    }
    review
}

/// The marker `generate_unified_patch` emits for a no-op candidate. It is a
/// legitimate empty result, not a malformed one.
const NO_CHANGES_MARKER: &str = "(no changes)";

fn has_hunk_header(patch: &str) -> bool {
    patch
        .lines()
        .any(|line| line.starts_with("@@") && parse_hunk_header(line).is_some())
}

/// Whether a patch is a known-empty result rather than an unreadable one.
fn is_known_empty(patch: &str) -> bool {
    let trimmed = patch.trim();
    trimmed.is_empty() || trimmed == NO_CHANGES_MARKER
}

/// Parse one unified diff into hunks.
pub fn parse_patch(path: &str, patch: &str) -> ParsedPatch {
    let mut parsed = ParsedPatch {
        path: path.to_string(),
        hunks: Vec::new(),
        truncated: None,
    };
    if patch.len() > MAX_REVIEW_PATCH_BYTES {
        parsed.truncated = Some(Truncation {
            reason: format!(
                "diff for {path} is {} bytes; showing the first {MAX_REVIEW_PATCH_BYTES}",
                patch.len()
            ),
        });
    }

    let body: &str = if patch.len() > MAX_REVIEW_PATCH_BYTES {
        // Respect the byte bound on a UTF-8 boundary rather than mid-codepoint.
        let mut end = MAX_REVIEW_PATCH_BYTES;
        while end > 0 && !patch.is_char_boundary(end) {
            end -= 1;
        }
        &patch[..end]
    } else {
        patch
    };

    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0usize;
    let mut new_line = 0usize;

    for line in body.lines() {
        // File headers are metadata, never diff content.
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("diff ") {
            continue;
        }
        if line.starts_with("\\ ") {
            // `\ No newline at end of file`
            continue;
        }
        if let Some((old_start, new_start)) = parse_hunk_header(line) {
            // A new header closes the previous hunk. Without this the parse
            // would retain only the last hunk of a multi-hunk patch, which
            // would silently hide most of the change from the reviewer.
            if let Some(finished) = current.take() {
                if !finished.lines.is_empty() {
                    parsed.hunks.push(finished);
                }
            }
            if parsed.hunks.len() >= MAX_REVIEW_HUNKS_PER_PATCH {
                parsed.truncated = Some(Truncation {
                    reason: format!(
                        "{path} has more than {MAX_REVIEW_HUNKS_PER_PATCH} hunks; showing the first {MAX_REVIEW_HUNKS_PER_PATCH}"
                    ),
                });
                break;
            }
            current = Some(DiffHunk {
                old_start,
                old_count: 0,
                new_start,
                new_count: 0,
                lines: Vec::new(),
            });
            old_line = old_start;
            new_line = new_start;
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            // Preamble text before the first hunk header. Not an error.
            continue;
        };
        if hunk.lines.len() >= MAX_REVIEW_LINES_PER_HUNK {
            parsed.truncated = Some(Truncation {
                reason: format!(
                    "a hunk in {path} has more than {MAX_REVIEW_LINES_PER_HUNK} lines; showing the first {MAX_REVIEW_LINES_PER_HUNK}"
                ),
            });
            break;
        }

        let (tag, content, line_number) = match line.chars().next() {
            Some('-') => (ChangeTag::Delete, &line[1..], old_line),
            Some('+') => (ChangeTag::Insert, &line[1..], new_line),
            Some(' ') => (ChangeTag::Equal, &line[1..], new_line),
            // A bare empty line is a context line whose leading space was lost
            // to trimming; treating it as context matches how patches are
            // commonly produced and never invents an insertion.
            None => (ChangeTag::Equal, "", new_line),
            // `@@` inside a hunk, or any other prefix, is not a diff line.
            Some(_) => continue,
        };
        match tag {
            ChangeTag::Delete => {
                old_line += 1;
                hunk.old_count += 1;
                hunk.lines.push(DiffLine {
                    line_number_old: Some(line_number),
                    line_number_new: None,
                    content: content.to_string(),
                    tag,
                });
            }
            ChangeTag::Insert => {
                new_line += 1;
                hunk.new_count += 1;
                hunk.lines.push(DiffLine {
                    line_number_old: None,
                    line_number_new: Some(line_number),
                    content: content.to_string(),
                    tag,
                });
            }
            _ => {
                old_line += 1;
                new_line += 1;
                hunk.old_count += 1;
                hunk.new_count += 1;
                hunk.lines.push(DiffLine {
                    line_number_old: Some(line_number),
                    line_number_new: Some(line_number),
                    content: content.to_string(),
                    tag,
                });
            }
        }
    }

    if let Some(hunk) = current.take() {
        if !hunk.lines.is_empty() {
            parsed.hunks.push(hunk);
        }
    }
    parsed
}

/// Parse `@@ -old,count +new,count @@` into its two start line numbers.
fn parse_hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let ranges = &rest[..end];
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?;
    let new = parts.next()?;
    let old_start = old.strip_prefix('-')?.split(',').next()?.parse().ok()?;
    let new_start = new.strip_prefix('+')?.split(',').next()?.parse().ok()?;
    Some((old_start, new_start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_hunk_with_exact_line_numbers() {
        let patch = "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        let parsed = parse_patch("src/lib.rs", patch);
        assert_eq!(parsed.hunks.len(), 1);
        let hunk = &parsed.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.old_count, 3);
        assert_eq!(hunk.new_count, 3);

        let deletes: Vec<_> = hunk
            .lines
            .iter()
            .filter(|line| line.tag == ChangeTag::Delete)
            .collect();
        assert_eq!(deletes.len(), 1);
        assert_eq!(deletes[0].content, "    old();");
        assert_eq!(deletes[0].line_number_old, Some(2));
        assert_eq!(deletes[0].line_number_new, None);

        let inserts: Vec<_> = hunk
            .lines
            .iter()
            .filter(|line| line.tag == ChangeTag::Insert)
            .collect();
        assert_eq!(inserts.len(), 1);
        assert_eq!(inserts[0].content, "    new();");
        assert_eq!(inserts[0].line_number_new, Some(2));
        assert_eq!(inserts[0].line_number_old, None);

        let context: Vec<_> = hunk
            .lines
            .iter()
            .filter(|line| line.tag == ChangeTag::Equal)
            .collect();
        assert_eq!(context.len(), 2, "two context lines");
    }

    #[test]
    fn file_headers_are_metadata_not_content() {
        let patch = "--- a/x.rs\n+++ b/x.rs\n@@ -1,1 +1,1 @@\n-a\n+b\n";
        let parsed = parse_patch("x.rs", patch);
        assert_eq!(parsed.hunks[0].lines.len(), 2, "only the -/+ lines");
        assert!(
            !parsed.hunks[0]
                .lines
                .iter()
                .any(|line| line.content.contains("---")),
            "a file header must never render as a diff line"
        );
    }

    #[test]
    fn no_newline_marker_is_ignored() {
        let patch = "@@ -1,1 +1,1 @@\n-a\n\\ No newline at end of file\n+b\n";
        let parsed = parse_patch("x.rs", patch);
        assert_eq!(parsed.hunks[0].lines.len(), 2);
    }

    #[test]
    fn multiple_hunks_are_separate_with_their_own_line_numbers() {
        let patch = "@@ -1,2 +1,2 @@\n a\n-b\n+B\n@@ -10,2 +10,3 @@\n c\n+d\n e\n";
        let parsed = parse_patch("x.rs", patch);
        assert_eq!(parsed.hunks.len(), 2);
        assert_eq!(parsed.hunks[0].old_start, 1);
        assert_eq!(parsed.hunks[1].old_start, 10);
        // The second hunk's insert is on new line 11 (start 10, first line is
        // context).
        let second_insert = parsed.hunks[1]
            .lines
            .iter()
            .find(|line| line.tag == ChangeTag::Insert)
            .expect("insert in the second hunk");
        assert_eq!(second_insert.line_number_new, Some(11));
    }

    #[test]
    fn a_malformed_header_does_not_panic_and_is_reported() {
        let review = parse_review(&[(
            "bad.rs".to_string(),
            "this is not a diff at all".to_string(),
        )]);
        assert_eq!(review.hunk_count(), 0);
        assert_eq!(review.malformed_patches, vec!["bad.rs".to_string()]);
    }

    #[test]
    fn a_hunk_with_no_lines_is_dropped() {
        let parsed = parse_patch("x.rs", "@@ -1,1 +1,1 @@\n");
        assert!(parsed.hunks.is_empty());
    }

    #[test]
    fn an_empty_patch_is_not_reported_as_malformed() {
        let review = parse_review(&[("x.rs".to_string(), "(no changes)\n".to_string())]);
        assert!(review.malformed_patches.is_empty());
    }

    #[test]
    fn a_very_long_patch_is_truncated_and_says_so() {
        let mut patch = String::from("@@ -1,1 +1,1 @@\n");
        for _ in 0..(MAX_REVIEW_LINES_PER_HUNK * 2) {
            patch.push_str("+line\n");
        }
        let parsed = parse_patch("x.rs", &patch);
        assert!(
            parsed.truncated.is_some(),
            "an oversized patch must truncate"
        );
        assert!(parsed.hunks[0].lines.len() <= MAX_REVIEW_LINES_PER_HUNK);
    }

    #[test]
    fn an_oversized_byte_count_truncates_on_a_char_boundary() {
        // Multi-byte content around the cut so a naive byte slice would panic.
        let mut patch = String::from("@@ -1,1 +1,1 @@\n");
        while patch.len() < MAX_REVIEW_PATCH_BYTES + 100 {
            patch.push_str("+é\n");
        }
        let parsed = parse_patch("x.rs", &patch);
        assert!(parsed.truncated.is_some());
        // The parse completing at all is the assertion: a mid-codepoint slice
        // would have panicked inside the parser.
        assert!(!parsed.hunks.is_empty());
    }

    #[test]
    fn the_patch_count_cap_truncates_the_candidate() {
        let patches: Vec<(String, String)> = (0..(MAX_REVIEW_PATCHES + 5))
            .map(|i| (format!("f{i}.rs"), "@@ -1,1 +1,1 @@\n-a\n+b\n".to_string()))
            .collect();
        let review = parse_review(&patches);
        assert_eq!(review.patches.len(), MAX_REVIEW_PATCHES);
        assert!(
            review.truncated.is_some(),
            "exceeding the file cap must be reported at the candidate level"
        );
    }

    #[test]
    fn patch_content_is_inert_and_never_evaluated() {
        // A patch whose content looks like a command, and which also carries a
        // raw ESC byte, must survive as literal text. This module has no
        // evaluation path; the test pins that the bytes round-trip unchanged
        // rather than being interpreted, stripped, or escaped.
        let escape = '\u{1b}';
        let patch = format!("@@ -1,1 +1,1 @@\n-a\n+$(rm -rf /); `whoami`; {escape}[31m\n");
        let parsed = parse_patch("x.rs", &patch);
        let inserted = parsed.hunks[0]
            .lines
            .iter()
            .find(|line| line.tag == ChangeTag::Insert)
            .expect("insert line");
        assert_eq!(
            inserted.content,
            format!("$(rm -rf /); `whoami`; {escape}[31m")
        );
    }

    #[test]
    fn a_candidate_with_several_files_reports_each_hunk_count() {
        let review = parse_review(&[
            ("a.rs".to_string(), "@@ -1,1 +1,1 @@\n-a\n+b\n".to_string()),
            (
                "b.rs".to_string(),
                "@@ -1,1 +1,1 @@\n-c\n+d\n@@ -5,1 +5,1 @@\n-e\n+f\n".to_string(),
            ),
        ]);
        assert_eq!(review.patches.len(), 2);
        assert_eq!(review.hunk_count(), 3);
        assert!(review.malformed_patches.is_empty());
    }
}
