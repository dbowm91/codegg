use crate::agent::r#loop::AgentLoop;
use crate::agent::{self, processor::EventProcessor};
use crate::config::schema::Config;
use crate::error::{AppError, ProviderError};
use crate::permission::PermissionChecker;
use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecInput {
    pub prompt: String,
    pub model: Option<String>,
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecOutput {
    pub success: bool,
    pub result: Option<String>,
    pub tools_used: Vec<String>,
    pub tokens_used: Option<usize>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub code: Option<String>,
}

impl ExecOutput {
    pub fn success(
        result: String,
        tools_used: Vec<String>,
        tokens_used: usize,
        duration_ms: u64,
    ) -> Self {
        Self {
            success: true,
            result: Some(result),
            tools_used,
            tokens_used: Some(tokens_used),
            duration_ms: Some(duration_ms),
            error: None,
            code: None,
        }
    }

    pub fn error(error: String, code: String) -> Self {
        Self {
            success: false,
            result: None,
            tools_used: Vec::new(),
            tokens_used: None,
            duration_ms: None,
            error: Some(error),
            code: Some(code),
        }
    }
}

pub struct ExecMode {
    quiet: bool,
    json_output: bool,
    session_id: Option<String>,
}

impl ExecMode {
    pub fn new(quiet: bool, json_output: bool, session_id: Option<String>) -> Self {
        Self {
            quiet,
            json_output,
            session_id,
        }
    }

    pub async fn run(&self, input: ExecInput) -> Result<ExecOutput, AppError> {
        let start = Instant::now();

        if !self.quiet {
            eprintln!("Starting exec mode...");
        }

        let config = Config::load().map_err(|e| AppError::Config(e))?;
        let mut registry = crate::provider::ProviderRegistry::new();
        crate::provider::register_builtin_with_config(&mut registry, &config);

        let default_model = config
            .model
            .clone()
            .unwrap_or_else(|| "openai/gpt-4o".to_string());
        let model = input.model.unwrap_or(default_model);
        let (provider_id, model_name) = Self::parse_model(&model);

        let provider = registry.get(&provider_id).ok_or_else(|| {
            AppError::Other(anyhow::anyhow!("Provider not found: {}", provider_id))
        })?;

        let all_agents = agent::resolve_agents(&config)?;
        let agent_name = input.agent.unwrap_or_else(|| "build".to_string());
        if !all_agents.iter().any(|a| a.name == agent_name) {
            return Err(AppError::Other(anyhow::anyhow!("Agent not found: {}", agent_name)));
        }

        let permission_checker = PermissionChecker::new(Some(&config), None);
        let tool_registry = crate::tool::ToolRegistry::with_defaults();

        let mcp_service = None;

        let mut loop_instance = AgentLoop::new(
            all_agents,
            provider.clone_box(),
            permission_checker,
            tool_registry,
            config,
            mcp_service,
            None,
        );

        let session_id = self.session_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        loop_instance.set_session_id(&session_id);
        loop_instance.setup_question_channel();

        let messages = vec![Message::User {
            content: vec![ContentPart::Text {
                text: input.prompt.into(),
            }],
        }];

        let request = ChatRequest {
            messages,
            model: model_name,
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
        };

        match loop_instance.run(request).await {
            Ok(events) => {
                let mut processor = EventProcessor::new();
                let mut tools_used = Vec::new();
                let mut total_tokens = 0;

                for event in &events {
                    processor.process(event.clone());
                    if let ChatEvent::ToolCall(tc) = event {
                        let tool_name = tc.name.to_string();
                        if !tools_used.contains(&tool_name) {
                            tools_used.push(tool_name);
                        }
                    }
                    if let ChatEvent::Finish { usage, .. } = event {
                        total_tokens = usage.input_tokens + usage.output_tokens;
                    }
                }

                let duration_ms = start.elapsed().as_millis() as u64;
                let result = processor.text().to_string();

                if !self.quiet {
                    eprintln!("Completed in {}ms, {} tokens", duration_ms, total_tokens);
                }

                Ok(ExecOutput::success(
                    result,
                    tools_used,
                    total_tokens,
                    duration_ms,
                ))
            }
            Err(e) => {
                let _duration_ms = start.elapsed().as_millis() as u64;
                let (code, msg) = Self::classify_error(&e);
                Ok(ExecOutput::error(format!("{}: {}", msg, e), code))
            }
        }
    }

    fn parse_model(model: &str) -> (String, String) {
        if let Some(pos) = model.find('/') {
            (model[..pos].to_string(), model[pos + 1..].to_string())
        } else {
            ("openai".to_string(), model.to_string())
        }
    }

    fn classify_error(error: &AppError) -> (String, String) {
        match error {
            AppError::Permission(_) => (
                "PERMISSION_ERROR".to_string(),
                "Permission denied".to_string(),
            ),
            AppError::Provider(ProviderError::Auth(_)) => (
                "AUTH_ERROR".to_string(),
                "Authentication failed".to_string(),
            ),
            AppError::Provider(ProviderError::RateLimit) => {
                ("RATE_LIMIT".to_string(), "Rate limit exceeded".to_string())
            }
            AppError::Provider(ProviderError::Timeout(_)) => {
                ("TIMEOUT".to_string(), "Request timed out".to_string())
            }
            AppError::Provider(ProviderError::ModelNotFound(_)) => {
                ("MODEL_NOT_FOUND".to_string(), "Model not found".to_string())
            }
            AppError::Provider(ProviderError::CircuitOpen(name)) => {
                ("CIRCUIT_OPEN".to_string(), format!("Provider circuit open: {}", name))
            }
            AppError::Provider(ProviderError::Api { code, message, .. }) => {
                ("API_ERROR".to_string(), format!("API error [{}]: {}", code, message))
            }
            AppError::Provider(ProviderError::Stream(_)) => {
                ("STREAM_ERROR".to_string(), "Stream error".to_string())
            }
            AppError::Io(_) => ("IO_ERROR".to_string(), "I/O error".to_string()),
            AppError::Config(_) => (
                "CONFIG_ERROR".to_string(),
                "Configuration error".to_string(),
            ),
            _ => ("EXECUTION_ERROR".to_string(), "Execution error".to_string()),
        }
    }

    pub fn print_output(&self, output: &ExecOutput) {
        if self.json_output {
            println!("{}", serde_json::to_string(output).unwrap_or_else(|_| r#"{"success":false,"error":"json serialization failed","code":"INTERNAL_ERROR"}"#.to_string()));
        } else if output.success {
            if let Some(ref result) = output.result {
                println!("{}", result);
            }
        } else {
            if let (Some(ref error), Some(ref code)) = (&output.error, &output.code) {
                eprintln!("Error [{}]: {}", code, error);
            } else if let Some(ref error) = output.error {
                eprintln!("Error: {}", error);
            }
        }
    }

    pub fn exit_code(output: &ExecOutput) -> i32 {
        if output.success {
            0
        } else {
            1
        }
    }
}
