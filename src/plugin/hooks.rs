use std::fmt;
use strum::{Display, EnumIter, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Auth,
    Provider,
    ToolDefinition,
    ToolExecuteBefore,
    ToolExecuteAfter,
    ChatParams,
    ChatHeaders,
    Event,
    Config,
    ShellEnv,
    TextComplete,
    SessionCompacting,
    MessagesTransform,
}

impl HookType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookType::Auth => "auth",
            HookType::Provider => "provider",
            HookType::ToolDefinition => "tool.definition",
            HookType::ToolExecuteBefore => "tool.execute.before",
            HookType::ToolExecuteAfter => "tool.execute.after",
            HookType::ChatParams => "chat.params",
            HookType::ChatHeaders => "chat.headers",
            HookType::Event => "event",
            HookType::Config => "config",
            HookType::ShellEnv => "shell.env",
            HookType::TextComplete => "text.complete",
            HookType::SessionCompacting => "session.compacting",
            HookType::MessagesTransform => "messages.transform",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auth" => Some(HookType::Auth),
            "provider" => Some(HookType::Provider),
            "tool.definition" => Some(HookType::ToolDefinition),
            "tool.execute.before" => Some(HookType::ToolExecuteBefore),
            "tool.execute.after" => Some(HookType::ToolExecuteAfter),
            "chat.params" => Some(HookType::ChatParams),
            "chat.headers" => Some(HookType::ChatHeaders),
            "event" => Some(HookType::Event),
            "config" => Some(HookType::Config),
            "shell.env" => Some(HookType::ShellEnv),
            "text.complete" => Some(HookType::TextComplete),
            "session.compacting" => Some(HookType::SessionCompacting),
            "messages.transform" => Some(HookType::MessagesTransform),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
}

impl HookResult {
    pub fn ok(output: serde_json::Value) -> Self {
        Self {
            output,
            blocked: false,
            error: None,
        }
    }

    pub fn blocked() -> Self {
        Self {
            output: serde_json::Value::Null,
            blocked: true,
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            output: serde_json::Value::Null,
            blocked: false,
            error: Some(msg.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
}

impl fmt::Display for HookRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}(priority={})",
            self.plugin_id, self.hook_type, self.priority
        )
    }
}
