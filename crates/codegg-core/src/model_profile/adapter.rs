use super::types::{PromptProfileKind, ReliabilityTier, ResolvedModelProfile, TaskStatePolicy};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/model_adapters.rs"));

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterDefinition {
    pub schema_version: u32,
    pub adapter: AdapterMetadata,
    pub r#match: Vec<AdapterMatch>,
    #[serde(default)]
    pub profile: AdapterProfile,
    #[serde(default)]
    pub tools: AdapterTools,
    #[serde(default)]
    pub prompt: AdapterPrompt,
    #[serde(default)]
    pub recovery: RecoveryPolicy,
    #[serde(default)]
    pub server_requirements: ServerRequirements,
    #[serde(default)]
    pub transforms: Vec<RequestTransform>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterMetadata {
    pub id: String,
    pub version: u32,
    pub priority: i32,
    pub description: String,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterMatch {
    #[serde(default)]
    pub provider: Vec<String>,
    #[serde(default)]
    pub exact_model: Vec<String>,
    pub model_prefix: Option<String>,
    pub model_suffix: Option<String>,
    pub model_regex: Option<String>,
    pub exclude_regex: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterProfile {
    pub prompt_profile: Option<String>,
    pub family: Option<String>,
    pub context_window: Option<usize>,
    pub max_output_tokens: Option<usize>,
    pub tool_call_reliability: Option<String>,
    pub instruction_adherence: Option<String>,
    pub patch_reliability: Option<String>,
    pub supports_late_system_messages: Option<bool>,
    pub prefers_user_control_messages: Option<bool>,
    pub prefers_small_patches: Option<bool>,
    pub requires_explicit_tool_contract: Option<bool>,
    pub requires_post_tool_continue_nudge: Option<bool>,
    pub default_reasoning_effort: Option<String>,
    pub default_thinking_budget: Option<usize>,
    pub max_parallel_tools: Option<usize>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterTools {
    pub format: Option<String>,
    pub tool_choice: Option<String>,
    pub max_parallel: Option<usize>,
    pub require_structured_calls: Option<bool>,
    /// Explicit compatibility grammar. This is deliberately separate from
    /// `require_structured_calls`: repair is an opt-in execution adaptation.
    pub text_tool_repair: Option<String>,
    #[serde(default)]
    pub rename: BTreeMap<String, String>,
    #[serde(default)]
    pub arguments: BTreeMap<String, BTreeMap<String, String>>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterPrompt {
    pub profile: Option<String>,
    #[serde(default)]
    pub fragments: Vec<String>,
    pub system_role: Option<String>,
    pub control_role: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    pub malformed_tool_retry: Option<usize>,
    pub no_action_turn_limit: Option<usize>,
    pub restore_full_palette_on_missing_tool: Option<bool>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerRequirements {
    pub tool_call_parser: Option<String>,
    pub reasoning_parser: Option<String>,
    pub auto_tool_choice: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServingMetadata {
    pub tool_call_parser: Option<String>,
    pub reasoning_parser: Option<String>,
    pub auto_tool_choice: bool,
}

pub fn serving_requirement_diagnostics(
    adapter: &ResolvedModelAdapter,
    serving: &ServingMetadata,
) -> Vec<String> {
    let mut diagnostics = Vec::new();
    if let Some(required) = &adapter.server_requirements.tool_call_parser {
        if serving.tool_call_parser.as_deref() != Some(required.as_str()) {
            diagnostics.push(format!(
                "{} requires tool-call parser '{}'; configured {:?}",
                adapter.adapter_id, required, serving.tool_call_parser
            ));
        }
    }
    if let Some(required) = &adapter.server_requirements.reasoning_parser {
        if serving.reasoning_parser.as_deref() != Some(required.as_str()) {
            diagnostics.push(format!(
                "{} requires reasoning parser '{}'; configured {:?}",
                adapter.adapter_id, required, serving.reasoning_parser
            ));
        }
    }
    if adapter.server_requirements.auto_tool_choice == Some(true) && !serving.auto_tool_choice {
        diagnostics.push(format!(
            "{} requires automatic tool choice to be enabled",
            adapter.adapter_id
        ));
    }
    diagnostics
}
/// Closed, data-only request mutations allowed by model adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestTransform {
    SetRequestField {
        field: String,
        value: Option<String>,
    },
    RemoveRequestField {
        field: String,
    },
    RenameToolArgument {
        field: String,
        value: Option<String>,
    },
    SetSystemRole {
        field: String,
        value: Option<String>,
    },
    SetToolChoice {
        field: String,
        value: Option<String>,
    },
    SetMaxParallelTools {
        field: String,
        value: Option<String>,
    },
    SetThinkingParameter {
        field: String,
        value: Option<String>,
    },
    RequireLateSystemMessages {
        field: String,
    },
    RequireContinueNudge {
        field: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedModelAdapter {
    pub profile: ResolvedModelProfile,
    pub adapter_id: String,
    pub adapter_version: u32,
    pub fingerprint: String,
    pub source_layers: Vec<String>,
    pub tool_format: Option<String>,
    pub tool_choice: Option<String>,
    pub max_parallel_tools: Option<usize>,
    pub require_structured_calls: bool,
    pub text_tool_repair: Option<String>,
    pub tool_aliases: BTreeMap<String, String>,
    pub argument_aliases: BTreeMap<String, BTreeMap<String, String>>,
    pub prompt_fragments: Vec<String>,
    pub prompt_system_role: Option<String>,
    pub prompt_control_role: Option<String>,
    pub recovery: RecoveryPolicy,
    pub server_requirements: ServerRequirements,
    pub transforms: Vec<RequestTransform>,
}

static DEFINITIONS: OnceLock<Vec<AdapterDefinition>> = OnceLock::new();
fn definitions() -> &'static [AdapterDefinition] {
    DEFINITIONS.get_or_init(|| {
        BUILTIN_ADAPTER_SOURCES
            .iter()
            .map(|(id, source)| {
                let definition: AdapterDefinition =
                    toml::from_str(source).unwrap_or_else(|e| panic!("built-in adapter {id}: {e}"));
                assert_eq!(&definition.adapter.id, id);
                definition
            })
            .collect()
    })
}

fn provider_for(model: &str) -> &str {
    if let Some((provider, _)) = model.split_once('/') {
        return provider;
    }
    let id = model.to_ascii_lowercase();
    if id.contains("gpt")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
        || id.contains("codex")
    {
        "openai"
    } else if id.contains("claude")
        || id.contains("sonnet")
        || id.contains("opus")
        || id.contains("haiku")
    {
        "anthropic"
    } else if id.contains("gemini") {
        "google"
    } else if id.contains("minimax") {
        "minimax"
    } else if id.contains("qwen")
        || id.contains("qwq")
        || id.contains("deepseek")
        || id.contains("kimi")
    {
        "local"
    } else {
        model
    }
}
fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}
fn match_score(m: &AdapterMatch, provider: &str, model: &str) -> Option<u32> {
    let p = lower(provider);
    let id = lower(model);
    if !m.provider.is_empty() && !m.provider.iter().any(|x| lower(x) == p) {
        return None;
    }
    if !m.exact_model.is_empty() && !m.exact_model.iter().any(|x| lower(x) == id) {
        return None;
    }
    if let Some(x) = &m.model_prefix {
        if !id.starts_with(&lower(x)) {
            return None;
        }
    }
    if let Some(x) = &m.model_suffix {
        if !id.ends_with(&lower(x)) {
            return None;
        }
    }
    if let Some(x) = &m.model_regex {
        if !Regex::new(x).ok()?.is_match(model) {
            return None;
        }
    }
    if let Some(x) = &m.exclude_regex {
        if Regex::new(x).ok()?.is_match(model) {
            return None;
        }
    }
    Some(
        (if !m.exact_model.is_empty() { 400 } else { 0 })
            + (if !m.provider.is_empty() { 200 } else { 0 })
            + (if m.model_prefix.is_some() || m.model_suffix.is_some() {
                100
            } else {
                0
            })
            + (if m.model_regex.is_some() { 50 } else { 0 }),
    )
}

fn parse_profile(name: Option<&str>, default: PromptProfileKind) -> PromptProfileKind {
    name.and_then(|x| serde_json::from_value(serde_json::Value::String(x.to_string())).ok())
        .unwrap_or(default)
}
fn parse_reliability(name: Option<&str>, default: ReliabilityTier) -> ReliabilityTier {
    name.and_then(|x| serde_json::from_value(serde_json::Value::String(x.to_string())).ok())
        .unwrap_or(default)
}
fn effective_profile(model: &str, a: &AdapterDefinition) -> ResolvedModelProfile {
    let p = &a.profile;
    let conservative = super::resolve::default_profile(model);
    ResolvedModelProfile {
        model: model.to_string(),
        prompt_profile: parse_profile(
            p.prompt_profile.as_deref().or(a.prompt.profile.as_deref()),
            conservative.prompt_profile,
        ),
        family: p.family.clone().unwrap_or(conservative.family),
        context_window: p.context_window.or(conservative.context_window),
        max_output_tokens: p.max_output_tokens.or(conservative.max_output_tokens),
        tool_call_reliability: parse_reliability(
            p.tool_call_reliability.as_deref(),
            conservative.tool_call_reliability,
        ),
        instruction_adherence: parse_reliability(
            p.instruction_adherence.as_deref(),
            conservative.instruction_adherence,
        ),
        patch_reliability: parse_reliability(
            p.patch_reliability.as_deref(),
            conservative.patch_reliability,
        ),
        supports_late_system_messages: p
            .supports_late_system_messages
            .unwrap_or(conservative.supports_late_system_messages),
        prefers_user_control_messages: p
            .prefers_user_control_messages
            .unwrap_or(conservative.prefers_user_control_messages),
        prefers_small_patches: p
            .prefers_small_patches
            .unwrap_or(conservative.prefers_small_patches),
        requires_explicit_tool_contract: p
            .requires_explicit_tool_contract
            .unwrap_or(conservative.requires_explicit_tool_contract),
        requires_post_tool_continue_nudge: p
            .requires_post_tool_continue_nudge
            .unwrap_or(conservative.requires_post_tool_continue_nudge),
        default_reasoning_effort: p
            .default_reasoning_effort
            .clone()
            .or(conservative.default_reasoning_effort),
        default_thinking_budget: p
            .default_thinking_budget
            .or(conservative.default_thinking_budget),
        max_parallel_tools: p
            .max_parallel_tools
            .or(a.tools.max_parallel)
            .or(conservative.max_parallel_tools),
        preferred_tools: conservative.preferred_tools,
        disabled_tools: conservative.disabled_tools,
        task_state_policy: if matches!(
            parse_profile(p.prompt_profile.as_deref(), PromptProfileKind::Default),
            PromptProfileKind::FastExecutor | PromptProfileKind::LocalStrict
        ) {
            TaskStatePolicy::guided_current_task()
        } else {
            conservative.task_state_policy
        },
    }
}

fn merge_adapter(base: &AdapterDefinition, overlay: &AdapterDefinition) -> AdapterDefinition {
    let mut merged = overlay.clone();
    macro_rules! inherit {
        ($field:ident) => {
            if merged.profile.$field.is_none() {
                merged.profile.$field = base.profile.$field.clone();
            }
        };
    }
    inherit!(prompt_profile);
    inherit!(family);
    inherit!(context_window);
    inherit!(max_output_tokens);
    inherit!(tool_call_reliability);
    inherit!(instruction_adherence);
    inherit!(patch_reliability);
    inherit!(supports_late_system_messages);
    inherit!(prefers_user_control_messages);
    inherit!(prefers_small_patches);
    inherit!(requires_explicit_tool_contract);
    inherit!(requires_post_tool_continue_nudge);
    inherit!(default_reasoning_effort);
    inherit!(default_thinking_budget);
    inherit!(max_parallel_tools);
    if merged.tools.format.is_none() {
        merged.tools.format = base.tools.format.clone();
    }
    if merged.tools.tool_choice.is_none() {
        merged.tools.tool_choice = base.tools.tool_choice.clone();
    }
    if merged.tools.max_parallel.is_none() {
        merged.tools.max_parallel = base.tools.max_parallel;
    }
    if merged.tools.require_structured_calls.is_none() {
        merged.tools.require_structured_calls = base.tools.require_structured_calls;
    }
    if merged.tools.text_tool_repair.is_none() {
        merged.tools.text_tool_repair = base.tools.text_tool_repair.clone();
    }
    for (k, v) in &base.tools.rename {
        merged
            .tools
            .rename
            .entry(k.clone())
            .or_insert_with(|| v.clone());
    }
    for (tool, args) in &base.tools.arguments {
        merged
            .tools
            .arguments
            .entry(tool.clone())
            .or_insert_with(|| args.clone());
    }
    if merged.prompt.profile.is_none() {
        merged.prompt.profile = base.prompt.profile.clone();
    }
    if merged.prompt.fragments.is_empty() {
        merged.prompt.fragments = base.prompt.fragments.clone();
    }
    if merged.prompt.system_role.is_none() {
        merged.prompt.system_role = base.prompt.system_role.clone();
    }
    if merged.prompt.control_role.is_none() {
        merged.prompt.control_role = base.prompt.control_role.clone();
    }
    if merged.recovery.malformed_tool_retry.is_none() {
        merged.recovery.malformed_tool_retry = base.recovery.malformed_tool_retry;
    }
    if merged.recovery.no_action_turn_limit.is_none() {
        merged.recovery.no_action_turn_limit = base.recovery.no_action_turn_limit;
    }
    if merged
        .recovery
        .restore_full_palette_on_missing_tool
        .is_none()
    {
        merged.recovery.restore_full_palette_on_missing_tool =
            base.recovery.restore_full_palette_on_missing_tool;
    }
    if merged.server_requirements.tool_call_parser.is_none() {
        merged.server_requirements.tool_call_parser =
            base.server_requirements.tool_call_parser.clone();
    }
    if merged.server_requirements.reasoning_parser.is_none() {
        merged.server_requirements.reasoning_parser =
            base.server_requirements.reasoning_parser.clone();
    }
    if merged.server_requirements.auto_tool_choice.is_none() {
        merged.server_requirements.auto_tool_choice = base.server_requirements.auto_tool_choice;
    }
    if merged.transforms.is_empty() {
        merged.transforms = base.transforms.clone();
    }
    merged
}

pub fn resolve_adapter(provider: Option<&str>, model: &str) -> ResolvedModelAdapter {
    let provider = provider.unwrap_or_else(|| provider_for(model));
    let Some(generic) = definitions().iter().find(|x| x.adapter.id == "generic") else {
        tracing::error!("generic model adapter is missing; using conservative fallback");
        return fallback_resolved_adapter(model);
    };
    let mut candidates = Vec::new();
    for a in definitions() {
        for m in &a.r#match {
            if let Some(score) = match_score(m, provider, model) {
                candidates.push((score, a.adapter.priority, a));
            }
        }
    }
    candidates.sort_by(|a, b| {
        (b.0, b.1, b.2.adapter.id.as_str()).cmp(&(a.0, a.1, a.2.adapter.id.as_str()))
    });
    let selected = candidates.first().map(|x| x.2).unwrap_or(generic);
    let merged;
    let a = if selected.adapter.id == generic.adapter.id {
        selected
    } else {
        merged = merge_adapter(generic, selected);
        &merged
    };
    let profile = effective_profile(model, a);
    let canonical = toml::to_string(a).unwrap_or_else(|error| {
        tracing::warn!(error = %error, adapter = %a.adapter.id, "model adapter serialization failed");
        format!("adapter:{}:v{}", a.adapter.id, a.adapter.version)
    });
    let fingerprint = hex_sha256(&canonical);
    ResolvedModelAdapter {
        profile,
        adapter_id: a.adapter.id.clone(),
        adapter_version: a.adapter.version,
        fingerprint,
        source_layers: if a.adapter.id == "generic" {
            vec!["builtin:generic".to_string()]
        } else {
            vec![
                "builtin:generic".to_string(),
                format!("builtin:{}", a.adapter.id),
            ]
        },
        tool_format: a.tools.format.clone(),
        tool_choice: a.tools.tool_choice.clone(),
        max_parallel_tools: a.tools.max_parallel.or(a.profile.max_parallel_tools),
        require_structured_calls: a.tools.require_structured_calls.unwrap_or(false),
        text_tool_repair: a.tools.text_tool_repair.clone(),
        tool_aliases: a.tools.rename.clone(),
        argument_aliases: a.tools.arguments.clone(),
        prompt_fragments: a.prompt.fragments.clone(),
        prompt_system_role: a.prompt.system_role.clone(),
        prompt_control_role: a.prompt.control_role.clone(),
        recovery: a.recovery.clone(),
        server_requirements: a.server_requirements.clone(),
        transforms: a.transforms.clone(),
    }
}

fn fallback_resolved_adapter(model: &str) -> ResolvedModelAdapter {
    ResolvedModelAdapter {
        profile: super::resolve::default_profile(model),
        adapter_id: "generic".to_string(),
        adapter_version: 0,
        fingerprint: hex_sha256("fallback:generic"),
        source_layers: vec!["fallback:generic".to_string()],
        tool_format: None,
        tool_choice: None,
        max_parallel_tools: None,
        require_structured_calls: false,
        text_tool_repair: None,
        tool_aliases: BTreeMap::new(),
        argument_aliases: BTreeMap::new(),
        prompt_fragments: Vec::new(),
        prompt_system_role: None,
        prompt_control_role: None,
        recovery: RecoveryPolicy::default(),
        server_requirements: ServerRequirements::default(),
        transforms: Vec::new(),
    }
}
fn hex_sha256(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimax_is_declarative() {
        let a = resolve_adapter(None, "minimax/minimax-2.7");
        assert_eq!(a.adapter_id, "minimax-fast-executor");
        assert_eq!(a.profile.prompt_profile, PromptProfileKind::FastExecutor);
        assert_eq!(
            a.tool_aliases.get("bash").map(String::as_str),
            Some("shell")
        );
        assert_eq!(a.argument_aliases["shell"]["command"], "cmd");
    }
    #[test]
    fn unknown_is_conservative_and_stable() {
        let a = resolve_adapter(None, "unknown/thing");
        assert_eq!(a.adapter_id, "generic");
        assert_eq!(a, resolve_adapter(None, "unknown/thing"));
        assert!(!a.fingerprint.is_empty());
        assert!(a.text_tool_repair.is_none());
    }

    #[test]
    fn textual_repair_requires_explicit_model_profile_override() {
        let config = codegg_config::schema::Config {
            model_profile: Some(std::collections::HashMap::from([(
                "fixture/hermes".to_string(),
                codegg_config::schema::ModelProfileConfig {
                    text_tool_repair: Some("hermes_xml".to_string()),
                    ..Default::default()
                },
            )])),
            ..Default::default()
        };
        let adapter = crate::model_profile::ModelProfileResolver::new(&config)
            .resolve_adapter(None, "fixture/hermes");
        assert_eq!(adapter.text_tool_repair.as_deref(), Some("hermes_xml"));
    }
    #[test]
    fn explicit_provider_controls_match() {
        let a = resolve_adapter(Some("anthropic"), "claude-sonnet");
        assert_eq!(a.adapter_id, "anthropic-frontier");
    }

    #[test]
    fn laguna_adapter_preserves_reasoning_and_reports_server_requirements() {
        let a = resolve_adapter(Some("local"), "poolside/Laguna-M.1");
        assert_eq!(a.adapter_id, "poolside-laguna-agentic");
        assert_eq!(a.tool_aliases["bash"], "shell");
        assert_eq!(a.argument_aliases["shell"]["command"], "cmd");
        assert_eq!(a.max_parallel_tools, Some(1));
        let diagnostics = serving_requirement_diagnostics(
            &a,
            &ServingMetadata {
                tool_call_parser: Some("wrong".into()),
                reasoning_parser: None,
                auto_tool_choice: false,
            },
        );
        assert_eq!(diagnostics.len(), 3);
        assert!(diagnostics.iter().any(|d| d.contains("reasoning parser")));
        assert!(matches!(
            a.transforms.first(),
            Some(RequestTransform::SetRequestField { field, .. }) if field == "reasoning_content"
        ));
        assert!(matches!(
            a.transforms.get(1),
            Some(RequestTransform::SetThinkingParameter { field, .. }) if field == "enable_thinking"
        ));
    }

    #[test]
    fn laguna_base_models_are_not_matched() {
        let a = resolve_adapter(Some("local"), "poolside/Laguna-base");
        assert_ne!(a.adapter_id, "poolside-laguna-agentic");
    }
}
