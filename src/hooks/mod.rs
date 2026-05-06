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
            env.insert("CODEGG_SESSION_ID".to_string(), sid.clone());
            env.insert("CODEGG_TOOL_NAME".to_string(), name.clone());
            env.insert("CODEGG_TOOL_ARGUMENTS".to_string(), args.to_string());
            env.insert("CODEGG_TOOL_RESULT".to_string(), result.clone());
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
}

impl ShellCommandHook {
    pub fn new(command: String, timeout_secs: Option<u64>) -> Self {
        Self {
            command,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(30)),
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
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
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
                        "Hook command failed: {}",
                        stderr
                    )))
                }
            }
            Ok(Err(e)) => Err(AppError::Io(e)),
            Err(_) => Err(AppError::Other(anyhow::anyhow!(
                "Hook command timed out after {:?}",
                self.timeout
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
                Err(_) => continue,
            };
            let hook: Box<dyn Hook> = match &def.hook {
                ConfigHookConfig::ShellCommand {
                    command,
                    timeout_secs,
                } => Box::new(ShellCommandHook::new(command.clone(), *timeout_secs)),
                ConfigHookConfig::InlineScript {
                    script: _,
                    timeout_secs,
                } => {
                    let cmd = "echo 'Inline scripts not yet supported'; exit 1".to_string();
                    Box::new(ShellCommandHook::new(cmd, *timeout_secs))
                }
            };
            registry.register(event, hook);
        }
        registry
    }

    pub async fn run_hooks(&self, event: HookEvent, ctx: &HookContext) -> Result<(), AppError> {
        if let Some(hooks) = self.hooks.get(&event) {
            for hook in hooks {
                hook.execute(ctx).await?;
            }
        }
        Ok(())
    }

    pub fn has_hooks(&self, event: HookEvent) -> bool {
        self.hooks.get(&event).is_some_and(|h| !h.is_empty())
    }
}
