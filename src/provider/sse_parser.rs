use crate::error::ProviderError;
use crate::provider::{ChatEvent, EventStream, TokenUsage, ToolCall, MAX_BUFFER_SIZE};
use futures::StreamExt;
use serde_json::json;
use std::collections::VecDeque;

pub struct SseParser {
    buffer: String,
    delimiter: &'static str,
    is_openai: bool,
    pending_tool_calls: VecDeque<ToolCall>,
    current_tool: Option<(String, String, String)>,
    args_buffer: String,
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
            let delta = choice.get("delta")?;

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                if tool_calls.is_empty() {
                    return None;
                }
                for tc in tool_calls {
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
                    let args_val = function.get("arguments");

                    let mut use_id = id;
                    let mut use_name = name;
                    if use_id.is_empty() || use_name.is_empty() {
                        if let Some((stored_id, stored_name, _)) = &self.current_tool {
                            if use_id.is_empty() {
                                use_id = stored_id.clone();
                            }
                            if use_name.is_empty() {
                                use_name = stored_name.clone();
                            }
                        }
                    }

                    if let Some(v) = args_val {
                        if !v.is_string() {
                            if !use_name.trim().is_empty() {
                                self.pending_tool_calls.push_back(ToolCall {
                                    id: use_id.into(),
                                    name: use_name.into(),
                                    arguments: v.clone(),
                                });
                            }
                            self.current_tool = None;
                            self.args_buffer.clear();
                            continue;
                        }
                    }

                    let args_str = args_val.and_then(|v| v.as_str()).unwrap_or("");

                    // Some providers send tool call deltas in fragments where the first
                    // chunk may contain name/id but empty arguments. Keep state so later
                    // argument chunks can be associated to the right tool.
                    if args_str.is_empty() {
                        if !use_name.trim().is_empty() {
                            self.current_tool =
                                Some((use_id.clone(), use_name.clone(), use_id.clone()));
                        }
                        continue;
                    }

                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_str) {
                        if !use_name.trim().is_empty() {
                            self.pending_tool_calls.push_back(ToolCall {
                                id: use_id.into(),
                                name: use_name.into(),
                                arguments: args,
                            });
                        }
                        self.current_tool = None;
                        self.args_buffer.clear();
                        continue;
                    }

                    match &self.current_tool {
                        Some((stored_id, stored_name, _))
                            if *stored_id == use_id && *stored_name == use_name =>
                        {
                            self.args_buffer.push_str(args_str);
                        }
                        _ => {
                            self.current_tool =
                                Some((use_id.clone(), use_name.clone(), use_id.clone()));
                            self.args_buffer.clear();
                            self.args_buffer.push_str(args_str);
                        }
                    }

                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&self.args_buffer) {
                        if !use_name.trim().is_empty() {
                            self.pending_tool_calls.push_back(ToolCall {
                                id: use_id.into(),
                                name: use_name.into(),
                                arguments: args,
                            });
                            self.current_tool = None;
                            self.args_buffer.clear();
                        }
                    }
                }
                if let Some(tc) = self.pending_tool_calls.pop_front() {
                    return Some(Ok(ChatEvent::ToolCall(tc)));
                }
                return None;
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
    let (current_tool, args_buffer) = pop_openai_state(buffer);
    let mut parser = SseParser::new(true);
    std::mem::swap(&mut parser.buffer, buffer);
    parser.current_tool = current_tool;
    parser.args_buffer = args_buffer;
    let result = parser.parse();
    let mut rebuilt = String::new();
    rebuilt.push_str(&parser.buffer);
    if !parser.pending_tool_calls.is_empty() {
        queue_remaining_tool_calls(&mut rebuilt, &parser.pending_tool_calls);
    }
    queue_openai_state(&mut rebuilt, &parser.current_tool, &parser.args_buffer);
    *buffer = rebuilt;
    result
}

fn pop_queued_tool_call(buffer: &mut String) -> Option<ToolCall> {
    const PREFIX: &str = "\n__TC__:";
    let prefix_idx = buffer.find(PREFIX)?;
    let json_start = prefix_idx + PREFIX.len();
    let json_end = buffer[json_start..]
        .find(PREFIX)
        .map(|i| json_start + i)
        .unwrap_or(buffer.len());

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

fn pop_openai_state(buffer: &mut String) -> (Option<(String, String, String)>, String) {
    const PREFIX: &str = "\n__OAI_STATE__:";
    let Some(prefix_idx) = buffer.find(PREFIX) else {
        return (None, String::new());
    };
    let json_start = prefix_idx + PREFIX.len();
    let json_str = &buffer[json_start..];
    let parsed = serde_json::from_str::<serde_json::Value>(json_str).ok();
    buffer.truncate(prefix_idx);
    let Some(val) = parsed else {
        return (None, String::new());
    };
    let current_tool = val
        .get("current_tool")
        .and_then(|v| v.as_array())
        .and_then(|a| {
            if a.len() == 3 {
                Some((
                    a[0].as_str()?.to_string(),
                    a[1].as_str()?.to_string(),
                    a[2].as_str()?.to_string(),
                ))
            } else {
                None
            }
        });
    let args_buffer = val
        .get("args_buffer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    (current_tool, args_buffer)
}

fn queue_openai_state(
    buffer: &mut String,
    current_tool: &Option<(String, String, String)>,
    args_buffer: &str,
) {
    if current_tool.is_none() && args_buffer.is_empty() {
        return;
    }
    const PREFIX: &str = "\n__OAI_STATE__:";
    let current_tool_json = current_tool.as_ref().map(|(a, b, c)| json!([a, b, c]));
    let payload = json!({
        "current_tool": current_tool_json,
        "args_buffer": args_buffer,
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
                    if let Ok(args) = serde_json::from_str::<serde_json::Value>(&args_str) {
                        return Some(Ok(ChatEvent::ToolCall(ToolCall {
                            id: id.into(),
                            name: name.into(),
                            arguments: args,
                        })));
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
        let delta = choice.get("delta")?;

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
        u.total_tokens = u.input_tokens + u.output_tokens;
        return Some(Ok(ChatEvent::Finish {
            stop_reason: "stop".to_string().into(),
            usage: u,
        }));
    }

    None
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
