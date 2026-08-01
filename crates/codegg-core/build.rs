#![allow(dead_code)]

use regex::Regex;
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterFile {
    schema_version: u32,
    adapter: AdapterMeta,
    r#match: Vec<Match>,
    #[serde(default)]
    profile: Profile,
    #[serde(default)]
    tools: Tools,
    #[serde(default)]
    prompt: Prompt,
    #[serde(default)]
    recovery: Recovery,
    #[serde(default)]
    server_requirements: ServerRequirements,
    #[serde(default)]
    transforms: Vec<Transform>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdapterMeta {
    id: String,
    version: u32,
    priority: i32,
    description: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Match {
    #[serde(default)]
    provider: Vec<String>,
    #[serde(default)]
    exact_model: Vec<String>,
    model_prefix: Option<String>,
    model_suffix: Option<String>,
    model_regex: Option<String>,
    exclude_regex: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    prompt_profile: Option<String>,
    family: Option<String>,
    context_window: Option<usize>,
    max_output_tokens: Option<usize>,
    tool_call_reliability: Option<String>,
    instruction_adherence: Option<String>,
    patch_reliability: Option<String>,
    supports_late_system_messages: Option<bool>,
    prefers_user_control_messages: Option<bool>,
    prefers_small_patches: Option<bool>,
    requires_explicit_tool_contract: Option<bool>,
    requires_post_tool_continue_nudge: Option<bool>,
    default_reasoning_effort: Option<String>,
    default_thinking_budget: Option<usize>,
    max_parallel_tools: Option<usize>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tools {
    format: Option<String>,
    tool_choice: Option<String>,
    max_parallel: Option<usize>,
    require_structured_calls: Option<bool>,
    #[serde(default)]
    rename: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    arguments: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prompt {
    profile: Option<String>,
    fragments: Vec<String>,
    system_role: Option<String>,
    control_role: Option<String>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Recovery {
    malformed_tool_retry: Option<usize>,
    no_action_turn_limit: Option<usize>,
    restore_full_palette_on_missing_tool: Option<bool>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerRequirements {
    tool_call_parser: Option<String>,
    reasoning_parser: Option<String>,
    auto_tool_choice: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Transform {
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

impl Transform {
    fn field(&self) -> &str {
        match self {
            Self::SetRequestField { field, .. }
            | Self::RemoveRequestField { field }
            | Self::RenameToolArgument { field, .. }
            | Self::SetSystemRole { field, .. }
            | Self::SetToolChoice { field, .. }
            | Self::SetMaxParallelTools { field, .. }
            | Self::SetThinkingParameter { field, .. }
            | Self::RequireLateSystemMessages { field }
            | Self::RequireContinueNudge { field } => field,
        }
    }
}

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let dir = manifest.join("assets/model-adapters");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no model adapter TOML files found in {}",
        dir.display()
    );
    let mut ids = std::collections::HashSet::new();
    let mut match_keys = std::collections::HashSet::new();
    let mut generated = String::from("pub static BUILTIN_ADAPTER_SOURCES: &[(&str, &str)] = &[\n");
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let parsed: AdapterFile =
            toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(
            parsed.schema_version,
            1,
            "{}: unsupported schema_version",
            path.display()
        );
        assert!(
            !parsed.adapter.id.is_empty() && parsed.adapter.id.len() <= 96,
            "{}: invalid adapter.id",
            path.display()
        );
        assert!(
            ids.insert(parsed.adapter.id.clone()),
            "{}: duplicate adapter id {}",
            path.display(),
            parsed.adapter.id
        );
        assert!(
            parsed.adapter.version > 0 && parsed.adapter.version <= 1000,
            "{}: invalid adapter.version",
            path.display()
        );
        assert!(
            parsed.adapter.description.len() <= 512,
            "{}: description too long",
            path.display()
        );
        assert!(
            !parsed.r#match.is_empty(),
            "{}: at least one match is required",
            path.display()
        );
        for m in &parsed.r#match {
            assert!(
                m.provider.len() + m.exact_model.len() <= 32,
                "{}: too many match keys",
                path.display()
            );
            if let Some(re) = &m.model_regex {
                Regex::new(re).unwrap_or_else(|e| panic!("{}: model_regex: {e}", path.display()));
                assert!(re.len() <= 512, "{}: regex too long", path.display());
            }
            if let Some(re) = &m.exclude_regex {
                Regex::new(re).unwrap_or_else(|e| panic!("{}: exclude_regex: {e}", path.display()));
                assert!(re.len() <= 512, "{}: regex too long", path.display());
            }
            let key = format!(
                "{:?}:{:?}:{:?}:{:?}:{:?}:{}",
                m.provider,
                m.exact_model,
                m.model_prefix,
                m.model_suffix,
                m.model_regex,
                parsed.adapter.priority
            );
            assert!(
                match_keys.insert(key),
                "{}: ambiguous duplicate match",
                path.display()
            );
        }
        assert!(
            parsed
                .profile
                .context_window
                .map_or(true, |x| x <= 10_000_000),
            "{}: context_window out of bounds",
            path.display()
        );
        assert!(
            parsed
                .profile
                .max_output_tokens
                .map_or(true, |x| x <= 1_000_000),
            "{}: max_output_tokens out of bounds",
            path.display()
        );
        assert!(
            parsed
                .tools
                .max_parallel
                .map_or(true, |x| (1..=64).contains(&x)),
            "{}: max_parallel out of bounds",
            path.display()
        );
        let mut transform_keys = std::collections::HashSet::new();
        for t in &parsed.transforms {
            let field = t.field();
            assert!(
                !field.is_empty() && field.len() <= 128,
                "{}: transform field required/bounded",
                path.display()
            );
            assert!(
                !field.contains('.') && !field.contains('[') && !field.contains(']'),
                "{}: transform field must be a safe top-level name",
                path.display()
            );
            assert!(
                !matches!(
                    field.to_ascii_lowercase().as_str(),
                    "authorization" | "headers" | "endpoint" | "url" | "tools" | "permissions"
                ),
                "{}: transform field cannot alter authority or transport",
                path.display()
            );
            let key = format!("{:?}:{field}", std::mem::discriminant(t));
            assert!(
                transform_keys.insert(key),
                "{}: duplicate/conflicting transform",
                path.display()
            );
            if matches!(t, Transform::SetRequestField { .. }) {
                assert!(
                    field == "reasoning_content",
                    "{}: unsupported private request field",
                    path.display()
                );
            }
            if matches!(t, Transform::SetThinkingParameter { .. }) {
                assert!(
                    field == "enable_thinking",
                    "{}: unsupported thinking field",
                    path.display()
                );
            }
        }
        let mut wire_names = std::collections::HashSet::new();
        for (canonical, wire) in &parsed.tools.rename {
            assert!(
                !canonical.is_empty() && !wire.is_empty(),
                "{}: empty tool alias",
                path.display()
            );
            assert!(
                wire_names.insert(wire),
                "{}: tool aliases are not reversible",
                path.display()
            );
        }
        for args in parsed.tools.arguments.values() {
            let mut names = std::collections::HashSet::new();
            for (canonical, wire) in args {
                assert!(
                    !canonical.is_empty() && !wire.is_empty(),
                    "{}: empty argument alias",
                    path.display()
                );
                assert!(
                    names.insert(wire),
                    "{}: argument aliases are not reversible",
                    path.display()
                );
            }
        }
        generated.push_str(&format!("    ({:?}, {:?}),\n", parsed.adapter.id, text));
    }
    generated.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("model_adapters.rs"),
        generated,
    )
    .unwrap();
}
