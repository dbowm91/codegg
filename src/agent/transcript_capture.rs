//! The in-execution capture of a delegated run's recorded conversation.
//!
//! A durable delegated run's transcript is the parent's window into what the
//! child actually did. The window is only useful while the child is still
//! running, so the capture happens **inside** the agent loop, at the semantic
//! boundaries where the model produced a message, the loop accepted a tool
//! call, and the loop delivered an executed tool's result back to the model.
//!
//! Three properties this module holds:
//!
//! - **The store is the only door.** Every body reaches the durable store
//!   through [`AgentRunTranscriptStore::append`], so the write-time redaction
//!   and both bounds apply to a live capture exactly as they applied to the
//!   post-run batch this replaced. There is no second journal and no bypass.
//! - **A call is recorded when it is accepted, not when it is answered.** A
//!   call the child never executed stays a call with no result; nothing here
//!   fabricates an outcome, and a failed tool records its actual failure.
//! - **Nothing here can fail the delegated work.** A write failure is logged
//!   and remembered as a recording failure; the child's own execution is not
//!   touched, and a failed write is never reported as an absent transcript.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use codegg_core::agent_run_transcript::{
    AgentRunTranscriptStore, TranscriptAppendStatus, TranscriptKind, TranscriptRole,
};
use codegg_core::identity::AgentRunId;

use crate::agent::transcript_publisher::{TranscriptAdvance, TranscriptAdvancePublisher};

/// One semantic checkpoint the agent loop reports while it runs.
#[derive(Debug, Clone)]
pub enum TranscriptCheckpoint {
    /// The model produced a complete assistant message for one turn.
    ///
    /// The text is the model-visible assistant content of that turn, taken at
    /// the provider-response boundary — never a partial delta, and never the
    /// provider's private reasoning.
    AssistantMessage { text: String },
    /// The loop accepted a structured tool call from the provider.
    ToolCall {
        tool_call_id: String,
        tool_name: String,
        arguments: String,
    },
    /// The loop executed a tool and delivered its result to the model.
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        success: bool,
        output_truncated: bool,
        output: String,
    },
}

/// A sink for the agent loop's semantic checkpoints.
///
/// The loop holds one of these only when the turn belongs to a durable
/// delegated run; every other turn holds none and records nothing.
#[async_trait::async_trait]
// Stable >=1.99 flags the #[must_use] injected by the async-trait expansion
// (the desugared future is already must_use). Suppressed at the macro site;
// no behavior change.
#[allow(clippy::double_must_use)]
pub trait AgentRunTranscriptCapture: Send + Sync {
    /// Record one semantic checkpoint.
    ///
    /// The call never fails the loop: an implementation records its own
    /// failure and returns.
    async fn record(&self, checkpoint: TranscriptCheckpoint);
}

/// The capture sink for a durable delegated run.
///
/// It appends the loop's semantic checkpoints to the run's durable transcript,
/// in the order the loop produced them.
pub struct StoreTranscriptCapture {
    store: Arc<dyn AgentRunTranscriptStore>,
    run_id: AgentRunId,
    /// The tool call ids already recorded in this attempt.
    ///
    /// One logical tool call and its matching result appear at most once per
    /// recorded attempt, so a provider that re-issues a call id does not
    /// produce a second call row for the same call.
    recorded_calls: Mutex<HashSet<String>>,
    /// The tool call ids whose result was already recorded in this attempt.
    ///
    /// The result is claimed separately from the call, because the two are
    /// written at different seams: a call is recorded when the loop accepts
    /// it, and its result when the loop delivers it. A provider that re-issues
    /// a call id is describing one logical call, so it must not produce a
    /// second result row either.
    recorded_results: Mutex<HashSet<String>>,
    /// Whether the run's transcript reached its per-run bound: the store
    /// refuses further appends, so recording is over.
    recording_stopped: AtomicBool,
    /// Whether at least one write failed. A failed write is remembered as a
    /// recording failure and never reported as an absent transcript.
    write_failed: AtomicBool,
    /// M002: the post-commit publisher that announces the run's transcript
    /// advanced.
    ///
    /// It is `None` when the daemon that owns this run does not publish
    /// transcript advisories; the sink then records exactly as it did
    /// before, and the durable store stays the only truth.
    ///
    /// The handle is **injected**: the delegated child's runtime is given
    /// one by the daemon that owns the subagent pool. There is no global
    /// registry and no lookup at write time.
    publisher: Option<Arc<dyn TranscriptAdvancePublisher>>,
}

impl StoreTranscriptCapture {
    /// Bind a capture sink to a run's durable transcript.
    pub fn new(store: Arc<dyn AgentRunTranscriptStore>, run_id: AgentRunId) -> Self {
        Self {
            store,
            run_id,
            recorded_calls: Mutex::new(HashSet::new()),
            recorded_results: Mutex::new(HashSet::new()),
            recording_stopped: AtomicBool::new(false),
            write_failed: AtomicBool::new(false),
            publisher: None,
        }
    }

    /// Bind a capture sink to a run's durable transcript **and** to the
    /// publisher that announces the run's committed transcript advancing.
    ///
    /// The publisher is called only after an append has committed, so a
    /// hint can never claim a row the store does not hold.
    pub fn with_publisher(
        store: Arc<dyn AgentRunTranscriptStore>,
        run_id: AgentRunId,
        publisher: Arc<dyn TranscriptAdvancePublisher>,
    ) -> Self {
        Self {
            store,
            run_id,
            recorded_calls: Mutex::new(HashSet::new()),
            recorded_results: Mutex::new(HashSet::new()),
            recording_stopped: AtomicBool::new(false),
            write_failed: AtomicBool::new(false),
            publisher: Some(publisher),
        }
    }

    /// The publisher this sink announces through, when it has one.
    pub fn publisher(&self) -> Option<&Arc<dyn TranscriptAdvancePublisher>> {
        self.publisher.as_ref()
    }

    /// Whether the run's transcript reached its per-run bound.
    pub fn recording_stopped(&self) -> bool {
        self.recording_stopped.load(Ordering::SeqCst)
    }

    /// Whether at least one append failed.
    pub fn write_failed(&self) -> bool {
        self.write_failed.load(Ordering::SeqCst)
    }

    /// Claim a tool call id for this attempt.
    ///
    /// Returns `false` when the id was already claimed, which is how one
    /// logical call stays one row.
    fn claim_call_id(&self, tool_call_id: &str) -> bool {
        let mut recorded = self
            .recorded_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recorded.insert(tool_call_id.to_string())
    }

    /// Claim a tool call id's result for this attempt.
    ///
    /// Returns `false` when the id's result was already claimed, which is how
    /// one logical call and its matching result stay one row each.
    fn claim_result_id(&self, tool_call_id: &str) -> bool {
        let mut recorded = self
            .recorded_results
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recorded.insert(tool_call_id.to_string())
    }
}

#[async_trait::async_trait]
impl AgentRunTranscriptCapture for StoreTranscriptCapture {
    async fn record(&self, checkpoint: TranscriptCheckpoint) {
        if self.recording_stopped.load(Ordering::SeqCst) {
            return;
        }
        let (kind, body) = match checkpoint {
            TranscriptCheckpoint::AssistantMessage { text } => {
                if text.is_empty() {
                    return;
                }
                (TranscriptKind::ModelMessage, text)
            }
            TranscriptCheckpoint::ToolCall {
                tool_call_id,
                tool_name,
                arguments,
            } => {
                if !self.claim_call_id(&tool_call_id) {
                    return;
                }
                (
                    TranscriptKind::ToolCall,
                    format!("tool_call_id={tool_call_id} tool={tool_name} arguments={arguments}"),
                )
            }
            TranscriptCheckpoint::ToolResult {
                tool_call_id,
                tool_name,
                success,
                output_truncated,
                output,
            } => {
                if !self.claim_result_id(&tool_call_id) {
                    return;
                }
                (
                    TranscriptKind::ToolResult,
                    format!(
                        "tool_call_id={tool_call_id} tool={tool_name} success={success} output_truncated={output_truncated} output={output}"
                    ),
                )
            }
        };
        match self
            .store
            .append(&self.run_id, TranscriptRole::Orchestrator, kind, &body)
            .await
        {
            Ok(outcome) => {
                let recording_stopped = outcome.status == TranscriptAppendStatus::RunTruncated;
                if recording_stopped {
                    self.recording_stopped.store(true, Ordering::SeqCst);
                    tracing::warn!(
                        target: "codegg::agent::transcript_capture",
                        run_id = %self.run_id,
                        kind = kind.as_str(),
                        "the delegated run's transcript reached its per-run bound; recording stopped"
                    );
                }
                // M002: the advisory is published **after** the append has
                // committed, and never inside the store's write
                // transaction. A hint can therefore only ever describe a
                // row the store already holds.
                self.announce(outcome.ordinal, recording_stopped).await;
            }
            Err(error) => {
                self.write_failed.store(true, Ordering::SeqCst);
                tracing::warn!(
                    target: "codegg::agent::transcript_capture",
                    run_id = %self.run_id,
                    kind = kind.as_str(),
                    error = %error,
                    "a delegated transcript write failed; the delegated run is unaffected"
                );
                // A dropped row would otherwise read as a child that never did
                // that thing. Say so in the transcript itself, best-effort: if
                // this note cannot be written either, the warning above is
                // the remaining evidence.
                let note = self
                    .store
                    .append(
                        &self.run_id,
                        TranscriptRole::Orchestrator,
                        TranscriptKind::TerminalNote,
                        &format!(
                            "the delegated run's transcript recording failed at a {} row; this transcript is incomplete",
                            kind.as_str()
                        ),
                    )
                    .await;
                // The note is a committed row too, so the watermark moves to
                // its ordinal — and only if it really was written.
                if let Ok(outcome) = note {
                    self.announce(outcome.ordinal, false).await;
                }
            }
        }
    }
}

impl StoreTranscriptCapture {
    /// Announce the run's committed transcript advancing.
    ///
    /// The advisory is body-free: it carries the run id, the committed
    /// ordinal, and whether the run's recording stopped. A publication
    /// failure is logged and swallowed — the durable store is the truth,
    /// and a lost advisory is recovered by a page read.
    async fn announce(&self, ordinal: i64, recording_stopped: bool) {
        let Some(publisher) = self.publisher.as_ref() else {
            return;
        };
        publisher
            .publish_advance(TranscriptAdvance {
                run_id: self.run_id.to_string(),
                ordinal,
                recording_stopped,
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;
    use tokio::sync::Mutex as AsyncMutex;

    /// A store that records what it was asked to append, and can be told to
    /// fail every write.
    #[derive(Default)]
    struct RecordingStore {
        appends: AsyncMutex<Vec<(TranscriptRole, TranscriptKind, String)>>,
        fail: AtomicBool,
    }

    #[async_trait::async_trait]
    impl AgentRunTranscriptStore for RecordingStore {
        async fn append(
            &self,
            _run_id: &AgentRunId,
            role: TranscriptRole,
            kind: TranscriptKind,
            body: &str,
        ) -> Result<
            codegg_core::agent_run_transcript::TranscriptAppendOutcome,
            codegg_core::agent_run_transcript::AgentRunTranscriptError,
        > {
            if self.fail.load(Ordering::SeqCst) {
                return Err(
                    codegg_core::agent_run_transcript::AgentRunTranscriptError::Storage(
                        "the transcript table is gone".to_string(),
                    ),
                );
            }
            self.appends
                .lock()
                .await
                .push((role, kind, body.to_string()));
            Ok(codegg_core::agent_run_transcript::TranscriptAppendOutcome {
                status: TranscriptAppendStatus::Stored,
                ordinal: 0,
                byte_len: body.len() as i64,
            })
        }

        async fn read(
            &self,
            _run_id: &AgentRunId,
            _from_ordinal: i64,
            _limit: usize,
        ) -> Result<
            codegg_core::agent_run_transcript::AgentRunTranscriptRead,
            codegg_core::agent_run_transcript::AgentRunTranscriptError,
        > {
            Err(
                codegg_core::agent_run_transcript::AgentRunTranscriptError::Storage(
                    "not used".to_string(),
                ),
            )
        }
    }

    fn capture(store: StdArc<RecordingStore>) -> StoreTranscriptCapture {
        StoreTranscriptCapture::new(store, AgentRunId::new())
    }

    #[tokio::test]
    async fn an_empty_assistant_message_is_not_recorded() {
        let store = StdArc::new(RecordingStore::default());
        let capture = capture(store.clone());
        capture
            .record(TranscriptCheckpoint::AssistantMessage {
                text: String::new(),
            })
            .await;
        assert!(
            store.appends.lock().await.is_empty(),
            "an empty assistant message must not become a row"
        );
    }

    #[tokio::test]
    async fn a_repeated_call_id_is_recorded_once() {
        let store = StdArc::new(RecordingStore::default());
        let capture = capture(store.clone());
        for _ in 0..2 {
            capture
                .record(TranscriptCheckpoint::ToolCall {
                    tool_call_id: "call-1".to_string(),
                    tool_name: "read".to_string(),
                    arguments: "{}".to_string(),
                })
                .await;
        }
        let appends = store.appends.lock().await;
        assert_eq!(
            appends.len(),
            1,
            "one logical call must become one row, got: {appends:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_write_is_remembered_and_leaves_no_row() {
        let store = StdArc::new(RecordingStore::default());
        store.fail.store(true, Ordering::SeqCst);
        let capture = capture(store.clone());
        capture
            .record(TranscriptCheckpoint::ToolResult {
                tool_call_id: "call-1".to_string(),
                tool_name: "read".to_string(),
                success: true,
                output_truncated: false,
                output: "body".to_string(),
            })
            .await;
        assert!(
            capture.write_failed(),
            "a failed write must be observable as a recording failure"
        );
        assert!(
            store.appends.lock().await.is_empty(),
            "a failed write must not leave a row"
        );
    }

    /// A publisher that records what it was asked to announce.
    #[derive(Default)]
    struct RecordingPublisher {
        advances: AsyncMutex<Vec<TranscriptAdvance>>,
    }

    #[async_trait::async_trait]
    impl TranscriptAdvancePublisher for RecordingPublisher {
        async fn publish_advance(&self, advance: TranscriptAdvance) {
            self.advances.lock().await.push(advance);
        }

        async fn flush(&self, _run_id: &str) {}
    }

    /// M002: a committed append announces the run's transcript advancing.
    #[tokio::test]
    async fn a_committed_append_announces_the_advance() {
        let store = StdArc::new(RecordingStore::default());
        let publisher = StdArc::new(RecordingPublisher::default());
        let run_id = AgentRunId::new();
        let capture = StoreTranscriptCapture::with_publisher(
            store.clone(),
            run_id.clone(),
            publisher.clone(),
        );
        capture
            .record(TranscriptCheckpoint::AssistantMessage {
                text: "hello".into(),
            })
            .await;
        let advances = publisher.advances.lock().await;
        assert_eq!(
            advances.len(),
            1,
            "a committed row must announce exactly one advance, got {advances:?}"
        );
        assert_eq!(advances[0].run_id, run_id.to_string());
        assert_eq!(
            advances[0].ordinal, 0,
            "the advance carries the store's committed ordinal"
        );
        assert!(
            !advances[0].recording_stopped,
            "a stored row is not a stopped recording"
        );
    }

    /// M002: a failed write announces nothing, because no ordinal was
    /// committed.
    ///
    /// The failed write also cannot write its own explanatory note, so the
    /// watermark must not move to an ordinal the store never accepted.
    #[tokio::test]
    async fn a_failed_write_announces_no_ordinal_the_store_did_not_commit() {
        let store = StdArc::new(RecordingStore::default());
        store.fail.store(true, Ordering::SeqCst);
        let publisher = StdArc::new(RecordingPublisher::default());
        let capture = StoreTranscriptCapture::with_publisher(
            store.clone(),
            AgentRunId::new(),
            publisher.clone(),
        );
        capture
            .record(TranscriptCheckpoint::AssistantMessage {
                text: "hello".into(),
            })
            .await;
        assert!(capture.write_failed(), "the failed write is remembered");
        assert!(
            publisher.advances.lock().await.is_empty(),
            "a failed write must not announce an ordinal the store did not commit"
        );
    }
}
