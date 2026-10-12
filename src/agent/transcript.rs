//! Turn transcript projection: a completed agent run rendered into the rows
//! the session transcript is stored and reloaded from.
//!
//! A live turn carries its conversation in memory only — the TUI builds the
//! provider context from its own transcript state — so nothing reached the
//! `message` table while the session was running, and reloading a session
//! replaced the transcript with whatever was there (usually nothing). This
//! module is the pure half of closing that gap: given the prompt that opened
//! the run and the events the run produced, produce the ordered
//! [`MessageData`] payloads to persist. The write itself belongs to
//! [`super::turn_completion`], which owns the run boundary.
//!
//! Only what the user would recognise as the conversation is persisted: the
//! prompt, the assistant's text and reasoning, and its tool calls with their
//! outputs. Provider bookkeeping (token usage, stop reasons, continuation
//! checkpoints) already has its own durable records and is deliberately not
//! duplicated here.

use std::collections::HashMap;

use crate::provider::{ChatEvent, ToolCall};
use crate::session::message::{MessageData, MessageRole, PartData, PartInfo, ToolStatus};

/// One persisted message plus the stable id it is stored under.
pub(super) struct TranscriptEntry {
    pub(super) id: String,
    pub(super) data: MessageData,
}

/// The ordered rows for one completed run: the prompt, then the reply.
///
/// `assistant` is `None` when the run produced nothing worth reloading, so a
/// failed turn stores the prompt alone instead of an empty assistant bubble.
pub(super) struct TurnTranscript {
    pub(super) entries: Vec<TranscriptEntry>,
}

/// Which open text buffer a streaming delta extends.
enum OpenKind {
    Text,
    Reasoning,
}

/// Project a completed run into transcript rows.
///
/// Part order is preserved: consecutive text deltas merge into one part, and a
/// tool call closes the open text so the reply reads in the order it was
/// produced rather than as a single block of text followed by a block of tool
/// calls.
///
/// A tool call with no matching [`ChatEvent::ToolResult`] is stored as
/// `Pending`. The run ended without an outcome, and inventing an error would
/// claim something that did not happen.
pub(super) fn build_turn_transcript(
    session_id: &str,
    run_prefix: &str,
    user_prompt: &str,
    events: &[ChatEvent],
) -> TurnTranscript {
    let mut entries = Vec::new();

    if !user_prompt.trim().is_empty() {
        entries.push(TranscriptEntry {
            id: format!("{run_prefix}_user"),
            data: message_data(
                session_id,
                &format!("{run_prefix}_user"),
                MessageRole::User,
                vec![PartInfo {
                    id: format!("{run_prefix}_user_0"),
                    session_id: session_id.to_string(),
                    message_id: format!("{run_prefix}_user"),
                    data: PartData::Text {
                        text: user_prompt.to_string(),
                    },
                }],
            ),
        });
    }

    let parts = assistant_parts(events);
    if parts.is_empty() {
        return TurnTranscript { entries };
    }

    let assistant_id = format!("{run_prefix}_assistant");
    let parts = parts
        .into_iter()
        .enumerate()
        .map(|(index, data)| PartInfo {
            id: format!("{assistant_id}_{index}"),
            session_id: session_id.to_string(),
            message_id: assistant_id.clone(),
            data,
        })
        .collect();

    entries.push(TranscriptEntry {
        id: assistant_id.clone(),
        data: message_data(session_id, &assistant_id, MessageRole::Assistant, parts),
    });

    TurnTranscript { entries }
}

fn message_data(
    session_id: &str,
    message_id: &str,
    role: MessageRole,
    parts: Vec<PartInfo>,
) -> MessageData {
    MessageData {
        id: message_id.to_string(),
        session_id: session_id.to_string(),
        message_id: message_id.to_string(),
        parts,
        role: Some(role),
    }
}

fn assistant_parts(events: &[ChatEvent]) -> Vec<PartData> {
    let mut parts: Vec<PartData> = Vec::new();
    // Where a `ChatEvent::ToolResult` for a given tool call id landed in
    // `parts`, so its output and status land on the same part.
    let mut tool_parts: HashMap<String, usize> = HashMap::new();
    let mut open: Option<(OpenKind, String)> = None;

    for event in events {
        match event {
            ChatEvent::TextDelta(delta) => {
                push_delta(&mut parts, &mut open, OpenKind::Text, delta);
            }
            ChatEvent::ReasoningDelta(delta) => {
                push_delta(&mut parts, &mut open, OpenKind::Reasoning, delta);
            }
            ChatEvent::ToolCall(call) => {
                flush(&mut parts, &mut open);
                tool_parts.insert(call.id.to_string(), parts.len());
                parts.push(tool_call_part(call));
            }
            ChatEvent::ToolResult {
                tool_call_id,
                content,
            } => {
                if let Some(index) = tool_parts.get(tool_call_id.as_ref()).copied() {
                    if let PartData::ToolCall { output, status, .. } = &mut parts[index] {
                        *output = Some(content.to_string());
                        *status = ToolStatus::Completed;
                    }
                }
            }
            // A run that failed is not persisted at all, so an inline error
            // reaching here is a provider-reported error inside an otherwise
            // completed turn. Keep it: dropping it would silently shorten the
            // transcript the user is about to reload.
            ChatEvent::Error(message) => {
                flush(&mut parts, &mut open);
                parts.push(PartData::Text {
                    text: message.to_string(),
                });
            }
            ChatEvent::Finish { .. } => {}
        }
    }

    flush(&mut parts, &mut open);
    parts
}

/// Extend the open text buffer, or start one. Switching between text and
/// reasoning closes the previous buffer so interleaved streams do not merge.
fn push_delta(
    parts: &mut Vec<PartData>,
    open: &mut Option<(OpenKind, String)>,
    kind: OpenKind,
    delta: &str,
) {
    match open {
        Some((OpenKind::Text, buffer)) if matches!(kind, OpenKind::Text) => buffer.push_str(delta),
        Some((OpenKind::Reasoning, buffer)) if matches!(kind, OpenKind::Reasoning) => {
            buffer.push_str(delta)
        }
        _ => {
            flush(parts, open);
            *open = Some((kind, delta.to_string()));
        }
    }
}

fn flush(parts: &mut Vec<PartData>, open: &mut Option<(OpenKind, String)>) {
    let Some((kind, buffer)) = open.take() else {
        return;
    };
    if buffer.is_empty() {
        return;
    }
    match kind {
        OpenKind::Text => parts.push(PartData::Text { text: buffer }),
        OpenKind::Reasoning => parts.push(PartData::Reasoning { reasoning: buffer }),
    }
}

fn tool_call_part(call: &ToolCall) -> PartData {
    PartData::ToolCall {
        id: call.id.to_string(),
        name: call.name.to_string(),
        input: call.arguments.clone(),
        output: None,
        status: ToolStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TokenUsage;
    use std::sync::Arc;

    fn transcript_parts(entries: &[TranscriptEntry]) -> Vec<PartData> {
        let assistant = entries.last().expect("assistant entry");
        assert_eq!(assistant.data.role, Some(MessageRole::Assistant));
        assistant
            .data
            .parts
            .iter()
            .map(|p| p.data.clone())
            .collect()
    }

    #[test]
    fn a_plain_reply_is_recorded_as_an_assistant_message() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "hello",
            &[
                ChatEvent::TextDelta(Arc::new("hi".to_string())),
                ChatEvent::Finish {
                    stop_reason: Arc::new("stop".to_string()),
                    usage: TokenUsage::default(),
                },
            ],
        );

        assert_eq!(transcript.entries.len(), 2);
        assert_eq!(transcript.entries[0].data.role, Some(MessageRole::User));
        // The whole point of persisting `role`: a text-only reply has no tool
        // or reasoning part, so role inference from parts alone calls it a
        // user message.
        let parts = transcript_parts(&transcript.entries);
        assert_eq!(parts.len(), 1);
        assert!(matches!(&parts[0], PartData::Text { text } if text == "hi"));
    }

    #[test]
    fn text_around_a_tool_call_keeps_its_position() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "fix it",
            &[
                ChatEvent::TextDelta(Arc::new("checking".to_string())),
                ChatEvent::ToolCall(ToolCall {
                    id: Arc::new("call_1".to_string()),
                    name: Arc::new("read".to_string()),
                    arguments: serde_json::json!({ "path": "a.rs" }),
                }),
                ChatEvent::ToolResult {
                    tool_call_id: Arc::new("call_1".to_string()),
                    content: Arc::new("contents".to_string()),
                },
                ChatEvent::TextDelta(Arc::new("fixed".to_string())),
            ],
        );

        let parts = transcript_parts(&transcript.entries);
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], PartData::Text { text } if text == "checking"));
        assert!(matches!(
            &parts[1],
            PartData::ToolCall { status, output, .. }
                if *status == ToolStatus::Completed && output.as_deref() == Some("contents")
        ));
        assert!(matches!(&parts[2], PartData::Text { text } if text == "fixed"));
    }

    #[test]
    fn a_tool_call_with_no_result_stays_pending() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "go",
            &[ChatEvent::ToolCall(ToolCall {
                id: Arc::new("call_1".to_string()),
                name: Arc::new("bash".to_string()),
                arguments: serde_json::json!({}),
            })],
        );

        let parts = transcript_parts(&transcript.entries);
        assert!(matches!(
            &parts[0],
            PartData::ToolCall { status, output, .. }
                if *status == ToolStatus::Pending && output.is_none()
        ));
    }

    #[test]
    fn a_run_that_produced_nothing_stores_only_the_prompt() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "hello",
            &[ChatEvent::Finish {
                stop_reason: Arc::new("stop".to_string()),
                usage: TokenUsage::default(),
            }],
        );

        assert_eq!(transcript.entries.len(), 1);
        assert_eq!(transcript.entries[0].data.role, Some(MessageRole::User));
    }

    #[test]
    fn an_empty_prompt_stores_no_user_row() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "   ",
            &[ChatEvent::TextDelta(Arc::new("hi".to_string()))],
        );

        assert_eq!(transcript.entries.len(), 1);
        assert_eq!(
            transcript.entries[0].data.role,
            Some(MessageRole::Assistant)
        );
    }

    #[test]
    fn interleaved_reasoning_and_text_do_not_merge() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "think",
            &[
                ChatEvent::ReasoningDelta(Arc::new("hmm".to_string())),
                ChatEvent::TextDelta(Arc::new("answer".to_string())),
                ChatEvent::ReasoningDelta(Arc::new("oh".to_string())),
            ],
        );

        let parts = transcript_parts(&transcript.entries);
        assert_eq!(parts.len(), 3);
        assert!(matches!(&parts[0], PartData::Reasoning { reasoning } if reasoning == "hmm"));
        assert!(matches!(&parts[1], PartData::Text { text } if text == "answer"));
        assert!(matches!(&parts[2], PartData::Reasoning { reasoning } if reasoning == "oh"));
    }

    #[test]
    fn an_inline_error_is_kept_in_the_transcript() {
        let transcript = build_turn_transcript(
            "sess",
            "run1",
            "go",
            &[
                ChatEvent::Error(Arc::new("provider exploded".to_string())),
                ChatEvent::TextDelta(Arc::new("recovered".to_string())),
            ],
        );

        let parts = transcript_parts(&transcript.entries);
        assert_eq!(parts.len(), 2);
        assert!(matches!(&parts[0], PartData::Text { text } if text == "provider exploded"));
        assert!(matches!(&parts[1], PartData::Text { text } if text == "recovered"));
    }
}
