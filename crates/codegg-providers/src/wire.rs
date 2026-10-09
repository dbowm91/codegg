//! CodeGG semantic bridge to the pinned `eggpool-wire` sans-I/O kernel.
//!
//! Provider transports remain CodeGG-owned; standard provider-family grammar
//! and streaming decoding are delegated to the pinned shared kernel.

use eggpool_wire::{
    encode_request_for_surface,
    ir::{
        CanonicalBlockKind, CanonicalContentBlock, CanonicalEvent, CanonicalEventType,
        CanonicalMessage, CanonicalRequest, CanonicalRole, CanonicalTool, CanonicalToolChoice,
        MediaSource, ReasoningMode, RequestPresence, ToolChoiceMode,
    },
    profile::WireSurface,
    CanonicalToolCallAccumulator, RequestEncodeOptions,
};
use futures_util::{stream::unfold, Stream, StreamExt};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeSet, VecDeque},
    fmt::Display,
    time::Duration,
};

use crate::openai_compatible::ToolChoice;
use crate::{
    ChatEvent, ChatRequest, ContentPart, Message, ProviderError, ResponseFormat, TokenUsage,
    ToolCall,
};

/// Project a CodeGG request into canonical wire semantics. Provider-private
/// reasoning is omitted: callers must opt in through a later resolved policy
/// projection before it can be represented here.
pub fn canonical_request(
    request: &ChatRequest,
    tool_choice: Option<&ToolChoice>,
) -> CanonicalRequest {
    let messages = crate::project_tool_call_history(&request.messages)
        .iter()
        .map(|message| {
            canonical_message(
                message,
                request
                    .context
                    .wire_policy
                    .as_deref()
                    .is_some_and(|policy| policy.allow_private_reasoning_round_trip),
            )
        })
        .collect();
    let mut canonical = CanonicalRequest::from_canonical(&request.model, messages);
    canonical.stream = true;
    canonical.max_output_tokens = request.max_tokens.map(|value| value as u64);
    canonical.temperature = request.temperature;
    canonical.top_p = request.top_p;
    canonical.tools = request
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|tool| CanonicalTool {
            kind: eggpool_wire::ir::CanonicalToolKind::Function,
            name: tool.name.clone(),
            description: Some(tool.description.clone()),
            parameters: tool.parameters.as_object().cloned().unwrap_or_default(),
            cache_control: None,
            defer_loading: tool.defer_loading,
        })
        .collect();
    canonical.tool_choice = request
        .context
        .wire_policy
        .as_deref()
        .and_then(|policy| policy.tool_choice.as_deref())
        .and_then(|choice| match choice {
            "auto" => Some(CanonicalToolChoice {
                mode: ToolChoiceMode::Auto,
                function_name: None,
            }),
            "required" => Some(CanonicalToolChoice {
                mode: ToolChoiceMode::Required,
                function_name: None,
            }),
            "none" => Some(CanonicalToolChoice {
                mode: ToolChoiceMode::None,
                function_name: None,
            }),
            _ => None,
        })
        .or_else(|| {
            tool_choice.map(|choice| match choice {
                ToolChoice::Auto => CanonicalToolChoice {
                    mode: ToolChoiceMode::Auto,
                    function_name: None,
                },
                ToolChoice::Required => CanonicalToolChoice {
                    mode: ToolChoiceMode::Required,
                    function_name: None,
                },
                ToolChoice::None => CanonicalToolChoice {
                    mode: ToolChoiceMode::None,
                    function_name: None,
                },
                ToolChoice::Specific(name) => CanonicalToolChoice {
                    mode: ToolChoiceMode::Function,
                    function_name: Some(name.clone()),
                },
            })
        });
    canonical.parallel_tool_calls = request
        .context
        .wire_policy
        .as_deref()
        .and_then(|policy| policy.max_parallel_tools)
        .map(|max| max != 1);
    canonical.reasoning = if request.thinking_budget.is_some() || request.reasoning_effort.is_some()
    {
        eggpool_wire::ir::ReasoningIntent {
            requested: Some(true),
            mode: if request.thinking_budget.is_some() {
                ReasoningMode::FixedBudget
            } else {
                ReasoningMode::Effort
            },
            effort: request.reasoning_effort.clone(),
            budget_tokens: request.thinking_budget.map(|value| value as u64),
            explicit_disable: false,
        }
    } else {
        eggpool_wire::ir::ReasoningIntent::default()
    };
    canonical.response_format = request.response_format.as_ref().map(response_format);
    canonical.presence = RequestPresence {
        stream: eggpool_wire::ir::Presence::Value(true),
        max_output_tokens: presence(request.max_tokens.map(|v| v as u64)),
        temperature: presence(request.temperature),
        top_p: presence(request.top_p),
        stop: eggpool_wire::ir::Presence::Missing,
        response_format: presence(canonical.response_format.clone()),
        parallel_tool_calls: presence(canonical.parallel_tool_calls),
    };
    canonical
}

/// Encode a canonical request for a finite shared wire surface.
pub fn encode(
    request: &CanonicalRequest,
    surface: WireSurface,
    include_stream_usage: bool,
) -> Result<Value, ProviderError> {
    let mut value = encode_request_for_surface(
        request,
        surface,
        &RequestEncodeOptions {
            include_stream_usage,
        },
    )
    .map_err(|_| ProviderError::api("wire_encode", "shared provider request encoding failed"))?
    .value;
    // CodeGG's direct OpenAI Chat contract still uses `max_tokens`. The
    // upstream canonical codec uses the newer `max_completion_tokens` form;
    // keep this closed compatibility correction at the application bridge.
    if surface == WireSurface::OpenaiChatCompletions {
        if let Some(value) = value.as_object_mut() {
            if let Some(max_tokens) = value.remove("max_completion_tokens") {
                value.insert("max_tokens".into(), max_tokens);
            }
        }
    }
    Ok(value)
}

/// Encode the common OpenAI Chat request grammar through the shared kernel.
pub fn encode_openai_chat(
    request: &ChatRequest,
    tool_choice: Option<&ToolChoice>,
    include_stream_usage: bool,
) -> Result<Value, ProviderError> {
    let mut canonical = canonical_request(request, tool_choice);
    if request.tools.is_none() {
        // The shared codec validates that a selected tool choice has a tool
        // collection. Existing CodeGG providers omitted tool choice entirely
        // when the request had no tools field.
        canonical.tool_choice = None;
    }
    let mut body = encode(
        &canonical,
        WireSurface::OpenaiChatCompletions,
        include_stream_usage,
    )?;
    // Historical CodeGG providers emitted tool_choice only when callers
    // supplied a tools field, including an explicitly empty one.
    if request.tools.is_none() {
        if let Some(object) = body.as_object_mut() {
            object.remove("tool_choice");
        }
    } else if request.tools.as_ref().is_some_and(Vec::is_empty) {
        // The canonical IR normalizes absent and empty tool collections;
        // preserve the existing OpenAI Chat distinction at the CodeGG edge.
        body["tools"] = Value::Array(Vec::new());
    }
    let assistant_text: Vec<String> = crate::project_tool_call_history(&request.messages)
        .iter()
        .filter_map(|message| match message {
            Message::Assistant { content, .. } => Some(
                content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for (message, text) in messages
            .iter_mut()
            .filter(|message| message.get("role").and_then(Value::as_str) == Some("assistant"))
            .zip(assistant_text)
        {
            // Preserve CodeGG's established Chat Completions assistant
            // content spelling, including text accompanying tool calls.
            message["content"] = Value::String(text);
        }
    }
    Ok(body)
}

/// Encode the common OpenAI Responses request grammar through the shared
/// kernel, for a direct **stateless** provider request.
///
/// This is deliberately the ordinary provider surface supplied by
/// `eggpool-wire`. It is not CodeGG's hosted/stateful Responses program
/// subsystem (`crate::responses_api`), which owns a different contract and is
/// explicitly out of scope for multi-surface dispatch.
pub fn encode_openai_responses(
    request: &ChatRequest,
    include_stream_usage: bool,
) -> Result<Value, ProviderError> {
    let mut canonical = canonical_request(request, None);
    if request.tools.is_none() {
        // The shared codec validates that a selected tool choice has a tool
        // collection; without a tools field there is nothing to select from.
        canonical.tool_choice = None;
    }
    encode(
        &canonical,
        WireSurface::OpenaiResponses,
        include_stream_usage,
    )
}

/// Adapt a direct stateless Responses SSE byte stream through the shared
/// kernel. Cancellation and chunk-deadline policy remain CodeGG transport
/// concerns and are supplied by the caller.
pub fn openai_responses_stream<S, B, E>(
    stream: S,
    chunk_timeout: Option<Duration>,
    policy: Option<std::sync::Arc<crate::ProviderWirePolicy>>,
) -> crate::EventStream
where
    S: Stream<Item = Result<B, E>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send + 'static,
    E: Display + Send + 'static,
{
    shared_stream(
        stream,
        eggpool_wire::codec::StreamAdapterKind::OpenaiResponsesSse,
        chunk_timeout,
        policy,
    )
}

/// Encode the Anthropic Messages request grammar for a **direct** provider.
///
/// This is a shared codec helper, deliberately not a provider identity. A
/// provider whose models resolve to the Messages surface (for example OpenCode
/// Go) reuses this encoding without inheriting the dedicated Anthropic
/// provider's base-URL or `anthropic-version` assumptions; those are transport
/// concerns the caller owns.
pub fn encode_anthropic_messages(request: &ChatRequest) -> Result<Value, ProviderError> {
    let mut canonical = canonical_request(request, None);
    if let Some(system) = request.system.as_deref().filter(|_| {
        !canonical
            .messages
            .iter()
            .any(|message| message.role == eggpool_wire::ir::CanonicalRole::System)
    }) {
        canonical.messages.insert(0, system_message(system));
    }
    let mut body = encode(&canonical, WireSurface::AnthropicMessages, false)?;
    // CodeGG's established Messages contract always represents message content
    // as typed blocks, even when the shared codec can compact text.
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for message in messages {
            if let Some(text) = message.get("content").and_then(Value::as_str) {
                message["content"] = serde_json::json!([{ "type": "text", "text": text }]);
            }
        }
    }
    Ok(body)
}

/// Preserve a provider's previously qualified OpenAI-compatible omission
/// contract for optional fields it does not accept.
pub fn omit_openai_fields(body: &mut Value, fields: &[&str]) {
    if let Some(object) = body.as_object_mut() {
        for field in fields {
            object.remove(*field);
        }
    }
}

/// Adapt an OpenAI Chat byte stream through the shared SSE decoder. Optional
/// chunk timeout remains transport policy and is supplied by the wrapper.
pub fn openai_chat_stream<S, B, E>(
    stream: S,
    policy: Option<std::sync::Arc<crate::ProviderWirePolicy>>,
    chunk_timeout: Option<Duration>,
) -> crate::EventStream
where
    S: Stream<Item = Result<B, E>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send + 'static,
    E: Display + Send + 'static,
{
    shared_stream(
        stream,
        eggpool_wire::codec::StreamAdapterKind::OpenaiChatSse,
        chunk_timeout,
        policy,
    )
}

/// Adapt a provider byte stream with the pinned kernel's family-specific SSE decoder.
pub fn shared_stream<S, B, E>(
    stream: S,
    adapter: eggpool_wire::codec::StreamAdapterKind,
    chunk_timeout: Option<Duration>,
    policy: Option<std::sync::Arc<crate::ProviderWirePolicy>>,
) -> crate::EventStream
where
    S: Stream<Item = Result<B, E>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send + 'static,
    E: Display + Send + 'static,
{
    let openai_policy = adapter == eggpool_wire::codec::StreamAdapterKind::OpenaiChatSse;
    let decoder = SharedStreamDecoder::new(adapter);
    Box::pin(unfold(
        (stream, decoder, VecDeque::new(), false),
        move |(mut stream, mut decoder, mut pending, mut finished)| {
            let policy = policy.clone();
            async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        let event = if openai_policy {
                            normalize_openai_event(event, policy.as_deref())
                        } else {
                            normalize_private_reasoning_event(event, policy.as_deref())
                        };
                        if let Some(event) = event {
                            return Some((event, (stream, decoder, pending, finished)));
                        }
                        continue;
                    }
                    if finished {
                        return None;
                    }
                    let next = if let Some(timeout) = chunk_timeout {
                        match tokio::time::timeout(timeout, stream.next()).await {
                            Ok(next) => next,
                            Err(_) => {
                                return Some((
                                    Err(ProviderError::Stream("stream chunk timeout".into())),
                                    (stream, decoder, pending, true),
                                ));
                            }
                        }
                    } else {
                        stream.next().await
                    };
                    match next {
                        Some(Ok(bytes)) => match decoder.push(bytes.as_ref()) {
                            Ok(events) => pending.extend(events.into_iter().map(Ok)),
                            Err(error) => {
                                pending.push_back(Err(error));
                                finished = true;
                            }
                        },
                        Some(Err(error)) => {
                            pending.push_back(Err(ProviderError::Stream(error.to_string())));
                            finished = true;
                        }
                        None => {
                            finished = true;
                            match decoder.finish() {
                                Ok(events) => pending.extend(events.into_iter().map(Ok)),
                                Err(error) => pending.push_back(Err(error)),
                            }
                        }
                    }
                }
            }
        },
    ))
}

fn normalize_private_reasoning_event(
    event: Result<ChatEvent, ProviderError>,
    policy: Option<&crate::ProviderWirePolicy>,
) -> Option<Result<ChatEvent, ProviderError>> {
    match event {
        Ok(ChatEvent::ReasoningDelta(_))
            if !policy.is_some_and(|policy| policy.include_reasoning_content) =>
        {
            None
        }
        other => Some(other),
    }
}

fn normalize_openai_event(
    event: Result<ChatEvent, ProviderError>,
    policy: Option<&crate::ProviderWirePolicy>,
) -> Option<Result<ChatEvent, ProviderError>> {
    let Some(policy) = policy else {
        return Some(event);
    };
    match event {
        Ok(ChatEvent::ReasoningDelta(_)) if !policy.include_reasoning_content => {
            // Preserve the existing conservative policy: private deltas are
            // visible only to explicitly opted-in compatible adapters.
            None
        }
        Ok(ChatEvent::ToolCall(mut call)) => {
            let wire_name = call.name.to_string();
            let canonical = policy
                .tool_aliases
                .iter()
                .find_map(|(name, alias)| (alias == &wire_name).then_some(name.as_str()))
                .unwrap_or(wire_name.as_str());
            if let Some(args) = call.arguments.as_object_mut() {
                if let Some(aliases) = policy.argument_aliases.get(&wire_name) {
                    for (name, alias) in aliases {
                        if let Some(value) = args.remove(alias) {
                            args.insert(name.clone(), value);
                        }
                    }
                }
            }
            call.name = canonical.to_string().into();
            Some(Ok(ChatEvent::ToolCall(call)))
        }
        other => Some(other),
    }
}

/// Convert one decoded canonical stream event to CodeGG events, accumulating
/// tool-call fragments until the shared kernel declares a complete call.
pub fn push_event(
    accumulator: &mut CanonicalToolCallAccumulator,
    event: &CanonicalEvent,
) -> Result<Vec<ChatEvent>, ProviderError> {
    let mut output = Vec::new();
    match event.event_type {
        CanonicalEventType::TextDelta => {
            if let Some(delta) = event.delta.as_ref() {
                output.push(ChatEvent::TextDelta(delta.clone().into()));
            }
        }
        CanonicalEventType::ReasoningDelta => {
            if let Some(delta) = event.delta.as_ref() {
                output.push(ChatEvent::ReasoningDelta(delta.clone().into()));
            }
        }
        CanonicalEventType::Error => {
            return Err(ProviderError::api(
                "provider_stream",
                "provider stream reported an error",
            ));
        }
        _ => {}
    }
    let completed = if matches!(
        event.event_type,
        CanonicalEventType::ToolCallStart
            | CanonicalEventType::ToolCallArgumentsDelta
            | CanonicalEventType::ToolCallStop
            | CanonicalEventType::ContentStop
            | CanonicalEventType::ResponseComplete
            | CanonicalEventType::ResponseIncomplete
            | CanonicalEventType::Error
    ) {
        match accumulator.push(event) {
            Ok(completed) => completed,
            // `ContentStop` / `ResponseComplete` / `ResponseIncomplete` /
            // `Error` describe the *stream*, not a tool call, and upstreams
            // close with more than one terminator shape: an Anthropic reply
            // sends `message_delta` and then `message_stop`, both of which the
            // shared decoder turns into `ResponseComplete`, so the accumulator
            // sees a second terminal event and reports post-terminal
            // corruption. Both failures below are expected dialect, not
            // corruption: a stream-terminating event with no identity has
            // nothing to flush, and a repeated terminator is already
            // terminal. Genuine `ToolCallStart`/`ArgumentsDelta`/`ToolCallStop`
            // keep failing closed, so real tool-call corruption is still
            // caught.
            Err(
                err @ (eggpool_wire::tool_calls::ToolCallAccumulatorError::MissingIdentity
                | eggpool_wire::tool_calls::ToolCallAccumulatorError::PostTerminalData),
            ) if matches!(
                event.event_type,
                CanonicalEventType::ContentStop
                    | CanonicalEventType::ResponseComplete
                    | CanonicalEventType::ResponseIncomplete
                    | CanonicalEventType::Error
            ) =>
            {
                let _ = err;
                // The shared accumulator latches `terminal` on *any* push error and
                // drops its in-flight calls, including the benign dialects tolerated
                // above. Swallowing the error while leaving that state behind would let
                // one tolerated event reject every genuine tool call still to come, so
                // rebuild the accumulator and let the rest of the stream decode.
                *accumulator = CanonicalToolCallAccumulator::new();
                Vec::new()
            }
            Err(err) => {
                return Err(ProviderError::api(
                    "provider_stream",
                    format!("provider stream tool-call state was invalid: {err}"),
                ));
            }
        }
    } else {
        Vec::new()
    };
    for call in completed {
        let arguments: Value = serde_json::from_str(&call.arguments).map_err(|_| {
            ProviderError::api(
                "malformed_tool_arguments",
                "provider tool arguments were malformed",
            )
        })?;
        output.push(ChatEvent::ToolCall(ToolCall {
            id: call
                .call_id
                .unwrap_or_else(|| call.source_index.map(|v| v.to_string()).unwrap_or_default())
                .into(),
            name: call.name.into(),
            arguments,
        }));
    }
    Ok(output)
}

/// Per-SSE-frame byte bound for the shared stream decoder.
///
/// The shared default (`eggpool_wire::stream::MAX_SSE_FRAME_BYTES`) is 64 KiB.
/// That is a fine bound for incremental text deltas, but OpenAI-compatible
/// Responses streams terminate with a single `response.completed` frame that
/// embeds the entire response object — including full tool-call arguments and
/// output — so a perfectly normal turn trips the default limit and the whole
/// turn fails with "SSE frame exceeded 65536 bytes".
///
/// 8 MiB keeps the decoder's memory bounded (it is a guard, not a budget) while
/// comfortably covering a large completed response. It is applied here rather
/// than by editing the shared crate so the bound is owned by the caller that
/// knows which wire surface it is decoding.
const MAX_SSE_FRAME_BYTES: usize = 8 * 1024 * 1024;

/// Per-stream adapter that keeps SSE framing, canonical decoding, and bounded
/// completed-call accumulation in one request-owned value.
pub struct SharedStreamDecoder {
    decoder: eggpool_wire::stream::StreamEventDecoder,
    calls: CanonicalToolCallAccumulator,
    terminal: bool,
    finish_reason: Option<String>,
    usage: Option<eggpool_wire::ir::CanonicalUsage>,
    /// Content-block indices that opened a tool call in this stream.
    ///
    /// An Anthropic stream closes *every* content block with one `ContentStop`
    /// event, text and reasoning blocks included. Only the boundary of a block
    /// that opened a tool call describes a tool call, so the decoder records
    /// which indices actually opened one and routes only those.
    tool_call_indices: BTreeSet<usize>,
}

impl SharedStreamDecoder {
    pub fn new(adapter: eggpool_wire::codec::StreamAdapterKind) -> Self {
        Self {
            decoder: eggpool_wire::stream::StreamEventDecoder::with_limit(
                adapter,
                MAX_SSE_FRAME_BYTES,
            ),
            calls: CanonicalToolCallAccumulator::new(),
            terminal: false,
            finish_reason: None,
            usage: None,
            tool_call_indices: BTreeSet::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<ChatEvent>, ProviderError> {
        // Preserve the decoder's own reason. Collapsing every failure mode into
        // one opaque string made provider stream bugs undiagnosable in the
        // field — an oversized SSE frame, an early EOF, and a malformed event
        // all reported identically.
        let events = self.decoder.push(bytes).map_err(|e| {
            ProviderError::api(
                "provider_stream",
                format!("provider stream framing or decoding failed: {e}"),
            )
        })?;
        self.consume_events(&events)
    }

    pub fn finish(&mut self) -> Result<Vec<ChatEvent>, ProviderError> {
        let (events, summary) = self.decoder.finalize_events().map_err(|_| {
            ProviderError::api(
                "provider_stream",
                "provider stream ended with malformed framing",
            )
        })?;
        if summary.outcome != eggpool_wire::stream::StreamTerminalOutcome::Success
            || summary.parser_error_count > 0
            || summary.incomplete_frame_at_eof
        {
            return Err(ProviderError::api(
                "provider_stream",
                "provider stream did not complete cleanly",
            ));
        }
        if summary.usage.is_some() {
            self.usage = summary.usage;
        }
        let mut output = self.consume_events(&events)?;
        self.calls.finish().map_err(|_| {
            ProviderError::api(
                "provider_stream",
                "provider stream ended without complete tool-call state",
            )
        })?;
        if self.terminal {
            output.push(ChatEvent::Finish {
                stop_reason: self
                    .finish_reason
                    .take()
                    .unwrap_or_else(|| "stop".into())
                    .into(),
                usage: self.usage.as_ref().map(token_usage).unwrap_or_default(),
            });
        }
        Ok(output)
    }

    fn consume_events(
        &mut self,
        events: &[CanonicalEvent],
    ) -> Result<Vec<ChatEvent>, ProviderError> {
        let mut output = Vec::new();
        for event in events {
            if let Some(usage) = event.usage.as_ref() {
                self.usage = Some(usage.clone());
            }
            if self.terminal {
                if matches!(
                    event.event_type,
                    CanonicalEventType::Error | CanonicalEventType::ResponseIncomplete
                ) {
                    return Err(ProviderError::api(
                        "provider_stream",
                        "provider stream failed after terminal completion",
                    ));
                }
                continue;
            }
            // An Anthropic upstream closes with `message_delta` and then `message_stop`,
            // and the shared decoder decodes BOTH to a single canonical `ResponseComplete`.
            // The tool-call accumulator treats a second terminal event as post-terminal
            // corruption ("canonical events arrived after terminal completion"), so a
            // perfectly normal Anthropic turn failed at the very end of every stream.
            // Fold the duplicate terminator here; the accumulator still sees the first
            // one, which is what flushes completed tool calls.
            let duplicate_terminator =
                event.event_type == CanonicalEventType::ResponseComplete && self.terminal;
            if event.event_type == CanonicalEventType::ToolCallStart {
                if let Some(index) = event.index {
                    self.tool_call_indices.insert(index);
                }
            }
            // An Anthropic stream closes every content block with `ContentStop`, text
            // and reasoning blocks included, but only a boundary for a block that opened
            // a tool call describes one. A text block's index has no tool-call identity,
            // so handing it to the accumulator answers `MissingIdentity` — and the shared
            // accumulator latches `terminal` on *any* push error, which poisons every
            // later tool call in the same turn with "canonical events arrived after
            // terminal completion". In practice that failed every OpenCode Go turn whose
            // model narrates before it calls a tool, because the narration block closed
            // first. Route only genuine tool-call boundaries into the accumulator.
            let closes_a_tool_call = match event.event_type {
                CanonicalEventType::ContentStop => {
                    event.call_id.is_some()
                        || event
                            .index
                            .is_some_and(|index| self.tool_call_indices.contains(&index))
                }
                _ => true,
            };
            if !duplicate_terminator && closes_a_tool_call {
                output.extend(push_event(&mut self.calls, event)?);
            }
            if event.event_type == CanonicalEventType::ResponseComplete {
                self.terminal = true;
                // Keep the first non-empty stop reason: the trailing
                // `message_stop`-derived event carries none.
                if event.finish_reason.is_some() {
                    self.finish_reason = event.finish_reason.clone();
                }
            }
        }
        Ok(output)
    }
}

fn canonical_message(message: &Message, allow_private_reasoning: bool) -> CanonicalMessage {
    match message {
        Message::System { content } => {
            semantic_message(CanonicalRole::System, vec![text_block(content)])
        }
        Message::User { content } => semantic_message(
            CanonicalRole::User,
            content.iter().map(content_block).collect(),
        ),
        Message::Assistant {
            content,
            tool_calls,
        } => {
            let mut blocks: Vec<_> = content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Reasoning { text, visibility }
                        if allow_private_reasoning
                            && *visibility == crate::ReasoningVisibility::Private =>
                    {
                        Some(CanonicalContentBlock {
                            kind: CanonicalBlockKind::Reasoning,
                            text: Some(text.to_string()),
                            media: None,
                            call_id: None,
                            name: None,
                            arguments: None,
                            tool_input: None,
                            tool_kind: eggpool_wire::ir::CanonicalToolKind::Function,
                            is_error: false,
                            signature: None,
                            cache_control: None,
                            prompt_cache_breakpoint: None,
                        })
                    }
                    ContentPart::Reasoning { .. } => None,
                    _ => Some(content_block(part)),
                })
                .collect();
            for call in tool_calls {
                blocks.push(CanonicalContentBlock {
                    kind: CanonicalBlockKind::ToolCall,
                    text: None,
                    media: None,
                    call_id: Some(call.id.to_string()),
                    name: Some(call.name.to_string()),
                    arguments: Some(call.arguments.to_string()),
                    tool_input: call.arguments.as_object().cloned(),
                    tool_kind: eggpool_wire::ir::CanonicalToolKind::Function,
                    is_error: false,
                    signature: None,
                    cache_control: None,
                    prompt_cache_breakpoint: None,
                });
            }
            semantic_message(CanonicalRole::Assistant, blocks)
        }
        Message::Tool {
            tool_call_id,
            content,
        } => {
            let mut message = semantic_message(
                CanonicalRole::Tool,
                vec![CanonicalContentBlock {
                    kind: CanonicalBlockKind::ToolResult,
                    text: Some(content.to_string()),
                    media: None,
                    call_id: Some(tool_call_id.to_string()),
                    name: None,
                    arguments: None,
                    tool_input: None,
                    tool_kind: eggpool_wire::ir::CanonicalToolKind::Function,
                    is_error: false,
                    signature: None,
                    cache_control: None,
                    prompt_cache_breakpoint: None,
                }],
            );
            message.tool_call_id = Some(tool_call_id.to_string());
            message
        }
    }
}

fn content_block(part: &ContentPart) -> CanonicalContentBlock {
    match part {
        ContentPart::Text { text } => CanonicalContentBlock::text(text.as_str()),
        ContentPart::Image { image_url } => {
            let uri = image_url.url.as_str();
            let (media_type, data) = uri
                .strip_prefix("data:")
                .and_then(|rest| rest.split_once(';'))
                .and_then(|(kind, rest)| {
                    rest.strip_prefix("base64,")
                        .map(|body| (Some(kind.to_string()), Some(body.to_string())))
                })
                .unwrap_or((None, None));
            CanonicalContentBlock {
                kind: CanonicalBlockKind::Image,
                text: None,
                media: Some(MediaSource {
                    media_type,
                    data,
                    uri: if uri.starts_with("data:") {
                        None
                    } else {
                        Some(uri.to_string())
                    },
                    detail: None,
                    file_id: None,
                }),
                call_id: None,
                name: None,
                arguments: None,
                tool_input: None,
                tool_kind: eggpool_wire::ir::CanonicalToolKind::Function,
                is_error: false,
                signature: None,
                cache_control: None,
                prompt_cache_breakpoint: None,
            }
        }
        ContentPart::Reasoning { .. } => CanonicalContentBlock::text(""),
    }
}

fn semantic_message(role: CanonicalRole, content: Vec<CanonicalContentBlock>) -> CanonicalMessage {
    CanonicalMessage {
        role,
        content,
        tool_call_id: None,
        name: None,
        refusal: None,
    }
}

fn text_block(text: &str) -> CanonicalContentBlock {
    CanonicalContentBlock::text(text)
}

pub fn system_message(text: &str) -> CanonicalMessage {
    semantic_message(CanonicalRole::System, vec![text_block(text)])
}

fn response_format(format: &ResponseFormat) -> Map<String, Value> {
    let value = match format {
        ResponseFormat::JsonObject => serde_json::json!({"type": "json_object"}),
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => serde_json::json!({
            "type": "json_schema", "json_schema": {"name": name, "schema": schema, "strict": strict}
        }),
    };
    value.as_object().cloned().unwrap_or_default()
}

fn presence<T>(value: Option<T>) -> eggpool_wire::ir::Presence<T> {
    value.map_or(
        eggpool_wire::ir::Presence::Missing,
        eggpool_wire::ir::Presence::Value,
    )
}

fn token_usage(usage: &eggpool_wire::ir::CanonicalUsage) -> TokenUsage {
    let bounded = |value: Option<u64>| value.unwrap_or_default().min(usize::MAX as u64) as usize;
    TokenUsage {
        input_tokens: bounded(usage.input_tokens),
        output_tokens: bounded(usage.output_tokens),
        total_tokens: bounded(usage.total_tokens),
        reasoning_tokens: bounded(usage.reasoning_tokens),
        cached_tokens: usage
            .cached_input_tokens
            .map(|value| value.min(usize::MAX as u64) as usize),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn canonical_request_preserves_semantics_and_drops_private_reasoning_by_default() {
        let request = ChatRequest {
            messages: vec![Message::Assistant {
                content: vec![ContentPart::Reasoning {
                    text: Arc::new("secret".into()),
                    visibility: crate::ReasoningVisibility::Private,
                }],
                tool_calls: vec![],
            }],
            model: "model".into(),
            tools: None,
            system: None,
            temperature: Some(0.2),
            top_p: None,
            max_tokens: Some(64),
            response_format: Some(ResponseFormat::JsonObject),
            thinking_budget: Some(128),
            reasoning_effort: None,
            context: Default::default(),
        };
        let canonical = canonical_request(&request, None);
        assert!(canonical.messages[0].content.is_empty());
        assert_eq!(canonical.max_output_tokens, Some(64));
        let body = encode(&canonical, WireSurface::OpenaiChatCompletions, true).unwrap();
        assert_eq!(body["stream_options"]["include_usage"], true);
        assert!(body["messages"][0]["content"]
            .as_array()
            .is_some_and(Vec::is_empty));
    }

    #[test]
    fn openai_legacy_and_shared_request_paths_match_for_core_contract() {
        let request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".into()),
                }],
            }],
            model: "gpt-test".into(),
            tools: None,
            system: None,
            temperature: Some(0.3),
            top_p: Some(0.8),
            max_tokens: Some(256),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };
        let legacy = crate::openai::OpenAiProvider::new(crate::openai::OpenAiConfig::default())
            .build_body(&request);
        let shared = encode(
            &canonical_request(&request, None),
            WireSurface::OpenaiChatCompletions,
            true,
        )
        .unwrap();
        assert_eq!(legacy, shared);
    }

    #[test]
    fn production_openai_gateway_builders_keep_transport_specific_body_contracts() {
        let request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: Arc::new("hello".into()),
                }],
            }],
            model: "deployment-model".into(),
            tools: Some(vec![crate::ToolDefinition {
                name: "read".into(),
                description: "read a file".into(),
                parameters: serde_json::json!({"type":"object","properties":{}}),
                defer_loading: None,
            }]),
            system: None,
            temperature: Some(0.2),
            top_p: Some(0.7),
            max_tokens: Some(300),
            response_format: Some(ResponseFormat::JsonObject),
            thinking_budget: None,
            reasoning_effort: Some("high".into()),
            context: Default::default(),
        };
        let azure = crate::azure::AzureProvider::new("key".into(), "https://example.test".into())
            .build_body(&request)
            .unwrap();
        assert!(azure.get("model").is_none());
        assert_eq!(azure["stream_options"]["include_usage"], true);
        assert!(azure.get("tool_choice").is_none());
        assert_eq!(azure["max_tokens"], 300);
        assert_eq!(azure["temperature"], 0.2);
        assert!(azure.get("response_format").is_none());
        assert!(azure.get("reasoning_effort").is_none());

        let router = crate::openrouter::OpenRouterProvider::new("key".into())
            .build_body(&request)
            .unwrap();
        assert_eq!(router["model"], "deployment-model");
        assert!(router.get("stream_options").is_none());
        assert!(router.get("tool_choice").is_none());
        assert_eq!(router["max_tokens"], 300);
        assert!(router.get("response_format").is_none());
        assert!(router.get("reasoning_effort").is_none());

        let zen = crate::opencode_zen::OpencodeZenProvider::new("key".into())
            .build_body(&request)
            .unwrap();
        assert_eq!(zen["model"], "deployment-model");
        assert!(zen.get("stream_options").is_none());
        assert!(zen.get("tool_choice").is_none());
        assert_eq!(zen["max_tokens"], 300);
        assert!(zen.get("response_format").is_none());
        assert!(zen.get("reasoning_effort").is_none());

        let generic = crate::openai_compatible::OpenAiCompatibleProvider::simple(
            "generic",
            "Generic",
            "key",
            "https://example.test/v1",
        )
        .build_body(&request);
        assert!(generic.get("temperature").is_none());
        assert!(generic.get("top_p").is_none());
        assert!(generic.get("max_tokens").is_none());
        assert!(generic.get("response_format").is_none());
        assert!(generic.get("reasoning_effort").is_none());

        let mut empty_tools = request.clone();
        empty_tools.tools = Some(Vec::new());
        let openai = crate::openai::OpenAiProvider::new(crate::openai::OpenAiConfig::default())
            .build_body(&empty_tools);
        assert_eq!(openai["tools"], serde_json::json!([]));
        assert_eq!(openai["tool_choice"], "auto");
        let generic = crate::openai_compatible::OpenAiCompatibleProvider::simple(
            "generic",
            "Generic",
            "key",
            "https://example.test/v1",
        )
        .build_body(&empty_tools);
        assert_eq!(generic["tools"], serde_json::json!([]));
        assert!(generic.get("tool_choice").is_none());
    }

    #[test]
    fn malformed_complete_tool_arguments_fail_closed() {
        let event = |event_type, delta: Option<&str>| CanonicalEvent {
            event_type,
            response_id: None,
            model: None,
            index: Some(0),
            delta: delta.map(str::to_string),
            call_id: Some("c".into()),
            name: Some("run".into()),
            arguments: None,
            finish_reason: None,
            usage: None,
            error_type: None,
            error_message: None,
        };
        let mut accumulator = CanonicalToolCallAccumulator::new();
        push_event(
            &mut accumulator,
            &event(CanonicalEventType::ToolCallStart, None),
        )
        .unwrap();
        push_event(
            &mut accumulator,
            &event(CanonicalEventType::ToolCallArgumentsDelta, Some("not-json")),
        )
        .unwrap();
        assert!(push_event(
            &mut accumulator,
            &event(CanonicalEventType::ToolCallStop, None)
        )
        .is_err());
    }

    #[test]
    fn request_bridge_covers_system_image_tool_history_schema_and_policy() {
        let mut request = ChatRequest {
            messages: vec![
                Message::System {
                    content: Arc::new("system".into()),
                },
                Message::User {
                    content: vec![
                        ContentPart::Text {
                            text: Arc::new("inspect".into()),
                        },
                        ContentPart::Image {
                            image_url: crate::ImageUrl {
                                url: Arc::new("data:image/png;base64,AA==".into()),
                            },
                        },
                    ],
                },
                Message::Assistant {
                    content: vec![ContentPart::Reasoning {
                        text: Arc::new("private".into()),
                        visibility: crate::ReasoningVisibility::Private,
                    }],
                    tool_calls: vec![ToolCall {
                        id: Arc::new("call-1".into()),
                        name: Arc::new("shell".into()),
                        arguments: serde_json::json!({"cmd":"pwd"}),
                    }],
                },
                Message::Tool {
                    tool_call_id: Arc::new("call-1".into()),
                    content: Arc::new("/tmp".into()),
                },
            ],
            model: "model".into(),
            tools: Some(vec![crate::ToolDefinition {
                name: "shell".into(),
                description: "run shell".into(),
                parameters: serde_json::json!({"type":"object","properties":{"cmd":{"type":"string"}}}),
                defer_loading: Some(true),
            }]),
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: Some(512),
            response_format: Some(ResponseFormat::JsonSchema {
                name: "answer".into(),
                schema: serde_json::json!({"type":"object"}),
                strict: true,
            }),
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };
        request.context.wire_policy = Some(std::sync::Arc::new(crate::ProviderWirePolicy {
            allow_private_reasoning_round_trip: true,
            ..Default::default()
        }));
        let canonical = canonical_request(&request, Some(&ToolChoice::Specific("shell".into())));
        assert_eq!(canonical.messages.len(), 4);
        assert_eq!(
            canonical.messages[1].content[1]
                .media
                .as_ref()
                .unwrap()
                .media_type
                .as_deref(),
            Some("image/png")
        );
        assert_eq!(
            canonical.messages[2].content[0].kind,
            CanonicalBlockKind::Reasoning
        );
        assert_eq!(
            canonical.messages[2].content[1].kind,
            CanonicalBlockKind::ToolCall
        );
        assert_eq!(
            canonical.messages[3].tool_call_id.as_deref(),
            Some("call-1")
        );
        assert_eq!(canonical.tools[0].defer_loading, Some(true));
        let encoded = encode(&canonical, WireSurface::OpenaiChatCompletions, true).unwrap();
        assert_eq!(encoded["tool_choice"]["function"]["name"], "shell");
        assert_eq!(encoded["response_format"]["type"], "json_schema");
        assert_eq!(encoded["max_tokens"], 512);
        let google = encode(&canonical, WireSurface::GeminiGenerateContent, false).unwrap();
        assert!(
            google.to_string().contains("AA=="),
            "Gemini image payload missing: {google}"
        );
    }

    #[test]
    fn anthropic_and_gemini_surface_projection_is_structural_and_provider_native() {
        let request = ChatRequest {
            messages: vec![
                Message::System {
                    content: Arc::new("system instruction".into()),
                },
                Message::User {
                    content: vec![ContentPart::Text {
                        text: Arc::new("hello".into()),
                    }],
                },
            ],
            model: "model".into(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: Some(64),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        };
        let canonical = canonical_request(&request, None);
        let anthropic = encode(&canonical, WireSurface::AnthropicMessages, false).unwrap();
        assert_eq!(anthropic["system"], "system instruction");
        assert_eq!(anthropic["messages"][0]["role"], "user");
        let gemini = encode(&canonical, WireSurface::GeminiGenerateContent, false).unwrap();
        assert!(gemini.get("contents").is_some());
        assert!(gemini.get("systemInstruction").is_some());
    }

    #[test]
    fn interleaved_complete_tool_calls_are_emitted_only_after_their_stop() {
        let event =
            |event_type, index, id: &str, name: Option<&str>, delta: Option<&str>| CanonicalEvent {
                event_type,
                response_id: None,
                model: None,
                index: Some(index),
                delta: delta.map(str::to_string),
                call_id: Some(id.into()),
                name: name.map(str::to_string),
                arguments: None,
                finish_reason: None,
                usage: None,
                error_type: None,
                error_message: None,
            };
        let mut accumulator = CanonicalToolCallAccumulator::new();
        for call in [
            event(CanonicalEventType::ToolCallStart, 0, "a", Some("one"), None),
            event(CanonicalEventType::ToolCallStart, 1, "b", Some("two"), None),
            event(
                CanonicalEventType::ToolCallArgumentsDelta,
                0,
                "a",
                None,
                Some("{"),
            ),
            event(
                CanonicalEventType::ToolCallArgumentsDelta,
                1,
                "b",
                None,
                Some("{}"),
            ),
            event(
                CanonicalEventType::ToolCallArgumentsDelta,
                0,
                "a",
                None,
                Some("}"),
            ),
        ] {
            assert!(push_event(&mut accumulator, &call).unwrap().is_empty());
        }
        let first = push_event(
            &mut accumulator,
            &event(CanonicalEventType::ToolCallStop, 1, "b", None, None),
        )
        .unwrap();
        assert!(matches!(&first[0], ChatEvent::ToolCall(call) if call.id.as_str() == "b"));
        let second = push_event(
            &mut accumulator,
            &event(CanonicalEventType::ToolCallStop, 0, "a", None, None),
        )
        .unwrap();
        assert!(matches!(&second[0], ChatEvent::ToolCall(call) if call.id.as_str() == "a"));
    }

    #[test]
    fn usage_counters_map_without_integer_wraparound() {
        let usage = eggpool_wire::ir::CanonicalUsage {
            input_tokens: Some(u64::MAX),
            output_tokens: Some(7),
            total_tokens: Some(10),
            cached_input_tokens: Some(3),
            cache_read_input_tokens: Some(3),
            cache_creation_input_tokens: None,
            cache_write_input_tokens: None,
            reasoning_tokens: Some(2),
            cache_counter_status: eggpool_wire::ir::CacheCounterStatus::Reported,
        };
        let mapped = token_usage(&usage);
        assert_eq!(mapped.input_tokens, usize::MAX);
        assert_eq!(mapped.output_tokens, 7);
        assert_eq!(mapped.total_tokens, 10);
        assert_eq!(mapped.cached_tokens, Some(3));
        assert_eq!(mapped.reasoning_tokens, 2);
    }

    /// Byte-exact shape OpenCode Go returns on its Anthropic-compatible
    /// `/messages` surface when a model narrates and then calls a tool:
    /// a text block closes before the tool block opens.
    ///
    /// The text block's `content_block_stop` describes no tool call, so it must
    /// not reach the shared tool-call accumulator. It used to: the accumulator
    /// answered `MissingIdentity` for the text block's index and latched
    /// `terminal` on that push error, which turned every later tool call in the
    /// same turn into "canonical events arrived after terminal completion" and
    /// failed every OpenCode Go turn that narrates before acting.
    const OPENCODE_GO_ANTHROPIC_NARRATE_THEN_ACT: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"type\":\"message\",",
        "\"role\":\"assistant\",\"stop_reason\":null,\"content\":[],\"usage\":",
        "{\"input_tokens\":264,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":",
        "{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":",
        "{\"type\":\"text_delta\",\"text\":\"I'll check that.\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":",
        "{\"type\":\"tool_use\",\"id\":\"call_1\",\"name\":\"get_weather\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":",
        "{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\": \\\"Paris\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":",
        "{\"input_tokens\":264,\"output_tokens\":36}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    #[test]
    fn anthropic_text_block_closing_before_a_tool_call_does_not_fail_the_turn() {
        let mut decoder =
            SharedStreamDecoder::new(eggpool_wire::codec::StreamAdapterKind::AnthropicMessagesSse);
        let mut output = decoder
            .push(OPENCODE_GO_ANTHROPIC_NARRATE_THEN_ACT.as_bytes())
            .unwrap();
        output.extend(decoder.finish().unwrap());

        assert!(
            output
                .iter()
                .any(|event| matches!(event, ChatEvent::TextDelta(_))),
            "narration must still reach the turn: {output:?}"
        );
        let call = output
            .iter()
            .find_map(|event| match event {
                ChatEvent::ToolCall(call) => Some(call),
                _ => None,
            })
            .expect("tool call survives a preceding text block");
        assert_eq!(call.name.as_str(), "get_weather");
        assert_eq!(call.arguments["city"], "Paris");
        assert!(
            matches!(output.last(), Some(ChatEvent::Finish { .. })),
            "turn must still terminate: {output:?}"
        );
    }

    #[test]
    fn anthropic_text_block_then_tool_call_is_chunk_boundary_independent() {
        let bytes = OPENCODE_GO_ANTHROPIC_NARRATE_THEN_ACT.as_bytes();
        let mut expected: Option<Vec<String>> = None;
        for split in eggpool_wire::sse_split_points(bytes.len()) {
            let mut decoder = SharedStreamDecoder::new(
                eggpool_wire::codec::StreamAdapterKind::AnthropicMessagesSse,
            );
            let mut output = decoder.push(&bytes[..split]).unwrap();
            output.extend(decoder.push(&bytes[split..]).unwrap());
            output.extend(decoder.finish().unwrap());
            let rendered: Vec<_> = output.iter().map(|event| format!("{event:?}")).collect();
            if let Some(previous) = &expected {
                assert_eq!(previous, &rendered);
            } else {
                expected = Some(rendered);
            }
        }
    }

    #[test]
    fn upstream_openai_stream_conformance_is_chunk_boundary_independent() {
        let vector = eggpool_wire::stream_conformance_vectors()
            .into_iter()
            .find(|vector| vector.name == "chat_success")
            .expect("upstream OpenAI success vector");
        let mut expected: Option<Vec<String>> = None;
        for split in eggpool_wire::sse_split_points(vector.bytes.len()) {
            let mut decoder = SharedStreamDecoder::new(vector.adapter);
            let mut output = decoder.push(&vector.bytes[..split]).unwrap();
            output.extend(decoder.push(&vector.bytes[split..]).unwrap());
            output.extend(decoder.finish().unwrap());
            let rendered: Vec<_> = output.iter().map(|event| format!("{event:?}")).collect();
            if let Some(previous) = &expected {
                assert_eq!(previous, &rendered);
            } else {
                expected = Some(rendered);
            }
        }
        assert!(expected
            .unwrap()
            .iter()
            .any(|event| event.contains("TextDelta")));
    }

    #[tokio::test]
    async fn production_openai_stream_adapter_flushes_terminal_event_at_eof() {
        use futures_util::StreamExt;

        let vector = eggpool_wire::stream_conformance_vectors()
            .into_iter()
            .find(|vector| vector.name == "chat_success")
            .expect("upstream OpenAI success vector");
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(vector.bytes)]);
        let output: Vec<_> = openai_chat_stream(stream, None, None).collect().await;
        assert!(output.iter().all(Result::is_ok));
        assert!(matches!(output.last(), Some(Ok(ChatEvent::Finish { .. }))));
    }

    #[test]
    fn successful_upstream_family_vectors_decode_through_codegg_bridge() {
        for vector in eggpool_wire::stream_conformance_vectors()
            .into_iter()
            .filter(|vector| {
                vector.expected == eggpool_wire::stream::StreamTerminalOutcome::Success
                    && !vector.expect_parser_errors
            })
        {
            let mut decoder = SharedStreamDecoder::new(vector.adapter);
            let mut output = decoder
                .push(vector.bytes)
                .unwrap_or_else(|error| panic!("{} failed in push: {error}", vector.name));
            output.extend(
                decoder
                    .finish()
                    .unwrap_or_else(|error| panic!("{} failed at EOF: {error}", vector.name)),
            );
            assert!(
                !output.is_empty(),
                "{} emitted no CodeGG events",
                vector.name
            );
            if vector.name == "chat_success" {
                let Some(ChatEvent::Finish { usage, .. }) = output.last() else {
                    panic!("OpenAI success vector did not finish");
                };
                assert_eq!(usage.input_tokens, 2);
                assert_eq!(usage.output_tokens, 3);
                assert_eq!(usage.total_tokens, 5);
            }
        }
    }

    #[test]
    fn upstream_provider_error_and_eof_vectors_remain_failures() {
        let vectors = eggpool_wire::stream_conformance_vectors();
        let provider_error = vectors
            .iter()
            .find(|vector| vector.name == "chat_provider_error")
            .unwrap();
        let mut decoder = SharedStreamDecoder::new(provider_error.adapter);
        assert!(decoder.push(provider_error.bytes).is_err());

        let eof = vectors
            .iter()
            .find(|vector| vector.name == "chat_eof_before_body")
            .unwrap();
        let mut decoder = SharedStreamDecoder::new(eof.adapter);
        assert!(decoder.push(eof.bytes).unwrap().is_empty());
        assert!(decoder.finish().is_err());

        let post_terminal = vectors
            .iter()
            .find(|vector| vector.name == "chat_post_terminal_data")
            .unwrap();
        let mut decoder = SharedStreamDecoder::new(post_terminal.adapter);
        decoder.push(post_terminal.bytes).unwrap();
        assert!(decoder.finish().is_err());
    }
}

/// Direct stateless OpenAI Responses surface (M011 WP-B).
///
/// These tests pin the *shared-kernel* grammar and stream projection for a
/// direct provider request. They deliberately exercise `encode_openai_responses`
/// and `openai_responses_stream` rather than the hosted/stateful Responses
/// program subsystem, which is a different contract and out of scope here.
#[cfg(test)]
mod responses_surface_tests {
    use super::*;
    use crate::{ChatEvent, ChatRequest, ContentPart, Message, ToolDefinition};
    use futures_util::StreamExt;

    fn responses_request() -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: "hello".to_string().into(),
                }],
            }],
            model: "gpt-6-luna".to_string(),
            tools: None,
            system: Some("be brief".to_string()),
            temperature: None,
            top_p: None,
            max_tokens: Some(256),
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: crate::ProviderRequestContext::default(),
        }
    }

    #[test]
    fn responses_body_uses_input_grammar_not_chat_messages() {
        let body = encode_openai_responses(&responses_request(), true).unwrap();
        let input = body
            .get("input")
            .and_then(Value::as_array)
            .expect("Responses uses `input`, not `messages`");
        assert!(body.get("messages").is_none());
        let user = input
            .iter()
            .find(|item| item.get("role").and_then(Value::as_str) == Some("user"))
            .expect("user message present");
        let block = &user["content"][0];
        assert_eq!(block["type"], "input_text");
        assert_eq!(block["text"], "hello");
    }

    #[test]
    fn responses_body_is_stateless_and_never_uses_the_hosted_program_path() {
        let body = encode_openai_responses(&responses_request(), true).unwrap();
        // `store: false` is what makes this a direct stateless request rather
        // than a server-side stored program.
        assert_eq!(body["store"], Value::Bool(false));
        // Hosted-program-only fields must not leak into the direct surface.
        for hosted_only in ["previous_response_id", "conversation", "background"] {
            assert!(
                body.get(hosted_only).is_none(),
                "hosted-only field {hosted_only} must not be sent on the direct surface"
            );
        }
    }

    #[test]
    fn responses_keeps_its_own_token_field_spelling() {
        let body = encode_openai_responses(&responses_request(), false).unwrap();
        // The Chat-only `max_completion_tokens` -> `max_tokens` correction must
        // not be applied to the Responses surface.
        assert_eq!(body["max_output_tokens"], 256);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn responses_preserves_tools_in_responses_grammar() {
        let mut request = responses_request();
        request.tools = Some(vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "read a file".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
            }),
            defer_loading: None,
        }]);
        let body = encode_openai_responses(&request, false).unwrap();
        let tools = body.get("tools").and_then(Value::as_array).expect("tools");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "read_file");
    }

    const RESPONSES_TEXT_STREAM: &str = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-6-luna\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\" world\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",",
        "\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n",
    );

    #[tokio::test]
    async fn responses_text_stream_projects_text_usage_and_finish() {
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(
            RESPONSES_TEXT_STREAM.as_bytes().to_vec(),
        )]);
        let output: Vec<_> = openai_responses_stream(stream, None, None).collect().await;
        assert!(output.iter().all(Result::is_ok), "{output:?}");

        let text: String = output
            .iter()
            .filter_map(|event| match event {
                Ok(ChatEvent::TextDelta(text)) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello world");

        let Some(Ok(ChatEvent::Finish { usage, .. })) = output.last() else {
            panic!("Responses stream must finish, got {:?}", output.last());
        };
        assert_eq!(usage.input_tokens, 2);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.total_tokens, 5);
    }

    const RESPONSES_TOOL_STREAM: &str = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_1\",",
        "\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",",
        "\"delta\":\"{\\\"path\\\":\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_1\",",
        "\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"read_file\",",
        "\"arguments\":\"{\\\"path\\\":\\\"a.txt\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",",
        "\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}\n\n",
    );

    #[tokio::test]
    async fn responses_tool_stream_accumulates_a_completed_tool_call() {
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(
            RESPONSES_TOOL_STREAM.as_bytes().to_vec(),
        )]);
        let output: Vec<_> = openai_responses_stream(stream, None, None).collect().await;
        assert!(output.iter().all(Result::is_ok), "{output:?}");

        let completed = output.iter().find_map(|event| match event {
            Ok(ChatEvent::ToolCall(call)) => {
                Some((call.name.to_string(), call.arguments.to_string()))
            }
            _ => None,
        });
        let (name, arguments) = completed.expect("a completed tool call must be accumulated");
        assert_eq!(name, "read_file");
        assert!(arguments.contains("a.txt"), "arguments were {arguments:?}");
    }

    #[tokio::test]
    async fn responses_stream_split_across_arbitrary_chunk_boundaries_is_stable() {
        // SSE framing must not depend on how the transport slices bytes.
        let bytes = RESPONSES_TEXT_STREAM.as_bytes();
        for split in 1..bytes.len() {
            let chunks = vec![
                Ok::<_, std::io::Error>(bytes[..split].to_vec()),
                Ok(bytes[split..].to_vec()),
            ];
            let output: Vec<_> =
                openai_responses_stream(futures_util::stream::iter(chunks), None, None)
                    .collect()
                    .await;
            assert!(
                output.iter().all(Result::is_ok),
                "split {split}: {output:?}"
            );
            let text: String = output
                .iter()
                .filter_map(|event| match event {
                    Ok(ChatEvent::TextDelta(text)) => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(text, "Hello world", "split {split}");
        }
    }

    #[tokio::test]
    async fn responses_stream_eof_before_completion_is_an_error() {
        // An upstream that stops before a terminal event must not be reported
        // as a successful completion.
        let truncated = "event: response.output_text.delta\n\
                          data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n";
        let stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(truncated.as_bytes().to_vec())]);
        let output: Vec<_> = openai_responses_stream(stream, None, None).collect().await;
        assert!(
            output.iter().any(Result::is_err),
            "unterminated Responses stream must fail, got {output:?}"
        );
    }

    #[tokio::test]
    async fn responses_provider_error_event_surfaces_an_error() {
        let failed = concat!(
            "event: error\n",
            "data: {\"type\":\"error\",\"error\":{\"message\":\"boom\",\"code\":\"server_error\"}}\n\n",
        );
        let stream =
            futures_util::stream::iter([Ok::<_, std::io::Error>(failed.as_bytes().to_vec())]);
        let output: Vec<_> = openai_responses_stream(stream, None, None).collect().await;
        assert!(
            output.iter().any(Result::is_err),
            "Responses error event must surface, got {output:?}"
        );
    }
}
