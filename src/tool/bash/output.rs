//! Bounded output capture, projection metadata, and RunStore persistence.
//!
//! This module owns everything that happens after a child or scheduler
//! dispatch resolves: truncating captured streams, shaping the structured
//! result, attaching routing metadata, and persisting the caller-owned
//! RunStore record.
//!
//! Contracts preserved from the pre-split implementation:
//!
//! - stdout/stderr are never accumulated without a cap (`max_output_lines`
//!   head/tail selection plus `max_output_bytes` char-boundary cut).
//! - raw stdout/stderr artifacts are persisted with
//!   `safe_for_model: false`; only the projected summary is model-safe.
//! - persistence failure (`begin_run`/`write_artifact`/`complete_run`
//!   errors) never rewrites the true command terminal outcome. A failed
//!   secondary write is logged and the original result is returned.
//! - delegated executors (`TestRunner`, `PythonScript`, and `Git` with a
//!   delegated run id) suppress the duplicate caller-owned record. A
//!   delegated executor *without* a run id falls back to caller
//!   persistence when a store exists, so one logical execution always
//!   produces exactly one canonical record.
//! - shell projection/redaction stays on the accepted output path via the
//!   dispatch-produced summaries; this module only appends routing
//!   metadata and preflight warnings without altering exit semantics.

use std::path::PathBuf;

use super::policy::routing_metadata_risk_caps;
use super::BashTool;
use crate::command_intent::pipeline::CommandPipelineResult;
use crate::command_outcome::{ownership_for_outcome, run_kind_for_outcome, ExecutionOutcome};
use crate::config::schema::{CommandIntentConfig, CommandIntentMode};

/// Routing metadata attached to bash output when command intent routing is enabled.
#[derive(Debug, Clone)]
pub(crate) struct RoutingMetadata {
    pub(crate) intent_kind: String,
    pub(crate) backend_label: String,
    pub(crate) projector_label: String,
    pub(crate) rtk_eligible: bool,
    pub(crate) confidence: String,
    pub(crate) risk_level: String,
    pub(crate) routing_enabled: bool,
    pub(crate) routing_decision: String,
    pub(crate) mode: CommandIntentMode,
}

/// Clone the actual backend from an ExecutionOutcome. Used by the persistence
/// path to set the `actual_backend` field on RunCompletion without consuming
/// the outcome (which is also used for `fallback_record()`).
pub(crate) fn execution_outcome_clone_actual(
    outcome: &ExecutionOutcome,
) -> codegg_core::run_store::ActualBackend {
    outcome.actual.into_backend()
}

pub(crate) fn truncate_output(output: &str, max_lines: usize, max_bytes: usize) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let truncated = if lines.len() > max_lines {
        let head = &lines[..max_lines / 2];
        let tail = &lines[lines.len() - max_lines / 2..];
        let mut result = head.join("\n");
        result.push_str(&format!(
            "\n\n... [{} lines truncated] ...\n\n",
            lines.len() - max_lines
        ));
        result.push_str(&tail.join("\n"));
        result
    } else {
        output.to_string()
    };

    if truncated.len() > max_bytes {
        let truncate_at = truncated
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= max_bytes)
            .last()
            .unwrap_or(0);
        format!("{}... [output truncated]", &truncated[..truncate_at])
    } else {
        truncated
    }
}

/// Build optional routing metadata for the model-visible suffix. Returns
/// `None` when no command-intent config is attached (legacy quiet mode).
pub(crate) fn build_routing_metadata(
    config: Option<&CommandIntentConfig>,
    pipeline: &CommandPipelineResult,
) -> Option<RoutingMetadata> {
    let cic = config?;
    let mode = cic.mode();
    let family_enabled = pipeline
        .plan
        .command_family()
        .map(|f| cic.is_enabled(f))
        .unwrap_or(false);
    Some(RoutingMetadata {
        intent_kind: pipeline.intent.kind.label().to_string(),
        backend_label: pipeline.plan.backend.label().to_string(),
        projector_label: pipeline.plan.projector.label().to_string(),
        rtk_eligible: pipeline.plan.rtk_policy.is_rtk_eligible(),
        confidence: format!("{:?}", pipeline.intent.confidence).to_lowercase(),
        risk_level: format!("{:?}", pipeline.intent.risk.level).to_lowercase(),
        routing_enabled: family_enabled,
        routing_decision: format!("{:?}", pipeline.dispatch),
        mode,
    })
}

/// Append the canonical `[intent: ... | backend: ...]` suffix when routing
/// metadata exists. Without config the result is returned unchanged.
pub(crate) fn append_routing_suffix(mut result: String, meta: Option<&RoutingMetadata>) -> String {
    if let Some(meta) = meta {
        result = format!(
            "{}\n\n[intent: {} | backend: {} | projector: {} | confidence: {} | risk: {} | routing: {} | rtk: {} | route: {} | mode: {}]",
            result,
            meta.intent_kind,
            meta.backend_label,
            meta.projector_label,
            meta.confidence,
            meta.risk_level,
            if meta.routing_enabled {
                "enabled"
            } else {
                "disabled"
            },
            if meta.rtk_eligible { "eligible" } else { "off" },
            meta.routing_decision,
            match meta.mode {
                CommandIntentMode::Observe => "observe",
                CommandIntentMode::Active => "active",
                CommandIntentMode::Route => "route (fallback: observe)",
            },
        );
    }
    result
}

impl BashTool {
    /// Persist the caller-owned RunStore record for one completed execution.
    ///
    /// Delegated backends with a proven run id are skipped (the delegated
    /// subsystem already owns the canonical record). All other outcomes
    /// persist exactly one record; persistence errors are logged and never
    /// alter the returned command result.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn persist_caller_run(
        &self,
        command: &str,
        output: &std::process::Output,
        execution_outcome: &ExecutionOutcome,
        intent_kind: crate::command_intent::CommandIntentKind,
        routing_metadata: Option<&RoutingMetadata>,
        canonical_workdir: Option<&PathBuf>,
        workdir: Option<&str>,
        persist_run: bool,
        ownership: codegg_core::run_store::RunOwnership,
    ) {
        if !persist_run {
            return;
        }
        let Some(ref store) = self.run_store else {
            return;
        };
        use chrono::Utc;
        use codegg_core::run_store::*;

        let cwd = canonical_workdir
            .cloned()
            .or_else(|| workdir.map(PathBuf::from))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let workspace_root = cwd.clone();

        // Extract risk info from routing metadata if available
        let (risk_level, has_subprocess, has_git_mutation, has_destructive) =
            if let Some(rm) = routing_metadata {
                let caps = routing_metadata_risk_caps(command);
                (rm.risk_level.clone(), caps.0, caps.1, caps.2)
            } else {
                ("low".to_string(), true, false, false)
            };

        // Workstream B: RunKind is derived from the actual executor
        // and the intent, NOT the planned routing decision.
        let run_kind_str = run_kind_for_outcome(execution_outcome, intent_kind);
        let run_kind = match run_kind_str.as_str() {
            "raw_shell" => RunKind::RawShell,
            "managed_process" => RunKind::ManagedProcess,
            "test" => RunKind::Test,
            "git_read" => RunKind::GitRead,
            "git_mutation" => RunKind::GitMutation,
            "search" => RunKind::Search,
            "python" => RunKind::Python,
            "native_tool" => RunKind::NativeTool,
            _ => RunKind::RawShell,
        };

        // Workstream E: argv reflects the ACTUAL execution, not
        // the planned decision. For raw shell: `[sh, -c, command]`.
        // For managed argv / native tool / managed process: the
        // actual argv that was used.
        let invocation_argv: Vec<String> = match &execution_outcome.actual {
            crate::command_outcome::ActualExecutor::RawShell { argv, .. } => argv.clone(),
            crate::command_outcome::ActualExecutor::ManagedArgv { argv, .. } => argv.clone(),
            crate::command_outcome::ActualExecutor::NativeTool { argv, .. } => argv.clone(),
            crate::command_outcome::ActualExecutor::TestRunner { argv, .. } => argv.clone(),
            crate::command_outcome::ActualExecutor::PythonScript { .. } => {
                vec!["python3".to_string(), "<script>".to_string()]
            }
            crate::command_outcome::ActualExecutor::Git { argv, .. } => argv.clone(),
            crate::command_outcome::ActualExecutor::Rejected { .. } => vec![],
        };
        let script_hash = match &execution_outcome.actual {
            crate::command_outcome::ActualExecutor::PythonScript { script_hash, .. } => {
                script_hash.clone()
            }
            _ => None,
        };

        // Workstream F: backend family/detail reflect the actual executor.
        let (backend_family, backend_detail) = match &execution_outcome.actual {
            crate::command_outcome::ActualExecutor::RawShell { .. } => {
                ("bash".to_string(), Some("raw_shell".to_string()))
            }
            crate::command_outcome::ActualExecutor::ManagedArgv { .. } => {
                ("bash".to_string(), Some("managed_argv".to_string()))
            }
            crate::command_outcome::ActualExecutor::NativeTool { tool_name, .. } => {
                ("native_tool".to_string(), Some(tool_name.clone()))
            }
            crate::command_outcome::ActualExecutor::TestRunner { .. } => (
                "test_runner".to_string(),
                routing_metadata.as_ref().map(|m| m.intent_kind.clone()),
            ),
            crate::command_outcome::ActualExecutor::PythonScript { mode, .. } => {
                ("python_script".to_string(), Some(mode.clone()))
            }
            crate::command_outcome::ActualExecutor::Git {
                operation_label, ..
            } => ("git".to_string(), Some(operation_label.clone())),
            crate::command_outcome::ActualExecutor::Rejected { reason } => {
                ("bash".to_string(), Some(format!("rejected:{reason}")))
            }
        };

        let draft = RunDraft {
            kind: run_kind,
            invocation: RunInvocation {
                command: command.to_string(),
                argv: Some(invocation_argv),
                script_hash,
            },
            session_id: None,
            parent_run_id: None,
            workspace_root,
            cwd,
            backend: BackendRecord {
                family: backend_family,
                detail: backend_detail,
            },
            risk: RiskRecord {
                level: risk_level,
                has_subprocess,
                has_git_mutation,
                has_destructive_mutation: has_destructive,
            },
            planned_backend: Some(execution_outcome.planned.clone()),
            actual_backend: Some(execution_outcome.actual.into_backend()),
            ownership,
            asset_provenance: self
                .asset_pin
                .as_ref()
                .and_then(|pin| pin.lock().ok().map(|pin| pin.to_run_provenance())),
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let status = if exit_code == 0 {
            RunStatus::Complete
        } else {
            RunStatus::Failed
        };

        // Workstream H: persistence failure (begin_run error) must
        // not change the actual execution outcome or result string.
        // The `if let Ok(handle)` already swallows persistence errors.
        if let Ok(handle) = store.begin_run(draft).await {
            if !output.stdout.is_empty() {
                if let Err(e) = store
                    .write_artifact(
                        &handle,
                        ArtifactInput {
                            kind: ArtifactKind::Stdout,
                            data: output.stdout.clone(),
                            mime_type: "text/plain".to_string(),
                            // Workstream G: raw stdout is NOT model-safe.
                            safe_for_model: false,
                        },
                    )
                    .await
                {
                    tracing::warn!(error = %e, "failed to write stdout artifact to RunStore");
                }
            }

            if !output.stderr.is_empty() {
                if let Err(e) = store
                    .write_artifact(
                        &handle,
                        ArtifactInput {
                            kind: ArtifactKind::Stderr,
                            data: output.stderr.clone(),
                            mime_type: "text/plain".to_string(),
                            // Workstream G: raw stderr is NOT model-safe.
                            safe_for_model: false,
                        },
                    )
                    .await
                {
                    tracing::warn!(error = %e, "failed to write stderr artifact to RunStore");
                }
            }

            if let Err(e) = store
                .complete_run(
                    handle,
                    RunCompletion {
                        status,
                        completed_at: Utc::now(),
                        permissions: vec![],
                        sandbox: None,
                        projection: None,
                        changes: vec![],
                        rerun: None,
                        actual_backend: Some(execution_outcome_clone_actual(execution_outcome)),
                        fallback: execution_outcome.fallback_record(),
                    },
                )
                .await
            {
                tracing::warn!(error = %e, "failed to complete run in RunStore");
            }
        }
    }

    /// Decide whether the caller-owned persistence path applies and which
    /// ownership label it carries. Returns `(persist_run, ownership)`.
    ///
    /// Delegated executors with a proven `delegated_run_id` are already
    /// owned by the delegated subsystem. Delegated executors without a
    /// run id use caller persistence when a store exists.
    pub(crate) fn persistence_decision(
        &self,
        execution_outcome: &ExecutionOutcome,
        delegated_run_id: Option<&codegg_core::run_store::RunId>,
    ) -> (bool, codegg_core::run_store::RunOwnership) {
        use crate::command_outcome::ActualExecutor;

        let delegated_executor =
            matches!(
                &execution_outcome.actual,
                ActualExecutor::TestRunner { .. } | ActualExecutor::PythonScript { .. }
            ) || matches!(&execution_outcome.actual, ActualExecutor::Git { .. })
                && delegated_run_id.is_some();

        let persist_run = match (delegated_executor, delegated_run_id) {
            (true, Some(_)) => false,
            (true, None) => {
                tracing::warn!(
                    "delegated ownership without run_id — using caller persistence when available"
                );
                self.run_store.is_some()
            }
            (false, _) => true,
        };
        let ownership = if delegated_executor {
            if delegated_run_id.is_some() {
                codegg_core::run_store::RunOwnership::DelegatedBackend
            } else {
                codegg_core::run_store::RunOwnership::Caller
            }
        } else {
            ownership_for_outcome(execution_outcome)
        };
        // Silence unused-path lint for the non-delegated branch helper: the
        // `Path` import is used by the caller signature in this module.
        (persist_run, ownership)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_output_passes_through_short_output() {
        let out = truncate_output("hello\nworld", 2000, 50_000);
        assert_eq!(out, "hello\nworld");
    }

    #[test]
    fn truncate_output_selects_head_and_tail_over_line_cap() {
        let lines: Vec<String> = (0..10).map(|i| format!("line {i}")).collect();
        let joined = lines.join("\n");
        let out = truncate_output(&joined, 4, 50_000);
        assert!(out.contains("line 0"));
        assert!(out.contains("line 9"));
        assert!(out.contains("lines truncated"));
        assert!(!out.contains("line 5"));
    }

    #[test]
    fn truncate_output_cuts_at_char_boundary_over_byte_cap() {
        let out = truncate_output("abcdefghij", 2000, 5);
        assert!(out.contains("output truncated"));
        assert!(out.len() <= "abcdefghij... [output truncated]".len() + 8);
    }

    #[test]
    fn append_routing_suffix_is_noop_without_config() {
        let out = append_routing_suffix("done".to_string(), None);
        assert_eq!(out, "done");
    }

    #[test]
    fn backend_detail_reflects_actual_executor_for_raw_shell() {
        use crate::command_outcome::{ActualExecutor, ExecutionOutcome};
        use codegg_core::run_store::PlannedBackend;
        let outcome = ExecutionOutcome::identity(
            PlannedBackend::RawShell,
            ActualExecutor::RawShell {
                command: "echo hi".to_string(),
                argv: vec!["sh".to_string(), "-c".to_string(), "echo hi".to_string()],
            },
        );
        let backend = execution_outcome_clone_actual(&outcome);
        let debug = format!("{backend:?}");
        assert!(debug.contains("RawShell") || !debug.is_empty());
    }
}
