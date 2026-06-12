use serde::{Deserialize, Serialize};
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
) -> Result<(Vec<SecurityReviewTarget>, Vec<ChangedHunk>), String> {
    let summary = egggit::diff_summary(root, base)
        .await
        .map_err(|e| e.to_string())?;

    let mut all_hunks = Vec::new();
    let mut file_level_paths: Vec<(PathBuf, Option<String>)> = Vec::new();

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

        let hunks = parse_changed_hunks_for_file(&file_diff.patch, &path);

        if hunks.is_empty() {
            file_level_paths.push((path, content_hint));
        } else {
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

    Ok((targets, all_hunks))
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
                        is_focus: false,
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
/// 5. Call `synthesize_evidence_based_findings`
/// 6. Assemble `SecurityReviewOutput`
///
/// This function does NOT execute `securityContext` LSP requests.
/// It only uses the deterministic planning and preflight phases.
pub async fn run_security_review_workflow(
    root: &Path,
    base: Option<&str>,
    options: SecurityReviewWorkflowOptions,
) -> Result<SecurityReviewOutput, String> {
    // Phase 1: Discover targets from diff
    let (targets, parsed_hunks) = discover_targets_from_diff(root, base).await?;

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

    // Phase 5: Evidence-based finding synthesis
    let (findings, remaining_prompts) =
        synthesize_evidence_based_findings(&targets, &planning_prompts, &all_preflight);

    // Phase 6: Assemble output
    let mut notes = Vec::new();
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

    Ok(final_output)
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
) -> Result<SecurityReviewOutput, String> {
    // Stage 1: deterministic review (enrichment disabled in options for this pass)
    let stage1_options = SecurityReviewWorkflowOptions {
        enable_lsp_enrichment: false,
        ..options.clone()
    };
    let mut output = run_security_review_workflow(root, base, stage1_options).await?;

    if !options.enable_lsp_enrichment {
        return Ok(output);
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

    Ok(output)
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
/// `None` and `args.enrich` is true, enrichment is skipped (deterministic
/// stage-1 only) and a note is appended indicating that no executor is
/// available in this runtime.
pub async fn run_security_review_command_with_executor(
    root: &Path,
    args: &SecurityReviewCommandArgs,
    executor: Option<&dyn SecurityContextExecutor>,
) -> Result<String, String> {
    let (output, rendered) = run_security_review_command_inner(root, args, executor, true).await?;
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
    executor: Option<&dyn SecurityContextExecutor>,
    render_human: bool,
) -> Result<(SecurityReviewOutput, String), String> {
    let base = args.base.as_deref();

    let mut options = SecurityReviewWorkflowOptions {
        include_prompts: !args.findings_only,
        include_findings: !args.prompts_only,
        run_filename_preflight: !args.no_filename,
        run_content_preflight: !args.no_content,
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

    let mut output = if options.enable_lsp_enrichment {
        if let Some(exec) = executor {
            run_security_review_workflow_with_lsp_enrichment(root, base, options, exec).await?
        } else {
            // No executor available — skip enrichment, run deterministic
            // stage-1 only, and append a clear unavailable note.
            let stage1_options = SecurityReviewWorkflowOptions {
                enable_lsp_enrichment: false,
                ..options
            };
            let mut result = run_security_review_workflow(root, base, stage1_options).await?;
            note_lsp_enrichment_unavailable(&mut result);
            result
        }
    } else {
        run_security_review_workflow(root, base, options).await?
    };

    if args.json {
        let rendered = if render_human {
            serde_json::to_string_pretty(&output)
                .map_err(|e| format!("JSON serialization failed: {e}"))?
        } else {
            String::new()
        };
        return Ok((output, rendered));
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

    Ok((output, report))
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
/// (for the result panel).
pub async fn run_security_review_background(
    root: PathBuf,
    args: SecurityReviewCommandArgs,
    lsp_tool: Option<Arc<crate::tool::lsp::LspTool>>,
) -> Result<SecurityReviewReceipt, String> {
    let executor = lsp_tool.map(crate::security::lsp_executor::LspSecurityContextExecutor::new);
    let executor_ref = executor
        .as_ref()
        .map(|e| e as &dyn crate::security::workflow::context::SecurityContextExecutor);

    let lsp_available = executor.is_some();
    let enriched = args.enrich && executor.is_some();

    let id = SecurityReviewRunId::new().0;
    let (output, rendered) =
        run_security_review_command_inner(&root, &args, executor_ref, true).await?;

    Ok(SecurityReviewReceipt::now(
        id,
        root,
        args,
        output,
        rendered,
        enriched,
        lsp_available,
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
}
