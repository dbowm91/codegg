use crate::error::ProviderError;
use crate::provider::{ChatEvent, EventStream, TokenUsage, ToolCall, MAX_BUFFER_SIZE};
use futures::StreamExt;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

static TOOL_CALL_FALLBACK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Default)]
struct OpenAiToolState {
    id: String,
    name: String,
    args_buffer: String,
}

pub struct SseParser {
    buffer: String,
    delimiter: &'static str,
    is_openai: bool,
    pending_tool_calls: VecDeque<ToolCall>,
    current_tool: Option<(String, String, String)>,
    args_buffer: String,
    openai_tool_states: HashMap<usize, OpenAiToolState>,
}

impl SseParser {
    pub fn new(is_openai: bool) -> Self {
        Self {
            buffer: String::new(),
            delimiter: if is_openai { "\n" } else { "\n\n" },
            is_openai,
            pending_tool_calls: VecDeque::new(),
            current_tool: None,
            args_buffer: String::new(),
            openai_tool_states: HashMap::new(),
        }
    }

    pub fn push(&mut self, data: &str) {
        self.buffer.push_str(data);
    }

    pub fn parse(&mut self) -> Option<Result<ChatEvent, ProviderError>> {
        if let Some(tc) = self.pending_tool_calls.pop_front() {
            return Some(Ok(ChatEvent::ToolCall(tc)));
        }

        while let Some(idx) = self.buffer.find(self.delimiter) {
            let chunk = self.buffer.drain(..idx).collect::<String>();
            self.buffer.drain(..self.delimiter.len());

            if chunk.is_empty() {
                continue;
            }

            if self.is_openai {
                if let Some(event) = self.parse_openai_line(&chunk) {
                    return Some(event);
                }
            } else {
                if let Some(event) = self.parse_anthropic_chunk_inner(&chunk) {
                    return Some(event);
                }
            }
        }
        None
    }

    fn parse_openai_line(&mut self, line: &str) -> Option<Result<ChatEvent, ProviderError>> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Some(data) = trimmed.strip_prefix("data: ") {
            if data == "[DONE]" {
                return None;
            }

            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data) {
                if let Some(error) = val.get("error") {
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown provider error");
                    let code = error
                        .get("code")
                        .and_then(|c| c.as_str())
                        .or_else(|| error.get("type").and_then(|t| t.as_str()))
                        .unwrap_or("unknown");
                    return Some(Err(ProviderError::api(code.to_string(), message.to_string())));
                }
                return self.parse_openai_chunk(&val);
            }
        } else if trimmed.starts_with('{') {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(error) = val.get("error") {
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown provider error");
                    let code = error
                        .get("code")
                        .and_then(|c| c.as_str())
                        .or_else(|| error.get("type").and_then(|t| t.as_str()))
                        .unwrap_or("unknown");
                    return Some(Err(ProviderError::api(code.to_string(), message.to_string())));
                }
            }
        }
        None
    }

    fn parse_anthropic_chunk_inner(
        &mut self,
        chunk: &str,
    ) -> Option<Result<ChatEvent, ProviderError>> {
        if let Some(tc) = self.pending_tool_calls.pop_front() {
            return Some(Ok(ChatEvent::ToolCall(tc)));
        }

        let mut event_type: Option<&str> = None;
        let mut data: Option<&str> = None;

        for line in chunk.lines() {
            if let Some(ev) = line.strip_prefix("event: ") {
                event_type = Some(ev);
            } else if let Some(d) = line.strip_prefix("data: ") {
                data = Some(d);
            }
        }

        let event_type = event_type?;
        let data_str = data?;

        parse_anthropic_event_with_state(
            event_type,
            data_str,
            &mut self.current_tool,
            &mut self.args_buffer,
            &mut self.pending_tool_calls,
        )
    }

    fn parse_openai_chunk(
        &mut self,
        val: &serde_json::Value,
    ) -> Option<Result<ChatEvent, ProviderError>> {
        if let Some(tc) = self.pending_tool_calls.pop_front() {
            return Some(Ok(ChatEvent::ToolCall(tc)));
        }

        if let Some(error) = val.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string();
            return Some(Err(msg.into()));
        }

        let choices = val.get("choices")?.as_array()?;

        for choice in choices {
            let empty = serde_json::Value::Null;
            let delta = choice.get("delta").unwrap_or(&empty);

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                if tool_calls.is_empty() {
                    return None;
                }
                for (arr_idx, tc) in tool_calls.iter().enumerate() {
                    let idx = tc
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .map(|n| n as usize)
                        .unwrap_or(arr_idx);
                    let state = self.openai_tool_states.entry(idx).or_default();

                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        if !id.is_empty() {
                            state.id = id.to_string();
                        }
                    }

                    let Some(function) = tc.get("function") else {
                        continue;
                    };

                    if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                        if !name.is_empty() {
                            state.name = name.to_string();
                        }
                    }

                    let args_val = function.get("arguments");
                    if let Some(v) = args_val {
                        if !v.is_string() {
                            if !state.name.trim().is_empty() {
                                self.pending_tool_calls.push_back(ToolCall {
                                    id: state.id.clone().into(),
                                    name: state.name.clone().into(),
                                    arguments: v.clone(),
                                });
                            }
                            state.args_buffer.clear();
                            continue;
                        }
                    }

                    let args_str = args_val.and_then(|v| v.as_str()).unwrap_or("");
                    if args_str.is_empty() {
                        continue;
                    }

                    state.args_buffer.push_str(args_str);
                    if let Ok(args) =
                        serde_json::from_str::<serde_json::Value>(&state.args_buffer)
                    {
                        if !state.name.trim().is_empty() {
                            self.pending_tool_calls.push_back(ToolCall {
                                id: state.id.clone().into(),
                                name: state.name.clone().into(),
                                arguments: args,
                            });
                        }
                        state.args_buffer.clear();
                    }
                }
                if let Some(tc) = self.pending_tool_calls.pop_front() {
                    return Some(Ok(ChatEvent::ToolCall(tc)));
                }
                return None;
            }

            if let Some(tool_calls) = choice.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tool_calls {
                    if let Some(parsed) = parse_openai_tool_call_value(tc) {
                        self.pending_tool_calls.push_back(parsed);
                    }
                }
                if let Some(tc) = self.pending_tool_calls.pop_front() {
                    return Some(Ok(ChatEvent::ToolCall(tc)));
                }
            }

            if let Some(tool_calls) = choice
                .get("message")
                .and_then(|m| m.get("tool_calls"))
                .and_then(|t| t.as_array())
            {
                for tc in tool_calls {
                    if let Some(parsed) = parse_openai_tool_call_value(tc) {
                        self.pending_tool_calls.push_back(parsed);
                    }
                }
                if let Some(tc) = self.pending_tool_calls.pop_front() {
                    return Some(Ok(ChatEvent::ToolCall(tc)));
                }
            }

            if let Some(function_call) = delta.get("function_call") {
                if let Some(parsed) = parse_openai_legacy_function_call(function_call) {
                    return Some(Ok(ChatEvent::ToolCall(parsed)));
                }
            }

            if let Some(function_call) = choice
                .get("message")
                .and_then(|m| m.get("function_call"))
            {
                if let Some(parsed) = parse_openai_legacy_function_call(function_call) {
                    return Some(Ok(ChatEvent::ToolCall(parsed)));
                }
            }

            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    return Some(Ok(ChatEvent::TextDelta(content.to_string().into())));
                }
            }

            if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                if !reasoning.is_empty() {
                    return Some(Ok(ChatEvent::ReasoningDelta(reasoning.to_string().into())));
                }
            }

            if let Some(reasoning) = delta.get("reasoning").and_then(|r| r.as_str()) {
                if !reasoning.is_empty() {
                    return Some(Ok(ChatEvent::ReasoningDelta(reasoning.to_string().into())));
                }
            }

            if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                if finish_reason != "null" {
                    if finish_reason == "tool_calls" && self.pending_tool_calls.is_empty() {
                        let mut recovered = Vec::new();
                        collect_tool_calls_loose(choice, &mut recovered);
                        if recovered.is_empty() {
                            collect_tool_calls_loose(val, &mut recovered);
                        }
                        let mut seen = HashSet::new();
                        for tc in recovered {
                            let key = format!("{}|{}|{}", tc.id, tc.name, tc.arguments);
                            if seen.insert(key) {
                                self.pending_tool_calls.push_back(tc);
                            }
                        }
                        if let Some(tc) = self.pending_tool_calls.pop_front() {
                            return Some(Ok(ChatEvent::ToolCall(tc)));
                        }
                    }
                    if finish_reason == "tool_calls" && self.pending_tool_calls.is_empty() {
                        debug_log!(
                            "[API-DEBUG] tool-calls-miss: finish_reason=tool_calls but no tool call parsed; has_delta_tool_calls={}, has_choice_tool_calls={}, has_message_tool_calls={}, has_delta_function_call={}, has_message_function_call={}, choice={}",
                            delta.get("tool_calls").is_some(),
                            choice.get("tool_calls").is_some(),
                            choice
                                .get("message")
                                .and_then(|m| m.get("tool_calls"))
                                .is_some(),
                            delta.get("function_call").is_some(),
                            choice
                                .get("message")
                                .and_then(|m| m.get("function_call"))
                                .is_some(),
                            choice
                        );
                    }
                    let mut usage = TokenUsage::default();
                    if let Some(u) = val.get("usage") {
                        if let Some(prompt) = u.get("prompt_tokens").and_then(|v| v.as_u64()) {
                            usage.input_tokens = prompt as usize;
                        }
                        if let Some(completion) =
                            u.get("completion_tokens").and_then(|v| v.as_u64())
                        {
                            usage.output_tokens = completion as usize;
                        }
                        if let Some(cached) = u
                            .get("prompt_tokens_details")
                            .and_then(|v| v.get("cached_tokens"))
                            .and_then(|v| v.as_u64())
                        {
                            usage.cached_tokens = Some(cached as usize);
                        }
                        usage.total_tokens = usage.input_tokens + usage.output_tokens;
                    }
                    return Some(Ok(ChatEvent::Finish {
                        stop_reason: finish_reason.to_string().into(),
                        usage,
                    }));
                }
            }
        }

        if let Some(usage) = val.get("usage") {
            let mut u = TokenUsage::default();
            if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                u.input_tokens = prompt as usize;
            }
            if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                u.output_tokens = completion as usize;
            }
            if let Some(cached) = usage
                .get("prompt_tokens_details")
                .and_then(|v| v.get("cached_tokens"))
                .and_then(|v| v.as_u64())
            {
                u.cached_tokens = Some(cached as usize);
            }
            u.total_tokens = u.input_tokens + u.output_tokens;
            return Some(Ok(ChatEvent::Finish {
                stop_reason: "stop".to_string().into(),
                usage: u,
            }));
        }

        None
    }
}

pub fn parse_openai_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>> {
    if let Some(tc) = pop_queued_tool_call(buffer) {
        return Some(Ok(ChatEvent::ToolCall(tc)));
    }
    let openai_tool_states = pop_openai_state(buffer);
    let mut parser = SseParser::new(true);
    std::mem::swap(&mut parser.buffer, buffer);
    parser.openai_tool_states = openai_tool_states;
    let result = parser.parse();
    let mut rebuilt = String::new();
    rebuilt.push_str(&parser.buffer);
    if !parser.pending_tool_calls.is_empty() {
        queue_remaining_tool_calls(&mut rebuilt, &parser.pending_tool_calls);
    }
    queue_openai_state(&mut rebuilt, &parser.openai_tool_states);
    *buffer = rebuilt;
    result
}

fn pop_queued_tool_call(buffer: &mut String) -> Option<ToolCall> {
    const PREFIX: &str = "\n__TC__:";
    const STATE_PREFIX: &str = "\n__OAI_STATE__:";
    let prefix_idx = buffer.find(PREFIX)?;
    let json_start = prefix_idx + PREFIX.len();
    let next_tc = buffer[json_start..]
        .find(PREFIX)
        .map(|i| json_start + i);
    let next_state = buffer[json_start..]
        .find(STATE_PREFIX)
        .map(|i| json_start + i);
    let json_end = match (next_tc, next_state) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => buffer.len(),
    };

    let json_str = &buffer[json_start..json_end];
    let tc = serde_json::from_str(json_str).ok()?;

    buffer.drain(prefix_idx..json_end);
    Some(tc)
}

fn queue_remaining_tool_calls(buffer: &mut String, queue: &VecDeque<ToolCall>) {
    const PREFIX: &str = "\n__TC__:";
    for tc in queue {
        if let Ok(json) = serde_json::to_string(tc) {
            buffer.push_str(&format!("{}{}", PREFIX, json));
        }
    }
}

fn pop_openai_state(buffer: &mut String) -> HashMap<usize, OpenAiToolState> {
    const PREFIX: &str = "\n__OAI_STATE__:";
    let Some(prefix_idx) = buffer.find(PREFIX) else {
        return HashMap::new();
    };
    let json_start = prefix_idx + PREFIX.len();
    let json_str = &buffer[json_start..];
    let parsed = serde_json::from_str::<serde_json::Value>(json_str).ok();
    buffer.truncate(prefix_idx);
    let Some(val) = parsed else {
        return HashMap::new();
    };
    let mut states = HashMap::new();
    if let Some(state_obj) = val.get("tool_states").and_then(|v| v.as_object()) {
        for (k, entry) in state_obj {
            let Ok(idx) = k.parse::<usize>() else {
                continue;
            };
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args_buffer = entry
                .get("args_buffer")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            states.insert(
                idx,
                OpenAiToolState {
                    id,
                    name,
                    args_buffer,
                },
            );
        }
    }
    states
}

fn queue_openai_state(buffer: &mut String, states: &HashMap<usize, OpenAiToolState>) {
    if states.is_empty() {
        return;
    }
    const PREFIX: &str = "\n__OAI_STATE__:";
    let tool_states: serde_json::Map<String, serde_json::Value> = states
        .iter()
        .map(|(idx, st)| {
            (
                idx.to_string(),
                json!({
                    "id": st.id,
                    "name": st.name,
                    "args_buffer": st.args_buffer,
                }),
            )
        })
        .collect();
    let payload = json!({
        "tool_states": tool_states,
    });
    if let Ok(state) = serde_json::to_string(&payload) {
        buffer.push_str(PREFIX);
        buffer.push_str(&state);
    }
}

pub fn parse_anthropic_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>> {
    parse_anthropic_buffer_with_state(buffer, &mut None, &mut String::new())
}

pub fn parse_anthropic_buffer_with_state(
    buffer: &mut String,
    current_tool: &mut Option<(String, String, String)>,
    args_buffer: &mut String,
) -> Option<Result<ChatEvent, ProviderError>> {
    let mut parser = SseParser::new(false);
    std::mem::swap(&mut parser.buffer, buffer);
    parser.current_tool = current_tool.take();
    parser.args_buffer = std::mem::take(args_buffer);
    let result = parser.parse();
    *current_tool = parser.current_tool.take();
    *args_buffer = std::mem::take(&mut parser.args_buffer);
    *buffer = parser.buffer;
    if result.is_none() && !parser.pending_tool_calls.is_empty() {
        if let Some(tc) = parser.pending_tool_calls.pop_front() {
            return Some(Ok(ChatEvent::ToolCall(tc)));
        }
    }
    result
}

#[allow(dead_code)]
fn parse_anthropic_event(
    event_type: &str,
    data_str: &str,
) -> Option<Result<ChatEvent, ProviderError>> {
    parse_anthropic_event_with_state(
        event_type,
        data_str,
        &mut None,
        &mut String::new(),
        &mut VecDeque::new(),
    )
}

fn parse_anthropic_event_with_state(
    event_type: &str,
    data_str: &str,
    current_tool: &mut Option<(String, String, String)>,
    args_buffer: &mut String,
    _pending_tool_calls: &mut VecDeque<ToolCall>,
) -> Option<Result<ChatEvent, ProviderError>> {
    match event_type {
        "message_start" => None,
        "content_block_start" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                let block = val.get("content_block")?;
                let btype = block.get("type")?.as_str()?;
                match btype {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                return Some(Ok(ChatEvent::TextDelta(text.to_string().into())));
                            }
                        }
                        None
                    }
                    "tool_use" => {
                        let id = block.get("id")?.as_str()?.to_string();
                        let name = block.get("name")?.as_str()?.to_string();
                        *current_tool = Some((id.clone(), name.clone(), id.clone()));
                        args_buffer.clear();
                        let args = block.get("input").cloned().unwrap_or(json!({}));
                        if args == json!({}) {
                            None
                        } else {
                            Some(Ok(ChatEvent::ToolCall(ToolCall {
                                id: id.into(),
                                name: name.into(),
                                arguments: args,
                            })))
                        }
                    }
                    "thinking" => None,
                    _ => None,
                }
            } else {
                None
            }
        }
        "content_block_delta" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                let delta = val.get("delta")?;
                let dtype = delta.get("type")?.as_str()?;
                match dtype {
                    "text_delta" => {
                        let text = delta.get("text")?.as_str()?.to_string();
                        Some(Ok(ChatEvent::TextDelta(text.into())))
                    }
                    "thinking_delta" => {
                        let thinking = delta.get("thinking")?.as_str()?.to_string();
                        Some(Ok(ChatEvent::ReasoningDelta(thinking.into())))
                    }
                    "input_json_delta" => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if partial.is_empty() {
                            None
                        } else {
                            args_buffer.push_str(&partial);
                            None
                        }
                    }
                    _ => None,
                }
            } else {
                None
            }
        }
        "content_block_stop" => {
            if let Some((id, name, _)) = current_tool.take() {
                let args_str = std::mem::take(args_buffer);
                if !args_str.is_empty() {
                    match serde_json::from_str::<serde_json::Value>(&args_str) {
                        Ok(args) => {
                            return Some(Ok(ChatEvent::ToolCall(ToolCall {
                                id: id.into(),
                                name: name.into(),
                                arguments: args,
                            })));
                        }
                        Err(e) => {
                            tracing::warn!(
                                "failed to parse Anthropic tool call JSON for '{}' (id={}): {}",
                                name,
                                id,
                                e
                            );
                            return Some(Err(ProviderError::Stream(format!(
                                "malformed tool call JSON for '{}': {}",
                                name, e
                            ))));
                        }
                    }
                }
            }
            None
        }
        "message_delta" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(data_str) {
                if let Some(usage) = val.get("usage") {
                    let mut u = TokenUsage::default();
                    if let Some(input_tokens) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                        u.input_tokens = input_tokens as usize;
                    }
                    if let Some(output_tokens) = usage.get("output_tokens").and_then(|v| v.as_u64())
                    {
                        u.output_tokens = output_tokens as usize;
                    }
                    if let Some(cached) = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                        u.cached_tokens = Some(cached as usize);
                    }
                    u.total_tokens = u.input_tokens + u.output_tokens;
                    return Some(Ok(ChatEvent::Finish {
                        stop_reason: "stop".to_string().into(),
                        usage: u,
                    }));
                }
            }
            None
        }
        "message_stop" => None,
        _ => None,
    }
}

#[allow(dead_code)]
pub fn parse_openai_chunk_standalone(
    val: &serde_json::Value,
) -> Option<Result<ChatEvent, ProviderError>> {
    if let Some(error) = val.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error")
            .to_string();
        return Some(Err(msg.into()));
    }

    let choices = val.get("choices")?.as_array()?;

    for choice in choices {
        let empty = serde_json::Value::Null;
        let delta = choice.get("delta").unwrap_or(&empty);

        if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
            if tool_calls.is_empty() {
                return None;
            }
            if let Some(tc) = tool_calls.first() {
                let id = tc
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let function = tc.get("function")?;
                let name = function
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args_str = function
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}");
                let args = serde_json::from_str::<serde_json::Value>(args_str)
                    .map_err(|e| ProviderError::Stream(format!("malformed tool arguments: {}", e)));
                return Some(args.map(|args| {
                    ChatEvent::ToolCall(ToolCall {
                        id: id.into(),
                        name: name.into(),
                        arguments: args,
                    })
                }));
            }
            return None;
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if finish_reason != "null" {
                let mut usage = TokenUsage::default();
                if let Some(u) = val.get("usage") {
                    if let Some(prompt) = u.get("prompt_tokens").and_then(|v| v.as_u64()) {
                        usage.input_tokens = prompt as usize;
                    }
                    if let Some(completion) = u.get("completion_tokens").and_then(|v| v.as_u64()) {
                        usage.output_tokens = completion as usize;
                    }
                    if let Some(cached) = u
                        .get("prompt_tokens_details")
                        .and_then(|v| v.get("cached_tokens"))
                        .and_then(|v| v.as_u64())
                    {
                        usage.cached_tokens = Some(cached as usize);
                    }
                    usage.total_tokens = usage.input_tokens + usage.output_tokens;
                }
                return Some(Ok(ChatEvent::Finish {
                    stop_reason: finish_reason.to_string().into(),
                    usage,
                }));
            }
        }

        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
            if !content.is_empty() {
                return Some(Ok(ChatEvent::TextDelta(content.to_string().into())));
            }
        }

        if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
            if !reasoning.is_empty() {
                return Some(Ok(ChatEvent::ReasoningDelta(reasoning.to_string().into())));
            }
        }

        if let Some(reasoning) = delta.get("reasoning").and_then(|r| r.as_str()) {
            if !reasoning.is_empty() {
                return Some(Ok(ChatEvent::ReasoningDelta(reasoning.to_string().into())));
            }
        }
    }

    if let Some(usage) = val.get("usage") {
        let mut u = TokenUsage::default();
        if let Some(prompt) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
            u.input_tokens = prompt as usize;
        }
        if let Some(completion) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
            u.output_tokens = completion as usize;
        }
        if let Some(cached) = usage
            .get("prompt_tokens_details")
            .and_then(|v| v.get("cached_tokens"))
            .and_then(|v| v.as_u64())
        {
            u.cached_tokens = Some(cached as usize);
        }
        u.total_tokens = u.input_tokens + u.output_tokens;
        return Some(Ok(ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: u,
        }));
    }

    None
}

fn parse_openai_tool_call_value(tc: &serde_json::Value) -> Option<ToolCall> {
    let id = tc
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            let seq = TOOL_CALL_FALLBACK_ID.fetch_add(1, Ordering::Relaxed);
            format!("call_fallback_{}", seq)
        });
    let (name, arguments) = if let Some(function) = tc.get("function") {
        (
            function.get("name").and_then(|v| v.as_str())?.to_string(),
            function.get("arguments")?,
        )
    } else {
        (
            tc.get("name").and_then(|v| v.as_str())?.to_string(),
            tc.get("arguments")?,
        )
    };
    let args = if let Some(s) = arguments.as_str() {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse tool call '{}' arguments: {}", name, e);
                serde_json::Value::Object(serde_json::Map::new())
            }
        }
    } else {
        arguments.clone()
    };
    Some(ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    })
}

fn parse_openai_legacy_function_call(function_call: &serde_json::Value) -> Option<ToolCall> {
    let name = function_call.get("name").and_then(|v| v.as_str())?.to_string();
    let arguments = function_call.get("arguments")?;
    let args = if let Some(s) = arguments.as_str() {
        match serde_json::from_str::<serde_json::Value>(s) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse legacy function call '{}' arguments: {}", name, e);
                serde_json::Value::Object(serde_json::Map::new())
            }
        }
    } else {
        arguments.clone()
    };
    Some(ToolCall {
        id: format!("call_legacy_{}", name).into(),
        name: name.into(),
        arguments: args,
    })
}

fn collect_tool_calls_loose(value: &serde_json::Value, out: &mut Vec<ToolCall>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(arr) = map.get("tool_calls").and_then(|v| v.as_array()) {
                for tc in arr {
                    if let Some(parsed) = parse_openai_tool_call_value(tc) {
                        out.push(parsed);
                    }
                }
            }
            if let Some(fc) = map.get("function_call") {
                if let Some(parsed) = parse_openai_legacy_function_call(fc) {
                    out.push(parsed);
                }
            }
            for child in map.values() {
                collect_tool_calls_loose(child, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for child in arr {
                collect_tool_calls_loose(child, out);
            }
        }
        _ => {}
    }
}

pub fn create_sse_stream<F, Fut>(
    send_request: F,
    parse_buffer: fn(&mut String) -> Option<Result<ChatEvent, ProviderError>>,
) -> Result<EventStream, ProviderError>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<reqwest::Response, ProviderError>> + Send,
{
    let buffer = String::new();
    let response = tokio::runtime::Handle::current().block_on(send_request())?;

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::RateLimit);
    }

    if !response.status().is_success() {
        let status = response.status();
        let err_text = tokio::runtime::Handle::current()
            .block_on(response.text())
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(ProviderError::api(
            status.as_u16().to_string(),
            format!("HTTP {}: {}", status, err_text),
        ));
    }

    let stream = response.bytes_stream();

    Ok(Box::pin(futures::stream::unfold(
        (stream, buffer),
        move |(mut stream, mut buffer)| async move {
            loop {
                if let Some(event) = parse_buffer(&mut buffer) {
                    return Some((event, (stream, buffer)));
                }

                let chunk = stream.next().await;
                match chunk {
                    Some(Ok(bytes)) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        buffer.push_str(&text);
                        if buffer.len() > MAX_BUFFER_SIZE {
                            return Some((
                                Err(ProviderError::Stream(
                                    "response buffer exceeded limit".to_string(),
                                )),
                                (stream, buffer),
                            ));
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(ProviderError::Stream(e.to_string())),
                            (stream, buffer),
                        ));
                    }
                    None => {
                        if buffer.is_empty() {
                            return None;
                        }
                        if let Some(event) = parse_buffer(&mut buffer) {
                            return Some((event, (stream, buffer)));
                        }
                        return None;
                    }
                }
            }
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_message_tool_calls_shape() {
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "read",
                            "arguments": "{\"filePath\":\"README.md\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let mut parser = SseParser::new(true);
        let evt = parser.parse_openai_chunk(&payload);
        match evt {
            Some(Ok(ChatEvent::ToolCall(tc))) => {
                assert_eq!(tc.id.as_ref(), "call_123");
                assert_eq!(tc.name.as_ref(), "read");
                assert_eq!(tc.arguments["filePath"], "README.md");
            }
            other => panic!("expected tool call event, got {:?}", other),
        }
    }

    #[test]
    fn parses_openai_legacy_message_function_call_shape() {
        let payload = serde_json::json!({
            "choices": [{
                "message": {
                    "function_call": {
                        "name": "glob",
                        "arguments": "{\"pattern\":\"src/**/*.rs\"}"
                    }
                },
                "finish_reason": "tool_calls"
            }]
        });

        let mut parser = SseParser::new(true);
        let evt = parser.parse_openai_chunk(&payload);
        match evt {
            Some(Ok(ChatEvent::ToolCall(tc))) => {
                assert_eq!(tc.name.as_ref(), "glob");
                assert_eq!(tc.arguments["pattern"], "src/**/*.rs");
            }
            other => panic!("expected tool call event, got {:?}", other),
        }
    }
}
