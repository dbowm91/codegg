//! M003 — Operation-scoped Git read adapter for Tool Programs.
//!
//! The multiplexed `git` tool mixes bounded reads (`status`, `diff`,
//! `log`, branch listing) with mutation-capable operations (typed
//! `mutation` actions, `recover`, raw subcommand fallback). Marking it
//! `DirectOrProgrammatic` wholesale would structurally admit mutations
//! to programs, so this hidden `ProgrammaticOnly` adapter exposes only
//! a minimal read subset with an input schema that cannot express a
//! mutation.
//!
//! Canonical delegation: every read executes through
//! [`crate::git_service::GitExecutionService`] (the canonical read
//! executor, itself delegating to `egggit` for structured payloads).
//! This adapter spawns no subprocess, implements no git parsing, and
//! shares no code path with the raw-subcommand fallback beyond what
//! the service already owns.
//!
//! Workspace authority: the broker-supplied
//! [`crate::tool::ToolExecutionContext::cwd`] (program workspace root
//! installed by `BrokerAdapter::with_cwd`) is the repository root.
//! There is no `workdir` input field, so a program cannot nominate an
//! out-of-workspace repository. Calls without an execution context
//! fail closed.
//!
//! State semantics: git state races with worktree mutations, so the
//! contract declares cache DISABLED and documents that a fresh rerun
//! may observe different results. Ledger replay still serves the
//! recorded execution-time result (`Idempotent` in the replay sense,
//! exactly like the M002 `repo_search` seam).

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::ToolError;
use crate::git_service::GitExecutionService;
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::{Tool, ToolCategory};

/// Maximum display bytes returned to a program before truncation.
/// Keeps program call results bounded; the broker artifact boundary
/// applies above this.
pub const MAX_GIT_READ_DISPLAY_BYTES: usize = 64 * 1024;

/// Default and maximum `log` entry counts.
pub const DEFAULT_LOG_COUNT: u32 = 20;
pub const MAX_LOG_COUNT: u32 = 50;

/// Maximum accepted `base_ref` length for `diff`.
pub const MAX_BASE_REF_LEN: usize = 128;

/// Allowed operations. Deliberately minimal: status, diff, log, and
/// local branch listing. Everything else on the multiplexed `git`
/// surface — `show`, `blame`, tags, remotes, worktrees, stashes,
/// `rev-parse`, `for-each-ref`, all typed `mutation` actions,
/// `recover`, and `operation_state` — is structurally unavailable
/// (no schema field can name it).
pub const ALLOWED_OPERATIONS: &[&str] = &["status", "diff", "log", "branches"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitReadInput {
    operation: String,
    #[serde(default)]
    base_ref: Option<String>,
    #[serde(default)]
    max_count: Option<u32>,
}

pub struct GitReadTool {
    workdir: PathBuf,
    timeout: Duration,
}

impl GitReadTool {
    pub fn new() -> Self {
        Self {
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            timeout: Duration::from_secs(30),
        }
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn contract_output_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "description": "Bounded git read result. Workspace-version dependent: a fresh rerun may observe different repository state; ledger replay serves the recorded execution-time result.",
            "properties": {
                "operation": {"type": "string"},
                "truncated": {"type": "boolean"},
                "results": {}
            },
            "required": ["operation", "truncated", "results"]
        })
    }

    /// Effective repository root: the broker execution-context cwd
    /// (program workspace root) when it names a directory, else the
    /// registry-installed tool root. A program cannot supply its own
    /// root — there is no `workdir` input.
    fn effective_root(&self, ctx: Option<&ToolExecutionContext>) -> Result<PathBuf, ToolError> {
        if let Some(context) = ctx {
            if context.cwd.is_dir() {
                return Ok(context.cwd.clone());
            }
        }
        if self.workdir.is_dir() {
            return Ok(self.workdir.clone());
        }
        Err(ToolError::Execution(
            "git_read requires a broker execution context with a workspace directory".to_string(),
        ))
    }

    /// Validate an optional diff `base_ref` without shell semantics.
    /// The argv path below never invokes a shell (`Command::args`),
    /// but a leading-dash ref could still be parsed as a flag, so
    /// refs must start alphanumeric and stay within a conservative
    /// revision-expression alphabet.
    fn validate_base_ref(raw: &str) -> Result<(), ToolError> {
        if raw.is_empty() {
            return Err(ToolError::Execution(
                "base_ref must not be empty".to_string(),
            ));
        }
        if raw.len() > MAX_BASE_REF_LEN {
            return Err(ToolError::Execution(format!(
                "base_ref exceeds maximum length of {MAX_BASE_REF_LEN}"
            )));
        }
        let mut chars = raw.chars();
        let first = chars.next().unwrap_or('\0');
        if !first.is_ascii_alphanumeric() {
            return Err(ToolError::Execution(
                "base_ref must start with an alphanumeric character".to_string(),
            ));
        }
        for c in raw.chars() {
            if !(c.is_ascii_alphanumeric() || "._/-~^:@{}".contains(c)) {
                return Err(ToolError::Execution(format!(
                    "base_ref contains unsupported character '{c}'"
                )));
            }
        }
        if raw.contains('\0') || raw.contains('\n') {
            return Err(ToolError::Execution(
                "base_ref contains illegal characters".to_string(),
            ));
        }
        Ok(())
    }

    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<(String, serde_json::Value, bool), ToolError> {
        // Strict parse: unknown fields (e.g. `mutation`, `recover`,
        // `subcommand`, `args`, `workdir`) fail closed here.
        let parsed: GitReadInput = serde_json::from_value(input)
            .map_err(|e| ToolError::Execution(format!("invalid git_read input: {e}")))?;
        let root = self.effective_root(ctx)?;

        let operation = match parsed.operation.as_str() {
            "status" => codegg_git::GitOperation::Status { short: false },
            "diff" => {
                let base_ref =
                    match parsed.base_ref.as_deref() {
                        None => None,
                        Some(raw) => {
                            Self::validate_base_ref(raw)?;
                            Some(codegg_git::ref_name::RevisionExpr::new(raw).map_err(|e| {
                                ToolError::Execution(format!("invalid base_ref: {e}"))
                            })?)
                        }
                    };
                codegg_git::GitOperation::Diff {
                    staged: false,
                    stat: false,
                    name_only: false,
                    base_ref,
                    paths: vec![],
                }
            }
            "log" => {
                let count = parsed
                    .max_count
                    .unwrap_or(DEFAULT_LOG_COUNT)
                    .clamp(1, MAX_LOG_COUNT);
                codegg_git::GitOperation::Log {
                    oneline: true,
                    max_count: Some(count),
                    paths: vec![],
                }
            }
            "branches" => codegg_git::GitOperation::BranchList {
                remotes: false,
                all: false,
            },
            other => {
                return Err(ToolError::Execution(format!(
                    "unsupported git_read operation '{other}'; allowed: status, diff, log, branches"
                )));
            }
        };

        let service = GitExecutionService::new().with_timeout(self.timeout);
        let result = service.execute(&operation, &root).await.map_err(|e| {
            ToolError::Execution(format!("git_read {} failed: {e}", parsed.operation))
        })?;
        if !result.success {
            return Err(ToolError::Execution(format!(
                "git_read {} failed (exit {}): {}",
                parsed.operation,
                result.exit_code,
                result.stderr.trim()
            )));
        }

        // Canonical display is the service stdout; structured results
        // are the unwrapped typed payload (the `GitPayload` enum is
        // externally tagged, so serialize the inner value to keep the
        // `results` shape stable per operation). Both are bounded below.
        let payload_value = match result.payload.as_ref() {
            Some(crate::git_service::GitPayload::Status(s)) => json_or_empty(s),
            Some(crate::git_service::GitPayload::DiffSummary(s)) => json_or_empty(s),
            Some(crate::git_service::GitPayload::DiffText(t)) => {
                serde_json::json!({"diff_text": t})
            }
            Some(crate::git_service::GitPayload::DiffResult(r)) => json_or_empty(r),
            Some(crate::git_service::GitPayload::Show(s)) => json_or_empty(s),
            Some(crate::git_service::GitPayload::ChangedFiles(f)) => {
                serde_json::to_value(f).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::Log(c)) => {
                serde_json::to_value(c).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::Branches(b)) => {
                serde_json::to_value(b).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::Tags(t)) => {
                serde_json::to_value(t).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::Remotes(r)) => {
                serde_json::to_value(r).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::Worktrees(w)) => {
                serde_json::to_value(w).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::Stashes(s)) => {
                serde_json::to_value(s).unwrap_or(serde_json::json!([]))
            }
            Some(crate::git_service::GitPayload::None) | None => serde_json::json!({}),
        };
        let (display, truncated) = truncate_display(result.stdout.clone());
        let value = serde_json::json!({
            "operation": parsed.operation,
            "truncated": truncated,
            "results": payload_value,
        });
        Ok((display, value, truncated))
    }
}

fn json_or_empty<T: serde::Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::json!({}))
}

fn truncate_display(text: String) -> (String, bool) {
    if text.len() <= MAX_GIT_READ_DISPLAY_BYTES {
        return (text, false);
    }
    let marker = format!(
        "\n... [git_read truncated at {} bytes; narrow the query to see more]\n",
        MAX_GIT_READ_DISPLAY_BYTES
    );
    let budget = MAX_GIT_READ_DISPLAY_BYTES.saturating_sub(marker.len());
    let mut end = budget.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_string();
    out.push_str(&marker);
    (out, true)
}

#[async_trait]
impl Tool for GitReadTool {
    fn name(&self) -> &str {
        "git_read"
    }

    fn description(&self) -> &str {
        "Program-only bounded git reads (status, diff, log, branches) delegating to the canonical git execution service. Hidden from ordinary model turns."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["status", "diff", "log", "branches"],
                    "description": "Bounded git read. status: branch/dirty/HEAD; diff: unstaged unified diff (optionally against base_ref); log: recent oneline commits; branches: local branch list."
                },
                "base_ref": {
                    "type": "string",
                    "description": "Optional base revision for diff (e.g. HEAD~1). Alphanumeric start, max 128 chars."
                },
                "max_count": {
                    "type": "number",
                    "description": "Maximum log entries (default 20, max 50)."
                }
            },
            "required": ["operation"],
            "additionalProperties": false
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    /// Hidden program-only adapter: never advertised to the model.
    fn expose_in_definitions(&self) -> bool {
        false
    }

    /// M003 programmatic read contract.
    ///
    /// Admission rationale (M001 matrix + M003 operation scoping):
    /// explicit `ProgrammaticOnly` caller policy (the multiplexed
    /// `git` tool stays `DirectOnly`); read-side effect (`ReadOnly`);
    /// declared output schema; bounded I/O (log count clamp,
    /// base_ref alphabet/length bound, 64 KiB display cap, broker
    /// artifact boundary above); execution-context workspace root
    /// (no `workdir` input); explicitly workspace-version-dependent
    /// `LocalTrusted` semantics with cache DISABLED so reruns
    /// re-observe repository state while ledger replay serves the
    /// recorded result; no retry; no subprocess/shell of its own.
    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            caller_policy: ToolCallerPolicy::ProgrammaticOnly,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            cache_policy: ToolCachePolicy {
                enabled: false,
                ttl_secs: 0,
                max_entries: 0,
            },
            output_schema: Some(Self::contract_output_schema()),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        // Context-free direct calls have no workspace authority and
        // always fail closed; the broker structured path supplies ctx.
        Ok(self.execute_with_context(input, None).await?.0)
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let start = std::time::Instant::now();
        let (display, value, truncated) = self.execute_with_context(input, ctx.as_ref()).await?;
        let provenance = ToolProvenance {
            backend: "native".to_string(),
            implementation: "codegg/git_read".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            elapsed_ms: Some(start.elapsed().as_millis() as u64),
            truncated,
            trust: ToolTrust::LocalTrusted,
        };
        Ok(StructuredToolResult::with_value(
            display,
            value,
            true,
            Some(provenance),
        ))
    }
}

impl Default for GitReadTool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::contract::ToolCallerPolicy;

    #[test]
    fn git_read_contract_is_programmatic_only_read_with_schema() {
        let tool = GitReadTool::new();
        let contract = tool.contract("git_read", tool.parameters());
        assert_eq!(contract.caller_policy, ToolCallerPolicy::ProgrammaticOnly);
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert_eq!(contract.idempotency, IdempotencyClass::Idempotent);
        assert!(
            !contract.cache_policy.enabled,
            "git_read program-call cache must be disabled (workspace-version dependent)"
        );
        assert_eq!(contract.retry_policy.max_retries, 0);
        assert!(contract.validate().is_ok());
        let schema = contract.output_schema.expect("git_read output schema");
        for key in ["operation", "truncated", "results"] {
            assert!(schema["properties"].get(key).is_some(), "missing {key}");
        }
    }

    #[test]
    fn git_read_is_hidden_from_definitions() {
        assert!(!GitReadTool::new().expose_in_definitions());
        assert_eq!(
            crate::tool::disclosure::disclosure_for("git_read"),
            crate::tool::disclosure::ToolDisclosure::Hidden
        );
    }

    #[test]
    fn base_ref_validation_rejects_flags_and_shell_chars() {
        assert!(GitReadTool::validate_base_ref("HEAD~1").is_ok());
        assert!(GitReadTool::validate_base_ref("main").is_ok());
        assert!(GitReadTool::validate_base_ref("-p").is_err());
        assert!(GitReadTool::validate_base_ref("--upload-pack=x").is_err());
        assert!(GitReadTool::validate_base_ref("HEAD; rm -rf /").is_err());
        assert!(GitReadTool::validate_base_ref("a$(id)").is_err());
        assert!(GitReadTool::validate_base_ref("").is_err());
        assert!(GitReadTool::validate_base_ref(&"a".repeat(129)).is_err());
    }

    #[test]
    fn unknown_fields_fail_closed_at_parse() {
        // `mutation`, `recover`, `subcommand`, `args`, and `workdir`
        // cannot be expressed: strict parsing rejects them before any
        // backend is touched.
        for extra in [
            "mutation",
            "recover",
            "subcommand",
            "args",
            "workdir",
            "operation_state",
        ] {
            let mut input = serde_json::json!({"operation": "status"});
            input[extra] = serde_json::json!("commit");
            let parsed: Result<GitReadInput, _> = serde_json::from_value(input);
            assert!(parsed.is_err(), "field '{extra}' must be rejected");
        }
    }
}
