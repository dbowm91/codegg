//! Permission checking and access control.
//!
//! This module provides permission enforcement for tools and file paths.
//! Permissions can be configured per-agent and are checked before tool execution.
//! The module includes HMAC-based persistence to prevent cache poisoning.

use globset::Glob;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub mod modes;

use crate::config::schema::{AgentConfig, Config, PermissionRule};
use crate::error::PermissionError;

const PERMISSION_SIGNATURE_KEY: &str = "CODEGG_PERM_KEY";
const PATH_CANONICALIZE_CACHE_TTL_SECS: u64 = 1;
const PATH_CANONICALIZE_NOT_FOUND_TTL_SECS: u64 = 1;

fn get_signature_key() -> Option<[u8; 32]> {
    std::env::var(PERMISSION_SIGNATURE_KEY).ok().map(|k| {
        let key = k.as_bytes();
        if key.len() >= 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&key[..32]);
            arr
        } else {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(key);
            hasher.finalize().into()
        }
    })
}

fn compute_signature(
    tool: &str,
    path: Option<&str>,
    level: &PermissionLevel,
    timestamp: i64,
    key: &[u8; 32],
) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(tool.as_bytes());
    if let Some(p) = path {
        mac.update(p.as_bytes());
    }
    mac.update(level.as_str().as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn verify_signature(decision: &PersistentDecision, key: &[u8; 32]) -> bool {
    let expected = compute_signature(
        &decision.tool,
        decision.path.as_deref(),
        &decision.level,
        decision.created_at,
        key,
    );
    expected == decision.signature
}

pub const PERMISSION_TYPES: &[&str] = &[
    "read",
    "edit",
    "glob",
    "grep",
    "list",
    "bash",
    "git",
    "task",
    "todowrite",
    "question",
    "webfetch",
    "websearch",
    "codesearch",
    "lsp",
    "doom_loop",
    "skill",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    Allow,
    Deny,
    Ask,
}

impl PermissionLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionLevel::Allow => "allow",
            PermissionLevel::Deny => "deny",
            PermissionLevel::Ask => "ask",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResult {
    Allow,
    Deny,
    Ask(PermissionRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub tool: String,
    pub path: Option<String>,
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
struct CanonicalizedToolRule {
    tool: String,
    level: PermissionLevel,
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}

impl PermissionChoice {
    pub fn allowed(&self) -> bool {
        matches!(
            self,
            PermissionChoice::AllowOnce | PermissionChoice::AlwaysAllow
        )
    }

    pub fn persist(&self) -> bool {
        matches!(
            self,
            PermissionChoice::AlwaysAllow | PermissionChoice::AlwaysDeny
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRule {
    pub tool: String,
    pub level: PermissionLevel,
    pub paths: Option<Vec<String>>,
    pub bash_patterns: Option<Vec<String>>,
}

impl ToolRule {
    pub fn matches(&self, tool_name: &str) -> bool {
        if self.tool == "*" {
            return true;
        }
        if self.tool == tool_name {
            return true;
        }
        if let Ok(glob) = Glob::new(&self.tool) {
            let matcher = glob.compile_matcher();
            return matcher.is_match(tool_name);
        }
        false
    }

    pub fn matches_bash_command(&self, command: &str) -> bool {
        let Some(patterns) = &self.bash_patterns else {
            return true;
        };
        if patterns.is_empty() {
            return true;
        }
        for pattern in patterns {
            if pattern == "*" {
                return true;
            }
            if let Ok(glob) = Glob::new(pattern) {
                let matcher = glob.compile_matcher();
                if matcher.is_match(command) {
                    return true;
                }
            } else if pattern == command {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRule {
    pub pattern: String,
    pub level: PermissionLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRuleset {
    pub default: PermissionLevel,
    pub tool_rules: Vec<ToolRule>,
    pub path_rules: Vec<PathRule>,
}

impl Default for PermissionRuleset {
    fn default() -> Self {
        Self {
            default: PermissionLevel::Ask,
            tool_rules: Vec::new(),
            path_rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentDecision {
    pub tool: String,
    pub path: Option<String>,
    pub level: PermissionLevel,
    pub created_at: i64,
    pub signature: String,
    pub session_id: Option<String>,
}

pub struct PermissionStore {
    decisions: Vec<PersistentDecision>,
    store_path: Option<std::path::PathBuf>,
}

impl PermissionStore {
    pub fn new(store_path: Option<std::path::PathBuf>) -> Self {
        let decisions = load_decisions(store_path.as_deref());
        Self {
            decisions,
            store_path,
        }
    }

    pub fn add_decision(
        &mut self,
        tool: &str,
        path: Option<&str>,
        level: PermissionLevel,
        session_id: Option<&str>,
    ) {
        let timestamp = chrono::Utc::now().timestamp();

        let signature = if let Some(key) = get_signature_key() {
            compute_signature(tool, path, &level, timestamp, &key)
        } else {
            String::new()
        };

        let decision = PersistentDecision {
            tool: tool.to_string(),
            path: path.map(|p| p.to_string()),
            level,
            created_at: timestamp,
            signature,
            session_id: session_id.map(|s| s.to_string()),
        };
        self.decisions.retain(|d| {
            !(d.tool == decision.tool
                && d.path == decision.path
                && d.session_id == decision.session_id)
        });
        self.decisions.push(decision);
        self.save();
    }

    pub fn get_decision(
        &self,
        tool: &str,
        path: Option<&str>,
        session_id: Option<&str>,
    ) -> Option<PermissionLevel> {
        let key = get_signature_key();

        if let Some(sid) = session_id {
            if let Some(k) = key {
                if let Some(level) = self.find_decision(tool, path, sid, &k) {
                    return Some(level);
                }
            } else {
                if let Some(level) = self.find_decision_no_sig(tool, path, sid) {
                    return Some(level);
                }
            }
        }

        self.decisions.iter().rev().find_map(|d| {
            if d.tool == tool && d.path.as_deref() == path && d.session_id.is_none() {
                if let Some(ref k) = key {
                    if d.signature.is_empty() {
                        return None;
                    }
                    if !verify_signature(d, k) {
                        return None;
                    }
                } else if !d.signature.is_empty() {
                    tracing::warn!(
                        "permission decision accepted with signature but no verification key configured for tool '{}' path {:?}",
                        tool, path
                    );
                    return None;
                }
                Some(d.level.clone())
            } else {
                None
            }
        })
    }

    fn find_decision(
        &self,
        tool: &str,
        path: Option<&str>,
        session_id: &str,
        key: &[u8; 32],
    ) -> Option<PermissionLevel> {
        self.decisions.iter().rev().find_map(|d| {
            if d.tool == tool
                && d.path.as_deref() == path
                && d.session_id.as_deref() == Some(session_id)
            {
                if d.signature.is_empty() {
                    return None;
                }
                if !verify_signature(d, key) {
                    return None;
                }
                Some(d.level.clone())
            } else {
                None
            }
        })
    }

    fn find_decision_no_sig(
        &self,
        tool: &str,
        path: Option<&str>,
        session_id: &str,
    ) -> Option<PermissionLevel> {
        self.decisions.iter().rev().find_map(|d| {
            if d.tool == tool
                && d.path.as_deref() == path
                && d.session_id.as_deref() == Some(session_id)
                && d.signature.is_empty()
            {
                Some(d.level.clone())
            } else {
                None
            }
        })
    }

    pub fn clear(&mut self) {
        self.decisions.clear();
        self.save();
    }

    fn save(&self) {
        if let Some(ref path) = self.store_path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&self.decisions) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}

fn load_decisions(path: Option<&std::path::Path>) -> Vec<PersistentDecision> {
    if let Some(path) = path {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(decisions) = serde_json::from_str(&content) {
                return decisions;
            }
        }
    }
    Vec::new()
}

fn canonicalize_tool_rules(rules: &[ToolRule]) -> Vec<CanonicalizedToolRule> {
    rules
        .iter()
        .map(|r| {
            let paths = r
                .paths
                .as_ref()
                .map(|p| {
                    p.iter()
                        .filter_map(|path_str| Path::new(path_str).canonicalize().ok())
                        .collect()
                })
                .unwrap_or_default();
            CanonicalizedToolRule {
                tool: r.tool.clone(),
                level: r.level.clone(),
                paths,
            }
        })
        .collect()
}

pub struct PermissionChecker {
    config_rules: PermissionRuleset,
    session_rules: PermissionRuleset,
    agent_rules: PermissionRuleset,
    store: Arc<RwLock<PermissionStore>>,
    compiled_globs: Vec<(globset::GlobMatcher, PermissionLevel)>,
    canonicalized_config_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_session_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_agent_tool_rules: Vec<CanonicalizedToolRule>,
    path_cache: Arc<RwLock<HashMap<String, (PathBuf, Instant)>>>,
}

impl PermissionChecker {
    pub fn new(config: Option<&Config>, store_path: Option<std::path::PathBuf>) -> Self {
        let config_rules = config_ruleset(config);
        let store = PermissionStore::new(store_path);
        let compiled_globs = compile_path_rules(&config_rules.path_rules);
        let canonicalized_config_tool_rules = canonicalize_tool_rules(&config_rules.tool_rules);

        Self {
            config_rules,
            session_rules: PermissionRuleset::default(),
            agent_rules: PermissionRuleset::default(),
            store: Arc::new(RwLock::new(store)),
            compiled_globs,
            canonicalized_config_tool_rules,
            canonicalized_session_tool_rules: Vec::new(),
            canonicalized_agent_tool_rules: Vec::new(),
            path_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_session_rules(mut self, rules: PermissionRuleset) -> Self {
        self.session_rules = rules;
        self.compiled_globs = compile_path_rules(&self.effective_path_rules());
        self.canonicalized_session_tool_rules =
            canonicalize_tool_rules(&self.session_rules.tool_rules);
        self
    }

    pub fn with_agent_rules(mut self, rules: PermissionRuleset) -> Self {
        self.agent_rules = rules;
        self.compiled_globs = compile_path_rules(&self.effective_path_rules());
        self.canonicalized_agent_tool_rules = canonicalize_tool_rules(&self.agent_rules.tool_rules);
        self
    }

    pub async fn check_legacy(&self, tool: &str, path: Option<&str>) -> PermissionResult {
        self.check(tool, path, None).await
    }

    pub async fn check(
        &self,
        tool: &str,
        path: Option<&str>,
        session_id: Option<&str>,
    ) -> PermissionResult {
        {
            let store = self.store.read().await;
            if let Some(level) = store.get_decision(tool, path, session_id) {
                return match level {
                    PermissionLevel::Allow => PermissionResult::Allow,
                    PermissionLevel::Deny => PermissionResult::Deny,
                    PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                        tool: tool.to_string(),
                        path: path.map(|p| p.to_string()),
                        args: None,
                    }),
                };
            }
        }

        let merged_default = self.effective_default();
        let tool_level = self.effective_tool_rule(tool, path).await;

        if let Some(level) = tool_level {
            return match level {
                PermissionLevel::Allow => PermissionResult::Allow,
                PermissionLevel::Deny => PermissionResult::Deny,
                PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                    tool: tool.to_string(),
                    path: path.map(|p| p.to_string()),
                    args: None,
                }),
            };
        }

        if let Some(p) = path {
            let canonical = match self.canonicalize_path(p).await {
                Ok(c) => c,
                Err(_) => {
                    return match merged_default {
                        PermissionLevel::Allow => PermissionResult::Allow,
                        PermissionLevel::Deny => PermissionResult::Deny,
                        PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                            tool: tool.to_string(),
                            path: Some(p.to_string()),
                            args: None,
                        }),
                    };
                }
            };
            let canonical_str = canonical.to_string_lossy();

            for (matcher, level) in &self.compiled_globs {
                if matcher.is_match(canonical_str.as_ref()) {
                    return match level {
                        PermissionLevel::Allow => PermissionResult::Allow,
                        PermissionLevel::Deny => PermissionResult::Deny,
                        PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                            tool: tool.to_string(),
                            path: Some(p.to_string()),
                            args: None,
                        }),
                    };
                }
            }
        }

        match merged_default {
            PermissionLevel::Allow => PermissionResult::Allow,
            PermissionLevel::Deny => PermissionResult::Deny,
            PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                tool: tool.to_string(),
                path: path.map(|p| p.to_string()),
                args: None,
            }),
        }
    }

    pub async fn check_bash(
        &self,
        path: Option<&str>,
        command: Option<&str>,
        session_id: Option<&str>,
    ) -> PermissionResult {
        self.check_with_args("bash", path, command, session_id)
            .await
    }

    pub async fn check_bash_legacy(
        &self,
        path: Option<&str>,
        command: Option<&str>,
    ) -> PermissionResult {
        self.check_with_args("bash", path, command, None).await
    }

    pub async fn check_git(
        &self,
        path: Option<&str>,
        subcommand: Option<&str>,
        session_id: Option<&str>,
    ) -> PermissionResult {
        self.check_with_args("git", path, subcommand, session_id)
            .await
    }

    pub async fn check_with_args(
        &self,
        tool: &str,
        path: Option<&str>,
        args: Option<&str>,
        session_id: Option<&str>,
    ) -> PermissionResult {
        {
            let store = self.store.read().await;
            if let Some(level) = store.get_decision(tool, path, session_id) {
                return match level {
                    PermissionLevel::Allow => PermissionResult::Allow,
                    PermissionLevel::Deny => PermissionResult::Deny,
                    PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                        tool: tool.to_string(),
                        path: path.map(|p| p.to_string()),
                        args: args.map(|a| serde_json::json!({ "command": a })),
                    }),
                };
            }
        }

        let merged_default = self.effective_default();
        let tool_level = self.effective_tool_rule_with_args(tool, path, args).await;

        if let Some(level) = tool_level {
            return match level {
                PermissionLevel::Allow => PermissionResult::Allow,
                PermissionLevel::Deny => PermissionResult::Deny,
                PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                    tool: tool.to_string(),
                    path: path.map(|p| p.to_string()),
                    args: args.map(|a| serde_json::json!({ "command": a })),
                }),
            };
        }

        if let Some(p) = path {
            let canonical = match self.canonicalize_path(p).await {
                Ok(c) => c,
                Err(_) => {
                    return match merged_default {
                        PermissionLevel::Allow => PermissionResult::Allow,
                        PermissionLevel::Deny => PermissionResult::Deny,
                        PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                            tool: tool.to_string(),
                            path: Some(p.to_string()),
                            args: args.map(|a| serde_json::json!({ "command": a })),
                        }),
                    };
                }
            };
            let canonical_str = canonical.to_string_lossy();

            for (matcher, level) in &self.compiled_globs {
                if matcher.is_match(canonical_str.as_ref()) {
                    return match level {
                        PermissionLevel::Allow => PermissionResult::Allow,
                        PermissionLevel::Deny => PermissionResult::Deny,
                        PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                            tool: tool.to_string(),
                            path: Some(p.to_string()),
                            args: args.map(|a| serde_json::json!({ "command": a })),
                        }),
                    };
                }
            }
        }

        match merged_default {
            PermissionLevel::Allow => PermissionResult::Allow,
            PermissionLevel::Deny => PermissionResult::Deny,
            PermissionLevel::Ask => PermissionResult::Ask(PermissionRequest {
                tool: tool.to_string(),
                path: path.map(|p| p.to_string()),
                args: args.map(|a| serde_json::json!({ "command": a })),
            }),
        }
    }

    pub async fn always_allow(&self, tool: &str, path: Option<&str>, session_id: Option<&str>) {
        self.store
            .write()
            .await
            .add_decision(tool, path, PermissionLevel::Allow, session_id);
    }

    pub async fn always_allow_legacy(&self, tool: &str, path: Option<&str>) {
        self.always_allow(tool, path, None).await;
    }

    pub async fn always_deny(&self, tool: &str, path: Option<&str>, session_id: Option<&str>) {
        self.store
            .write()
            .await
            .add_decision(tool, path, PermissionLevel::Deny, session_id);
    }

    pub async fn always_deny_legacy(&self, tool: &str, path: Option<&str>) {
        self.always_deny(tool, path, None).await;
    }

    pub async fn clear_decisions(&self) {
        self.store.write().await.clear();
    }

    async fn canonicalize_path(&self, path: &str) -> Result<PathBuf, PermissionError> {
        let now = Instant::now();
        let ttl = Duration::from_secs(PATH_CANONICALIZE_CACHE_TTL_SECS);
        let not_found_ttl = Duration::from_secs(PATH_CANONICALIZE_NOT_FOUND_TTL_SECS);

        {
            let cache = self.path_cache.read().await;
            if let Some((canonical, cached_at)) = cache.get(path) {
                let effective_ttl = if canonical.as_os_str().is_empty() {
                    not_found_ttl
                } else {
                    ttl
                };
                if now.duration_since(*cached_at) < effective_ttl {
                    return Ok(canonical.clone());
                }
            }
        }

        let canonical = tokio::task::spawn_blocking({
            let path = path.to_owned();
            move || {
                Path::new(&path).canonicalize().map_err(|e| {
                    PermissionError::Check(format!("path does not exist: {}: {}", path, e))
                })
            }
        })
        .await
        .unwrap_or_else(|e| {
            Err(PermissionError::Check(format!(
                "path canonicalization failed: {}",
                e
            )))
        })?;

        {
            let mut cache = self.path_cache.write().await;
            let cache_value = if canonical.as_os_str().is_empty() {
                (PathBuf::new(), now)
            } else {
                (canonical.clone(), now)
            };
            cache.insert(path.to_owned(), cache_value);
        }

        Ok(canonical)
    }

    fn effective_default(&self) -> &PermissionLevel {
        if self.agent_rules.default != PermissionLevel::Ask {
            return &self.agent_rules.default;
        }
        if self.session_rules.default != PermissionLevel::Ask {
            return &self.session_rules.default;
        }
        &self.config_rules.default
    }

    /// Returns the effective permission level for a tool by checking rules in priority order:
    /// agent_rules > session_rules > config_rules.
    ///
    /// If a tool-specific rule exists with a non-Ask level, it is returned.
    /// Otherwise returns None, indicating the default or path rules should be used.
    async fn effective_tool_rule(&self, tool: &str, path: Option<&str>) -> Option<PermissionLevel> {
        let canonical_path = match path {
            Some(p) => match self.canonicalize_path(p).await {
                Ok(c) => Some(c),
                Err(_) => return None,
            },
            None => None,
        };

        if let Some(level) = find_canonicalized_tool_rule(
            &self.canonicalized_agent_tool_rules,
            tool,
            canonical_path.as_deref(),
        )
        .await
        {
            return Some(level);
        }
        if let Some(level) = find_canonicalized_tool_rule(
            &self.canonicalized_session_tool_rules,
            tool,
            canonical_path.as_deref(),
        )
        .await
        {
            return Some(level);
        }
        find_canonicalized_tool_rule(
            &self.canonicalized_config_tool_rules,
            tool,
            canonical_path.as_deref(),
        )
        .await
    }

    fn effective_path_rules(&self) -> Vec<PathRule> {
        let mut rules = self.agent_rules.path_rules.clone();
        rules.extend(self.session_rules.path_rules.clone());
        rules.extend(self.config_rules.path_rules.clone());
        rules
    }

    async fn effective_tool_rule_with_args(
        &self,
        tool: &str,
        path: Option<&str>,
        args: Option<&str>,
    ) -> Option<PermissionLevel> {
        let canonical_path = match path {
            Some(p) => match self.canonicalize_path(p).await {
                Ok(c) => Some(c),
                Err(_) => return None,
            },
            None => None,
        };

        if let Some(level) = find_tool_rule_with_args(
            &self.agent_rules.tool_rules,
            tool,
            canonical_path.as_deref(),
            args,
        )
        .await
        {
            return Some(level);
        }
        if let Some(level) = find_tool_rule_with_args(
            &self.session_rules.tool_rules,
            tool,
            canonical_path.as_deref(),
            args,
        )
        .await
        {
            return Some(level);
        }
        find_tool_rule_with_args(
            &self.config_rules.tool_rules,
            tool,
            canonical_path.as_deref(),
            args,
        )
        .await
    }
}

async fn find_canonicalized_tool_rule(
    rules: &[CanonicalizedToolRule],
    tool: &str,
    canonical_path: Option<&Path>,
) -> Option<PermissionLevel> {
    let check_tool = |r: &CanonicalizedToolRule| -> bool {
        r.tool == tool
            || r.tool == "*"
            || ToolRule {
                tool: r.tool.clone(),
                level: PermissionLevel::Allow,
                paths: None,
                bash_patterns: None,
            }
            .matches(tool)
    };

    if let Some(canonical) = canonical_path {
        let canonical_str = canonical.to_string_lossy();
        rules.iter().rev().find_map(|r| {
            if check_tool(r) {
                if !r.paths.is_empty() {
                    let matches = r.paths.iter().any(|rule_path| {
                        canonical_str.starts_with(rule_path.to_string_lossy().as_ref())
                    });
                    if matches {
                        return Some(r.level.clone());
                    }
                    None
                } else {
                    Some(r.level.clone())
                }
            } else {
                None
            }
        })
    } else {
        rules.iter().rev().find_map(|r| {
            if check_tool(r) {
                Some(r.level.clone())
            } else {
                None
            }
        })
    }
}

async fn find_tool_rule_with_args(
    rules: &[ToolRule],
    tool: &str,
    canonical_path: Option<&Path>,
    args: Option<&str>,
) -> Option<PermissionLevel> {
    let canonical_str = canonical_path.map(|p| p.to_string_lossy().to_string());

    rules.iter().rev().find_map(|r| {
        if !r.matches(tool) {
            return None;
        }

        if let Some(ref paths) = r.paths {
            if !paths.is_empty() {
                if let Some(ref canonical) = canonical_str {
                    let matches = paths
                        .iter()
                        .any(|rule_path| canonical.starts_with(rule_path));
                    if !matches {
                        return None;
                    }
                } else {
                    return None;
                }
            }
        }

        if let Some(command) = args {
            if !r.matches_bash_command(command) {
                return None;
            }
        }

        Some(r.level.clone())
    })
}

fn compile_path_rules(rules: &[PathRule]) -> Vec<(globset::GlobMatcher, PermissionLevel)> {
    rules
        .iter()
        .filter_map(|rule| {
            Glob::new(&rule.pattern)
                .ok()
                .map(|g| g.compile_matcher())
                .map(|m| (m, rule.level.clone()))
        })
        .collect()
}

pub fn config_ruleset(config: Option<&Config>) -> PermissionRuleset {
    let Some(config) = config else {
        return PermissionRuleset::default();
    };
    let Some(perm) = &config.permission else {
        return default_ruleset();
    };

    let default = perm
        .default
        .as_deref()
        .map(parse_level)
        .unwrap_or(PermissionLevel::Ask);

    let mut tool_rules = Vec::new();
    let mut path_rules = Vec::new();

    let tool_mappings = [
        ("read", &perm.read),
        ("edit", &perm.edit),
        ("glob", &perm.glob),
        ("grep", &perm.grep),
        ("list", &perm.list),
        ("bash", &perm.bash),
        ("task", &perm.task),
        ("lsp", &perm.lsp),
        ("skill", &perm.skill),
    ];

    for (tool, rule) in tool_mappings {
        if let Some(rule) = rule {
            let level = match rule {
                PermissionRule::Action(s) => parse_level(s),
                PermissionRule::Object(obj) => {
                    if let Some(level) = obj.get("default").or_else(|| obj.get("action")) {
                        parse_level(level)
                    } else {
                        PermissionLevel::Ask
                    }
                }
            };
            tool_rules.push(ToolRule {
                tool: tool.to_string(),
                level,
                paths: None,
                bash_patterns: None,
            });
        }
    }

    let simple_tools = [
        ("todowrite", &perm.todowrite),
        ("question", &perm.question),
        ("webfetch", &perm.webfetch),
        ("websearch", &perm.websearch),
        ("codesearch", &perm.codesearch),
        ("doom_loop", &perm.doom_loop),
    ];

    for (tool, level) in simple_tools {
        if let Some(s) = level {
            tool_rules.push(ToolRule {
                tool: tool.to_string(),
                level: parse_level(s),
                paths: None,
                bash_patterns: None,
            });
        }
    }

    if let Some(tools) = &perm.tools {
        for (tool, level) in tools {
            tool_rules.push(ToolRule {
                tool: tool.clone(),
                level: parse_level(level),
                paths: None,
                bash_patterns: None,
            });
        }
    }

    if let Some(paths) = &perm.paths {
        for pattern in paths {
            path_rules.push(PathRule {
                pattern: pattern.clone(),
                level: default.clone(),
            });
        }
    }

    PermissionRuleset {
        default,
        tool_rules,
        path_rules,
    }
}

pub fn default_ruleset() -> PermissionRuleset {
    let mut tool_rules = Vec::new();

    let read_only = [
        "read",
        "glob",
        "grep",
        "list",
        "question",
        "webfetch",
        "websearch",
        "codesearch",
    ];
    for tool in read_only {
        tool_rules.push(ToolRule {
            tool: tool.to_string(),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: None,
        });
    }

    let destructive = ["edit", "bash", "task", "todowrite"];
    for tool in destructive {
        tool_rules.push(ToolRule {
            tool: tool.to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: None,
        });
    }

    let git_read_only = ["status", "log", "diff", "branch", "show", "ls-files", "cat-file", "rev-parse", "remote"];
    for subcommand in git_read_only {
        tool_rules.push(ToolRule {
            tool: "git".to_string(),
            level: PermissionLevel::Allow,
            paths: None,
            bash_patterns: Some(vec![subcommand.to_string()]),
        });
    }

    let git_write = ["add", "commit", "push", "pull", "merge", "checkout", "reset", "rebase", "stash", "branch", "tag", "clone", "fetch", "clean", "mv", "rm"];
    for subcommand in git_write {
        tool_rules.push(ToolRule {
            tool: "git".to_string(),
            level: PermissionLevel::Ask,
            paths: None,
            bash_patterns: Some(vec![subcommand.to_string()]),
        });
    }

    PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules,
        path_rules: Vec::new(),
    }
}

pub fn parse_level(s: &str) -> PermissionLevel {
    match s {
        "allow" => PermissionLevel::Allow,
        "deny" => PermissionLevel::Deny,
        "ask" => PermissionLevel::Ask,
        _ => PermissionLevel::Ask,
    }
}

pub fn agent_ruleset(agent_config: &AgentConfig) -> PermissionRuleset {
    let Some(perms) = &agent_config.permission else {
        return PermissionRuleset::default();
    };

    let mut tool_rules = Vec::new();
    let mut path_rules = Vec::new();

    for (key, rule) in perms {
        if key == "paths" {
            if let PermissionRule::Action(pattern) = rule {
                path_rules.push(PathRule {
                    pattern: pattern.clone(),
                    level: PermissionLevel::Ask,
                });
            }
            continue;
        }

        let level = match rule {
            PermissionRule::Action(s) => parse_level(s),
            PermissionRule::Object(obj) => {
                if let Some(level) = obj.get("default").or_else(|| obj.get("action")) {
                    parse_level(level)
                } else {
                    PermissionLevel::Ask
                }
            }
        };

        tool_rules.push(ToolRule {
            tool: key.clone(),
            level,
            paths: None,
            bash_patterns: None,
        });
    }

    PermissionRuleset {
        default: PermissionLevel::Ask,
        tool_rules,
        path_rules,
    }
}

pub fn merge_rulesets(a: &PermissionRuleset, b: &PermissionRuleset) -> PermissionRuleset {
    let default = if b.default != PermissionLevel::Ask {
        b.default.clone()
    } else {
        a.default.clone()
    };

    let mut tool_rules = a.tool_rules.clone();
    for rule in &b.tool_rules {
        if let Some(pos) = tool_rules
            .iter()
            .position(|r| r.tool == rule.tool || r.matches(&rule.tool))
        {
            tool_rules[pos] = rule.clone();
        } else {
            tool_rules.push(rule.clone());
        }
    }

    let mut path_rules = a.path_rules.clone();
    path_rules.extend(b.path_rules.clone());

    PermissionRuleset {
        default,
        tool_rules,
        path_rules,
    }
}

pub fn default_store_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join("codegg").join("permissions.json"))
}

/// DoomLoopDetector identifies when an agent gets stuck in a repetitive tool call loop.
/// It uses a sliding window of recent tool calls and tracks counts to detect cycles.
///
/// The detection algorithm:
/// 1. Maintains a history of recent tool calls (up to max_window)
/// 2. Uses a HashMap for O(1) count lookups instead of iterating history
/// 3. Considers it a doom loop when the most recent tool has been called threshold times
///    anywhere in the window (not necessarily consecutively)
///
/// This approach is O(1) for both recording and detection, making it efficient even with large windows.
pub struct DoomLoopDetector {
    history: VecDeque<String>,
    counts: HashMap<String, usize>,
    max_window: usize,
    threshold: usize,
}

impl DoomLoopDetector {
    pub fn new(max_window: usize, threshold: usize) -> Self {
        const MAX_WINDOW_LIMIT: usize = 1000;
        const MAX_THRESHOLD_LIMIT: usize = 100;
        const MIN_THRESHOLD: usize = 1;

        #[allow(clippy::manual_clamp)]
        let max_window = max_window.max(1).min(MAX_WINDOW_LIMIT);
        #[allow(clippy::manual_clamp)]
        let threshold = if threshold < MIN_THRESHOLD {
            MIN_THRESHOLD
        } else if threshold > MAX_THRESHOLD_LIMIT {
            MAX_THRESHOLD_LIMIT
        } else {
            threshold
        };

        Self {
            history: VecDeque::with_capacity(max_window),
            counts: HashMap::with_capacity(max_window),
            max_window,
            threshold,
        }
    }

    pub fn record_tool_call(&mut self, tool_name: &str, arguments: &serde_json::Value) {
        let key = Self::make_key(tool_name, arguments);
        if self.history.len() >= self.max_window {
            if let Some(evicted) = self.history.pop_front() {
                if let Some(count) = self.counts.get_mut(&evicted) {
                    *count -= 1;
                    if *count == 0 {
                        self.counts.remove(&evicted);
                    }
                }
            }
        }
        self.history.push_back(key.clone());
        *self.counts.entry(key).or_insert(0) += 1;
    }

    pub fn is_doom_loop(&self) -> bool {
        if self.history.is_empty() || self.threshold == 0 {
            return false;
        }

        let Some(last_key) = self.history.back() else {
            return false;
        };

        self.counts
            .get(last_key)
            .map(|&c| c >= self.threshold)
            .unwrap_or(false)
    }

    pub fn reset(&mut self) {
        self.history.clear();
        self.counts.clear();
    }

    fn make_key(tool_name: &str, arguments: &serde_json::Value) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        tool_name.hash(&mut hasher);
        arguments.to_string().hash(&mut hasher);
        format!("{}:{:x}", tool_name, hasher.finish())
    }
}

/// Checks if a path is within a project root directory.
/// This is a security utility function for path traversal prevention.
///
/// Returns `true` if the path is inside the project root (safe),
/// `false` if the path is outside (potential security risk).
#[allow(dead_code)]
pub fn check_external_directory(path: &str, project_root: &str) -> bool {
    let path = Path::new(path);
    let root = Path::new(project_root);

    if let Ok(canonical_path) = path.canonicalize() {
        if let Ok(canonical_root) = root.canonicalize() {
            return canonical_path.starts_with(&canonical_root);
        }
    }

    path.starts_with(root)
}
