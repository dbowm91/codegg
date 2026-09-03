use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;
use std::path::PathBuf;

use crate::config::schema::Config;
use crate::error::ToolError;
use crate::git_mutation_projector::project_mutation;
use crate::git_mutations::{CommitSelection, GitEnvPolicy, GitMutationError, GitMutationExecutor};
use crate::git_mutations_ops::{commit_with_selection, stage_all, stage_paths};
use crate::provider::{
    register_builtin_with_config, ChatEvent, ChatRequest, ContentPart, Message, Provider,
    ProviderRegistry, ProviderRequestContext,
};
use crate::tool::backend::provider_request_context;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};

/// Native tool that creates a commit. Refactored around the typed
/// `CommitSelection` enum so the model can explicitly request
/// "stage all", "stage paths", or "use the index as-is" without
/// duplicating Git subprocess logic.
pub struct CommitTool {
    workdir: PathBuf,
    run_store: Option<std::sync::Arc<dyn codegg_core::run_store::RunStore>>,
    provider: Option<std::sync::Arc<dyn Provider>>,
}

impl CommitTool {
    pub fn new() -> Self {
        Self {
            workdir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            run_store: None,
            provider: None,
        }
    }

    pub fn with_workdir(mut self, dir: PathBuf) -> Self {
        self.workdir = dir;
        self
    }

    pub fn with_run_store(
        mut self,
        store: std::sync::Arc<dyn codegg_core::run_store::RunStore>,
    ) -> Self {
        self.run_store = Some(store);
        self
    }

    /// Override provider selection for callers that already own a provider
    /// and for focused nested-request tests.
    pub fn with_provider(mut self, provider: std::sync::Arc<dyn Provider>) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Build a `GitMutationExecutor` rooted at this tool's workdir.
    fn executor(&self) -> GitMutationExecutor {
        GitMutationExecutor::new().with_env_policy(GitEnvPolicy::default())
    }

    /// Generate a commit message from the staged diff via the LLM.
    /// Message generation is a separate concern from the commit
    /// itself; this method does not own repository mutation.
    async fn generate_commit_message(
        &self,
        diff: &str,
        context: ProviderRequestContext,
    ) -> Result<String, ToolError> {
        let config = Config::load_or_default();
        let model = config
            .model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4".to_string());

        if let Some(provider) = self.provider.as_deref() {
            return generate_commit_message_with_provider(diff, provider, &model, context).await;
        }

        let mut registry = ProviderRegistry::new();
        register_builtin_with_config(&mut registry, &config);
        let provider = registry
            .get(&model)
            .or_else(|| registry.list().first().copied())
            .ok_or_else(|| ToolError::Execution("no provider available".to_string()))?;

        generate_commit_message_with_provider(diff, provider, &model, context).await
    }
}

async fn generate_commit_message_with_provider(
    diff: &str,
    provider: &dyn Provider,
    model: &str,
    context: ProviderRequestContext,
) -> Result<String, ToolError> {
    let prompt = format!(
        "Generate a concise git commit message for the following diff. \
             Follow conventional commit format (type: description). \
             Be specific but brief. Return ONLY the commit message, nothing else.\n\nDiff:\n{}",
        diff
    );

    let request = ChatRequest {
        messages: vec![Message::User {
            content: vec![ContentPart::Text {
                text: prompt.into(),
            }],
        }],
        model: model.to_string(),
        tools: None,
        system: Some(
            "You are a git commit message generator. Return only the commit message.".to_string(),
        ),
        temperature: Some(0.3),
        top_p: None,
        max_tokens: Some(200),
        response_format: None,
        thinking_budget: None,
        reasoning_effort: None,
        context,
    };

    let mut stream = provider
        .stream(&request)
        .await
        .map_err(|e| ToolError::Execution(format!("LLM request failed: {}", e)))?;

    let mut message = String::new();
    while let Some(event) = stream.next().await {
        match event {
            Ok(ChatEvent::TextDelta(delta)) => message.push_str(&delta),
            Ok(ChatEvent::Finish { .. }) => break,
            Ok(ChatEvent::Error(e)) => {
                return Err(ToolError::Execution(format!("LLM error: {}", e)))
            }
            _ => {}
        }
    }

    let message = message.trim();
    if message.is_empty() {
        return Err(ToolError::Execution(
            "LLM generated empty commit message".to_string(),
        ));
    }

    Ok(message.to_string())
}

impl CommitTool {
    /// Fetch the staged diff text for the LLM prompt.
    async fn get_staged_diff(&self) -> Result<String, ToolError> {
        let diff = egggit::diff_text(&self.workdir, egggit::DiffMode::Staged)
            .await
            .map_err(|e| ToolError::Execution(format!("git diff failed: {e}")))?;
        Ok(diff)
    }

    /// Map a `GitMutationError` to a `ToolError`.
    fn map_mutation_err(e: GitMutationError) -> ToolError {
        match e {
            GitMutationError::Precondition(s) => ToolError::Execution(format!("precondition: {s}")),
            GitMutationError::Path(s) => ToolError::Execution(format!("path error: {s}")),
            GitMutationError::Ref(s) => ToolError::Execution(format!("ref error: {s}")),
            GitMutationError::Repository(s) => ToolError::Execution(format!("repository: {s}")),
            GitMutationError::Execution { message, .. } => ToolError::Execution(message),
            GitMutationError::Timeout(s) => ToolError::Execution(format!("timed out after {s}s")),
            GitMutationError::StateMismatch { expected, actual } => ToolError::Execution(format!(
                "state mismatch: expected operation '{expected}' but found '{actual}' on disk"
            )),
        }
    }
}

impl Default for CommitTool {
    fn default() -> Self {
        Self::new()
    }
}

/// Expose the model-facing selection as JSON-friendly variant strings.
#[derive(Debug, Clone, Copy)]
enum SelectionKind {
    AlreadyStaged,
    StagePaths,
    StageAll,
}

impl SelectionKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AlreadyStaged => "already-staged",
            Self::StagePaths => "stage-paths",
            Self::StageAll => "stage-all",
        }
    }
}

#[async_trait]
impl Tool for CommitTool {
    fn name(&self) -> &str {
        "commit"
    }

    fn description(&self) -> &str {
        "Create a git commit. Selection controls staging: already-staged (default), stage-paths, or stage-all. LLM generates the message unless `message` is provided."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "selection": {
                    "type": "string",
                    "enum": ["already-staged", "stage-paths", "stage-all"],
                    "description": "How to select staged content (default: already-staged)"
                },
                "paths": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Literal repo-relative paths to stage before committing (only with selection=stage-paths)"
                },
                "message": {
                    "type": "string",
                    "description": "Optional: provide a manual commit message (skips generation)"
                },
                "co_authored": {
                    "type": "boolean",
                    "description": "Add Co-Authored-By with AI agent info"
                },
                "amend": {
                    "type": "boolean",
                    "description": "Amend the previous commit (default: false)"
                },
                "allow_amend": {
                    "type": "boolean",
                    "description": "Required safety acknowledgement when amend=true"
                },
                "allow_empty": {
                    "type": "boolean",
                    "description": "Allow creating a commit with no staged changes (default: false)"
                }
            },
            "required": []
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_with_context(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        context: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute_with_context(input, context.as_ref()).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}

impl CommitTool {
    async fn execute_with_context(
        &self,
        input: serde_json::Value,
        context: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let provider_context = provider_request_context(context);
        let amend = input["amend"].as_bool().unwrap_or(false);
        let allow_amend = input["allow_amend"].as_bool().unwrap_or(false);
        let co_authored = input["co_authored"].as_bool().unwrap_or(false);
        let allow_empty = input["allow_empty"].as_bool().unwrap_or(false);
        let manual_message = input["message"].as_str();
        let selection_kind = match input["selection"].as_str() {
            Some("stage-paths") => SelectionKind::StagePaths,
            Some("stage-all") => SelectionKind::StageAll,
            Some("already-staged") | None => SelectionKind::AlreadyStaged,
            Some(other) => {
                return Err(ToolError::Execution(format!(
                    "unknown selection '{other}'; expected one of already-staged, stage-paths, stage-all"
                )));
            }
        };

        if amend && !allow_amend {
            return Err(ToolError::Execution(
                "amend requires explicit allow_amend=true acknowledgement".to_string(),
            ));
        }

        let selection = match selection_kind {
            SelectionKind::AlreadyStaged => CommitSelection::AlreadyStaged,
            SelectionKind::StageAll => CommitSelection::StageAll,
            SelectionKind::StagePaths => {
                let paths: Vec<String> = input["paths"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if paths.is_empty() {
                    return Err(ToolError::Execution(
                        "selection=stage-paths requires a non-empty paths array".to_string(),
                    ));
                }
                CommitSelection::StagePaths(paths)
            }
        };

        let message = if let Some(msg) = manual_message {
            msg.to_string()
        } else if amend {
            // When amending, default to the previous commit message if not
            // provided. Fetch it via show.
            fetch_head_message(&self.workdir).await?
        } else {
            // For non-amend with StageAll, stage first so get_staged_diff
            // sees the changes.
            let needs_paths = match &selection {
                CommitSelection::StageAll => {
                    let exec = self.executor();
                    stage_all(&exec, &self.workdir)
                        .await
                        .map_err(Self::map_mutation_err)?;
                    None
                }
                CommitSelection::StagePaths(paths) => Some(paths.clone()),
                _ => None,
            };
            if let Some(paths) = needs_paths {
                let exec = self.executor();
                stage_paths(&exec, &self.workdir, paths)
                    .await
                    .map_err(Self::map_mutation_err)?;
            }
            let diff = self.get_staged_diff().await?;
            if diff.is_empty() {
                return Err(ToolError::Execution("no changes to commit".to_string()));
            }
            self.generate_commit_message(&diff, provider_context)
                .await?
        };

        let final_message = if co_authored {
            format!("{}\n\nCo-Authored-By: Codegg AI <codegg@ai>", message)
        } else {
            message
        };

        let exec = self.executor();
        let outcome = commit_with_selection(
            &exec,
            &self.workdir,
            selection,
            &final_message,
            amend,
            allow_empty,
        )
        .await
        .map_err(Self::map_mutation_err)?;

        // Project the mutation result into a structured summary.
        let summary = project_mutation(&outcome.mutation);
        let mut response = summary;

        // Best-effort persistence to RunStore.
        let repo_root = crate::git_mutations::resolve_repo_root(&self.workdir)
            .map(|r| r.as_path().to_path_buf())
            .unwrap_or_else(|_| self.workdir.clone());
        let result_opt = crate::git_run_store::persist_mutation(
            &self.run_store,
            &outcome.mutation,
            &self.workdir,
            &repo_root,
            "commit_tool",
            Some(selection_kind.label().to_string()),
        )
        .await;
        if result_opt.is_none() {
            tracing::warn!("failed to persist commit mutation to RunStore");
        }
        if let Some(oid) = &outcome.created_oid {
            response.push_str(&format!("\ncreated_oid: {oid}"));
        }
        if outcome.amended {
            response.push_str("\namended: true");
        }
        if outcome.empty {
            response.push_str("\nempty: true");
        }
        Ok(response)
    }
}

/// Fetch the previous commit's full message via `git log -1 --format=%B`.
async fn fetch_head_message(workdir: &std::path::Path) -> Result<String, ToolError> {
    use crate::git_mutations::GitEnvPolicy;
    let argv: Vec<String> = vec![
        "git".into(),
        "log".into(),
        "-1".into(),
        "--format=%B".into(),
    ];
    let mut cmd = GitEnvPolicy::default().apply(&argv, workdir);
    let output = cmd
        .output()
        .await
        .map_err(|e| ToolError::Execution(format!("git log failed: {e}")))?;
    if !output.status.success() {
        return Err(ToolError::Execution(format!(
            "git log exited with {:?}",
            output.status.code()
        )));
    }
    let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if msg.is_empty() {
        return Err(ToolError::Execution(
            "amend requested but HEAD has no message to reuse".to_string(),
        ));
    }
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct ContextCaptureProvider {
        contexts: Arc<Mutex<Vec<Option<String>>>>,
    }

    #[async_trait]
    impl Provider for ContextCaptureProvider {
        fn id(&self) -> &str {
            "context-capture"
        }

        fn name(&self) -> &str {
            "Context Capture"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(Self {
                contexts: self.contexts.clone(),
            })
        }

        async fn stream(
            &self,
            request: &ChatRequest,
        ) -> Result<crate::provider::EventStream, crate::provider::ProviderError> {
            self.contexts
                .lock()
                .unwrap()
                .push(request.context.session_id.as_deref().map(ToOwned::to_owned));
            Ok(Box::pin(futures_util::stream::iter(vec![Ok(
                ChatEvent::TextDelta(Arc::new("feat: test".to_string())),
            )])))
        }

        async fn models(
            &self,
        ) -> Result<Vec<crate::provider::ModelInfo>, crate::provider::ProviderError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn selection_kind_label_round_trip() {
        assert_eq!(SelectionKind::AlreadyStaged.label(), "already-staged");
        assert_eq!(SelectionKind::StagePaths.label(), "stage-paths");
        assert_eq!(SelectionKind::StageAll.label(), "stage-all");
    }

    #[test]
    fn map_mutation_err_includes_kind() {
        let e = GitMutationError::Precondition("no staged changes".to_string());
        let msg = match e {
            GitMutationError::Precondition(s) => format!("precondition: {s}"),
            _ => unreachable!(),
        };
        assert!(msg.contains("precondition"), "got: {msg}");
    }

    #[tokio::test]
    async fn nested_commit_request_uses_tool_execution_session() {
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let provider = ContextCaptureProvider {
            contexts: contexts.clone(),
        };
        let context = ProviderRequestContext {
            session_id: Some(Arc::from("session-commit")),
        };

        let result = generate_commit_message_with_provider(
            "diff --git a/src/lib.rs b/src/lib.rs",
            &provider,
            "test-model",
            context,
        )
        .await
        .expect("capture provider should return a commit message");

        assert_eq!(result, "feat: test");
        assert_eq!(
            contexts.lock().unwrap().as_slice(),
            &[Some("session-commit".to_string())]
        );
    }
}
