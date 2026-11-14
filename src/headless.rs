//! Shared loop setup for `exec` and `host`.
//!
//! This is the sequence `ExecMode::run` performs before it drives a turn:
//! config, registry, model/provider resolution, agents, policy, search,
//! tools and the loop itself. A caller chooses whether a provider descriptor
//! replaces config registration, and whether config may be read at all.

use crate::agent::r#loop::AgentLoop;
use crate::agent::{self, Agent, EMERGENCY_DEFAULT_MODEL};
use crate::config::schema::Config;
use crate::error::{AgentError, AppError};
use crate::permission::PermissionChecker;
use crate::provider::inline::{register_inline_provider, InlineProviderDescriptor};
use crate::provider::Provider;
use codegg_core::approval::{ApprovalMode, SandboxProfile};
use std::path::PathBuf;

/// How a runtime obtains its configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlessConfig {
    /// Load configuration; a failure is returned.
    Load,
    /// Use compiled defaults without touching the filesystem.
    Default,
    /// Load, but fall back to defaults when a descriptor is present.
    /// This is `exec`'s compatibility behaviour.
    LoadOrDefaultOnDescriptor,
}

/// One headless session's choices.
pub struct HeadlessOptions {
    pub workspace_root: PathBuf,
    pub config: HeadlessConfig,
    pub model: Option<String>,
    pub agent: Option<String>,
    pub approval: Option<ApprovalMode>,
    pub sandbox: Option<SandboxProfile>,
    pub session_id: Option<String>,
    pub descriptor: Option<InlineProviderDescriptor>,
    pub policy_notices: bool,
    #[cfg(test)]
    pub provider_override: Option<Box<dyn Provider>>,
    #[cfg(test)]
    pub config_override: Option<Config>,
}

impl HeadlessOptions {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            config: HeadlessConfig::Load,
            model: None,
            agent: None,
            approval: None,
            sandbox: None,
            session_id: None,
            descriptor: None,
            policy_notices: false,
            #[cfg(test)]
            provider_override: None,
            #[cfg(test)]
            config_override: None,
        }
    }
}

/// The resolved pieces one loop is built from.
pub struct HeadlessRuntime {
    config: Config,
    agents: Vec<Agent>,
    provider: Box<dyn Provider>,
    tool_registry: crate::tool::ToolRegistry,
    mcp_service: Option<std::sync::Arc<tokio::sync::RwLock<crate::mcp::McpService>>>,
    workspace_root: PathBuf,
    session_id: String,
    agent_name: String,
    model_request: String,
    model_selection: String,
    approval: Option<ApprovalMode>,
    sandbox: Option<SandboxProfile>,
}

impl HeadlessRuntime {
    pub async fn build(mut options: HeadlessOptions) -> Result<Self, HeadlessError> {
        let (injected_provider, config_override) = take_test_seams(&mut options);

        let mut config = if let Some(config) = config_override {
            config
        } else {
            match options.config {
                HeadlessConfig::Default => Config::default(),
                HeadlessConfig::Load => Config::load()?,
                HeadlessConfig::LoadOrDefaultOnDescriptor => match Config::load() {
                    Ok(config) => config,
                    Err(error) if options.descriptor.is_some() => {
                        tracing::warn!(
                            error = %error,
                            "provider descriptor session continues without a config file"
                        );
                        Config::default()
                    }
                    Err(error) => return Err(HeadlessError::Config(error)),
                },
            }
        };

        let mut registry = crate::provider::ProviderRegistry::new();
        match &options.descriptor {
            Some(descriptor) => register_inline_provider(&mut registry, descriptor)?,
            None => {
                let locators =
                    crate::core::session_selection::user_connection_credential_locators().await;
                crate::provider::register_builtin_with_config_connections(
                    &mut registry,
                    &config,
                    &locators,
                    None,
                );
            }
        }

        let default_model = config
            .model
            .clone()
            .unwrap_or_else(|| EMERGENCY_DEFAULT_MODEL.to_string());
        let model_selection = match (&options.model, &options.descriptor) {
            (Some(model), _) => model.clone(),
            (None, Some(descriptor)) => descriptor
                .models
                .first()
                .map(|declared| format!("{}/{}", descriptor.id, declared.id))
                .unwrap_or(default_model),
            (None, None) => default_model,
        };
        let (provider_id, model_request) = match &options.descriptor {
            // A descriptor session has exactly one provider, so the model id is
            // the host's own string. Only the descriptor's own `id/` prefix is
            // stripped; a model id that contains a slash keeps it, because
            // remapping it would send a different model than the host asked for.
            Some(descriptor) => {
                let prefix = format!("{}/", descriptor.id);
                let model_request = model_selection
                    .strip_prefix(&prefix)
                    .unwrap_or(&model_selection)
                    .to_string();
                (descriptor.id.clone(), model_request)
            }
            None => split_model_selection(&model_selection),
        };
        if let Some(descriptor) = &options.descriptor {
            if let Some(declared) = descriptor
                .models
                .iter()
                .find(|declared| declared.id == model_request)
            {
                crate::model_profile::InlineModelProfile::from_inline_model(declared)
                    .apply_to_config(&mut config);
            }
        }

        let agents = agent::resolve_agents_with_context(&config, Some(&options.workspace_root))?;
        let agent_name = options.agent.unwrap_or_else(|| "build".to_string());
        if !agents.iter().any(|candidate| candidate.name == agent_name) {
            return Err(HeadlessError::AgentNotFound(agent_name));
        }

        let provider = match injected_provider {
            Some(provider) => provider,
            None => registry
                .get(&provider_id)
                .ok_or_else(|| HeadlessError::ProviderNotFound(provider_id.clone()))?
                .clone_box(),
        };

        if options.policy_notices {
            if let Some(mode) = options.approval {
                eprintln!("exec approval mode: {}", mode.as_str());
            }
            if let Some(profile) = options.sandbox {
                eprintln!("exec sandbox profile: {}", profile.as_str());
                if profile.is_full_host() {
                    eprintln!(
                        "WARNING: exec requested full-host access: CodeGG filesystem containment is disabled and the process has the OS user's host authority"
                    );
                }
            }
        }

        let (search_runtime, _report) =
            crate::search_backend::bootstrap::bootstrap_search_runtime(&config).await;
        let mcp_service = search_runtime.mcp();
        // A workspace-scoped tool refuses a call that carries no root, so the
        // registry must be built for this session's workspace, exactly as
        // `exec` builds it for its own.
        let tool_registry = crate::tool::ToolRegistry::with_config_and_search_runtime_for_workspace(
            &config,
            search_runtime,
            Some(options.workspace_root.clone()),
        );

        let raw_session_id = options
            .session_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let session_id = codegg_core::context::SessionId::parse(&raw_session_id)
            .map_err(|_| HeadlessError::SessionId)?
            .as_str()
            .to_owned();

        Ok(Self {
            config,
            agents,
            provider,
            tool_registry,
            mcp_service,
            workspace_root: options.workspace_root,
            session_id,
            agent_name,
            model_request,
            model_selection,
            approval: options.approval,
            sandbox: options.sandbox,
        })
    }

    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    pub fn model_request(&self) -> &str {
        &self.model_request
    }

    pub fn model_selection(&self) -> &str {
        &self.model_selection
    }

    pub fn into_loop(self) -> AgentLoop {
        let permission_checker = PermissionChecker::new(
            Some(&self.config),
            crate::permission::approval::canonical_permission_store_path(),
        )
        .with_exec_mode();
        let writer_fence: Option<crate::tool::writer_fence::SharedWriterFence> =
            Some(std::sync::Arc::new(
                crate::tool::writer_fence::WorkspaceWriterFence::new(self.workspace_root.clone()),
            ));
        let workspace_root = self.workspace_root;
        let mut loop_instance = AgentLoop::new(
            self.agents,
            self.provider,
            permission_checker,
            self.tool_registry,
            self.config,
            self.mcp_service,
            None,
            std::sync::Arc::new(crate::context::InMemoryArtifactStore::new()),
            workspace_root.clone(),
            self.session_id,
            writer_fence,
            None,
        );
        loop_instance.set_workspace_path_policy(
            codegg_core::workspace_services::WorkspacePathPolicy::for_root(workspace_root),
        );
        loop_instance.set_workspace_id(codegg_core::workspace::WorkspaceId::new());
        loop_instance.setup_question_channel_for_exec();
        if let Some(mode) = self.approval {
            loop_instance.set_approval_mode(mode);
        }
        if let Some(profile) = self.sandbox {
            loop_instance.set_sandbox_profile(profile);
        }
        loop_instance
    }
}

#[cfg(test)]
fn take_test_seams(options: &mut HeadlessOptions) -> (Option<Box<dyn Provider>>, Option<Config>) {
    (
        options.provider_override.take(),
        options.config_override.take(),
    )
}

#[cfg(not(test))]
fn take_test_seams(_options: &mut HeadlessOptions) -> (Option<Box<dyn Provider>>, Option<Config>) {
    (None, None)
}

/// Split `provider/model` selection strings, matching `exec`.
fn split_model_selection(model: &str) -> (String, String) {
    if let Some(position) = model.find('/') {
        (
            model[..position].to_string(),
            model[position + 1..].to_string(),
        )
    } else {
        ("openai".to_string(), model.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HeadlessError {
    #[error("configuration could not be loaded: {0}")]
    Config(#[from] crate::config::error::ConfigError),
    #[error("agent not available: {0}")]
    AgentNotFound(String),
    #[error("agents could not be resolved: {0}")]
    Agents(#[from] AgentError),
    #[error("provider not available: {0}")]
    ProviderNotFound(String),
    #[error("invalid provider descriptor: {0}")]
    Descriptor(#[from] codegg_providers::inline::InlineProviderError),
    #[error("sessionId is not a valid session id")]
    SessionId,
}

impl From<HeadlessError> for AppError {
    fn from(error: HeadlessError) -> Self {
        match error {
            HeadlessError::Config(error) => AppError::Config(error.into()),
            HeadlessError::AgentNotFound(agent) => AppError::Agent(AgentError::NotFound(agent)),
            HeadlessError::Agents(error) => AppError::Agent(error),
            HeadlessError::ProviderNotFound(provider) => {
                AppError::Other(anyhow::anyhow!("Provider not found: {provider}"))
            }
            HeadlessError::Descriptor(error) => {
                AppError::Other(anyhow::anyhow!("Invalid provider descriptor: {error}"))
            }
            HeadlessError::SessionId => {
                AppError::Other(anyhow::anyhow!("sessionId is not a valid session id"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> InlineProviderDescriptor {
        serde_json::from_value(serde_json::json!({
            "id": "fake",
            "baseUrl": "https://example.invalid/v1",
            "protocol": "chat_completions",
            "credential": {"kind": "api_key", "value": "unit-test-key"},
            "models": [{"id": "test-model", "contextWindow": 64000}],
        }))
        .expect("descriptor")
    }

    fn options(workspace: &std::path::Path) -> HeadlessOptions {
        let config = Config {
            search: Some(crate::config::schema::SearchConfig {
                backend: Some(crate::config::schema::SearchBackendConfig::Disabled),
                ..Default::default()
            }),
            ..Config::default()
        };
        let mut options = HeadlessOptions::new(workspace);
        options.config = HeadlessConfig::Default;
        options.model = Some("fake/test-model".to_string());
        options.descriptor = Some(descriptor());
        options.approval = Some(ApprovalMode::Yolo);
        options.sandbox = Some(SandboxProfile::WorkspaceWrite);
        options.session_id = Some("descriptor-session".to_string());
        options.config_override = Some(config);
        options
    }

    #[tokio::test]
    async fn a_descriptor_builds_a_loop_without_a_config_file() {
        let workspace = std::env::temp_dir();
        let runtime = HeadlessRuntime::build(options(&workspace))
            .await
            .expect("descriptor runtime builds");
        assert_eq!(runtime.agent_name(), "build");
        assert_eq!(runtime.model_request(), "test-model");
        assert_eq!(runtime.model_selection(), "fake/test-model");
        assert_eq!(runtime.session_id(), "descriptor-session");
        let loop_instance = runtime.into_loop();
        assert_eq!(loop_instance.session_id(), "descriptor-session");
    }

    #[tokio::test]
    async fn an_unknown_agent_is_named_and_refused() {
        let mut options = options(&std::env::temp_dir());
        options.agent = Some("missing-agent".to_string());
        let error = HeadlessRuntime::build(options)
            .await
            .err()
            .expect("unknown agent");
        assert!(matches!(error, HeadlessError::AgentNotFound(name) if name == "missing-agent"));
    }
}
