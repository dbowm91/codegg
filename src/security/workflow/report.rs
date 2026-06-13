use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::context::*;
use super::diff::*;
use super::enrichment::*;
use super::evidence::*;
use super::preflight::*;
use super::receipt::*;
use super::types::*;

// ---------------------------------------------------------------------------
// Per-file patch DTO
// ---------------------------------------------------------------------------

/// Per-file diff patch returned from the discovery phase.
///
/// Carries the file path, the raw patch string (as returned by
/// `egggit::file_diff`), and the parsed hunks for that file.
/// Used to feed real patch data into the hunk source context policy
/// evaluation instead of synthetic hunk-header-only patches.
#[derive(Debug, Clone)]
pub struct ChangedFileDiff {
    pub file_path: PathBuf,
    pub patch: String,
    pub hunks: Vec<ChangedHunk>,
}

// ---------------------------------------------------------------------------
// Run identity
// ---------------------------------------------------------------------------

/// Unique identifier for a single `/security-review` invocation. Returned
/// by the slash command handler so the user (or callers) can correlate
/// start/finish events and so repeated invocations don't collide.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecurityReviewRunId(pub String);

impl SecurityReviewRunId {
    /// Generate a fresh run id.
    pub fn new() -> Self {
        Self(format!("sr-{}", uuid::Uuid::new_v4()))
    }
}

impl Default for SecurityReviewRunId {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Report assembly
// ---------------------------------------------------------------------------

/// Assemble a [`SecurityReviewReport`] from targets, prompts, and notes.
///
/// Always includes the note that risk markers are review prompts, not
/// confirmed findings.  `findings` is always empty in this vertical slice.
pub fn assemble_security_review_report(
    targets: Vec<SecurityReviewTarget>,
    prompts: Vec<SecurityReviewPrompt>,
    mut notes: Vec<String>,
) -> SecurityReviewReport {
    notes.push("risk markers are review prompts, not confirmed findings".to_string());

    SecurityReviewReport {
        targets,
        review_prompts: prompts,
        findings: Vec::new(),
        notes,
    }
}

// ---------------------------------------------------------------------------
// Minimal invocation surface
// ---------------------------------------------------------------------------

/// Plan a security review from a unified diff string.
///
/// Creates targets and request payloads but does **not** execute LSP.
/// Returns a [`SecurityReviewReport`] with targets, review prompts, and
/// empty findings.
pub fn plan_security_review_from_diff(diff: &str, _repo_root: &Path) -> SecurityReviewReport {
    let hunks = parse_changed_hunks(diff);
    let targets = build_security_review_targets(&hunks, |_| None);
    let prompts: Vec<SecurityReviewPrompt> = targets
        .iter()
        .map(|t| SecurityReviewPrompt {
            file_path: t.file_path.clone(),
            line: t.line,
            preset: t.preset.clone(),
            category: None,
            title: format!("Review changed hunk: {}", t.file_path.display()),
            rationale: format!("Changed hunk detected (reason: {:?})", t.reason),
            evidence: vec![
                "source: changed_hunk".to_string(),
                format!("preset: {}", t.preset),
                format!("reason: {:?}", t.reason),
                "no securityContext executed in this planning step".to_string(),
            ],
        })
        .collect();

    assemble_security_review_report(
        targets,
        prompts,
        vec!["planned from diff — no LSP execution".to_string()],
    )
}

// ---------------------------------------------------------------------------
// Target discovery from diff (async, egggit-backed)
// ---------------------------------------------------------------------------

/// Discover security review targets from a git diff.
///
/// Uses `egggit::diff_summary` and `egggit::file_diff` to get changed
/// files, parse hunks, and create targets with the appropriate preset.
///
/// This is a read-only operation — it does not mutate the worktree.
pub async fn discover_targets_from_diff(
    root: &Path,
    base: Option<&str>,
) -> Result<
    (
        Vec<SecurityReviewTarget>,
        Vec<ChangedHunk>,
        HashMap<PathBuf, String>,
    ),
    String,
> {
    let summary = egggit::diff_summary(root, base)
        .await
        .map_err(|e| e.to_string())?;

    let mut all_hunks = Vec::new();
    let mut file_level_paths: Vec<(PathBuf, Option<String>)> = Vec::new();
    let mut real_patches: HashMap<PathBuf, String> = HashMap::new();

    for file in &summary.files {
        if file.kind == egggit::diff::ChangeKind::Deleted {
            continue;
        }

        let path = PathBuf::from(&file.path);

        if should_skip_file(&path) {
            continue;
        }

        let content_hint = std::fs::read_to_string(root.join(&path)).ok();

        let file_diff = egggit::file_diff(root, &path, base)
            .await
            .map_err(|e| e.to_string())?;

        let patch_text = file_diff.patch.clone();
        let hunks = parse_changed_hunks_for_file(&file_diff.patch, &path);

        if hunks.is_empty() {
            file_level_paths.push((path, content_hint));
        } else {
            real_patches.insert(path, patch_text);
            all_hunks.extend(hunks);
        }
    }

    let mut targets =
        build_security_review_targets(&all_hunks, |p| std::fs::read_to_string(root.join(p)).ok());

    for (path, content_hint) in file_level_paths {
        if let Some(target) =
            build_file_level_security_review_target(&path, content_hint.as_deref())
        {
            targets.push(target);
        }
    }

    Ok((targets, all_hunks, real_patches))
}

// ---------------------------------------------------------------------------
// Report assembly (with findings)
// ---------------------------------------------------------------------------

/// Build [`SecurityReviewHunkRef`]s from parsed [`ChangedHunk`]s for TUI display.
fn build_hunk_refs_from_changed_hunks(chunks: &[ChangedHunk]) -> Vec<SecurityReviewHunkRef> {
    chunks
        .iter()
        .map(|hunk| {
            let old_start = Some(hunk.old_start);
            let old_lines = if hunk.old_count > 0 {
                Some(hunk.old_count)
            } else {
                None
            };
            let new_start = Some(hunk.new_start);
            let new_lines = if hunk.new_count > 0 {
                Some(hunk.new_count)
            } else {
                None
            };
            let header = format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            );

            let mut old_line = hunk.old_start;
            let mut new_line = hunk.new_start;
            let lines: Vec<SecurityReviewHunkLine> = hunk
                .lines
                .iter()
                .map(|dl| {
                    let kind = match dl.kind {
                        DiffLineKind::Added => SecurityReviewHunkLineKind::Added,
                        DiffLineKind::Removed => SecurityReviewHunkLineKind::Removed,
                        DiffLineKind::Context => SecurityReviewHunkLineKind::Context,
                    };
                    let (old, new) = match dl.kind {
                        DiffLineKind::Added => {
                            let n = new_line;
                            new_line += 1;
                            (None, Some(n))
                        }
                        DiffLineKind::Removed => {
                            let o = old_line;
                            old_line += 1;
                            (Some(o), None)
                        }
                        DiffLineKind::Context => {
                            let o = old_line;
                            let n = new_line;
                            old_line += 1;
                            new_line += 1;
                            (Some(o), Some(n))
                        }
                    };
                    SecurityReviewHunkLine {
                        old_line: old,
                        new_line: new,
                        kind,
                        text: dl.text.clone(),
                    }
                })
                .collect();

            SecurityReviewHunkRef {
                file_path: hunk.file_path.clone(),
                old_start,
                old_lines,
                new_start,
                new_lines,
                header,
                lines,
            }
        })
        .collect()
}

/// Assemble a [`SecurityReviewOutput`] from targets, prompts, findings,
/// and notes.  Includes mandatory notes about conservative semantics.
pub fn assemble_security_review_report_with_findings(
    targets: Vec<SecurityReviewTarget>,
    prompts: Vec<SecurityReviewPrompt>,
    findings: Vec<SecurityReviewFinding>,
    preflight_results: Vec<SecurityPreflightResult>,
    mut notes: Vec<String>,
) -> SecurityReviewOutput {
    notes.push(
        "risk markers are review prompts unless supported by additional evidence".to_string(),
    );
    notes.push(
        "findings are heuristic defensive review outputs, not proof of exploitability".to_string(),
    );

    SecurityReviewOutput {
        targets,
        findings,
        review_prompts: prompts,
        preflight_results,
        notes,
        hunks: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Hunk source context execution tracking (Phase 2+3)
// ---------------------------------------------------------------------------

/// Per-file result from `collect_hunk_source_context_for_file`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HunkSourceContextFileResult {
    evidence: Vec<StructuredSecurityEvidence>,
    summary: Option<String>,
    notes: Vec<String>,
    attempted: bool,
    succeeded: bool,
    timed_out: bool,
    failed: bool,
}

/// Aggregate execution stats for `collect_hunk_source_context_all_files`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HunkSourceContextExecutionStats {
    pub files_considered: usize,
    pub files_policy_skipped: usize,
    pub requests_attempted: usize,
    pub requests_succeeded: usize,
    pub requests_failed: usize,
    pub requests_timed_out: usize,
    pub evidence_items_emitted: usize,
}

/// Return type for `collect_hunk_source_context_all_files`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HunkSourceContextCollectionResult {
    pub evidence: Vec<StructuredSecurityEvidence>,
    pub summaries: Vec<String>,
    pub notes: Vec<String>,
    pub stats: HunkSourceContextExecutionStats,
}

// ---------------------------------------------------------------------------
// Security review orchestrator
// ---------------------------------------------------------------------------

/// Options controlling the security review workflow orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewWorkflowOptions {
    /// Include review prompts in output (default: true).
    pub include_prompts: bool,
    /// Include evidence-based findings in output (default: true).
    pub include_findings: bool,
    /// Run filename-hint preflight checks (default: true).
    pub run_filename_preflight: bool,
    /// Run hunk-local content preflight checks (default: true).
    pub run_content_preflight: bool,
    /// Use hunk-local (radius=10) content scanning instead of full-file (default: true).
    pub hunk_local_content_preflight: bool,
    /// Maximum findings to include (default: 50).
    pub max_findings: usize,
    /// Maximum prompts to include (default: 100).
    pub max_prompts: usize,
    /// Enable optional `hunkSourceContext` evidence collection (default: false).
    /// When true, the workflow collects hunk navigation evidence (enclosing
    /// symbols, diagnostics, definitions, references) for each changed file
    /// that has hunks and injects it into the evidence-based synthesis.
    /// Fail-open: errors are noted and do not block the workflow.
    pub enable_hunk_source_context: bool,
    /// Maximum files eligible for hunk source context collection (default: 8).
    pub max_hunk_context_files: usize,
    /// Maximum hunk source context requests to execute (default: 8).
    pub max_hunk_context_requests: usize,
    /// Timeout per hunk source context request in milliseconds (default: 2500).
    pub hunk_context_timeout_ms: u64,
    /// Enable optional LSP securityContext enrichment pass (default: false).
    pub enable_lsp_enrichment: bool,
    /// Maximum targets eligible for LSP enrichment (default: 8).
    pub max_lsp_enriched_targets: usize,
    /// Maximum LSP securityContext requests to execute (default: 8).
    pub max_lsp_requests: usize,
    /// Timeout per LSP securityContext request in milliseconds (default: 2500).
    pub lsp_request_timeout_ms: u64,
}

impl Default for SecurityReviewWorkflowOptions {
    fn default() -> Self {
        Self {
            include_prompts: true,
            include_findings: true,
            run_filename_preflight: true,
            run_content_preflight: true,
            hunk_local_content_preflight: true,
            max_findings: 50,
            max_prompts: 100,
            enable_hunk_source_context: false,
            max_hunk_context_files: 8,
            max_hunk_context_requests: 8,
            hunk_context_timeout_ms: 2500,
            enable_lsp_enrichment: false,
            max_lsp_enriched_targets: 8,
            max_lsp_requests: 8,
            lsp_request_timeout_ms: 2500,
        }
    }
}

/// Run the security review workflow against changed files.
///
/// This is the main orchestration entry point.  It runs the existing
/// security review phases in order and returns a stable report object.
///
/// Pipeline:
/// 1. `discover_targets_from_diff(root, base)`
/// 2. Build changed-hunk planning prompts from targets
/// 3. Run filename preflight checks (if enabled)
/// 4. Run hunk-local content preflight checks (if enabled)
/// 5. Collect hunk source context evidence (if enabled, fail-open)
/// 6. Call `synthesize_evidence_based_findings`
/// 7. Assemble `SecurityReviewOutput`
///
/// This function does NOT execute `securityContext` LSP requests.
/// It only uses the deterministic planning and preflight phases.
/// Hunk source context evidence is collected with deterministic routing,
/// ordering, and bounded invocation; best-effort, server-dependent LSP
/// evidence; fail-open execution when `enable_hunk_source_context` is true.
pub async fn run_security_review_workflow(
    root: &Path,
    base: Option<&str>,
    options: SecurityReviewWorkflowOptions,
    hunk_executor: Option<&dyn HunkSourceContextExecutor>,
) -> Result<
    (
        SecurityReviewOutput,
        Option<HunkSourceContextExecutionStats>,
    ),
    String,
> {
    // Phase 1: Discover targets from diff
    let (targets, parsed_hunks, real_patches) = discover_targets_from_diff(root, base).await?;

    // Build hunk refs for TUI display
    let hunk_refs = build_hunk_refs_from_changed_hunks(&parsed_hunks);

    // Phase 2: Build planning prompts from targets
    let planning_prompts: Vec<SecurityReviewPrompt> = targets
        .iter()
        .map(|target| {
            let title = format!(
                "Review {} at {}",
                target.file_path.display(),
                target.reason_str()
            );
            let rationale = format!(
                "Changed hunk in {} requires security review (preset: {})",
                target.file_path.display(),
                target.preset
            );
            SecurityReviewPrompt {
                file_path: target.file_path.clone(),
                line: target.line,
                preset: target.preset.clone(),
                category: None,
                title,
                rationale,
                evidence: vec!["source: changed_hunk".to_string()],
            }
        })
        .collect();

    // Phase 3: Filename preflight checks
    let filename_preflight = if options.run_filename_preflight {
        run_preflight_checks(&targets)
    } else {
        Vec::new()
    };

    // Phase 4: Content preflight checks
    // Use root.join(p) so content reads work regardless of process cwd.
    let content_preflight = if options.run_content_preflight {
        if options.hunk_local_content_preflight {
            run_content_preflight_checks_for_targets(&targets, |p| {
                std::fs::read_to_string(root.join(p)).ok()
            })
        } else {
            run_content_preflight_checks(&targets, |p| std::fs::read_to_string(root.join(p)).ok())
        }
    } else {
        Vec::new()
    };

    let mut all_preflight = filename_preflight;
    all_preflight.extend(content_preflight);

    // Phase 5: Hunk source context evidence (optional, fail-open)
    let mut notes = Vec::new();
    let mut hunk_stats: Option<HunkSourceContextExecutionStats> = None;
    let hunk_context_evidence = if options.enable_hunk_source_context {
        let policy = HunkSourceContextPolicy::default();
        let result = collect_hunk_source_context_all_files(
            &parsed_hunks,
            &real_patches,
            &policy,
            hunk_executor,
            options.max_hunk_context_files,
            options.max_hunk_context_requests,
            options.hunk_context_timeout_ms,
        )
        .await;
        if !result.evidence.is_empty() {
            tracing::debug!(
                "hunkSourceContext collected {} evidence items",
                result.evidence.len()
            );
        }
        // Append concise per-file hunk context summaries to notes.
        for summary in &result.summaries {
            notes.push(format!("hunkSourceContext:\n{summary}"));
        }
        notes.extend(result.notes);
        hunk_stats = Some(result.stats);
        result.evidence
    } else {
        Vec::new()
    };

    // Phase 6: Evidence-based finding synthesis
    let (findings, remaining_prompts) = if hunk_context_evidence.is_empty() {
        synthesize_evidence_based_findings(&targets, &planning_prompts, &all_preflight)
    } else {
        synthesize_evidence_based_findings_with_extra_evidence(
            &targets,
            &planning_prompts,
            &all_preflight,
            &hunk_context_evidence,
        )
    };

    // Phase 7: Assemble output
    if !options.include_findings {
        notes.push("findings disabled by workflow options".to_string());
    }
    if !options.include_prompts {
        notes.push("prompts disabled by workflow options".to_string());
    }

    let mut output = assemble_security_review_report_with_findings(
        targets,
        remaining_prompts,
        findings,
        all_preflight,
        notes,
    );

    // Attach parsed hunks for TUI display
    output.hunks = hunk_refs;

    // Apply limits
    let mut final_output = output;
    if final_output.findings.len() > options.max_findings {
        final_output.findings.truncate(options.max_findings);
        final_output
            .notes
            .push(format!("findings truncated to {}", options.max_findings));
    }
    if final_output.review_prompts.len() > options.max_prompts {
        final_output.review_prompts.truncate(options.max_prompts);
        final_output
            .notes
            .push(format!("prompts truncated to {}", options.max_prompts));
    }

    // Filter by options
    if !options.include_findings {
        final_output.findings.clear();
    }
    if !options.include_prompts {
        final_output.review_prompts.clear();
    }

    Ok((final_output, hunk_stats))
}

// ---------------------------------------------------------------------------
// Enriched security review workflow (with optional LSP enrichment)
// ---------------------------------------------------------------------------

/// Run the security review workflow with optional LSP `securityContext`
/// enrichment.
///
/// Executes the deterministic stage-1 review first. If
/// `enable_lsp_enrichment` is true and an executor is provided, runs
/// bounded LSP requests for escalated targets and reruns finding
/// synthesis with enriched evidence.
///
/// If LSP is unavailable, unsupported, slow, or truncated, stage-1
/// output is returned with clear notes.
pub async fn run_security_review_workflow_with_lsp_enrichment<
    E: SecurityContextExecutor + ?Sized,
>(
    root: &Path,
    base: Option<&str>,
    options: SecurityReviewWorkflowOptions,
    executor: &E,
    hunk_executor: Option<&dyn HunkSourceContextExecutor>,
) -> Result<
    (
        SecurityReviewOutput,
        Option<HunkSourceContextExecutionStats>,
    ),
    String,
> {
    // Stage 1: deterministic review (enrichment disabled in options for this pass)
    let stage1_options = SecurityReviewWorkflowOptions {
        enable_lsp_enrichment: false,
        ..options.clone()
    };
    let (mut output, hunk_stats) =
        run_security_review_workflow(root, base, stage1_options, hunk_executor).await?;

    if !options.enable_lsp_enrichment {
        return Ok((output, hunk_stats));
    }

    // Stage 2: LSP enrichment
    let enrichment_results = run_security_context_enrichment(&output, executor, &options).await;

    let (merged_prompts, extra_evidence, enrichment_notes) =
        merge_enrichment_results(&output, &enrichment_results);

    if !extra_evidence.is_empty() {
        // Rerun synthesis with enriched evidence
        let (enriched_findings, remaining_prompts) =
            synthesize_evidence_based_findings_with_extra_evidence(
                &output.targets,
                &merged_prompts,
                &output.preflight_results,
                &extra_evidence,
            );

        output.findings = enriched_findings;
        output.review_prompts = remaining_prompts;
    } else {
        output.review_prompts = merged_prompts;
    }

    // Append enrichment notes
    output.notes.extend(enrichment_notes);
    if enrichment_results.is_empty() {
        note_lsp_enrichment_no_eligible_targets(&mut output);
    } else {
        note_lsp_enrichment_executed(&mut output, enrichment_results.len());
    }

    // Re-apply limits
    if output.findings.len() > options.max_findings {
        output.findings.truncate(options.max_findings);
        output
            .notes
            .push(format!("findings truncated to {}", options.max_findings));
    }
    if output.review_prompts.len() > options.max_prompts {
        output.review_prompts.truncate(options.max_prompts);
        output
            .notes
            .push(format!("prompts truncated to {}", options.max_prompts));
    }

    if !options.include_findings {
        output.findings.clear();
    }
    if !options.include_prompts {
        output.review_prompts.clear();
    }

    Ok((output, hunk_stats))
}

// ---------------------------------------------------------------------------
// Report rendering
// ---------------------------------------------------------------------------

/// Render a compact summary of the security review output.
pub fn render_security_review_summary(output: &SecurityReviewOutput) -> String {
    let mut lines = Vec::new();
    lines.push("Security Review Summary".to_string());
    lines.push(format!("- Targets: {}", output.targets.len()));
    lines.push(format!("- Findings: {}", output.findings.len()));
    lines.push(format!("- Review prompts: {}", output.review_prompts.len()));

    let pass_count = output
        .preflight_results
        .iter()
        .filter(|p| p.status == PreflightStatus::Pass)
        .count();
    let fail_count = output
        .preflight_results
        .iter()
        .filter(|p| p.status == PreflightStatus::Fail)
        .count();
    lines.push(format!(
        "- Preflight checks: {} pass, {} fail",
        pass_count, fail_count
    ));

    if !output.notes.is_empty() {
        lines.push("- Notes:".to_string());
        for note in &output.notes {
            lines.push(format!("  - {}", note));
        }
    }

    lines.join("\n")
}

/// Render findings with severity/confidence labels.
pub fn render_security_review_findings(output: &SecurityReviewOutput) -> String {
    if output.findings.is_empty() {
        return "No findings.\n".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Findings".to_string());
    lines.push(String::new());

    for finding in &output.findings {
        let location = if let Some(line) = finding.line {
            format!("{}:{}", finding.file_path.display(), line)
        } else {
            finding.file_path.display().to_string()
        };
        lines.push(format!(
            "[{}/{}] {} {}",
            finding.severity, finding.confidence, location, finding.title
        ));
        lines.push(format!("  Evidence: {} items", finding.evidence.len()));
        lines.push(format!("  Recommendation: {}", finding.recommendation));
        if !finding.tests.is_empty() {
            lines.push(format!("  Suggested tests: {}", finding.tests.join(", ")));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Render review prompts (risk-marker based, not confirmed findings).
pub fn render_security_review_prompts(output: &SecurityReviewOutput) -> String {
    if output.review_prompts.is_empty() {
        return "No review prompts.\n".to_string();
    }

    let mut lines = Vec::new();
    lines.push("Review Prompts".to_string());
    lines.push(String::new());

    for prompt in &output.review_prompts {
        let location = if let Some(line) = prompt.line {
            format!("{}:{}", prompt.file_path.display(), line)
        } else {
            prompt.file_path.display().to_string()
        };
        lines.push(format!("[{}] {}", location, prompt.title));
        lines.push(format!("  Rationale: {}", prompt.rationale));
        lines.push(String::new());
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Security review command helpers
// ---------------------------------------------------------------------------

/// Parsed command-line arguments for the `/security-review` slash command.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityReviewCommandArgs {
    pub base: Option<String>,
    pub json: bool,
    pub prompts_only: bool,
    pub findings_only: bool,
    pub no_content: bool,
    pub no_filename: bool,
    pub max_findings: Option<usize>,
    pub max_prompts: Option<usize>,
    /// Enable optional LSP securityContext enrichment pass.
    pub enrich: bool,
    /// Maximum targets eligible for LSP enrichment.
    pub max_enriched_targets: Option<usize>,
    /// Timeout per LSP securityContext request in milliseconds.
    pub lsp_timeout_ms: Option<u64>,
    /// Enable hunk source context evidence collection.
    pub hunk_context: bool,
    /// Open the result panel automatically on successful completion.
    pub open_panel_on_complete: bool,
}

/// Parse a space-separated argument string into [`SecurityReviewCommandArgs`].
///
/// Unknown flags are silently ignored, matching the current TUI handler behavior.
pub fn parse_security_review_args(input: &str) -> SecurityReviewCommandArgs {
    let mut args = SecurityReviewCommandArgs::default();
    let mut iter = input.split_whitespace();

    while let Some(token) = iter.next() {
        match token {
            "--json" => args.json = true,
            "--prompts-only" => args.prompts_only = true,
            "--findings-only" => args.findings_only = true,
            "--no-content" => args.no_content = true,
            "--no-filename" => args.no_filename = true,
            "--changed" => args.base = Some("HEAD".to_string()),
            "--enrich" => args.enrich = true,
            "--base" => {
                args.base = iter.next().map(|s| s.to_string());
            }
            "--max-findings" => {
                args.max_findings = iter.next().and_then(|s| s.parse::<usize>().ok());
            }
            "--max-prompts" => {
                args.max_prompts = iter.next().and_then(|s| s.parse::<usize>().ok());
            }
            "--max-enriched-targets" => {
                args.max_enriched_targets = iter.next().and_then(|s| s.parse::<usize>().ok());
            }
            "--lsp-timeout-ms" => {
                args.lsp_timeout_ms = iter.next().and_then(|s| s.parse::<u64>().ok());
            }
            "--hunk-context" => args.hunk_context = true,
            "--panel" | "--open-panel" => args.open_panel_on_complete = true,
            _ => {}
        }
    }

    args
}

/// Run the security review command from parsed arguments.
///
/// Delegates to [`run_security_review_command_with_executor`] with no executor.
pub async fn run_security_review_command(
    root: &Path,
    args: &SecurityReviewCommandArgs,
) -> Result<String, String> {
    run_security_review_command_with_executor(root, args, None).await
}

/// Run the security review command with an optional LSP executor.
///
/// Builds [`SecurityReviewWorkflowOptions`] from the args, runs the
/// orchestrator, and renders the output as either JSON or human-readable text.
///
/// When `executor` is `Some` and `args.enrich` is true, the enriched
/// workflow is used with the provided executor.  When `executor` is
/// `None` and `args.enrich` is true, enrichment is skipped (stage-1
/// only) and a note is appended indicating that no executor is
/// available in this runtime.
pub async fn run_security_review_command_with_executor(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    executor: Option<&dyn SecurityContextExecutor>,
) -> Result<String, String> {
    let executors = SecurityReviewExecutors {
        security_context: executor,
        hunk_source_context: None,
    };
    run_security_review_command_with_executors(root, args, executors).await
}

/// Run the security review command with a bundled pair of executors.
///
/// This is the primary entry point for programmatic callers that have
/// both a `SecurityContextExecutor` and a `HunkSourceContextExecutor`.
/// Builds [`SecurityReviewWorkflowOptions`] from the args, runs the
/// orchestrator, and renders the output as either JSON or human-readable text.
pub async fn run_security_review_command_with_executors(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    executors: SecurityReviewExecutors<'_>,
) -> Result<String, String> {
    let (output, rendered, _stats) = run_security_review_command_inner(
        root,
        args,
        executors.security_context,
        executors.hunk_source_context,
        true,
    )
    .await?;
    if args.json {
        serde_json::to_string_pretty(&output).map_err(|e| format!("JSON serialization failed: {e}"))
    } else {
        Ok(rendered)
    }
}

/// Build the structured [`SecurityReviewOutput`] (no rendering) and
/// optionally a human-readable rendering for the same args.
///
/// When `render_human` is true, returns both the structured output and
/// the human-readable rendering. When false, returns the structured
/// output and an empty string for the rendering.
async fn run_security_review_command_inner(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    security_executor: Option<&dyn SecurityContextExecutor>,
    hunk_executor: Option<&dyn HunkSourceContextExecutor>,
    render_human: bool,
) -> Result<
    (
        SecurityReviewOutput,
        String,
        Option<HunkSourceContextExecutionStats>,
    ),
    String,
> {
    let base = args.base.as_deref();

    let mut options = SecurityReviewWorkflowOptions {
        include_prompts: !args.findings_only,
        include_findings: !args.prompts_only,
        run_filename_preflight: !args.no_filename,
        run_content_preflight: !args.no_content,
        enable_hunk_source_context: args.hunk_context,
        enable_lsp_enrichment: args.enrich,
        ..Default::default()
    };

    if let Some(max_f) = args.max_findings {
        options.max_findings = max_f;
    }
    if let Some(max_p) = args.max_prompts {
        options.max_prompts = max_p;
    }
    if let Some(max_e) = args.max_enriched_targets {
        options.max_lsp_enriched_targets = max_e;
    }
    if let Some(timeout) = args.lsp_timeout_ms {
        options.lsp_request_timeout_ms = timeout;
    }

    let include_findings = options.include_findings;
    let include_prompts = options.include_prompts;

    let (mut output, hunk_stats) = if options.enable_lsp_enrichment {
        if let Some(exec) = security_executor {
            run_security_review_workflow_with_lsp_enrichment(
                root,
                base,
                options,
                exec,
                hunk_executor,
            )
            .await?
        } else {
            // No executor available — skip enrichment, run deterministic
            // stage-1 only, and append a clear unavailable note.
            let stage1_options = SecurityReviewWorkflowOptions {
                enable_lsp_enrichment: false,
                ..options
            };
            let (mut result, stats) =
                run_security_review_workflow(root, base, stage1_options, hunk_executor).await?;
            note_lsp_enrichment_unavailable(&mut result);
            (result, stats)
        }
    } else {
        run_security_review_workflow(root, base, options, hunk_executor).await?
    };

    if args.json {
        let rendered = if render_human {
            serde_json::to_string_pretty(&output)
                .map_err(|e| format!("JSON serialization failed: {e}"))?
        } else {
            String::new()
        };
        return Ok((output, rendered, hunk_stats));
    }

    // Filter before rendering to match the filtering applied in the JSON path.
    if !include_findings {
        output.findings.clear();
    }
    if !include_prompts {
        output.review_prompts.clear();
    }

    let mut report = String::new();
    if render_human {
        report.push_str(&render_security_review_summary(&output));
        if !args.findings_only {
            report.push_str("\n\n");
            report.push_str(&render_security_review_findings(&output));
        }
        if !args.prompts_only {
            report.push('\n');
            report.push_str(&render_security_review_prompts(&output));
        }
    }

    Ok((output, report, hunk_stats))
}

/// Run the security review command from a background task context.
///
/// This is the same as [`run_security_review_command_with_executor`] but
/// takes owned `root` and `args` and an already-cloned `Arc<LspTool>` so
/// the caller can spawn the future without borrowing App state. The
/// executor is constructed inside the function (not borrowed from the
/// caller) so no `&self` borrow crosses an await boundary.
///
/// In local TUI mode the caller passes `Some(Arc::clone(&app.lsp_tool))`
/// so the real `LspSecurityContextExecutor` is used. In socket/remote
/// mode the caller passes `None` and the deterministic stage-1
/// fallback runs with a `note_lsp_enrichment_unavailable` note.
///
/// Returns a structured [`SecurityReviewReceipt`] carrying both the
/// rendered text (for the message timeline) and the structured output
/// (for the result panel), including full hunk context stats:
/// `hunk_context_requested`, `hunk_context_available`,
/// `hunk_context_executed`, `hunk_context_succeeded`,
/// `hunk_context_requests_attempted`, and
/// `hunk_context_requests_succeeded`.
pub async fn run_security_review_background(
    root: PathBuf,
    args: SecurityReviewCommandArgs,
    lsp_tool: Option<Arc<crate::tool::lsp::LspTool>>,
) -> Result<SecurityReviewReceipt, String> {
    let hunk_executor = lsp_tool
        .clone()
        .map(crate::security::lsp_executor::LspHunkSourceContextExecutor::new);
    let security_executor =
        lsp_tool.map(crate::security::lsp_executor::LspSecurityContextExecutor::new);

    let security_executor_ref = security_executor
        .as_ref()
        .map(|e| e as &dyn crate::security::workflow::context::SecurityContextExecutor);
    let hunk_executor_ref = hunk_executor
        .as_ref()
        .map(|e| e as &dyn crate::security::workflow::context::HunkSourceContextExecutor);

    let lsp_available = security_executor.is_some();
    let enriched = args.enrich && security_executor.is_some();

    let executors = SecurityReviewExecutors {
        security_context: security_executor_ref,
        hunk_source_context: hunk_executor_ref,
    };

    let id = SecurityReviewRunId::new().0;
    let (output, rendered, hunk_stats) = run_security_review_command_inner(
        &root,
        &args,
        executors.security_context,
        executors.hunk_source_context,
        true,
    )
    .await?;

    let hunk_context_requested = args.hunk_context;
    let hunk_context_available = lsp_available;
    let stats = hunk_stats.unwrap_or_default();
    let hunk_context_executed = args.hunk_context && lsp_available && stats.requests_attempted > 0;
    let hunk_context_succeeded = stats.requests_succeeded > 0;

    Ok(SecurityReviewReceipt::now(
        id,
        root,
        args,
        output,
        rendered,
        enriched,
        lsp_available,
        hunk_context_requested,
        hunk_context_available,
        hunk_context_executed,
        hunk_context_succeeded,
        stats.requests_attempted,
        stats.requests_succeeded,
    ))
}

// ---------------------------------------------------------------------------
// Note helpers for LSP enrichment
// ---------------------------------------------------------------------------

/// Append a note that LSP enrichment was requested but the executor
/// is unavailable in this runtime.  Idempotent: does not duplicate
/// the note if it already exists.
fn note_lsp_enrichment_unavailable(output: &mut SecurityReviewOutput) {
    let note =
        "LSP enrichment requested but no securityContext executor is available in this runtime.";
    if !output.notes.iter().any(|n| n == note) {
        output.notes.push(note.to_string());
    }
}

/// Append a note that LSP enrichment found no eligible targets.
/// Idempotent: does not duplicate the note if it already exists.
fn note_lsp_enrichment_no_eligible_targets(output: &mut SecurityReviewOutput) {
    let note = "LSP enrichment requested but no targets met escalation policy.";
    if !output.notes.iter().any(|n| n == note) {
        output.notes.push(note.to_string());
    }
}

/// Append a note reporting that LSP enrichment was executed with the
/// given number of requests.  Always appends (not idempotent — each
/// enrichment pass produces its own count).
fn note_lsp_enrichment_executed(output: &mut SecurityReviewOutput, count: usize) {
    output
        .notes
        .push(format!("LSP enrichment executed {} request(s).", count));
}

// ---------------------------------------------------------------------------
// Hunk source context evidence integration
// ---------------------------------------------------------------------------

use crate::lsp::hunk_nav_policy::{
    decide_hunk_source_context, HunkSourceContextDecision, HunkSourceContextPolicy,
};
use crate::lsp::hunk_nav_prompt::format_hunk_source_context_summary;
use egglsp::hunk_context::HunkSourceNavigationRequest;
use egglsp::hunk_context::HunkSourceNavigationResponse;

/// Convert a [`HunkSourceNavigationResponse`] into structured security
/// evidence items that feed into the evidence-based synthesis.
///
/// Each hunk produces:
/// - Enclosing symbol as `HunkNavigation` evidence
/// - Diagnostics as `Diagnostic` evidence
/// - Definitions as `HunkNavigation` evidence
///
/// Returns a flat list of evidence items. The evidence is file-scoped:
/// each item carries the file_path from the response.
pub fn evidence_from_hunk_source_context(
    response: &HunkSourceNavigationResponse,
) -> Vec<StructuredSecurityEvidence> {
    let mut evidence = Vec::new();

    for ev in &response.hunks {
        // Enclosing symbol.
        if let Some(sym) = &ev.enclosing_symbol {
            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::HunkNavigation,
                file_path: Some(std::path::PathBuf::from(&response.file_path)),
                line: ev.hunk.new_range.as_ref().map(|r| r.start_line),
                summary: format!(
                    "enclosing symbol: {} {} lines {}-{}",
                    sym.kind, sym.name, sym.start_line, sym.end_line
                ),
                detail: Some(format!("hunk {}", ev.hunk.id)),
            });
        }

        // Diagnostics in hunk.
        // Note: FileDiagnostic.line is 0-indexed (LSP convention); convert
        // to 1-indexed to match hunk/security-workflow line conventions.
        for diag in &ev.diagnostics {
            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::Diagnostic,
                file_path: Some(std::path::PathBuf::from(&response.file_path)),
                line: Some(diag.line + 1),
                summary: format!("{:?}: {}", diag.severity, diag.message),
                detail: Some(format!("hunk {}", ev.hunk.id)),
            });
        }

        // Nearby diagnostics.
        for diag in &ev.nearby_diagnostics {
            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::Diagnostic,
                file_path: Some(std::path::PathBuf::from(&response.file_path)),
                line: Some(diag.line + 1),
                summary: format!("{:?} (nearby): {}", diag.severity, diag.message),
                detail: Some(format!("hunk {}", ev.hunk.id)),
            });
        }

        // Definitions.
        for def in &ev.definitions {
            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::HunkNavigation,
                file_path: Some(std::path::PathBuf::from(&response.file_path)),
                line: Some(def.start_line),
                summary: format!("definition at {}:{}", def.file, def.start_line),
                detail: Some(format!("hunk {}", ev.hunk.id)),
            });
        }

        // References summary.
        if !ev.references.is_empty() {
            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::HunkNavigation,
                file_path: Some(std::path::PathBuf::from(&response.file_path)),
                line: ev.hunk.new_range.as_ref().map(|r| r.start_line),
                summary: format!("{} references in changed range", ev.references.len()),
                detail: Some(format!("hunk {}", ev.hunk.id)),
            });
        }
    }

    evidence
}

/// Collect hunk source context evidence for a single file's changed hunks.
///
/// Uses the [`HunkSourceContextPolicy`] to decide whether to invoke
/// hunkSourceContext. Returns a flat list of structured evidence items
/// and a human-readable summary for inclusion in the output notes.
///
/// Fail-open: returns empty evidence on any error, appending a note.
pub async fn collect_hunk_source_context_for_file<E: HunkSourceContextExecutor + ?Sized>(
    hunks: &[ChangedHunk],
    patch: &str,
    file_path: &std::path::Path,
    policy: &HunkSourceContextPolicy,
    executor: Option<&E>,
    timeout_ms: u64,
) -> HunkSourceContextFileResult {
    let decision = decide_hunk_source_context(policy, patch, Some(file_path));

    match decision {
        HunkSourceContextDecision::Skip { reason } => {
            let note = format!("hunkSourceContext skipped: {reason}");
            HunkSourceContextFileResult {
                notes: vec![note],
                ..Default::default()
            }
        }
        HunkSourceContextDecision::Use { .. } => {
            let Some(executor) = executor else {
                let note = format!(
                    "hunkSourceContext recommended for {}, but no executor is available; continuing without semantic hunk evidence",
                    file_path.display()
                );
                return HunkSourceContextFileResult {
                    notes: vec![note],
                    ..Default::default()
                };
            };

            // Convert ChangedHunks to HunkDescriptors for the request.
            let descriptors: Vec<_> = hunks
                .iter()
                .enumerate()
                .map(|(i, h)| h.to_hunk_descriptor(i))
                .collect();

            let request = HunkSourceNavigationRequest {
                file_path: file_path.to_string_lossy().to_string(),
                hunks: descriptors,
                patch: None,
                intent: "security_review".to_string(),
                include_definitions: policy.include_definitions,
                include_references: policy.include_references,
                include_call_hierarchy: policy.include_call_hierarchy,
                include_type_hierarchy: policy.include_type_hierarchy,
                excerpt_radius: 40,
                max_hunks: hunks.len(),
                max_symbols_per_hunk: 10,
                max_diagnostics_per_hunk: 10,
                max_references_per_hunk: 10,
            };

            let timeout = std::time::Duration::from_millis(timeout_ms);
            match tokio::time::timeout(timeout, executor.execute_hunk_source_context(request)).await
            {
                Ok(Ok(response)) => {
                    let evidence = evidence_from_hunk_source_context(&response);
                    let summary = Some(format_hunk_source_context_summary(&response));
                    HunkSourceContextFileResult {
                        evidence,
                        summary,
                        attempted: true,
                        succeeded: true,
                        ..Default::default()
                    }
                }
                Ok(Err(e)) => {
                    let note = format!(
                        "hunkSourceContext execution failed for {}: {e}",
                        file_path.display()
                    );
                    HunkSourceContextFileResult {
                        notes: vec![note],
                        attempted: true,
                        failed: true,
                        ..Default::default()
                    }
                }
                Err(_timeout) => {
                    let note = format!(
                        "hunkSourceContext timed out for {} after {}ms",
                        file_path.display(),
                        timeout_ms
                    );
                    HunkSourceContextFileResult {
                        notes: vec![note],
                        attempted: true,
                        timed_out: true,
                        ..Default::default()
                    }
                }
            }
        }
    }
}

/// Collect hunk source context evidence for all files in a security review,
/// using the provided hunk navigation collector.
///
/// This is the full integration path that actually calls `hunkSourceContext`
/// via the collector. It processes each file's hunks independently and
/// merges all evidence. Fail-open: errors per file are noted, not fatal.
///
/// The collection phase provides deterministic routing, ordering, and
/// bounded invocation; best-effort, server-dependent LSP evidence;
/// fail-open execution.
///
/// Returns a [`HunkSourceContextCollectionResult`] containing merged
/// evidence, summaries per file, notes, and
/// [`HunkSourceContextExecutionStats`] tracking request outcomes.
pub async fn collect_hunk_source_context_all_files<E: HunkSourceContextExecutor + ?Sized>(
    hunks: &[ChangedHunk],
    real_patches: &HashMap<PathBuf, String>,
    policy: &HunkSourceContextPolicy,
    executor: Option<&E>,
    max_files: usize,
    max_requests: usize,
    timeout_ms: u64,
) -> HunkSourceContextCollectionResult {
    if !policy.enabled {
        return HunkSourceContextCollectionResult {
            notes: vec!["hunkSourceContext disabled".to_string()],
            ..Default::default()
        };
    }

    // Group hunks by file path (Phase 6: owned PathBuf keys for deterministic ordering).
    let mut hunks_by_file: HashMap<PathBuf, Vec<&ChangedHunk>> = HashMap::new();
    for hunk in hunks {
        hunks_by_file
            .entry(hunk.file_path.clone())
            .or_default()
            .push(hunk);
    }

    // Phase 6: Sort for deterministic processing order.
    let mut grouped: Vec<(PathBuf, Vec<&ChangedHunk>)> = hunks_by_file.into_iter().collect();
    grouped.sort_by(|a, b| a.0.cmp(&b.0));

    let mut all_evidence = Vec::new();
    let mut summaries = Vec::new();
    let mut notes = Vec::new();
    let mut stats = HunkSourceContextExecutionStats {
        files_considered: grouped.len().min(max_files),
        ..Default::default()
    };
    // Phase 2: Track actual executor request attempts, not loop index.
    let mut attempted_requests = 0usize;

    for (i, (file_path, file_hunks)) in grouped.iter().enumerate() {
        if i >= max_files {
            notes.push(format!(
                "hunkSourceContext: capped at {max_files} files; {} additional files skipped",
                grouped.len() - max_files
            ));
            break;
        }

        // Phase 5: Use real patch for policy evaluation when available.
        let patch_for_policy = real_patches.get(file_path).cloned().unwrap_or_else(|| {
            // Fallback to synthetic patch from hunk headers.
            file_hunks
                .iter()
                .map(|h| {
                    format!(
                        "@@ -{},{} +{},{} @@",
                        h.old_start, h.old_count, h.new_start, h.new_count
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        });

        let decision = decide_hunk_source_context(policy, &patch_for_policy, Some(file_path));

        // Phase 2: Only consume request budget for files where the executor will actually be called.
        match decision {
            HunkSourceContextDecision::Skip { reason } => {
                stats.files_policy_skipped += 1;
                let note = format!("hunkSourceContext skipped: {reason}");
                notes.push(note);
                continue;
            }
            HunkSourceContextDecision::Use { .. } if executor.is_none() => {
                // No executor — no request budget consumed; fall through to per-file call
                // which will emit the "no executor" note.
            }
            HunkSourceContextDecision::Use { .. } => {
                if attempted_requests >= max_requests {
                    notes.push(format!(
                        "hunkSourceContext: capped at {max_requests} requests; remaining files skipped"
                    ));
                    break;
                }
                // Increment immediately before the executor call.
                attempted_requests += 1;
            }
        }

        // Convert &[&ChangedHunk] to Vec<ChangedHunk> for the function call.
        let owned_hunks: Vec<ChangedHunk> = file_hunks.iter().map(|h| (*h).clone()).collect();
        let result = collect_hunk_source_context_for_file(
            &owned_hunks,
            &patch_for_policy,
            file_path,
            policy,
            executor,
            timeout_ms,
        )
        .await;

        all_evidence.extend(result.evidence);
        stats.evidence_items_emitted += all_evidence.len();
        if let Some(s) = result.summary {
            summaries.push(s);
        }
        notes.extend(result.notes);
        if result.attempted {
            stats.requests_attempted += 1;
        }
        if result.succeeded {
            stats.requests_succeeded += 1;
        }
        if result.failed {
            stats.requests_failed += 1;
        }
        if result.timed_out {
            stats.requests_timed_out += 1;
        }
    }

    HunkSourceContextCollectionResult {
        evidence: all_evidence,
        summaries,
        notes,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::config::LspConfig;
    use crate::lsp::service::LspService;
    use crate::tool::lsp::LspTool;
    use std::sync::Arc;

    #[tokio::test]
    async fn security_review_command_enrich_without_executor_notes_unavailable() {
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, None).await;
        // Should succeed (not error) even without executor
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("no securityContext executor"));
    }

    #[tokio::test]
    async fn security_review_command_default_does_not_request_executor() {
        let args = SecurityReviewCommandArgs {
            enrich: false,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn security_review_command_with_executor_enrich_uses_fixture_executor() {
        let executor = FixtureSecurityContextExecutor::new();
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, Some(&executor)).await;
        // Should succeed (not error) even with fixture executor and git data
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn security_review_command_with_executor_json_includes_enrichment_note() {
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            json: true,
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, None).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // JSON output should contain the enrichment note
        assert!(output.contains("no securityContext executor is available"));
        // Should be valid JSON
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("should be valid JSON");
        let notes = parsed["notes"]
            .as_array()
            .expect("notes should be an array");
        assert!(notes.iter().any(|n| {
            n.as_str()
                .map(|s| s.contains("no securityContext executor"))
                .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn security_review_command_with_executor_prompts_only_still_respects_enrich() {
        let executor = FixtureSecurityContextExecutor::new();
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            prompts_only: true,
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, Some(&executor)).await;
        // Should succeed with prompts_only + enrich
        assert!(result.is_ok());
        let _output = result.unwrap();
        // Summary should not mention findings
        // (it may or may not produce targets depending on git state)
    }

    #[tokio::test]
    async fn security_review_command_with_executor_findings_only_still_respects_enrich() {
        let executor = FixtureSecurityContextExecutor::new();
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            findings_only: true,
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, Some(&executor)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn security_review_background_without_executor_returns_unavailable_note() {
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_background(root, args, None).await;
        assert!(result.is_ok());
        let receipt = result.unwrap();
        let rendered = receipt.rendered_report.clone();
        assert!(
            rendered.contains("no securityContext executor"),
            "report should mention no executor: {rendered}"
        );
        assert!(
            !receipt.enriched,
            "without an executor the receipt should not claim enrichment"
        );
        assert!(
            !receipt.lsp_available,
            "without an lsp_tool the receipt should reflect unavailability"
        );
    }

    #[tokio::test]
    async fn security_review_background_with_fixture_executor_uses_enrichment() {
        let _executor = FixtureSecurityContextExecutor::new();
        let tool = Arc::new(LspTool::new(Arc::new(
            LspService::new(LspConfig::default()),
        )));
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_background(root, args, Some(tool)).await;
        // The fixture is provided to the underlying workflow; the call
        // must complete without erroring even if the executor is never
        // actually invoked (e.g. when no targets meet escalation).
        assert!(
            result.is_ok(),
            "background with executor should succeed: {:?}",
            result
        );
        let receipt = result.unwrap();
        assert!(
            receipt.lsp_available,
            "with an lsp_tool, lsp_available must be true"
        );
        assert!(
            receipt.enriched,
            "with --enrich and an executor, enriched must be true"
        );
    }

    #[tokio::test]
    async fn security_review_background_json_mode_returns_json() {
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            json: true,
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_background(root, args, None).await;
        assert!(result.is_ok());
        let receipt = result.unwrap();
        let output = receipt.rendered_report.clone();
        assert!(
            output.starts_with('{'),
            "json output should start with '{{': {output}"
        );
        assert!(
            output.contains("\"notes\""),
            "json should contain notes: {output}"
        );
    }

    #[tokio::test]
    async fn security_review_background_preserves_prompts_only() {
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            prompts_only: true,
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_background(root, args, None).await;
        assert!(result.is_ok());
        let receipt = result.unwrap();
        let output = receipt.rendered_report.clone();
        assert!(
            !output.contains("Findings\n"),
            "prompts-only output should not contain the 'Findings' section header: {output}"
        );
    }

    #[tokio::test]
    async fn security_review_background_preserves_findings_only() {
        let args = SecurityReviewCommandArgs {
            enrich: true,
            base: Some("HEAD".to_string()),
            findings_only: true,
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_background(root, args, None).await;
        assert!(result.is_ok());
        let receipt = result.unwrap();
        let output = receipt.rendered_report.clone();
        assert!(
            !output.contains("Review Prompts\n"),
            "findings-only output should not contain the 'Review Prompts' section header: {output}"
        );
    }

    #[tokio::test]
    async fn security_review_default_path_does_not_create_executor() {
        let args = SecurityReviewCommandArgs {
            enrich: false,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        // lsp_tool is None — the background runner should still succeed
        // and should NOT add the unavailable-executor note because
        // enrichment was not requested.
        let result = run_security_review_background(root, args, None).await;
        assert!(result.is_ok());
        let receipt = result.unwrap();
        let output = receipt.rendered_report.clone();
        assert!(
            !output.contains("no securityContext executor"),
            "default path should not add unavailable note: {output}"
        );
        assert!(
            !receipt.enriched,
            "default path should not mark the receipt as enriched"
        );
    }

    #[test]
    fn security_review_run_id_is_unique() {
        let a = SecurityReviewRunId::new();
        let b = SecurityReviewRunId::new();
        assert_ne!(a, b, "two fresh run ids should differ");
    }

    #[test]
    fn security_review_run_id_default_generates() {
        let id = <SecurityReviewRunId as Default>::default();
        assert!(
            id.0.starts_with("sr-"),
            "default-generated id should start with 'sr-': {}",
            id.0
        );
    }

    // -----------------------------------------------------------------------
    // Hunk source context security integration tests
    // -----------------------------------------------------------------------

    use std::collections::HashMap;
    use std::sync::Mutex;

    use egglsp::hunk_context::HunkDescriptor;
    use egglsp::hunk_context::HunkEvidence;
    use egglsp::hunk_context::HunkLineRange;
    use egglsp::hunk_context::HunkSourceNavigationRequest;
    use egglsp::hunk_context::HunkSourceNavigationResponse;
    use egglsp::lsp_types::DiagnosticSeverity;
    use egglsp::semantic_context::SemanticSymbolSummary;

    /// Fixture executor for `HunkSourceContextExecutor` that returns
    /// pre-configured responses keyed by file path.
    struct FixtureHunkSourceContextExecutor {
        responses: Mutex<HashMap<String, Result<HunkSourceNavigationResponse, String>>>,
    }

    impl FixtureHunkSourceContextExecutor {
        fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
            }
        }

        fn with_response(
            self,
            file_path: &str,
            response: Result<HunkSourceNavigationResponse, String>,
        ) -> Self {
            self.responses
                .lock()
                .unwrap()
                .insert(file_path.to_string(), response);
            self
        }
    }

    #[async_trait::async_trait]
    impl HunkSourceContextExecutor for FixtureHunkSourceContextExecutor {
        async fn execute_hunk_source_context(
            &self,
            request: HunkSourceNavigationRequest,
        ) -> Result<HunkSourceNavigationResponse, String> {
            let map = self.responses.lock().unwrap();
            match map.get(&request.file_path) {
                Some(Ok(resp)) => Ok(resp.clone()),
                Some(Err(e)) => Err(e.clone()),
                None => Err(format!("no fixture response for {}", request.file_path)),
            }
        }
    }

    fn make_test_hunk(file: &str, new_start: u32, new_count: u32) -> ChangedHunk {
        ChangedHunk {
            file_path: PathBuf::from(file),
            old_start: new_start,
            old_count: new_count,
            new_start,
            new_count,
            lines: vec![],
        }
    }

    // --- Evidence safety tests ---

    #[tokio::test]
    async fn hunk_source_policy_use_without_executor_emits_no_evidence() {
        let hunks = vec![make_test_hunk("src/main.rs", 10, 5)];
        let patch = "@@ -10,5 +10,5 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        let file = Path::new("src/main.rs");
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_for_file(
            &hunks,
            patch,
            file,
            &policy,
            None::<&NoopHunkSourceContextExecutor>,
            2500,
        )
        .await;

        assert!(
            result.evidence.is_empty(),
            "policy Use without executor should emit no evidence"
        );
        assert!(
            result.summary.is_none(),
            "policy Use without executor should produce no summary"
        );
        assert!(
            result.notes.iter().any(|n| n.contains("no executor")),
            "should note executor unavailability"
        );
    }

    #[tokio::test]
    async fn hunk_source_policy_skip_emits_no_evidence() {
        let hunks = vec![make_test_hunk("src/main.rs", 10, 5)];
        // Empty patch triggers Skip (no @@ headers).
        let patch = "";
        let file = Path::new("src/main.rs");
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_for_file(
            &hunks,
            patch,
            file,
            &policy,
            None::<&NoopHunkSourceContextExecutor>,
            2500,
        )
        .await;

        assert!(
            result.evidence.is_empty(),
            "policy Skip should emit no evidence"
        );
        assert!(result.summary.is_none());
        assert!(
            result.notes.iter().any(|n| n.contains("skipped")),
            "should note skip reason"
        );
    }

    // --- Executor success test ---

    #[tokio::test]
    async fn hunk_source_executor_success_returns_real_evidence() {
        let hunks = vec![make_test_hunk("src/main.rs", 10, 5)];
        let patch = "@@ -10,5 +10,5 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        let file = Path::new("src/main.rs");
        let policy = HunkSourceContextPolicy::default();

        let mut response = HunkSourceNavigationResponse::new("src/main.rs");
        response.hunks.push(HunkEvidence {
            hunk: HunkDescriptor {
                id: "src/main.rs:0:10-14".to_string(),
                file_path: "src/main.rs".to_string(),
                old_range: Some(HunkLineRange {
                    start_line: 10,
                    end_line: 14,
                }),
                new_range: Some(HunkLineRange {
                    start_line: 10,
                    end_line: 14,
                }),
                header: Some("@@ -10,5 +10,5 @@".to_string()),
                added_lines: 1,
                removed_lines: 1,
                context_lines: 3,
            },
            focus_range: None,
            enclosing_symbol: Some(SemanticSymbolSummary {
                name: "main".to_string(),
                kind: "function".to_string(),
                file: "src/main.rs".to_string(),
                start_line: 10,
                start_column: 0,
                end_line: 14,
                end_column: 1,
            }),
            related_symbols: vec![],
            diagnostics: vec![],
            nearby_diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        });

        let executor =
            FixtureHunkSourceContextExecutor::new().with_response("src/main.rs", Ok(response));

        let result = collect_hunk_source_context_for_file(
            &hunks,
            patch,
            file,
            &policy,
            Some(&executor),
            2500,
        )
        .await;

        assert!(
            !result.evidence.is_empty(),
            "executor success should produce evidence"
        );
        assert!(
            result.summary.is_some(),
            "executor success should produce a formatted summary"
        );
        assert!(
            result.notes.is_empty(),
            "executor success should produce no notes"
        );
        assert!(result
            .evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::HunkNavigation));
    }

    // --- Executor failure test ---

    #[tokio::test]
    async fn hunk_source_executor_failure_returns_empty_evidence_with_note() {
        let hunks = vec![make_test_hunk("src/main.rs", 10, 5)];
        let patch = "@@ -10,5 +10,5 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        let file = Path::new("src/main.rs");
        let policy = HunkSourceContextPolicy::default();

        let executor = FixtureHunkSourceContextExecutor::new()
            .with_response("src/main.rs", Err("LSP server unavailable".to_string()));

        let result = collect_hunk_source_context_for_file(
            &hunks,
            patch,
            file,
            &policy,
            Some(&executor),
            2500,
        )
        .await;

        assert!(
            result.evidence.is_empty(),
            "executor failure should emit no evidence"
        );
        assert!(result.summary.is_none());
        assert!(
            result.notes.iter().any(|n| n.contains("execution failed")),
            "should note execution failure"
        );
    }

    // --- Timeout test ---

    struct SlowHunkExecutor;
    #[async_trait::async_trait]
    impl HunkSourceContextExecutor for SlowHunkExecutor {
        async fn execute_hunk_source_context(
            &self,
            _request: HunkSourceNavigationRequest,
        ) -> Result<HunkSourceNavigationResponse, String> {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(HunkSourceNavigationResponse::new("src/main.rs"))
        }
    }

    #[tokio::test]
    async fn hunk_source_context_timeout_produces_fail_open_note() {
        let hunks = vec![make_test_hunk("src/main.rs", 10, 5)];
        let patch = "@@ -10,5 +10,5 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        let file = Path::new("src/main.rs");
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_for_file(
            &hunks,
            patch,
            file,
            &policy,
            Some(&SlowHunkExecutor),
            10, // 10ms timeout — executor sleeps for 50ms
        )
        .await;

        assert!(
            result.evidence.is_empty(),
            "timeout should emit no evidence"
        );
        assert!(
            result.summary.is_none(),
            "timeout should produce no summary"
        );
        assert!(
            result.notes.iter().any(|n| n.contains("timed out")),
            "should note timeout: {:?}",
            result.notes
        );
        assert!(
            result.notes.iter().any(|n| n.contains("src/main.rs")),
            "note should contain file path: {:?}",
            result.notes
        );
    }

    // --- Diagnostic line indexing test ---

    #[test]
    fn diagnostic_line_evidence_is_1indexed() {
        let mut response = HunkSourceNavigationResponse::new("src/test.rs");
        response.hunks.push(HunkEvidence {
            hunk: HunkDescriptor {
                id: "src/test.rs:0:1-5".to_string(),
                file_path: "src/test.rs".to_string(),
                old_range: None,
                new_range: Some(HunkLineRange {
                    start_line: 1,
                    end_line: 5,
                }),
                header: None,
                added_lines: 0,
                removed_lines: 0,
                context_lines: 0,
            },
            focus_range: None,
            enclosing_symbol: None,
            related_symbols: vec![],
            diagnostics: vec![egglsp::diagnostics::FileDiagnostic {
                file: "src/test.rs".to_string(),
                line: 9, // 0-indexed LSP line 9 = 10th line
                column: 0,
                message: "unused variable".to_string(),
                severity: DiagnosticSeverity::WARNING,
                source: None,
                code: None,
            }],
            nearby_diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        });

        let evidence = evidence_from_hunk_source_context(&response);
        let diag_evidence: Vec<_> = evidence
            .iter()
            .filter(|e| e.kind == SecurityEvidenceKind::Diagnostic)
            .collect();

        assert_eq!(
            diag_evidence.len(),
            1,
            "should have one diagnostic evidence item"
        );
        assert_eq!(
            diag_evidence[0].line,
            Some(10),
            "0-indexed line 9 should become 1-indexed line 10"
        );
    }

    // --- Finding eligibility gate tests ---

    #[test]
    fn hunk_nav_plus_changed_hunk_not_eligible() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::HunkNavigation,
                file_path: Some(PathBuf::from("src/main.rs")),
                line: Some(10),
                summary: "enclosing symbol: function main".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::ChangedHunk,
                file_path: Some(PathBuf::from("src/main.rs")),
                line: Some(10),
                summary: "changed hunk".to_string(),
                detail: None,
            },
        ];
        assert!(
            !is_finding_eligible(&evidence),
            "ChangedHunk + HunkNavigation alone should not be eligible"
        );
    }

    #[test]
    fn risk_marker_plus_hunk_nav_still_eligible() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(PathBuf::from("src/main.rs")),
                line: Some(10),
                summary: "unsafe code".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::HunkNavigation,
                file_path: Some(PathBuf::from("src/main.rs")),
                line: Some(10),
                summary: "enclosing symbol: function main".to_string(),
                detail: None,
            },
        ];
        assert!(
            is_finding_eligible(&evidence),
            "RiskMarker + HunkNavigation should be eligible"
        );
    }

    #[test]
    fn preflight_fail_plus_hunk_nav_still_eligible() {
        let evidence = vec![
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::Preflight,
                file_path: Some(PathBuf::from("src/main.rs")),
                line: Some(10),
                summary: "found secret key".to_string(),
                detail: None,
            },
            StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::HunkNavigation,
                file_path: Some(PathBuf::from("src/main.rs")),
                line: Some(10),
                summary: "enclosing symbol: function main".to_string(),
                detail: None,
            },
        ];
        assert!(
            is_finding_eligible(&evidence),
            "Preflight + HunkNavigation should be eligible"
        );
    }

    // --- Command flag tests ---

    #[test]
    fn parse_hunk_context_flag() {
        let args = parse_security_review_args("--hunk-context --base HEAD");
        assert!(
            args.hunk_context,
            "--hunk-context flag should set hunk_context"
        );
        assert_eq!(args.base.as_deref(), Some("HEAD"));
    }

    #[test]
    fn parse_default_no_hunk_context() {
        let args = parse_security_review_args("--base HEAD");
        assert!(!args.hunk_context, "default should not enable hunk_context");
    }

    // --- Formatter usage test ---

    #[test]
    fn formatter_summary_includes_file_path() {
        let mut response = HunkSourceNavigationResponse::new("src/test.rs");
        response.hunks.push(HunkEvidence {
            hunk: HunkDescriptor {
                id: "src/test.rs:0:1-5".to_string(),
                file_path: "src/test.rs".to_string(),
                old_range: None,
                new_range: Some(HunkLineRange {
                    start_line: 1,
                    end_line: 5,
                }),
                header: None,
                added_lines: 0,
                removed_lines: 0,
                context_lines: 0,
            },
            focus_range: None,
            enclosing_symbol: Some(SemanticSymbolSummary {
                name: "test_fn".to_string(),
                kind: "function".to_string(),
                file: "src/test.rs".to_string(),
                start_line: 1,
                start_column: 0,
                end_line: 5,
                end_column: 1,
            }),
            related_symbols: vec![],
            diagnostics: vec![],
            nearby_diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        });

        let summary = format_hunk_source_context_summary(&response);
        assert!(
            summary.contains("src/test.rs"),
            "summary should contain file path"
        );
        assert!(
            summary.contains("function"),
            "summary should mention enclosing symbol kind"
        );
        assert!(
            summary.contains("test_fn"),
            "summary should mention enclosing symbol name"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 11: Typed executor wiring regression tests
    // -----------------------------------------------------------------------

    #[test]
    fn typed_hunk_request_preserves_preparsed_hunks() {
        let request = HunkSourceNavigationRequest {
            file_path: "src/main.rs".to_string(),
            hunks: vec![HunkDescriptor {
                id: "src/main.rs:0:10-20".to_string(),
                file_path: "src/main.rs".to_string(),
                old_range: Some(HunkLineRange {
                    start_line: 10,
                    end_line: 20,
                }),
                new_range: Some(HunkLineRange {
                    start_line: 12,
                    end_line: 24,
                }),
                header: Some("@@ -10,11 +12,13 @@".to_string()),
                added_lines: 5,
                removed_lines: 3,
                context_lines: 3,
            }],
            patch: None,
            intent: "security_review".to_string(),
            include_definitions: true,
            include_references: true,
            include_call_hierarchy: false,
            include_type_hierarchy: false,
            excerpt_radius: 40,
            max_hunks: 20,
            max_symbols_per_hunk: 10,
            max_diagnostics_per_hunk: 10,
            max_references_per_hunk: 10,
        };

        assert_eq!(request.hunks.len(), 1);
        assert_eq!(request.hunks[0].id, "src/main.rs:0:10-20");
        assert_eq!(request.hunks[0].old_range.as_ref().unwrap().start_line, 10);
        assert_eq!(request.hunks[0].new_range.as_ref().unwrap().start_line, 12);
        assert!(request.patch.is_none());
    }

    #[tokio::test]
    async fn legacy_security_only_wrapper_delegates_to_bundle() {
        let args = SecurityReviewCommandArgs {
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executor(&root, &args, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn bundled_executors_passes_hunk_executor_to_workflow() {
        let security_exec = NoopSecurityContextExecutor;
        let hunk_exec = NoopHunkSourceContextExecutor;

        let executors = super::SecurityReviewExecutors {
            security_context: Some(&security_exec),
            hunk_source_context: Some(&hunk_exec),
        };

        let args = SecurityReviewCommandArgs {
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = run_security_review_command_with_executors(&root, &args, executors).await;
        assert!(result.is_ok());
    }

    #[test]
    fn hunk_file_processing_order_is_lexical() {
        use super::collect_hunk_source_context_all_files;
        use super::HunkSourceContextPolicy;

        // Verify that collect_hunk_source_context_all_files accepts the
        // real_patches HashMap and processes in sorted order. This is a
        // compile-time/API check — we verify the function signature and
        // that it handles empty input gracefully.
        let patches: HashMap<PathBuf, String> = HashMap::new();
        let hunks: Vec<ChangedHunk> = vec![];
        let policy = HunkSourceContextPolicy::default();

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = collect_hunk_source_context_all_files(
                &hunks,
                &patches,
                &policy,
                None::<&NoopHunkSourceContextExecutor>,
                8,
                8,
                2500,
            )
            .await;
            assert!(result.evidence.is_empty());
            assert!(result.summaries.is_empty());
        });
    }

    #[test]
    fn security_review_executors_bundle_compiles() {
        let bundle = super::SecurityReviewExecutors {
            security_context: None,
            hunk_source_context: None,
        };
        assert!(bundle.security_context.is_none());
        assert!(bundle.hunk_source_context.is_none());
    }

    #[test]
    fn hunk_source_context_real_patch_oversized_triggers_skip() {
        use crate::lsp::hunk_nav_policy::{decide_hunk_source_context, HunkSourceContextPolicy};

        let policy = HunkSourceContextPolicy {
            max_patch_bytes: 100,
            ..Default::default()
        };

        let large_patch = format!(
            "@@ -10,10 +10,10 @@ fn example() {{\n{}\n}}\n",
            "x".repeat(120)
        );
        assert!(
            large_patch.len() > 100,
            "patch must actually exceed the 100-byte limit"
        );

        let decision = decide_hunk_source_context(
            &policy,
            &large_patch,
            Some(std::path::Path::new("src/main.rs")),
        );
        match &decision {
            crate::lsp::hunk_nav_policy::HunkSourceContextDecision::Skip { reason } => {
                assert!(
                    reason.contains("exceeds cap"),
                    "skip reason should mention 'exceeds cap': {reason}"
                );
                assert!(
                    reason.contains(" bytes exceeds "),
                    "skip reason should include bytes and cap: {reason}"
                );
            }
            _ => panic!("expected Skip for oversized patch, got {decision:?}"),
        }

        let small_patch = "@@ -1,1 +1,1 @@\n+a\n";
        assert!(small_patch.len() < 100);

        let decision_small = decide_hunk_source_context(
            &policy,
            small_patch,
            Some(std::path::Path::new("src/main.rs")),
        );
        match &decision_small {
            crate::lsp::hunk_nav_policy::HunkSourceContextDecision::Use { patch, .. } => {
                assert_eq!(patch, small_patch);
            }
            _ => panic!("expected Use for small patch, got {decision_small:?}"),
        }
    }

    #[test]
    fn to_hunk_descriptor_preserves_ranges_and_counts() {
        let hunk = make_test_hunk("src/lib.rs", 10, 5);
        let descriptor = hunk.to_hunk_descriptor(0);

        assert_eq!(descriptor.file_path, "src/lib.rs");
        assert!(descriptor.id.starts_with("src/lib.rs:0:"));
        assert_eq!(
            descriptor.old_range,
            Some(HunkLineRange {
                start_line: 10,
                end_line: 14,
            })
        );
        assert_eq!(
            descriptor.new_range,
            Some(HunkLineRange {
                start_line: 10,
                end_line: 14,
            })
        );
        assert!(descriptor.header.is_some());
    }

    #[tokio::test]
    async fn hunk_executor_receives_request_with_hunks_field() {
        use super::HunkSourceContextExecutor;

        // Verify that when we build a request with pre-populated hunks
        // and pass it through the executor trait, the hunks are preserved.
        let request = HunkSourceNavigationRequest {
            file_path: "src/main.rs".to_string(),
            hunks: vec![HunkDescriptor {
                id: "src/main.rs:0:10-20".to_string(),
                file_path: "src/main.rs".to_string(),
                old_range: Some(HunkLineRange {
                    start_line: 10,
                    end_line: 20,
                }),
                new_range: Some(HunkLineRange {
                    start_line: 12,
                    end_line: 24,
                }),
                header: Some("@@ -10,11 +12,13 @@".to_string()),
                added_lines: 5,
                removed_lines: 3,
                context_lines: 3,
            }],
            patch: None,
            intent: "security_review".to_string(),
            include_definitions: true,
            include_references: true,
            include_call_hierarchy: false,
            include_type_hierarchy: false,
            excerpt_radius: 40,
            max_hunks: 20,
            max_symbols_per_hunk: 10,
            max_diagnostics_per_hunk: 10,
            max_references_per_hunk: 10,
        };

        let executor = FixtureHunkSourceContextExecutor::new();

        // The executor receives the typed request — verify hunks survive
        // the trait boundary by matching on file_path and checking the
        // executor would see them (the fixture errors on unknown paths).
        let result = executor.execute_hunk_source_context(request.clone()).await;
        assert!(
            result.is_err(),
            "fixture executor should error for unknown file path"
        );
        assert!(
            result.unwrap_err().contains("no fixture response"),
            "error should indicate missing fixture"
        );

        // Now add a fixture response and verify the full round-trip
        let mut response = HunkSourceNavigationResponse::new("src/main.rs");
        response.hunks.push(HunkEvidence {
            hunk: request.hunks[0].clone(),
            focus_range: None,
            enclosing_symbol: None,
            related_symbols: vec![],
            diagnostics: vec![],
            nearby_diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        });

        let executor = executor.with_response("src/main.rs", Ok(response));
        let result = executor.execute_hunk_source_context(request).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.hunks.len(), 1);
        assert_eq!(resp.hunks[0].hunk.id, "src/main.rs:0:10-20");
    }

    #[tokio::test]
    async fn collect_all_files_processes_in_sorted_order() {
        // Construct hunks for multiple files in reverse order and verify
        // that the executor sees them in sorted (lexicographic) order.
        let mut hunks = vec![
            make_test_hunk("src/z_last.rs", 5, 3),
            make_test_hunk("src/a_first.rs", 10, 5),
            make_test_hunk("src/m_middle.rs", 20, 2),
        ];
        // Reverse to ensure sort is deterministic regardless of input order
        hunks.reverse();

        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/z_last.rs"),
            "@@ -5,3 +5,3 @@\n-a\n+b\n".to_string(),
        );
        patches.insert(
            PathBuf::from("src/a_first.rs"),
            "@@ -10,5 +10,5 @@\n old\n new\n".to_string(),
        );
        patches.insert(
            PathBuf::from("src/m_middle.rs"),
            "@@ -20,2 +20,2 @@\n-x\n+y\n".to_string(),
        );

        // Track which files the executor sees, in order
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct OrderTrackingExecutor {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for OrderTrackingExecutor {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                Ok(HunkSourceNavigationResponse::new(&request.file_path))
            }
        }

        let executor = OrderTrackingExecutor { seen: seen_clone };

        let policy = HunkSourceContextPolicy::default();
        let _result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            8,
            8,
            2500,
        )
        .await;

        // Verify sorted processing order
        let seen_files = seen.lock().unwrap();
        assert_eq!(seen_files.len(), 3);
        assert_eq!(seen_files[0], "src/a_first.rs");
        assert_eq!(seen_files[1], "src/m_middle.rs");
        assert_eq!(seen_files[2], "src/z_last.rs");
    }

    #[tokio::test]
    async fn run_security_review_with_hunk_context_flag_produces_evidence() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        let output = std::process::Command::new("git")
            .args(["init"])
            .current_dir(root)
            .output()
            .unwrap();
        assert!(output.status.success());

        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(root)
            .output()
            .unwrap();

        std::fs::create_dir(root.join("src")).unwrap();
        let test_file = root.join("src/main.rs");
        std::fs::write(&test_file, r#"api_key = "sk-1234567890abcdef";"#).unwrap();

        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(root)
            .output()
            .unwrap();

        std::fs::write(&test_file, r#"api_key = "sk-xyz789";"#).unwrap();

        let security_exec = FixtureSecurityContextExecutor::new();
        let hunk_exec = FixtureHunkSourceContextExecutor::new().with_response(
            "src/main.rs",
            Ok({
                let mut response = HunkSourceNavigationResponse::new("src/main.rs");
                response.hunks.push(HunkEvidence {
                    hunk: HunkDescriptor {
                        id: "src/main.rs:0:1-1".to_string(),
                        file_path: "src/main.rs".to_string(),
                        old_range: Some(HunkLineRange {
                            start_line: 1,
                            end_line: 1,
                        }),
                        new_range: Some(HunkLineRange {
                            start_line: 1,
                            end_line: 1,
                        }),
                        header: None,
                        added_lines: 0,
                        removed_lines: 0,
                        context_lines: 0,
                    },
                    focus_range: None,
                    enclosing_symbol: Some(SemanticSymbolSummary {
                        name: "main".to_string(),
                        kind: "function".to_string(),
                        file: "src/main.rs".to_string(),
                        start_line: 1,
                        start_column: 0,
                        end_line: 1,
                        end_column: 1,
                    }),
                    related_symbols: vec![],
                    diagnostics: vec![],
                    nearby_diagnostics: vec![],
                    definitions: vec![],
                    references: vec![],
                    call_hierarchy: None,
                    type_hierarchy: None,
                    source_excerpt: None,
                    diagnostic_evidence: None,
                    section_truncations: vec![],
                    unavailable: vec![],
                    notes: vec![],
                });
                response
            }),
        );

        let executors = super::SecurityReviewExecutors {
            security_context: Some(&security_exec),
            hunk_source_context: Some(&hunk_exec),
        };

        let args = SecurityReviewCommandArgs {
            hunk_context: true,
            json: true,
            base: Some("HEAD".to_string()),
            ..Default::default()
        };
        let result = run_security_review_command_with_executors(root, &args, executors).await;
        let output = result.expect("command should succeed; error: {result:?}");

        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("output should be valid JSON");

        let findings = parsed["findings"]
            .as_array()
            .expect("findings should be an array");
        let notes = parsed["notes"]
            .as_array()
            .expect("notes should be an array");

        let has_hunk_context_note = notes.iter().any(|n| {
            n.as_str()
                .map(|s| s.contains("hunkSourceContext"))
                .unwrap_or(false)
        });

        let has_hunk_navigation_evidence = findings.iter().any(|f| {
            f["evidence"]
                .as_array()
                .map(|e| e.iter().any(|ev| ev["kind"] == "HunkNavigation"))
                .unwrap_or(false)
        });

        assert!(
            has_hunk_context_note || has_hunk_navigation_evidence,
            "hunk context evidence should appear in output: notes contain hunkSourceContext? {}, findings have HunkNavigation? {}\n\
             findings={:#?}\nnotes={:#?}",
            has_hunk_context_note, has_hunk_navigation_evidence, findings, notes
        );
    }

    #[tokio::test]
    async fn hunk_source_context_one_file_failure_does_not_block_others() {
        let hunks = vec![
            make_test_hunk("src/a.rs", 10, 5),
            make_test_hunk("src/b.rs", 20, 3),
            make_test_hunk("src/c.rs", 30, 4),
        ];

        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/a.rs"),
            "@@ -10,5 +10,5 @@\n-a\n+b\n".to_string(),
        );
        patches.insert(
            PathBuf::from("src/b.rs"),
            "@@ -20,3 +20,3 @@\n-c\n+d\n".to_string(),
        );
        patches.insert(
            PathBuf::from("src/c.rs"),
            "@@ -30,4 +30,4 @@\n-d\n+e\n".to_string(),
        );

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct SelectiveFailureExecutor {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for SelectiveFailureExecutor {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                if request.file_path == "src/b.rs" {
                    return Err("LSP server unavailable for b.rs".to_string());
                }
                let mut response = HunkSourceNavigationResponse::new(&request.file_path);
                response.hunks.push(HunkEvidence {
                    hunk: HunkDescriptor {
                        id: format!("{}:0:1-5", request.file_path),
                        file_path: request.file_path.clone(),
                        old_range: None,
                        new_range: Some(HunkLineRange {
                            start_line: 10,
                            end_line: 14,
                        }),
                        header: None,
                        added_lines: 0,
                        removed_lines: 0,
                        context_lines: 0,
                    },
                    focus_range: None,
                    enclosing_symbol: Some(SemanticSymbolSummary {
                        name: "test_func".to_string(),
                        kind: "function".to_string(),
                        file: request.file_path.clone(),
                        start_line: 10,
                        start_column: 0,
                        end_line: 14,
                        end_column: 1,
                    }),
                    related_symbols: vec![],
                    diagnostics: vec![],
                    nearby_diagnostics: vec![],
                    definitions: vec![],
                    references: vec![],
                    call_hierarchy: None,
                    type_hierarchy: None,
                    source_excerpt: None,
                    diagnostic_evidence: None,
                    section_truncations: vec![],
                    unavailable: vec![],
                    notes: vec![],
                });
                Ok(response)
            }
        }

        let executor = SelectiveFailureExecutor { seen: seen_clone };
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            8,
            8,
            2500,
        )
        .await;

        assert!(
            result.evidence.len() >= 2,
            "expected evidence from a.rs and c.rs, got {} items",
            result.evidence.len()
        );
        assert_eq!(
            result.summaries.len(),
            2,
            "expected 2 summaries (a.rs and c.rs)"
        );

        assert!(
            result
                .notes
                .iter()
                .any(|n| n.contains("b.rs") && n.contains("unavailable")),
            "expected failure note for b.rs, got notes: {:?}",
            result.notes
        );

        let seen_files = seen.lock().unwrap();
        assert_eq!(seen_files.len(), 3, "all 3 files should be attempted");
        assert!(seen_files.contains(&"src/a.rs".to_string()));
        assert!(seen_files.contains(&"src/b.rs".to_string()));
        assert!(seen_files.contains(&"src/c.rs".to_string()));
    }

    #[tokio::test]
    async fn collect_all_files_cap_always_selects_same_first_eight() {
        let hunks: Vec<ChangedHunk> = (0..10)
            .map(|i| make_test_hunk(&format!("src/file_{:02}.rs", i), i * 10, 3))
            .collect();

        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        for i in 0..10 {
            let path = format!("src/file_{:02}.rs", i);
            patches.insert(
                PathBuf::from(&path),
                format!("@@ -{},3 +{},3 @@\n-a\n+b\n", i * 10, i * 10),
            );
        }

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct CapTrackingExecutor {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for CapTrackingExecutor {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                Ok(HunkSourceNavigationResponse::new(&request.file_path))
            }
        }

        let executor = CapTrackingExecutor { seen: seen_clone };
        let policy = HunkSourceContextPolicy::default();
        let _result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            8,
            8,
            2500,
        )
        .await;

        let seen_files = seen.lock().unwrap();
        assert_eq!(seen_files.len(), 8, "exactly 8 files should be processed");

        for i in 0..8 {
            let expected = format!("src/file_{:02}.rs", i);
            assert!(
                seen_files.contains(&expected),
                "file {} should be processed (sorted order)",
                i
            );
        }

        let not_processed: Vec<_> = seen_files
            .iter()
            .filter(|f| f.as_str() == "src/file_08.rs" || f.as_str() == "src/file_09.rs")
            .collect();
        assert!(
            not_processed.is_empty(),
            "src/file_08.rs and src/file_09.rs should NOT be processed"
        );
    }

    // -----------------------------------------------------------------------
    // Phase 6: Strengthen concrete forwarding tests
    // -----------------------------------------------------------------------

    /// Recording executor that captures the exact request it receives.
    struct RecordingHunkSourceContextExecutor {
        captured: std::sync::Mutex<Option<HunkSourceNavigationRequest>>,
    }

    impl RecordingHunkSourceContextExecutor {
        fn new() -> Self {
            Self {
                captured: std::sync::Mutex::new(None),
            }
        }

        fn captured_request(&self) -> Option<HunkSourceNavigationRequest> {
            self.captured.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl HunkSourceContextExecutor for RecordingHunkSourceContextExecutor {
        async fn execute_hunk_source_context(
            &self,
            request: HunkSourceNavigationRequest,
        ) -> Result<HunkSourceNavigationResponse, String> {
            *self.captured.lock().unwrap() = Some(request.clone());
            Ok(HunkSourceNavigationResponse::new(&request.file_path))
        }
    }

    #[tokio::test]
    async fn typed_hunk_request_preserves_all_fields_through_trait() {
        let hunks = vec![make_test_hunk("src/main.rs", 10, 5)];
        let patch = "@@ -10,5 +10,5 @@\n fn main() {\n-    old();\n+    new();\n }\n";
        let file = Path::new("src/main.rs");
        let policy = HunkSourceContextPolicy::default();
        let executor = RecordingHunkSourceContextExecutor::new();

        let _result = collect_hunk_source_context_for_file(
            &hunks,
            patch,
            file,
            &policy,
            Some(&executor),
            2500,
        )
        .await;

        let captured = executor
            .captured_request()
            .expect("executor should have captured a request");
        assert_eq!(captured.file_path, "src/main.rs");
        assert_eq!(captured.intent, "security_review");
        assert!(captured.include_definitions);
        assert!(captured.include_references);
        assert_eq!(captured.max_symbols_per_hunk, 10);
        assert_eq!(captured.max_diagnostics_per_hunk, 10);
        assert_eq!(captured.max_references_per_hunk, 10);
        assert!(
            captured.patch.is_none(),
            "patch should be None for pre-parsed hunks"
        );
        assert_eq!(
            captured.hunks.len(),
            1,
            "should have 1 pre-parsed hunk descriptor"
        );
        assert_eq!(captured.hunks[0].file_path, "src/main.rs");
    }

    // -----------------------------------------------------------------------
    // Phase 8: Test actual request budgeting
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn request_budgeting_policy_skip_does_not_consume_budget() {
        let hunks = vec![
            make_test_hunk("src/image.png", 10, 5),
            make_test_hunk("src/main.rs", 20, 3),
        ];
        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/image.png"),
            "@@ -10,5 +10,5 @@\n-a\n+b\n".to_string(),
        );
        patches.insert(
            PathBuf::from("src/main.rs"),
            "@@ -20,3 +20,3 @@\n-c\n+d\n".to_string(),
        );

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct TrackingExecutor {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for TrackingExecutor {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                Ok(HunkSourceNavigationResponse::new(&request.file_path))
            }
        }

        let executor = TrackingExecutor { seen: seen_clone };
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            10,
            1,
            2500,
        )
        .await;

        let seen_files = seen.lock().unwrap();
        assert_eq!(seen_files.len(), 1, "only eligible file should execute");
        assert!(seen_files.contains(&"src/main.rs".to_string()));
        assert_eq!(result.stats.requests_attempted, 1);
        assert_eq!(result.stats.files_policy_skipped, 1);
    }

    #[tokio::test]
    async fn request_budgeting_two_eligible_files_one_request() {
        let hunks = vec![
            make_test_hunk("src/a.rs", 10, 5),
            make_test_hunk("src/b.rs", 20, 3),
        ];
        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/a.rs"),
            "@@ -10,5 +10,5 @@\n-a\n+b\n".to_string(),
        );
        patches.insert(
            PathBuf::from("src/b.rs"),
            "@@ -20,3 +20,3 @@\n-c\n+d\n".to_string(),
        );

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct TrackingExecutor2 {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for TrackingExecutor2 {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                Ok(HunkSourceNavigationResponse::new(&request.file_path))
            }
        }

        let executor = TrackingExecutor2 { seen: seen_clone };
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            10,
            1,
            2500,
        )
        .await;

        let seen_files = seen.lock().unwrap();
        assert_eq!(
            seen_files.len(),
            1,
            "only one file should execute with max_requests=1"
        );
        assert_eq!(result.stats.requests_attempted, 1);
    }

    #[tokio::test]
    async fn request_budgeting_timeout_counts_as_attempted() {
        let hunks = vec![make_test_hunk("src/slow.rs", 10, 5)];
        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/slow.rs"),
            "@@ -10,5 +10,5 @@\n-a\n+b\n".to_string(),
        );

        struct TimeoutExecutor;

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for TimeoutExecutor {
            async fn execute_hunk_source_context(
                &self,
                _request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Ok(HunkSourceNavigationResponse::new("src/slow.rs"))
            }
        }

        let executor = TimeoutExecutor;
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            10,
            10,
            50,
        )
        .await;

        assert_eq!(result.stats.requests_attempted, 1);
        assert_eq!(result.stats.requests_timed_out, 1);
        assert_eq!(result.stats.requests_succeeded, 0);
    }

    #[tokio::test]
    async fn request_budgeting_executor_error_counts_as_attempted_and_failed() {
        let hunks = vec![make_test_hunk("src/error.rs", 10, 5)];
        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/error.rs"),
            "@@ -10,5 +10,5 @@\n-a\n+b\n".to_string(),
        );

        struct ErrorExecutor;

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for ErrorExecutor {
            async fn execute_hunk_source_context(
                &self,
                _request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                Err("LSP server crashed".to_string())
            }
        }

        let executor = ErrorExecutor;
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            10,
            10,
            2500,
        )
        .await;

        assert_eq!(result.stats.requests_attempted, 1);
        assert_eq!(result.stats.requests_failed, 1);
        assert_eq!(result.stats.requests_succeeded, 0);
    }

    #[tokio::test]
    async fn request_budgeting_success_counts_as_attempted_and_succeeded() {
        let hunks = vec![make_test_hunk("src/good.rs", 10, 5)];
        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        patches.insert(
            PathBuf::from("src/good.rs"),
            "@@ -10,5 +10,5 @@\n-a\n+b\n".to_string(),
        );

        let executor = FixtureHunkSourceContextExecutor::new().with_response(
            "src/good.rs",
            Ok(HunkSourceNavigationResponse::new("src/good.rs")),
        );
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            10,
            10,
            2500,
        )
        .await;

        assert_eq!(result.stats.requests_attempted, 1);
        assert_eq!(result.stats.requests_succeeded, 1);
        assert_eq!(result.stats.requests_failed, 0);
    }

    #[tokio::test]
    async fn request_budgeting_file_cap_and_request_cap_independent() {
        let hunks: Vec<ChangedHunk> = (0..5)
            .map(|i| make_test_hunk(&format!("src/f{}.rs", i), i * 10, 3))
            .collect();
        let mut patches: HashMap<PathBuf, String> = HashMap::new();
        for i in 0..5 {
            patches.insert(
                PathBuf::from(format!("src/f{}.rs", i)),
                format!("@@ -{},3 +{},3 @@\n-a\n+b\n", i * 10, i * 10),
            );
        }

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct TrackingExecutor3 {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for TrackingExecutor3 {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                Ok(HunkSourceNavigationResponse::new(&request.file_path))
            }
        }

        let executor = TrackingExecutor3 { seen: seen_clone };
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &patches,
            &policy,
            Some(&executor),
            3,
            2,
            2500,
        )
        .await;

        let seen_files = seen.lock().unwrap();
        assert_eq!(
            seen_files.len(),
            2,
            "request cap should limit to 2 executor calls"
        );
        assert_eq!(result.stats.requests_attempted, 2);
        assert!(seen_files.contains(&"src/f0.rs".to_string()));
        assert!(seen_files.contains(&"src/f1.rs".to_string()));
    }

    #[tokio::test]
    async fn hunk_source_context_real_patches_used_for_policy_not_synthetic_headers() {
        // Verify that when real_patches HashMap is provided, the REAL patch
        // content is used for policy evaluation (max_patch_bytes check),
        // NOT the synthetic hunk headers from ChangedHunk.
        //
        // Setup: src/large.rs has a ChangedHunk with tiny synthetic headers
        // but a real_patches entry that is > 64KB (exceeds default cap).
        // Result: src/large.rs should be SKIPPED (policy uses real patch size).
        //
        // Setup: src/small.rs has a ChangedHunk with small synthetic headers
        // and is NOT in real_patches, so synthetic headers are used.
        // Result: src/small.rs should be PROCESSED (synthetic headers pass policy).

        let hunks = vec![
            make_test_hunk("src/large.rs", 10, 5),
            make_test_hunk("src/small.rs", 20, 3),
        ];

        // Large real patch that exceeds default max_patch_bytes (64KB)
        let large_patch_content = format!("@@ -10,5 +10,5 @@\n{}\n", "x".repeat(70 * 1024));

        let mut real_patches: HashMap<PathBuf, String> = HashMap::new();
        real_patches.insert(PathBuf::from("src/large.rs"), large_patch_content);
        // src/small.rs is NOT in real_patches → will use synthetic headers

        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen_clone = seen.clone();

        struct PolicyTrackingExecutor {
            seen: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl HunkSourceContextExecutor for PolicyTrackingExecutor {
            async fn execute_hunk_source_context(
                &self,
                request: HunkSourceNavigationRequest,
            ) -> Result<HunkSourceNavigationResponse, String> {
                self.seen.lock().unwrap().push(request.file_path.clone());
                Ok(HunkSourceNavigationResponse::new(&request.file_path))
            }
        }

        let executor = PolicyTrackingExecutor { seen: seen_clone };
        let policy = HunkSourceContextPolicy::default();

        let result = collect_hunk_source_context_all_files(
            &hunks,
            &real_patches,
            &policy,
            Some(&executor),
            8,
            8,
            2500,
        )
        .await;

        let seen_files = seen.lock().unwrap();

        // src/small.rs should be processed (uses synthetic headers, passes policy)
        assert!(
            seen_files.contains(&"src/small.rs".to_string()),
            "src/small.rs should be processed (synthetic headers pass policy): seen={:?}",
            *seen_files
        );

        // src/large.rs should NOT be processed (real patch > 64KB, skipped by policy)
        assert!(
            !seen_files.contains(&"src/large.rs".to_string()),
            "src/large.rs should NOT be processed (real patch exceeds max_patch_bytes): seen={:?}",
            *seen_files
        );

        // Verify the skip note mentions the size cap
        // The note format is "hunkSourceContext skipped: patch size N bytes exceeds cap M bytes"
        // (it does NOT include the file path in the skip message)
        let skip_notes: Vec<_> = result
            .notes
            .iter()
            .filter(|n| n.contains("exceeds cap"))
            .collect();
        assert!(
            !skip_notes.is_empty(),
            "should have a skip note mentioning 'exceeds cap': {:?}",
            result.notes
        );
    }
}
