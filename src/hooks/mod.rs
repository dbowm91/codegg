//! Hooks system for codegg.
//!
//! This module provides a hooks system that allows users to run custom
//! commands or scripts at specific points in the agent loop execution.

use crate::config::schema::{HookConfig as ConfigHookConfig, HookConfigEntry};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    PreToolExecute,
    PostToolExecute,
    SessionStart,
    SessionEnd,
    AgentStart,
    AgentEnd,
}

impl HookEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookEvent::PreToolExecute => "pre_tool_execute",
            HookEvent::PostToolExecute => "post_tool_execute",
            HookEvent::SessionStart => "session_start",
            HookEvent::SessionEnd => "session_end",
            HookEvent::AgentStart => "agent_start",
            HookEvent::AgentEnd => "agent_end",
        }
    }
}

impl std::str::FromStr for HookEvent {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pre_tool_execute" => Ok(HookEvent::PreToolExecute),
            "post_tool_execute" => Ok(HookEvent::PostToolExecute),
            "session_start" => Ok(HookEvent::SessionStart),
            "session_end" => Ok(HookEvent::SessionEnd),
            "agent_start" => Ok(HookEvent::AgentStart),
            "agent_end" => Ok(HookEvent::AgentEnd),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    pub event: HookEvent,
    pub session_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<serde_json::Value>,
    pub tool_result: Option<String>,
    pub timestamp: i64,
}

impl HookContext {
    pub fn to_env_vars(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert(
            "CODEGG_HOOK_EVENT".to_string(),
            self.event.as_str().to_string(),
        );
        if let Some(ref sid) = self.session_id {
            env.insert("CODEGG_SESSION_ID".to_string(), sid.clone());
        }
        if let Some(ref name) = self.tool_name {
            env.insert("CODEGG_TOOL_NAME".to_string(), name.clone());
        }
        if let Some(ref args) = self.tool_arguments {
            env.insert("CODEGG_TOOL_ARGUMENTS".to_string(), args.to_string());
        }
        if let Some(ref result) = self.tool_result {
            env.insert("CODEGG_TOOL_RESULT".to_string(), result.clone());
        }
        env.insert("CODEGG_TIMESTAMP".to_string(), self.timestamp.to_string());
        env
    }
}

#[async_trait::async_trait]
pub trait Hook: Send + Sync {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError>;
}

pub struct ShellCommandHook {
    pub command: String,
    pub timeout: Duration,
    pub event: HookEvent,
}

impl ShellCommandHook {
    pub fn new(command: String, timeout_secs: Option<u64>, event: HookEvent) -> Self {
        Self {
            command,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
            event,
        }
    }
}

#[async_trait::async_trait]
impl Hook for ShellCommandHook {
    async fn execute(&self, ctx: &HookContext) -> Result<(), AppError> {
        let env_vars = ctx.to_env_vars();
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&self.command)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in env_vars {
            cmd.env(key, value);
        }

        let output = tokio::time::timeout(self.timeout, cmd.output()).await;
        match output {
            Ok(Ok(out)) => {
                if out.status.success() {
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                Err(AppError::Other(anyhow::anyhow!(
                    "Hook command failed (event={}): {}",
                    self.event.as_str(), stderr
                )))
                }
            }
            Ok(Err(e)) => Err(AppError::Io(e)),
            Err(_) => Err(AppError::Other(anyhow::anyhow!(
                "Hook command timed out after {:?} (event={})",
                self.timeout, self.event.as_str()
            ))),
        }
    }
}

#[derive(Default)]
pub struct HookRegistry {
    hooks: HashMap<HookEvent, Vec<Box<dyn Hook>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: HashMap::new(),
        }
    }

    pub fn register(&mut self, event: HookEvent, hook: Box<dyn Hook>) {
        self.hooks.entry(event).or_default().push(hook);
    }

    pub fn from_config(hook_defs: &[HookConfigEntry]) -> Self {
        let mut registry = Self::new();
        for def in hook_defs {
            let event = match def.event.parse::<HookEvent>() {
                Ok(e) => e,
                Err(_) => {
                    warn!(event = %def.event, "invalid hook event name, skipping");
                    continue;
                }
            };
            let hook: Box<dyn Hook> = match &def.hook {
                ConfigHookConfig::ShellCommand {
                    command,
                    timeout_secs,
                } => Box::new(ShellCommandHook::new(command.clone(), *timeout_secs, event)),
                #[allow(deprecated)]
                ConfigHookConfig::InlineScript { .. } => {
                    warn!("InlineScript hook type is not implemented, skipping");
                    continue;
                }
            };
            registry.register(event, hook);
        }
        registry
    }

    pub async fn run_hooks(&self, event: HookEvent, ctx: &HookContext) -> Vec<AppError> {
        let mut errors = Vec::new();
        if let Some(hooks) = self.hooks.get(&event) {
            for hook in hooks {
                if let Err(e) = hook.execute(ctx).await {
                    errors.push(e);
                }
            }
        }
        errors
    }

    pub fn has_hooks(&self, event: HookEvent) -> bool {
        self.hooks.get(&event).is_some_and(|h| !h.is_empty())
    }
}
