use serde::{Deserialize, Serialize};

pub const API_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stability {
    Stable,
    Beta,
    Alpha,
}

impl Stability {
    pub fn is_stable(self) -> bool {
        matches!(self, Stability::Stable)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiVersion {
    pub version: String,
    pub stability: Stability,
    pub features: Vec<String>,
}

impl ApiVersion {
    pub fn current() -> Self {
        Self {
            version: API_VERSION.to_string(),
            stability: Stability::Stable,
            features: vec![
                "hooks".to_string(),
                "custom_tools".to_string(),
                "provider_middleware".to_string(),
            ],
        }
    }
}

pub mod hooks {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
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
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct HookContext {
        pub hook_type: HookType,
        pub input: serde_json::Value,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
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
}

pub mod tools {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolDefinition {
        pub name: String,
        pub description: String,
        pub parameters: serde_json::Value,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolInput {
        pub name: String,
        pub arguments: serde_json::Value,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolOutput {
        pub tool_name: String,
        pub input: serde_json::Value,
        pub output: String,
        pub success: bool,
    }
}

pub mod provider {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChatRequest {
        pub messages: Vec<super::Message>,
        pub model: String,
        pub tools: Option<Vec<super::tools::ToolDefinition>>,
        pub system: Option<String>,
        pub temperature: Option<f64>,
        pub top_p: Option<f64>,
        pub max_tokens: Option<usize>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "role", rename_all = "lowercase")]
    pub enum Message {
        System {
            content: String,
        },
        User {
            content: Vec<super::ContentPart>,
        },
        Assistant {
            content: Vec<super::ContentPart>,
        },
        Tool {
            tool_call_id: String,
            content: String,
        },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum ContentPart {
        Text { text: String },
        Image { image_url: super::ImageUrl },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ImageUrl {
        pub url: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum ChatEvent {
        TextDelta(String),
        ReasoningDelta(String),
        ToolCall(super::ToolCall),
        ToolResult {
            tool_call_id: String,
            content: String,
        },
        Finish {
            stop_reason: String,
            usage: super::TokenUsage,
        },
        Error(String),
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolCall {
        pub id: String,
        pub name: String,
        pub arguments: serde_json::Value,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct TokenUsage {
        pub input_tokens: usize,
        pub output_tokens: usize,
        pub total_tokens: usize,
    }
}

pub use provider::ChatEvent;
pub use provider::ChatRequest;
pub use provider::ContentPart;
pub use provider::ImageUrl;
pub use provider::Message;
pub use provider::TokenUsage;
pub use provider::ToolCall;
pub use tools::ToolDefinition;
pub use tools::ToolInput;
pub use tools::ToolOutput;
