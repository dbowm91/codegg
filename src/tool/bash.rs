//! Model-facing `bash` tool facade.
//!
//! `BashTool` remains the single model-facing shell tool. This file owns
//! only configuration/builders and the high-level execution sequence:
//!
//! ```text
//! parse/classify
//!  -> authorize/path/child policy
//!  -> route to scheduler or local supervised process
//!  -> collect bounded result
//!  -> project/redact/persist
//!  -> return structured tool result
//! ```
//!
//! Responsibility owners:
//!
//! - policy/classification glue: [`policy`] (blocked patterns, allowlist,
//!   child-workspace ceiling, kill switches, intent-family adapters).
//! - supervised execution: [`process`] (raw-shell spawn, native/managed
//!   dispatch, scheduler submission, [`process::DispatchOutcome`]).
//! - output/result handling: [`output`] (truncation, routing metadata,
//!   RunStore persistence, result shaping).
//!
//! Existing canonical owners are invoked, never duplicated:
//! `tool::destructive`, scheduler services, shell/projector helpers,
//! sandbox modules, and command-intent services.

pub mod output;
pub mod policy;
pub mod process;

pub(crate) use policy::validate_child_workspace_command;
pub use process::DispatchOutcome;

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::command_intent::pipeline::prepare_command;
use crate::command_intent::CommandIntentContext;
use crate::command_outcome::{ActualExecutor, ExecutionOutcome};
use crate::config::schema::CommandIntentConfig;
use crate::error::ToolError;
use crate::preflight::{PreflightDecision, PreflightService};
use crate::security::sandbox::{get_default_allowed_paths, get_sensitive_paths, SandboxConfig};
use crate::tool::{Tool, ToolCategory};

pub struct BashTool {
    pub(crate) timeout: Duration,
    pub(crate) max_output_lines: usize,
    pub(crate) max_output_bytes: usize,
    pub(crate) blocked_commands: HashSet<&'static str>,
    pub(crate) allowed_paths: Option<Vec<String>>,
    pub(crate) deny_all: bool,
    pub(crate) allowlist: Option<HashSet<&'static str>>,
    pub(crate) landlock_sandbox: Option<SandboxConfig>,
    pub(crate) preflight: Option<Arc<PreflightService>>,
    pub(crate) command_intent_config: Option<CommandIntentConfig>,
    pub(crate) run_store: Option<Arc<dyn codegg_core::run_store::RunStore>>,
    pub(crate) submission: Option<Arc<crate::scheduler::JobSubmissionService>>,
    pub(crate) asset_pin:
        Option<Arc<std::sync::Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>>,
    pub(crate) routing_disabled_override: Option<bool>,
    pub(crate) workspace_root: Option<PathBuf>,
}

impl BashTool {
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            max_output_lines: 2000,
            max_output_bytes: 50_000,
            blocked_commands: policy::default_blocked_commands(),
            allowed_paths: None,
            deny_all: false,
            allowlist: None,
            landlock_sandbox: None,
            preflight: None,
            command_intent_config: None,
            run_store: None,
            submission: None,
            asset_pin: None,
            routing_disabled_override: None,
            workspace_root: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_blocked_commands(mut self, commands: Vec<&'static str>) -> Self {
        self.blocked_commands = commands.into_iter().collect();
        self
    }

    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = Some(paths);
        self
    }

    pub fn with_workspace_root(mut self, root: PathBuf) -> Self {
        self.workspace_root = Some(root);
        self
    }

    pub fn with_deny_all(mut self) -> Self {
        self.deny_all = true;
        self
    }

    pub fn with_allowlist(mut self, commands: Vec<&'static str>) -> Self {
        self.allowlist = Some(commands.into_iter().collect());
        self
    }

    pub fn with_run_store(mut self, store: Arc<dyn codegg_core::run_store::RunStore>) -> Self {
        self.run_store = Some(store);
        self
    }

    pub fn with_submission(
        mut self,
        submission: Arc<crate::scheduler::JobSubmissionService>,
    ) -> Self {
        self.submission = Some(submission);
        self
    }

    pub fn with_asset_pin(
        mut self,
        asset_pin: Arc<std::sync::Mutex<crate::agent::asset_snapshot::RuntimeAssetPin>>,
    ) -> Self {
        self.asset_pin = Some(asset_pin);
        self
    }

    pub fn with_landlock_sandbox(mut self, enabled: bool) -> Self {
        if enabled {
            let mut config = SandboxConfig::new();
            config.enabled = true;
            config.allowed_paths = get_default_allowed_paths();
            config.deny_paths = get_sensitive_paths();
            self.landlock_sandbox = Some(config);
        }
        self
    }

    pub fn with_landlock_sandbox_custom(mut self, config: SandboxConfig) -> Self {
        self.landlock_sandbox = Some(config);
        self
    }

    pub fn with_preflight(mut self, service: PreflightService) -> Self {
        self.preflight = Some(Arc::new(service));
        self
    }

    pub fn with_command_intent_config(mut self, config: CommandIntentConfig) -> Self {
        self.command_intent_config = Some(config);
        self
    }

    pub fn with_routing_disabled_env(mut self, disabled: bool) -> Self {
        self.routing_disabled_override = Some(disabled);
        self
    }

    pub fn with_sandbox_mode(mut self, mode: crate::security::sandbox::SandboxMode) -> Self {
        if let Some(ref mut config) = self.landlock_sandbox {
            config.mode = mode;
        } else {
            let mut config = SandboxConfig::new();
            config.enabled = true;
            config.mode = mode;
            config.allowed_paths = crate::security::sandbox::get_default_allowed_paths();
            config.deny_paths = crate::security::sandbox::get_sensitive_paths();
            self.landlock_sandbox = Some(config);
        }
        self
    }
}

impl Default for BashTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return its output. For web search and URL fetching, prefer the `websearch` and `webfetch` tools — they handle rate limits, SSRF protection, and bot detection. `curl`/`wget` to arbitrary URLs is permitted but discouraged; use them only when a tool is genuinely unsuitable."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for command execution"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ShellExec
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        use crate::command_intent::pipeline::CommandPipelineResult;
        use policy::{plan_family, plan_to_planned_backend, RoutingMetric};

        let command = input["command"]
            .as_str()
            .ok_or_else(|| ToolError::Execution("missing 'command' parameter".to_string()))?;

        // 1. parse/classify gate: length and deterministic pre-spawn policy.
        if self.deny_all {
            return Err(ToolError::Permission("bash tool is disabled".to_string()));
        }

        if command.len() > policy::MAX_COMMAND_LENGTH {
            return Err(ToolError::Execution(format!(
                "command exceeds maximum length of {} bytes",
                policy::MAX_COMMAND_LENGTH
            )));
        }

        let parts: Vec<&str> = command.split_whitespace().collect();
        self.check_command_security(command, &parts)?;

        let preflight_warning = if let Some(ref svc) = self.preflight {
            match svc.check_command(command).await {
                PreflightDecision::Block { findings } => {
                    return Err(ToolError::Execution(format!(
                        "preflight blocked command: {}",
                        PreflightDecision::Block { findings }.summary()
                    )));
                }
                PreflightDecision::Warn { findings } => {
                    let warning = PreflightDecision::Warn { findings }.summary();
                    tracing::warn!(target: "preflight", "{}", warning);
                    Some(warning)
                }
                _ => None,
            }
        } else {
            None
        };

        // 2. authorize/path/child policy (all fail-closed, all pre-spawn).
        if let Some(root) = self.workspace_root.as_ref() {
            policy::validate_child_workspace_command(command, &[], root)?;
        }

        let timeout_secs = input["timeout"].as_u64().unwrap_or(120);
        let execution_timeout = Duration::from_secs(timeout_secs.max(1));

        let workdir = input["workdir"].as_str().map(str::to_string).or_else(|| {
            self.workspace_root
                .as_ref()
                .map(|root| root.to_string_lossy().into_owned())
        });

        // Resolve canonical working directory
        let mut canonical_workdir: Option<PathBuf> = None;

        if let Some(ref paths) = self.allowed_paths {
            if let Some(ref dir) = workdir {
                let workdir = dir.clone();
                let paths = paths.clone();
                let (allowed, canonical) = tokio::task::spawn_blocking(move || {
                    let canonical_dir = std::fs::canonicalize(&workdir).map_err(|_| {
                        ToolError::Permission(format!(
                            "working directory '{workdir}' could not be resolved"
                        ))
                    })?;

                    let mut allowed = false;
                    for path in &paths {
                        let canonical_path = std::fs::canonicalize(path).map_err(|_| {
                            ToolError::Permission(format!(
                                "allowed path '{path}' could not be resolved"
                            ))
                        })?;
                        if canonical_dir.starts_with(&canonical_path) {
                            allowed = true;
                            break;
                        }
                    }
                    Ok::<_, ToolError>((allowed, canonical_dir))
                })
                .await
                .map_err(|e| ToolError::Execution(format!("spawn_blocking failed: {}", e)))??;

                if allowed {
                    canonical_workdir = Some(canonical);
                } else {
                    return Err(ToolError::Permission(format!(
                        "working directory '{dir}' is not in allowed paths"
                    )));
                }
            } else if !paths.is_empty() {
                return Err(ToolError::Permission(
                    "workdir must be specified when allowed_paths is set".to_string(),
                ));
            }
        }

        // M006 canonical pipeline: normalize/classify, plan, and derive the
        // executor target once. The explicit context keeps production cwd
        // authority out of the classifier and planner.
        let context = CommandIntentContext {
            workspace_root: self.workspace_root.clone(),
            cwd: canonical_workdir
                .clone()
                .or_else(|| workdir.as_ref().map(PathBuf::from)),
        };
        let pipeline: CommandPipelineResult = prepare_command(command, &context);
        let (intent, plan, decision, routing_metadata) = {
            let metadata =
                output::build_routing_metadata(self.command_intent_config.as_ref(), &pipeline);
            (
                Some(pipeline.intent.clone()),
                Some(pipeline.plan.clone()),
                Some(pipeline.dispatch.clone()),
                metadata,
            )
        };

        tracing::info!("Running: {command}");
        let start = std::time::Instant::now();

        // 3. route to scheduler or local supervised process.
        let should_active_route = if let (Some(ref cic), Some(ref _intent), Some(ref plan)) =
            (&self.command_intent_config, &intent, &plan)
        {
            // Use plan_family so destructive / network / local mutations get
            // their own RouteLevel gate. Fall back to intent_kind_to_family
            // when the plan family is unresolved (e.g., RawShell intent).
            let family = plan_family(plan);
            let active_for_family = family.map(|f| cic.is_active_for_family(f)).unwrap_or(false);
            let plan_valid = plan.validate_for_active_routing().is_ok();
            let kill_switch_active = family.map(|f| self.check_kill_switches(f)).unwrap_or(true);
            let sandbox_enabled = self
                .landlock_sandbox
                .as_ref()
                .is_some_and(|config| config.enabled);
            active_for_family && plan_valid && !kill_switch_active && !sandbox_enabled
        } else {
            false
        };

        let (mut result, output, execution_outcome, delegated_run_id) = if should_active_route {
            // ACTIVE ROUTING: dispatch to structured backend
            tracing::info!("Active routing dispatch for: {command}");
            // `should_active_route` requires `(command_intent_config,
            // intent, plan)` to all be `Some`; mirror that here so we
            // don't unwrap against a future refactor that decouples
            // active-routing from these options.
            let (decision_ref, plan_ref) = match (decision.as_ref(), plan.as_ref()) {
                (Some(d), Some(p)) => (d, p),
                _ => {
                    return Err(ToolError::Execution(
                        "internal invariant violated: active routing requires a decision and plan"
                            .to_string(),
                    ));
                }
            };
            let planned_backend = plan_to_planned_backend(Some(&plan_ref.backend));

            match self
                .dispatch_command_target(
                    decision_ref,
                    plan_ref,
                    canonical_workdir.as_deref(),
                    workdir.as_deref().map(std::path::Path::new),
                    execution_timeout,
                )
                .await
            {
                Ok(outcome) => {
                    // A missing RunId means persistence was unavailable, not
                    // that execution failed. The delegated backend has already
                    // run the command; retrying through the shell here would
                    // execute tests/scripts twice and could repeat mutations.
                    let is_delegated_intent = matches!(
                        &outcome.executor,
                        ActualExecutor::TestRunner { .. } | ActualExecutor::PythonScript { .. }
                    );
                    if is_delegated_intent && outcome.delegated_run_id.is_none() {
                        tracing::warn!(
                            "Delegated dispatcher for {:?} returned no RunId; \
                             retaining the delegated result and using caller persistence when available",
                            outcome.executor
                        );
                        if let Some(ref plan) = plan {
                            self.record_routing_metric(RoutingMetric {
                                family: plan_family(plan)
                                    .unwrap_or(crate::config::schema::CommandIntentFamily::Tests),
                                decision: "active_routing_delegation_without_runid".to_string(),
                                fallback: self.run_store.is_some(),
                            });
                        }
                    }
                    let exec_outcome =
                        ExecutionOutcome::identity(planned_backend, outcome.executor.clone());
                    (
                        outcome.result,
                        outcome.output,
                        exec_outcome,
                        outcome.delegated_run_id,
                    )
                }
                Err(e) => {
                    // Admission or executor failure is terminal for the
                    // active route. Falling back to raw shell here would
                    // execute the command a second time or bypass the
                    // scheduler entirely.
                    if let Some(ref plan) = plan {
                        self.record_routing_metric(RoutingMetric {
                            family: plan_family(plan)
                                .unwrap_or(crate::config::schema::CommandIntentFamily::Tests),
                            decision: "active_routing_rejected".to_string(),
                            fallback: false,
                        });
                    }
                    return Err(e);
                }
            }
        } else {
            // OBSERVE MODE: run via raw shell (existing behavior)
            let argv = vec!["sh".to_string(), "-c".to_string(), command.to_string()];
            let (result, output) = self
                .execute_via_raw_shell(command, canonical_workdir.as_deref(), execution_timeout)
                .await?;
            let planned = plan_to_planned_backend(plan.as_ref().map(|p| &p.backend));
            let actual = ActualExecutor::RawShell {
                command: command.to_string(),
                argv,
            };
            (
                result,
                output,
                ExecutionOutcome::identity(planned, actual),
                None,
            )
        };

        let elapsed = start.elapsed();
        tracing::info!("Completed in {elapsed:?}");

        // 4. collect bounded result (already bounded by the process module)
        // and persist with session/run/workspace attribution.
        let intent_kind = intent
            .as_ref()
            .map(|i| i.kind)
            .unwrap_or(crate::command_intent::CommandIntentKind::RawShell);
        let (persist_run, ownership) =
            self.persistence_decision(&execution_outcome, delegated_run_id.as_ref());
        self.persist_caller_run(
            command,
            &output,
            &execution_outcome,
            intent_kind,
            routing_metadata.as_ref(),
            canonical_workdir.as_ref(),
            workdir.as_deref(),
            persist_run,
            ownership,
        )
        .await;

        // 5. project/redact/persist suffixes and return the structured result.
        if let Some(warning) = preflight_warning {
            result = format!("{}\n\n{}", warning, result);
        }

        result = output::append_routing_suffix(result, routing_metadata.as_ref());

        Ok(result)
    }
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;
    use crate::command_intent::CommandIntentKind;
    use crate::config::schema::CommandIntentFamily;
    use crate::config::schema::RouteLevel;

    #[test]
    fn builder_defaults_match_documented_contract() {
        let tool = BashTool::new();
        assert_eq!(tool.timeout, Duration::from_secs(120));
        assert_eq!(tool.max_output_lines, 2000);
        assert_eq!(tool.max_output_bytes, 50_000);
        assert!(!tool.deny_all);
        assert!(tool.allowed_paths.is_none());
        assert!(tool.allowlist.is_none());
        assert!(tool.landlock_sandbox.is_none());
        assert!(tool.run_store.is_none());
        assert!(tool.submission.is_none());
        assert_eq!(tool.name(), "bash");
        assert_eq!(tool.category(), ToolCategory::ShellExec);
    }

    #[test]
    fn command_intent_mode_default_is_observe() {
        let mode = crate::config::schema::CommandIntentMode::default();
        assert_eq!(mode, crate::config::schema::CommandIntentMode::Observe);
    }

    #[test]
    fn command_intent_config_mode_helper() {
        let mut config = CommandIntentConfig::default();
        assert_eq!(
            config.mode(),
            crate::config::schema::CommandIntentMode::Observe
        );
        assert!(!config.is_route_mode());
        assert!(!config.is_active_mode());

        config.mode = Some(crate::config::schema::CommandIntentMode::Route);
        assert_eq!(
            config.mode(),
            crate::config::schema::CommandIntentMode::Route
        );
        assert!(config.is_route_mode());
        assert!(config.is_active_mode());
    }

    #[test]
    fn active_mode_is_active() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        assert!(config.is_active_mode());
        assert!(config.is_route_mode());
    }

    #[test]
    fn family_level_defaults_to_observe_when_mode_is_observe() {
        let config = CommandIntentConfig::default();
        assert_eq!(
            config.family_level(CommandIntentFamily::Tests),
            RouteLevel::Observe
        );
    }

    #[test]
    fn family_level_defaults_to_active_when_mode_is_active() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        assert_eq!(
            config.family_level(CommandIntentFamily::Tests),
            RouteLevel::Active
        );
    }

    #[test]
    fn family_level_uses_override_when_set() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        config.route_tests = Some(RouteLevel::Off);
        assert_eq!(
            config.family_level(CommandIntentFamily::Tests),
            RouteLevel::Off
        );
        assert_eq!(
            config.family_level(CommandIntentFamily::GitRead),
            RouteLevel::Active
        );
    }

    #[test]
    fn is_active_for_family_requires_active_mode() {
        let mut config = CommandIntentConfig::default();
        config.route_safe_commands = Some(true);
        config.route_tests = Some(RouteLevel::Active);
        assert!(!config.is_active_for_family(CommandIntentFamily::Tests));

        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        assert!(config.is_active_for_family(CommandIntentFamily::Tests));
    }

    #[test]
    fn is_active_for_family_requires_active_level() {
        let mut config = CommandIntentConfig::default();
        config.mode = Some(crate::config::schema::CommandIntentMode::Active);
        config.route_tests = Some(RouteLevel::Observe);
        assert!(!config.is_active_for_family(CommandIntentFamily::Tests));

        config.route_tests = Some(RouteLevel::Active);
        assert!(config.is_active_for_family(CommandIntentFamily::Tests));
    }

    #[test]
    fn route_level_default_is_observe() {
        assert_eq!(RouteLevel::default(), RouteLevel::Observe);
    }

    #[test]
    fn config_all_new_families_default_to_off() {
        let config = CommandIntentConfig::default();
        assert!(!config.is_enabled(CommandIntentFamily::Build));
        assert!(!config.is_enabled(CommandIntentFamily::Lint));
        assert!(!config.is_enabled(CommandIntentFamily::Format));
    }

    #[tokio::test]
    async fn bash_no_config_produces_no_routing_metadata() {
        let tool = BashTool::new();
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("hello"));
        assert!(!result.contains("[intent:"));
    }

    #[tokio::test]
    async fn bash_with_config_attaches_routing_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("[intent:"));
        assert!(result.contains("backend:"));
        assert!(result.contains("routing:"));
    }

    #[tokio::test]
    async fn bash_test_command_metadata_when_enabled() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"));
        assert!(result.contains("backend: test-runner"));
        assert!(result.contains("projector: test-report"));
        assert!(result.contains("routing: enabled"));
    }

    #[tokio::test]
    async fn bash_test_command_metadata_when_disabled() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"));
        assert!(result.contains("routing: disabled"));
    }

    #[tokio::test]
    async fn bash_git_readonly_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: git-readonly"));
        assert!(result.contains("backend: git"));
        assert!(result.contains("routing: enabled"));
    }

    #[tokio::test]
    async fn bash_search_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_search = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "grep -rn 'pattern' src/"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: search-readonly"));
        assert!(result.contains("backend: managed-argv"));
        assert!(result.contains("projector: file-search"));
        assert!(result.contains("routing: enabled"));
    }

    #[tokio::test]
    async fn bash_python_metadata_when_disabled() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_python = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "python3 -c 'print(1)'"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("python"));
        assert!(result.contains("routing: disabled"));
    }

    #[tokio::test]
    async fn bash_raw_shell_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Off);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: raw-shell"));
        assert!(result.contains("backend: raw-shell"));
        assert!(result.contains("routing: disabled"));
    }

    #[tokio::test]
    async fn observe_mode_runs_raw_shell_for_test_command() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo observe-test-ok"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("observe-test-ok"),
            "command must execute via raw shell"
        );
        assert!(result.contains("mode: observe"));
        assert!(result.contains("intent: raw-shell"));
    }

    #[tokio::test]
    async fn observe_mode_appends_metadata() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo hello"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("[intent:"), "metadata must be present");
        assert!(
            result.contains("mode: observe"),
            "mode must appear in metadata"
        );
    }

    #[tokio::test]
    async fn observe_mode_is_default_when_not_set() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo default-mode"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("mode: observe"),
            "default mode must be observe"
        );
    }

    #[tokio::test]
    async fn route_mode_falls_back_to_observe_and_warns() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Route);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo route-fallback-ok"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("route-fallback-ok"),
            "command must execute even in route mode"
        );
        assert!(
            result.contains("mode: route (fallback: observe)"),
            "metadata must show route fallback"
        );
    }

    #[tokio::test]
    async fn route_mode_does_not_change_execution_path() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);
        cic.route_git_read = Some(RouteLevel::Observe);
        cic.route_search = Some(RouteLevel::Observe);
        cic.route_python = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Route);

        let tool = BashTool::new().with_command_intent_config(cic);

        let input = serde_json::json!({"command": "echo routing-inactive"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("routing-inactive"),
            "must execute via raw shell"
        );

        let input = serde_json::json!({"command": "echo git-fallback"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("git-fallback"),
            "must execute via raw shell"
        );
    }

    #[tokio::test]
    async fn no_config_produces_no_metadata_no_mode() {
        let tool = BashTool::new();
        let input = serde_json::json!({"command": "echo clean"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("clean"));
        assert!(!result.contains("[intent:"), "no metadata without config");
        assert!(!result.contains("mode:"), "no mode without config");
    }

    #[tokio::test]
    async fn route_safe_commands_true_alone_does_not_enable_routing() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("intent: test"), "must classify as test");
        assert!(
            result.contains("mode: observe"),
            "must remain in observe mode"
        );
        assert!(
            result.contains("routing: enabled"),
            "family can be enabled for metadata annotation"
        );
    }

    #[tokio::test]
    async fn active_mode_routes_git_to_native_tool() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("[exit code:"),
            "command must produce exit code in output: {}",
            result
        );
        assert!(
            result.contains("mode: active"),
            "metadata must show active mode: {}",
            result
        );
    }

    #[tokio::test]
    async fn active_mode_test_command_requires_scheduler() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_tests = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo test --help"});
        let error = tool
            .execute(input)
            .await
            .expect_err("active test routing must not bypass the scheduler");
        assert!(error.to_string().contains("requires the daemon scheduler"));
    }

    #[tokio::test]
    async fn observe_mode_still_runs_raw_shell_for_git() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Observe);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Observe);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("mode: observe"));
        assert!(result.contains("[exit code:"));
    }

    #[tokio::test]
    async fn active_mode_off_level_kill_switch_prevents_routing() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Off);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("mode: active"),
            "metadata shows active mode"
        );
        assert!(result.contains("routing: disabled"), "routing is disabled");
    }

    #[tokio::test]
    async fn env_kill_switch_disables_active_routing() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new()
            .with_routing_disabled_env(true)
            .with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("[exit code:"));
    }

    #[tokio::test]
    async fn active_mode_build_command_requires_scheduler() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_build = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "cargo check"});
        let error = tool
            .execute(input)
            .await
            .expect_err("build routing must not bypass the scheduler");
        assert!(error.to_string().contains("requires the daemon scheduler"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn active_mode_python_command_routes() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_python = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "python3 -c 'print(42)'"});
        let error = tool
            .execute(input)
            .await
            .expect_err("python routing must not bypass the scheduler");
        assert!(
            error.to_string().contains("requires scheduler admission"),
            "python routing must return typed scheduler-unavailable error: {}",
            error
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn scheduler_unavailable_python_returns_typed_error() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_python = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "python3 -c 'import sys; print(sys.version)'"});
        let error = tool
            .execute(input)
            .await
            .expect_err("scheduler unavailability must not panic");
        let msg = error.to_string();
        assert!(
            msg.contains("scheduler admission") || msg.contains("scheduler is disabled"),
            "must return typed scheduler error without panic: {}",
            msg
        );
    }

    #[tokio::test]
    async fn active_mode_search_command_requires_scheduler() {
        std::env::remove_var("CODEGG_ROUTING_DISABLE");
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_search = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "rg --version"});
        let error = tool
            .execute(input)
            .await
            .expect_err("search routing must not bypass the scheduler");
        assert!(error.to_string().contains("requires the daemon scheduler"));
    }

    #[tokio::test]
    async fn route_mode_still_falls_back_to_observe() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.route_git_read = Some(RouteLevel::Active);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Route);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "git status"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("[exit code:"));
        assert!(
            result.contains("mode: route (fallback: observe)"),
            "metadata must show route mode"
        );
    }

    #[tokio::test]
    async fn active_mode_raw_shell_falls_back_to_raw_shell() {
        let mut cic = CommandIntentConfig::default();
        cic.route_safe_commands = Some(true);
        cic.mode = Some(crate::config::schema::CommandIntentMode::Active);

        let tool = BashTool::new().with_command_intent_config(cic);
        let input = serde_json::json!({"command": "echo active-fallback"});
        let result = tool.execute(input).await.unwrap();
        assert!(
            result.contains("active-fallback"),
            "command must execute via raw shell fallback"
        );
    }

    #[test]
    fn pre_spawn_order_rejects_blocked_command_before_any_dispatch() {
        // Regression guard for the extraction: policy checks must precede
        // any process construction. A blocked command fails in the policy
        // layer even when scheduler submission is configured.
        let tool = BashTool::new();
        let parts: Vec<&str> = "rm -rf /".split_whitespace().collect();
        let err = tool
            .check_command_security("rm -rf /", &parts)
            .expect_err("blocked command must not reach spawn");
        assert!(err.to_string().contains("blocked list"));
    }

    #[test]
    fn child_git_ceiling_cannot_be_bypassed_by_shell_spelling() {
        let dir = tempfile::tempdir().expect("child ceiling test root");
        let root = dir.path();
        assert!(super::policy::validate_child_workspace_command("echo ok", &[], root).is_ok());
        assert!(super::policy::validate_child_workspace_command("cd /tmp", &[], root).is_err());
        assert!(
            super::policy::validate_child_workspace_command("echo hi; cd ..", &[], root).is_err()
        );
    }

    #[test]
    fn unused_intent_import_stays_available_for_future_routing() {
        let _ = CommandIntentKind::RawShell;
    }
}
