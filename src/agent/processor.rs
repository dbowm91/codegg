use crate::provider::{ChatEvent, ContentPart, Message, ToolCall};

pub struct EventProcessor {
    accumulated_text: String,
    accumulated_reasoning: String,
    tool_calls: Vec<ToolCall>,
    tool_results: Vec<(String, String)>,
    stop_reason: Option<String>,
    input_tokens: usize,
    output_tokens: usize,
    cached_tokens: Option<usize>,
    is_complete: bool,
}

impl EventProcessor {
    pub fn new() -> Self {
        Self {
            accumulated_text: String::new(),
            accumulated_reasoning: String::new(),
            tool_calls: Vec::new(),
            tool_results: Vec::new(),
            stop_reason: None,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: None,
            is_complete: false,
        }
    }

    pub fn process(&mut self, event: ChatEvent) {
        match event {
            ChatEvent::TextDelta(text) => {
                self.accumulated_text.push_str(&text);
            }
            ChatEvent::ReasoningDelta(reasoning) => {
                self.accumulated_reasoning.push_str(&reasoning);
            }
            ChatEvent::ToolCall(tc) => {
                self.tool_calls.push(tc);
            }
            ChatEvent::ToolResult {
                tool_call_id,
                content,
            } => {
                self.tool_results
                    .push((tool_call_id.to_string(), content.to_string()));
            }
            ChatEvent::Finish {
                stop_reason, usage, ..
            } => {
                self.stop_reason = Some(stop_reason.to_string());
                self.input_tokens = usage.input_tokens;
                self.output_tokens = usage.output_tokens;
                self.cached_tokens = usage.cached_tokens;
                self.is_complete = true;
            }
            ChatEvent::Error(err) => {
                tracing::error!("Stream error: {}", err);
            }
        }
    }

    pub fn reset(&mut self) {
        self.accumulated_text.clear();
        self.accumulated_reasoning.clear();
        self.tool_calls.clear();
        self.tool_results.clear();
        self.stop_reason = None;
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.cached_tokens = None;
        self.is_complete = false;
    }

    pub fn text(&self) -> &str {
        &self.accumulated_text
    }

    pub fn reasoning(&self) -> &str {
        &self.accumulated_reasoning
    }

    pub fn tool_calls(&self) -> &[ToolCall] {
        &self.tool_calls
    }

    pub fn tool_results(&self) -> &[(String, String)] {
        &self.tool_results
    }

    pub fn stop_reason(&self) -> Option<&str> {
        self.stop_reason.as_deref()
    }

    pub fn input_tokens(&self) -> usize {
        self.input_tokens
    }

    pub fn output_tokens(&self) -> usize {
        self.output_tokens
    }

    pub fn cached_tokens(&self) -> Option<usize> {
        self.cached_tokens
    }

    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }

    pub fn to_assistant_message(&self) -> Option<Message> {
        if self.accumulated_text.is_empty() && self.tool_calls.is_empty() {
            return None;
        }

        let mut content = Vec::new();

        if !self.accumulated_text.is_empty() {
            content.push(ContentPart::Text {
                text: self.accumulated_text.clone().into(),
            });
        }

        Some(Message::Assistant {
            content,
            tool_calls: self.tool_calls.clone(),
        })
    }

    pub fn to_tool_messages(&self) -> Vec<Message> {
        self.tool_results
            .iter()
            .map(|(id, content)| Message::Tool {
                tool_call_id: id.clone().into(),
                content: content.clone().into(),
            })
            .collect()
    }
}

impl Default for EventProcessor {
    fn default() -> Self {
        Self::new()
    }
}
