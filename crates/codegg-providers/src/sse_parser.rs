use crate::error::ProviderError;
use crate::{ChatEvent, EventStream, TokenUsage, ToolCall, MAX_BUFFER_SIZE};
use futures_util::StreamExt;
use serde_json::json;
use std::collections::VecDeque;

pub fn parse_anthropic_buffer(buffer: &mut String) -> Option<Result<ChatEvent, ProviderError>> {
    parse_anthropic_buffer_with_state(buffer, &mut None, &mut String::new())
}

pub fn parse_anthropic_buffer_with_state(
    buffer: &mut String,
    current_tool: &mut Option<(String, String, String)>,
    args_buffer: &mut String,
) -> Option<Result<ChatEvent, ProviderError>> {
    let idx = buffer.find("\n\n")?;
    let chunk = buffer.drain(..idx).collect::<String>();
    buffer.drain(..2);
    let mut event_type = None;
    let mut data = None;
    for line in chunk.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            event_type = Some(value);
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = Some(value);
        }
    }
    parse_anthropic_event_with_state(
        event_type?,
        data?,
        current_tool,
        args_buffer,
        &mut VecDeque::new(),
    )
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
                        u.input_tokens = usize::try_from(input_tokens).unwrap_or(usize::MAX);
                    }
                    if let Some(output_tokens) = usage.get("output_tokens").and_then(|v| v.as_u64())
                    {
                        u.output_tokens = usize::try_from(output_tokens).unwrap_or(usize::MAX);
                    }
                    if let Some(cached) = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                    {
                        u.cached_tokens = Some(usize::try_from(cached).unwrap_or(usize::MAX));
                    }
                    u.total_tokens = u.input_tokens + u.output_tokens;
                    if std::env::var("CODEGG_DIAG_USAGE").is_ok() {
                        eprintln!(
                            "[usage-dump] message_delta.usage = {}",
                            serde_json::to_string(usage).unwrap_or_default()
                        );
                    }
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

pub fn create_sse_stream<F, Fut>(
    send_request: F,
    parse_buffer: fn(&mut String) -> Option<Result<ChatEvent, ProviderError>>,
) -> Result<EventStream, ProviderError>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<eggfetch_core::Response, ProviderError>> + Send,
{
    // Raw bytes awaiting UTF-8 decoding.
    let pending_bytes: Vec<u8> = Vec::new();
    // Decoded text awaiting parsing.
    let buffer = String::new();
    let mut response = tokio::runtime::Handle::current().block_on(send_request())?;

    if response.status() == http::StatusCode::TOO_MANY_REQUESTS {
        return Err(ProviderError::rate_limit_from_headers(response.headers()));
    }

    if !response.status().is_success() {
        let status = response.status();
        if status == http::StatusCode::UNAUTHORIZED || status == http::StatusCode::FORBIDDEN {
            let err_text = tokio::runtime::Handle::current()
                .block_on(response.text())
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(ProviderError::from_http_status(status.as_u16(), err_text));
        }
        let err_text = tokio::runtime::Handle::current()
            .block_on(response.text())
            .unwrap_or_else(|_| "unknown error".to_string());
        return Err(ProviderError::api(
            status.as_u16().to_string(),
            format!("HTTP {}: {}", status, err_text),
        ));
    }

    let stream = response.bytes_stream().map_err(ProviderError::from)?;

    Ok(Box::pin(futures_util::stream::unfold(
        (stream, pending_bytes, buffer),
        move |(mut stream, mut pending_bytes, mut buffer)| async move {
            loop {
                if let Some(event) = parse_buffer(&mut buffer) {
                    return Some((event, (stream, pending_bytes, buffer)));
                }

                let chunk = stream.next().await;
                match chunk {
                    Some(Ok(bytes)) => {
                        // Buffer raw bytes and decode incrementally so a
                        // multi-byte character split across chunk
                        // boundaries is never corrupted.
                        pending_bytes.extend_from_slice(&bytes);
                        append_decoded_utf8(&mut pending_bytes, &mut buffer);
                        if pending_bytes.len() + buffer.len() > MAX_BUFFER_SIZE {
                            return Some((
                                Err(ProviderError::Stream(
                                    "response buffer exceeded limit".to_string(),
                                )),
                                (stream, pending_bytes, buffer),
                            ));
                        }
                    }
                    Some(Err(e)) => {
                        return Some((
                            Err(ProviderError::Stream(e.to_string())),
                            (stream, pending_bytes, buffer),
                        ));
                    }
                    None => {
                        // End of stream: flush any trailing partial
                        // sequence (replacement chars are the best we
                        // can do for a truncated tail).
                        if !pending_bytes.is_empty() {
                            let rest = std::mem::take(&mut pending_bytes);
                            buffer.push_str(&String::from_utf8_lossy(&rest));
                        }
                        if buffer.is_empty() {
                            return None;
                        }
                        if let Some(event) = parse_buffer(&mut buffer) {
                            return Some((event, (stream, pending_bytes, buffer)));
                        }
                        return None;
                    }
                }
            }
        },
    )))
}

/// Decode as much of the buffered bytes as possible without splitting a
/// multi-byte UTF-8 character. Valid bytes are appended to `text` and
/// drained from `buf`; an incomplete trailing sequence is kept for the
/// next chunk; a genuinely invalid sequence becomes U+FFFD instead of
/// being spliced mid-character.
fn append_decoded_utf8(buf: &mut Vec<u8>, text: &mut String) {
    while !buf.is_empty() {
        match std::str::from_utf8(buf) {
            Ok(s) => {
                text.push_str(s);
                buf.clear();
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if valid_up_to > 0 {
                    if let Ok(s) = std::str::from_utf8(&buf[..valid_up_to]) {
                        text.push_str(s);
                    }
                    buf.drain(..valid_up_to);
                }
                match e.error_len() {
                    Some(invalid_len) => {
                        text.push('\u{FFFD}');
                        buf.drain(..invalid_len.min(buf.len()));
                    }
                    None => break,
                }
            }
        }
    }
}
