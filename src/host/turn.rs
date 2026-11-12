//! Turn admission, event mapping, cancellation and terminal frames.

use crate::agent::r#loop::RunUsageSnapshot;
use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::error::AppError;
use crate::host::protocol::{
    self, ErrorClass, NotificationBody, NotificationFrame, StopReason, TurnDone, TurnError,
    TurnText, TurnTool, TurnUsage, Usage,
};
use crate::host::session::{Session, TurnAdmission};
use crate::host::FrameWriter;
use crate::provider::{
    ChatEvent, ChatRequest, ContentPart, Message, ProviderError, ProviderErrorClass,
    ProviderRequestContext,
};
use std::sync::atomic::Ordering;

pub(crate) fn map_event(
    event: &AppEvent,
    session_id: &str,
    turn_id: &str,
) -> Option<NotificationBody> {
    match event {
        AppEvent::TextDelta {
            session_id: event_session,
            delta,
        } if event_session.as_ref() == session_id => Some(NotificationBody::Text(TurnText {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            delta: delta.to_string(),
        })),
        AppEvent::ToolCallStarted {
            session_id: event_session,
            tool_name,
            tool_id,
            arguments,
        } if event_session == session_id => Some(NotificationBody::Tool(TurnTool {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            phase: protocol::ToolPhase::Started,
            tool_id: tool_id.clone(),
            tool_name: tool_name.clone(),
            arguments: Some(arguments.clone()),
            output: None,
            success: None,
            truncated: None,
        })),
        AppEvent::ToolResult {
            tool_id,
            tool_name,
            session_id: event_session,
            output,
            success,
        } if event_session == session_id => {
            let (output, truncated) = protocol::bound_tool_output(output);
            Some(NotificationBody::Tool(TurnTool {
                session_id: session_id.to_owned(),
                turn_id: turn_id.to_owned(),
                phase: protocol::ToolPhase::Finished,
                tool_id: tool_id.clone(),
                tool_name: tool_name.clone(),
                arguments: None,
                output: Some(output),
                success: Some(*success),
                truncated: Some(truncated),
            }))
        }
        _ => None,
    }
}

pub(crate) fn stop_reason_from_events(events: &[ChatEvent]) -> StopReason {
    let raw = events.iter().rev().find_map(|event| match event {
        ChatEvent::Finish { stop_reason, .. } => Some(stop_reason.as_str()),
        _ => None,
    });
    map_stop_reason(raw)
}

pub(crate) fn map_stop_reason(raw: Option<&str>) -> StopReason {
    match raw.unwrap_or("end_turn") {
        "max_tokens" | "length" => StopReason::MaxTokens,
        "max_turns" | "max_steps" | "max_tool_calls" | "tool_calls" => StopReason::MaxTurns,
        "cancelled" | "interrupted" => StopReason::Cancelled,
        _ => StopReason::EndTurn,
    }
}

pub(crate) fn usage_from_snapshot(snapshot: &RunUsageSnapshot) -> Usage {
    if snapshot.provider_responses == 0 {
        return Usage::absent();
    }
    Usage {
        reported: true,
        input_tokens: Some(to_u64(snapshot.input_tokens)),
        output_tokens: Some(to_u64(snapshot.output_tokens)),
        cached_tokens: snapshot.cached_tokens.map(to_u64),
        reasoning_tokens: snapshot.reasoning_tokens.map(to_u64),
    }
}

fn to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

pub(crate) fn turn_error(
    session_id: &str,
    turn_id: &str,
    error: &AppError,
    usage: Usage,
) -> TurnError {
    let (class, provider_status) = classify(error);
    TurnError {
        session_id: session_id.to_owned(),
        turn_id: turn_id.to_owned(),
        class,
        message: safe_message(class).to_owned(),
        retryable: class.retryable(),
        provider_status,
        usage,
    }
}

fn classify(error: &AppError) -> (ErrorClass, Option<u16>) {
    match error {
        AppError::Provider(error) => (provider_class(error.class()), provider_status(error)),
        AppError::Cancelled => (ErrorClass::Cancelled, None),
        _ => (ErrorClass::Internal, None),
    }
}

fn provider_class(class: ProviderErrorClass) -> ErrorClass {
    match class {
        ProviderErrorClass::Auth => ErrorClass::Auth,
        ProviderErrorClass::ModelNotFound => ErrorClass::ModelNotFound,
        ProviderErrorClass::ModelNotSupported => ErrorClass::ModelNotSupported,
        ProviderErrorClass::ContextLength => ErrorClass::ContextLength,
        ProviderErrorClass::RateLimit => ErrorClass::RateLimit,
        ProviderErrorClass::InvalidRequest => ErrorClass::InvalidRequest,
        ProviderErrorClass::Transport => ErrorClass::Transport,
        ProviderErrorClass::Protocol => ErrorClass::Protocol,
        ProviderErrorClass::Upstream => ErrorClass::Upstream,
    }
}

fn provider_status(error: &ProviderError) -> Option<u16> {
    match error {
        ProviderError::Api { code, .. } => code.parse::<u16>().ok(),
        _ => None,
    }
}

fn safe_message(class: ErrorClass) -> &'static str {
    match class {
        ErrorClass::Auth => "the upstream rejected the credential",
        ErrorClass::ModelNotFound => "the upstream does not have that model",
        ErrorClass::ModelNotSupported => "the model does not support this request",
        ErrorClass::ContextLength => "the request exceeds the model's context",
        ErrorClass::RateLimit => "the upstream is throttling requests",
        ErrorClass::InvalidRequest => "the upstream rejected the request",
        ErrorClass::Transport => "the upstream could not be reached",
        ErrorClass::Protocol => "the upstream returned an unsupported response",
        ErrorClass::Upstream => "the upstream reported a failure",
        ErrorClass::Busy => "a turn is already active",
        ErrorClass::Cancelled => "the turn was cancelled",
        ErrorClass::Workspace => "the workspace root is not usable",
        ErrorClass::NotInitialized => "initialize has not succeeded",
        ErrorClass::UnsupportedCapability => "a required capability is not supported",
        ErrorClass::Internal => "the turn failed inside CodeGG",
    }
}

pub(crate) fn spawn<W>(
    session: Session,
    writer: FrameWriter<W>,
    admission: TurnAdmission,
    prompt: String,
    turn_id: String,
) where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let session_id = session.id.clone();
        let request = ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: prompt.into(),
                }],
            }],
            model: session.model_request.clone(),
            tools: None,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: ProviderRequestContext {
                session_id: Some(session_id.clone().into()),
                ..Default::default()
            },
        };
        let mut bus = GlobalEventBus::subscribe();
        let TurnAdmission {
            mut loop_instance,
            cancel_rx,
            cancelled,
        } = admission;
        loop_instance.set_turn_id(Some(turn_id.clone()));
        loop_instance.set_cancel_receiver(cancel_rx);

        let result = {
            let run = loop_instance.run(request);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    result = &mut run => break result,
                    event = bus.recv() => {
                        if let Ok(event) = event {
                            emit(&writer, &event, &session_id, &turn_id).await;
                        }
                    }
                }
            }
        };
        while let Ok(event) = bus.try_recv() {
            emit(&writer, &event, &session_id, &turn_id).await;
        }

        let usage = usage_from_snapshot(&loop_instance.run_usage());
        let cancelled =
            cancelled.load(Ordering::SeqCst) || matches!(&result, Err(AppError::Cancelled));
        let terminal = match result {
            Ok(_events) if cancelled => Ok(StopReason::Cancelled),
            Ok(events) => Ok(stop_reason_from_events(&events)),
            Err(_) if cancelled => Ok(StopReason::Cancelled),
            Err(error) => Err(error),
        };
        match terminal {
            Ok(stop_reason) => {
                write_usage(&writer, &session_id, &turn_id, usage).await;
                write_done(&writer, &session_id, &turn_id, stop_reason, usage).await;
            }
            Err(error) => {
                let error = turn_error(&session_id, &turn_id, &error, usage);
                let frame = NotificationFrame {
                    jsonrpc: "2.0".to_owned(),
                    body: NotificationBody::Error(error),
                };
                if let Err(error) = writer.send(&frame).await {
                    tracing::debug!(error = %error, "host notification write failed");
                }
            }
        }
        session.complete_turn(loop_instance, &turn_id).await;
    });
}

async fn emit<W>(writer: &FrameWriter<W>, event: &AppEvent, session_id: &str, turn_id: &str)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let Some(body) = map_event(event, session_id, turn_id) else {
        return;
    };
    let frame = NotificationFrame {
        jsonrpc: "2.0".to_owned(),
        body,
    };
    if let Err(error) = writer.send(&frame).await {
        tracing::debug!(error = %error, "host notification write failed");
    }
}

async fn write_usage<W>(writer: &FrameWriter<W>, session_id: &str, turn_id: &str, usage: Usage)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let frame = NotificationFrame {
        jsonrpc: "2.0".to_owned(),
        body: NotificationBody::Usage(TurnUsage {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            usage,
        }),
    };
    if let Err(error) = writer.send(&frame).await {
        tracing::debug!(error = %error, "host usage write failed");
    }
}

async fn write_done<W>(
    writer: &FrameWriter<W>,
    session_id: &str,
    turn_id: &str,
    stop_reason: StopReason,
    usage: Usage,
) where
    W: tokio::io::AsyncWrite + Unpin,
{
    let frame = NotificationFrame {
        jsonrpc: "2.0".to_owned(),
        body: NotificationBody::Done(TurnDone {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
            stop_reason,
            usage,
        }),
    };
    if let Err(error) = writer.send(&frame).await {
        tracing::debug!(error = %error, "host done write failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn text_event(session: &str) -> AppEvent {
        AppEvent::TextDelta {
            session_id: Arc::from(session),
            delta: Arc::from("hi"),
        }
    }

    #[test]
    fn events_from_another_session_are_not_mapped() {
        assert!(map_event(&text_event("other"), "s1", "t1").is_none());
        let mapped = map_event(&text_event("s1"), "s1", "t1").expect("mapped");
        assert_eq!(mapped.method(), "turn/text");
    }

    #[test]
    fn tool_events_map_to_started_and_bounded_finished() {
        let started = map_event(
            &AppEvent::ToolCallStarted {
                session_id: "s1".to_owned(),
                tool_name: "read".to_owned(),
                tool_id: "call_1".to_owned(),
                arguments: "{}".to_owned(),
            },
            "s1",
            "t1",
        )
        .expect("started");
        let value = serde_json::to_value(&started).expect("serialize");
        assert_eq!(value["params"]["phase"], "started");
        assert_eq!(value["params"]["arguments"], "{}");

        let output = "x".repeat(protocol::MAX_TOOL_OUTPUT_BYTES + 10);
        let finished = map_event(
            &AppEvent::ToolResult {
                tool_id: "call_1".to_owned(),
                tool_name: "read".to_owned(),
                session_id: "s1".to_owned(),
                output,
                success: true,
            },
            "s1",
            "t1",
        )
        .expect("finished");
        let value = serde_json::to_value(&finished).expect("serialize");
        assert_eq!(value["params"]["phase"], "finished");
        assert_eq!(value["params"]["success"], true);
        assert_eq!(value["params"]["truncated"], true);
        assert_eq!(
            value["params"]["output"].as_str().expect("output").len(),
            protocol::MAX_TOOL_OUTPUT_BYTES
        );
    }

    #[test]
    fn finish_reasons_map_to_the_wire_vocabulary() {
        assert_eq!(map_stop_reason(Some("end_turn")), StopReason::EndTurn);
        assert_eq!(map_stop_reason(Some("max_tokens")), StopReason::MaxTokens);
        assert_eq!(map_stop_reason(Some("length")), StopReason::MaxTokens);
        assert_eq!(map_stop_reason(Some("max_turns")), StopReason::MaxTurns);
        assert_eq!(map_stop_reason(Some("cancelled")), StopReason::Cancelled);
        assert_eq!(map_stop_reason(None), StopReason::EndTurn);
        let events = vec![ChatEvent::Finish {
            stop_reason: Arc::new("max_tokens".to_owned()),
            usage: Default::default(),
        }];
        assert_eq!(stop_reason_from_events(&events), StopReason::MaxTokens);
    }

    #[test]
    fn provider_failures_carry_the_typed_class_and_status() {
        let auth = AppError::Provider(ProviderError::Auth("secret body".to_owned()));
        let error = turn_error("s1", "t1", &auth, Usage::absent());
        assert_eq!(error.class, ErrorClass::Auth);
        assert_eq!(error.provider_status, None);
        assert!(!error.message.contains("secret"));

        let throttled = AppError::Provider(ProviderError::api("429", "slow down"));
        let error = turn_error("s1", "t1", &throttled, Usage::absent());
        assert_eq!(error.class, ErrorClass::RateLimit);
        assert_eq!(error.provider_status, Some(429));
        assert!(error.retryable);

        let internal = AppError::Cancelled;
        let error = turn_error("s1", "t1", &internal, Usage::absent());
        assert_eq!(error.class, ErrorClass::Cancelled);
    }

    #[test]
    fn usage_is_absent_when_no_response_reported_it() {
        let absent = usage_from_snapshot(&RunUsageSnapshot::default());
        assert!(!absent.reported);
        assert_eq!(absent.input_tokens, None);
        assert_eq!(absent.output_tokens, None);

        let reported = usage_from_snapshot(&RunUsageSnapshot {
            provider_responses: 1,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: None,
            reasoning_tokens: None,
        });
        assert!(reported.reported);
        assert_eq!(reported.input_tokens, Some(0));
        assert_eq!(reported.output_tokens, Some(0));
    }
}
