//! The desktop's terminal surface: the host half (M007-J).
//!
//! # What this module owns
//!
//! - the seven bounded **view types** the renderer reads. They are the
//!   Rust mirror of `apps/desktop/src/terminal/contract.ts`, and
//!   `apps/desktop/scripts/check-bindings.mjs` holds the two in one map
//!   so the contract cannot drift.
//! - the six **named** operations the renderer calls. Each one is a
//!   bounded operation over the daemon's interactive-process service, and
//!   none of them is a generic request passthrough
//!   (`scripts/check-desktop-boundary.sh:23-26`).
//!
//! # What this module deliberately does not own
//!
//! - **The process.** The daemon owns every interactive process and every
//!   attachment (`src/core/daemon.rs:4811-4820`). The desktop never
//!   spawns, signals or inspects one; it asks the daemon for the daemon's
//!   processes.
//! - **The scrollback.** The host serves the bytes the daemon produced, in
//!   bounded chunks. It keeps no history of its own.
//! - **A second connection.** Every operation travels over the host's one
//!   daemon client (`route.rs:253-270`), so the daemon's
//!   connection-scoped ownership holds for all of them.
//!
//! # The rules
//!
//! - **A handle is not an attachment.** `attach` names a *handle* and
//!   returns an *attachment id*; every later operation names the
//!   attachment, so an id cannot be replayed across owners
//!   (`crates/codegg-protocol/src/core.rs:3628-3630`).
//! - **A resync is a typed answer.** `InteractiveProcessResyncRequired`
//!   carries a reason and no bytes. It is reported as a resync, never
//!   flattened into a successful attach with an empty chunk.
//! - **An unexpected answer is an error.** A reply the daemon did not
//!   send is a typed failure, never a default view: an empty terminal
//!   would assert a guarantee the desktop was never given.
//! - **The bytes are bounded.** One read asks for at most
//!   [`TERMINAL_CHUNK_BYTES`], and one write carries at most
//!   [`TERMINAL_INPUT_BYTES`] — both at or below the protocol's own caps
//!   (`crates/codegg-protocol/src/interactive_process.rs:90`, `:93`).
//!
//! # The scope of these operations
//!
//! The terminal is scoped by the daemon's **connection**, not by the
//! desktop's project route. The daemon resolves an attachment to a
//! process server-side and derives ownership from the transport
//! connection's client id (`src/core/daemon.rs:4849`); a route change —
//! another project, another workspace, another session — does not
//! invalidate an attachment that is already open. So these operations
//! carry no `route_generation`: fencing them on the route generation
//! would refuse valid work after an ordinary route switch, and the
//! desktop's editor commands make the same distinction for the same
//! reason (`document.rs`: the host-state-scoped operations carry no route
//! generation either). What does fence them is the connection, and
//! [`HostState::route_request`] is that fence.

use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::interactive_process::{
    InteractiveOutputChunk, InteractiveProcessMetadata, InteractiveResync, InteractiveResyncReason,
    MAX_INTERACTIVE_CHUNK_BYTES, MAX_INTERACTIVE_INPUT_BYTES,
};
use serde::{Deserialize, Serialize};

use crate::{route::unexpected_daemon_response, HostState};

/// The bounded read size one attach or resume asks the daemon for.
///
/// It is the protocol's own default
/// (`crates/codegg-protocol/src/interactive_process.rs:87`) and stays at or
/// below the hard cap, so the host can never ask for a read the daemon
/// would refuse.
pub const TERMINAL_CHUNK_BYTES: usize = 64 * 1024;

/// The bounded write size one input call may carry.
///
/// The daemon refuses anything larger
/// (`crates/codegg-protocol/src/interactive_process.rs:93`); the host
/// refuses it first, with a message that names the bound.
pub const TERMINAL_INPUT_BYTES: usize = 32 * 1024;

/// Compile-time proof that the host's own bounds stay inside the
/// protocol's. A bound the host may exceed is a bound the daemon has to
/// enforce alone.
const _: () = assert!(TERMINAL_CHUNK_BYTES <= MAX_INTERACTIVE_CHUNK_BYTES);
const _: () = assert!(TERMINAL_INPUT_BYTES <= MAX_INTERACTIVE_INPUT_BYTES);

// ── The bounded DTOs the host serves ─────────────────────────────────────

/// One daemon-owned interactive process, as the terminal panel sees it.
///
/// The daemon's metadata also carries `child_pid`
/// (`crates/codegg-protocol/src/interactive_process.rs:242`); the view
/// **deliberately omits it**, because a process id is a machine detail the
/// renderer has no use for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalProcessView {
    /// The handle that names the process on the daemon.
    pub handle: String,
    /// The workspace the process was created in.
    pub workspace_id: String,
    /// The command line the process was created with.
    pub command: String,
    /// The daemon's own state word for the process.
    pub state: String,
    /// The process's current width, in cells.
    pub cols: u16,
    /// The process's current height, in cells.
    pub rows: u16,
    /// The sequence the next read resumes from.
    pub next_seq: u64,
    /// How many bytes the daemon still retains for this process.
    pub retained_bytes: u64,
    /// How many bytes the process produced in total.
    pub total_bytes: u64,
    /// `true` when the daemon's retention dropped the oldest bytes.
    pub truncated: bool,
    /// The process's exit code, once it exited.
    pub exit_code: Option<i32>,
    /// The signal that ended the process, once it was signalled.
    pub exit_signal: Option<i32>,
}

/// The daemon's bounded process list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalListView {
    /// The processes the daemon knows, in the daemon's own order.
    pub processes: Vec<TerminalProcessView>,
    /// `true` when the daemon's list was cut at its own bound.
    pub truncated: bool,
}

/// One bounded read of a process's output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalChunkView {
    /// The handle the bytes came from.
    pub handle: String,
    /// The sequence the read started from.
    pub from_seq: u64,
    /// The sequence the next read resumes from.
    pub next_seq: u64,
    /// `true` when the returned bytes are the oldest retained bytes
    /// rather than the requested range.
    pub gap: bool,
    /// The raw output bytes, base64-encoded.
    pub data_b64: String,
}

/// The result of attaching to a handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAttachView {
    /// The attachment the daemon issued. Every later operation names
    /// this, never the handle.
    pub attachment_id: String,
    /// The handle the attachment is bound to.
    pub handle: String,
    /// The first bounded read of the process's output.
    pub chunk: TerminalChunkView,
    /// Why the cursor could not be resumed incrementally, when the daemon
    /// said so.
    pub resync_reason: Option<String>,
}

/// The result of writing input to an attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInputView {
    /// The attachment the bytes were written through.
    pub attachment_id: String,
    /// How many bytes the daemon accepted.
    pub bytes_accepted: usize,
}

/// The result of resizing an attachment's process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalResizeView {
    /// The attachment the resize was applied through.
    pub attachment_id: String,
    /// The applied width, in cells.
    pub cols: u16,
    /// The applied height, in cells.
    pub rows: u16,
}

/// The result of releasing an attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDetachView {
    /// The attachment that was released. The process is unaffected.
    pub attachment_id: String,
}

/// The caller's input bytes, in either wire form.
///
/// The renderer sends the user's keystrokes; the **host** is what
/// guarantees the daemon receives valid base64
/// (`src/interactive_process_attach.rs:827-833`). Both forms are
/// accepted, because the caller's bytes and the daemon's encoding are
/// two different things and the host is the boundary between them:
///
/// - [`TerminalInputData::Bytes`] — the raw bytes, which the host encodes;
/// - [`TerminalInputData::Encoded`] — the bytes already encoded, which
///   the host decodes, bounds and re-encodes.
///
/// Either way the route sends the daemon an encoding of the caller's
/// bytes, and the host is what refuses an over-long write.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum TerminalInputData {
    /// The raw bytes the user typed.
    Bytes(Vec<u8>),
    /// The bytes, already base64-encoded.
    Encoded(String),
}

// ── Mapping the daemon's answers ─────────────────────────────────────────

/// The name a resync reason carries across the bridge.
///
/// The reason is a typed enum
/// (`crates/codegg-protocol/src/interactive_process.rs:275-287`), and the
/// bridge carries its **name**: the renderer reports the reason in words
/// and never guesses one.
fn resync_reason_name(reason: InteractiveResyncReason) -> String {
    let name = match reason {
        InteractiveResyncReason::HistoryExpired => "HistoryExpired",
        InteractiveResyncReason::CursorAhead => "CursorAhead",
        InteractiveResyncReason::HandleGone => "HandleGone",
        InteractiveResyncReason::VersionMismatch => "VersionMismatch",
    };
    name.to_string()
}

/// Carry one daemon process metadata into the view.
fn terminal_process_view(metadata: &InteractiveProcessMetadata) -> TerminalProcessView {
    TerminalProcessView {
        handle: metadata.handle.clone(),
        workspace_id: metadata.workspace_id.clone(),
        command: metadata.command.clone(),
        state: metadata.state.clone(),
        cols: metadata.cols,
        rows: metadata.rows,
        next_seq: metadata.next_seq,
        retained_bytes: metadata.retained_bytes as u64,
        total_bytes: metadata.total_bytes,
        truncated: metadata.truncated,
        exit_code: metadata.exit_code,
        exit_signal: metadata.exit_signal,
    }
}

/// Carry one daemon output chunk into the view.
fn terminal_chunk_view(chunk: &InteractiveOutputChunk) -> TerminalChunkView {
    TerminalChunkView {
        handle: chunk.handle.clone(),
        from_seq: chunk.from_seq,
        next_seq: chunk.next_seq,
        gap: chunk.gap,
        data_b64: chunk.data_b64.clone(),
    }
}

/// The chunk that a resync answer carries: no bytes, the oldest retained
/// sequence, and the newest sequence.
///
/// The bytes are absent because the daemon sent none. The sequences are
/// not: the renderer resets its grid to the daemon's `base_seq` and says
/// so (the plan's work package C), so the positions have to reach it.
fn resync_chunk_view(resync: &InteractiveResync) -> TerminalChunkView {
    TerminalChunkView {
        handle: resync.handle.clone(),
        from_seq: resync.base_seq,
        next_seq: resync.next_seq,
        gap: true,
        data_b64: String::new(),
    }
}

/// Map the daemon's process list.
///
/// A reply the daemon did not send is a typed failure, not an empty
/// list: an empty list would assert that the daemon has no processes.
fn map_terminal_list(response: CoreResponse) -> Result<TerminalListView, String> {
    let CoreResponse::InteractiveProcessList {
        processes,
        truncated,
    } = response
    else {
        return Err(unexpected_daemon_response(&response));
    };
    Ok(TerminalListView {
        processes: processes.iter().map(terminal_process_view).collect(),
        truncated,
    })
}

/// Map the daemon's attach answer.
///
/// The daemon answers an attach with either an attachment and its first
/// chunk, or a typed resync. The resync is reported as a resync: it
/// carries the reason and no bytes, and it is never flattened into a
/// successful attach whose chunk happens to be empty.
fn map_terminal_attach(response: CoreResponse) -> Result<TerminalAttachView, String> {
    match response {
        CoreResponse::InteractiveProcessAttached {
            attachment_id,
            handle,
            chunk,
            resync,
        } => Ok(TerminalAttachView {
            attachment_id,
            handle,
            chunk: terminal_chunk_view(&chunk),
            resync_reason: resync.map(|resync| resync_reason_name(resync.reason)),
        }),
        CoreResponse::InteractiveProcessResyncRequired {
            attachment_id,
            resync,
        } => Ok(TerminalAttachView {
            attachment_id,
            handle: resync.handle.clone(),
            chunk: resync_chunk_view(&resync),
            resync_reason: Some(resync_reason_name(resync.reason)),
        }),
        other => Err(unexpected_daemon_response(&other)),
    }
}

/// Map the daemon's resume answer.
fn map_terminal_resume(response: CoreResponse) -> Result<TerminalChunkView, String> {
    match response {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => Ok(terminal_chunk_view(&chunk)),
        other => Err(unexpected_daemon_response(&other)),
    }
}

/// Map the daemon's input answer.
fn map_terminal_input(response: CoreResponse) -> Result<TerminalInputView, String> {
    let CoreResponse::InteractiveProcessInputAccepted {
        attachment_id,
        bytes_accepted,
    } = response
    else {
        return Err(unexpected_daemon_response(&response));
    };
    Ok(TerminalInputView {
        attachment_id,
        bytes_accepted,
    })
}

/// Map the daemon's resize answer.
fn map_terminal_resize(response: CoreResponse) -> Result<TerminalResizeView, String> {
    let CoreResponse::InteractiveProcessResized {
        attachment_id,
        cols,
        rows,
    } = response
    else {
        return Err(unexpected_daemon_response(&response));
    };
    Ok(TerminalResizeView {
        attachment_id,
        cols,
        rows,
    })
}

/// Map the daemon's detach answer.
fn map_terminal_detach(response: CoreResponse) -> Result<TerminalDetachView, String> {
    let CoreResponse::InteractiveProcessDetached { attachment_id } = response else {
        return Err(unexpected_daemon_response(&response));
    };
    Ok(TerminalDetachView { attachment_id })
}

// ── The requests ─────────────────────────────────────────────────────────

/// The request that attaches to a handle and reads its first chunk.
fn terminal_attach_request(handle: &str) -> CoreRequest {
    CoreRequest::InteractiveProcessAttach {
        handle: handle.to_owned(),
        from_seq: None,
        max_bytes: Some(TERMINAL_CHUNK_BYTES),
    }
}

/// The request that resumes a bounded read from a cursor.
fn terminal_resume_request(attachment_id: &str, from_seq: u64) -> CoreRequest {
    CoreRequest::InteractiveProcessResume {
        attachment_id: attachment_id.to_owned(),
        from_seq,
        max_bytes: Some(TERMINAL_CHUNK_BYTES),
    }
}

/// The request that writes the caller's bytes to an attachment.
///
/// The daemon requires **valid base64**
/// (`src/interactive_process_attach.rs:827-833`), so the host encodes the
/// bytes itself rather than trusting the renderer to have done it.
fn terminal_input_request(attachment_id: &str, bytes: &[u8]) -> CoreRequest {
    CoreRequest::InteractiveProcessInput {
        attachment_id: attachment_id.to_owned(),
        data_b64: encode_base64(bytes),
    }
}

/// Encode bytes as standard base64 (RFC 4648, padded).
///
/// The desktop host holds no base64 dependency of its own, and the
/// daemon's decoder is the standard alphabet with padding
/// (`src/interactive_process_attach.rs:48`), so the encoder is written
/// here rather than pulled in.
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let first = group[0] as u32;
        let second = u32::from(group.get(1).copied().unwrap_or(0));
        let third = u32::from(group.get(2).copied().unwrap_or(0));
        let triple = (first << 16) | (second << 8) | third;
        encoded.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        encoded.push(if group.len() > 1 {
            ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        encoded.push(if group.len() > 2 {
            ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

/// Decode standard base64 (RFC 4648, padded).
///
/// Returns `None` for anything the daemon's decoder would refuse, so the
/// host rejects a malformed write here rather than sending it.
fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    fn digit(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = encoded.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for group in bytes.chunks(4) {
        let padding = group.iter().filter(|byte| **byte == b'=').count();
        // Padding is only ever the group's last one or two symbols.
        if padding > 2 || (padding > 0 && group[3] != b'=') {
            return None;
        }
        let mut triple = 0u32;
        for (index, byte) in group.iter().enumerate() {
            let value = if *byte == b'=' { 0 } else { digit(*byte)? };
            triple |= value << (18 - 6 * index);
        }
        decoded.push(((triple >> 16) & 0xff) as u8);
        if padding < 2 {
            decoded.push(((triple >> 8) & 0xff) as u8);
        }
        if padding < 1 {
            decoded.push((triple & 0xff) as u8);
        }
    }
    Some(decoded)
}

// ── The routes ───────────────────────────────────────────────────────────

impl HostState {
    /// Send one bounded interactive-process request over the host's single
    /// daemon client.
    ///
    /// The client is the host's one connection, so the daemon's
    /// connection-scoped ownership holds for every terminal operation.
    async fn terminal_request(&self, payload: CoreRequest) -> Result<CoreResponse, String> {
        let (client, _) = self.route_request().await?;
        client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload,
            })
            .await
            .map_err(|error| error.to_string())
    }

    /// List the daemon's interactive processes.
    pub(crate) async fn route_terminal_list(&self) -> Result<TerminalListView, String> {
        let response = self
            .terminal_request(CoreRequest::InteractiveProcessList {
                workspace_id: None,
                limit: None,
            })
            .await?;
        map_terminal_list(response)
    }

    /// Attach to one handle and read its first bounded chunk.
    pub(crate) async fn route_terminal_attach(
        &self,
        handle: String,
    ) -> Result<TerminalAttachView, String> {
        let response = self
            .terminal_request(terminal_attach_request(&handle))
            .await?;
        map_terminal_attach(response)
    }

    /// Resume a bounded read from a cursor.
    pub(crate) async fn route_terminal_resume(
        &self,
        attachment_id: String,
        from_seq: u64,
    ) -> Result<TerminalChunkView, String> {
        let response = self
            .terminal_request(terminal_resume_request(&attachment_id, from_seq))
            .await?;
        map_terminal_resume(response)
    }

    /// Write bounded input through an attachment.
    ///
    /// The caller's bytes are decoded (when they arrive encoded) and
    /// bounded here, and the daemon is sent the host's own encoding of
    /// them. The bound is enforced here as well as by the daemon: a
    /// write the daemon would refuse is refused here first, with a
    /// message that names the bound.
    pub(crate) async fn route_terminal_input(
        &self,
        attachment_id: String,
        data_b64: TerminalInputData,
    ) -> Result<TerminalInputView, String> {
        let bytes = match &data_b64 {
            TerminalInputData::Bytes(bytes) => bytes.clone(),
            TerminalInputData::Encoded(encoded) => decode_base64(encoded)
                .ok_or_else(|| "terminal input is not valid base64".to_owned())?,
        };
        if bytes.len() > TERMINAL_INPUT_BYTES {
            return Err(format!(
                "terminal input exceeds the {TERMINAL_INPUT_BYTES}-byte bound"
            ));
        }
        let response = self
            .terminal_request(terminal_input_request(&attachment_id, &bytes))
            .await?;
        map_terminal_input(response)
    }

    /// Resize an attachment's process.
    pub(crate) async fn route_terminal_resize(
        &self,
        attachment_id: String,
        cols: u16,
        rows: u16,
    ) -> Result<TerminalResizeView, String> {
        let response = self
            .terminal_request(CoreRequest::InteractiveProcessResize {
                attachment_id,
                cols,
                rows,
            })
            .await?;
        map_terminal_resize(response)
    }

    /// Release an attachment. The process itself is unaffected.
    pub(crate) async fn route_terminal_detach(
        &self,
        attachment_id: String,
    ) -> Result<TerminalDetachView, String> {
        let response = self
            .terminal_request(CoreRequest::InteractiveProcessDetach { attachment_id })
            .await?;
        map_terminal_detach(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> InteractiveProcessMetadata {
        InteractiveProcessMetadata {
            handle: "handle-1".to_owned(),
            workspace_id: "workspace-1".to_owned(),
            command: "bash".to_owned(),
            state: "running".to_owned(),
            cols: 80,
            rows: 24,
            child_pid: Some(4242),
            next_seq: 12,
            retained_bytes: 4096,
            total_bytes: 9000,
            truncated: true,
            exit_code: None,
            exit_signal: None,
        }
    }

    fn chunk() -> InteractiveOutputChunk {
        InteractiveOutputChunk {
            handle: "handle-1".to_owned(),
            from_seq: 0,
            next_seq: 5,
            gap: false,
            data_b64: "aGVsbG8=".to_owned(),
        }
    }

    fn resync() -> InteractiveResync {
        InteractiveResync {
            reason: InteractiveResyncReason::HandleGone,
            handle: "handle-1".to_owned(),
            base_seq: 3,
            next_seq: 11,
            snapshot: None,
        }
    }

    #[test]
    fn the_list_route_carries_the_daemons_processes() {
        let view = map_terminal_list(CoreResponse::InteractiveProcessList {
            processes: vec![metadata()],
            truncated: true,
        })
        .expect("the list answer maps");
        assert_eq!(view.processes.len(), 1);
        assert_eq!(view.processes[0].handle, "handle-1");
        assert_eq!(view.processes[0].workspace_id, "workspace-1");
        assert_eq!(view.processes[0].cols, 80);
        assert_eq!(view.processes[0].rows, 24);
        assert_eq!(view.processes[0].next_seq, 12);
        assert_eq!(view.processes[0].retained_bytes, 4096);
        assert_eq!(view.processes[0].total_bytes, 9000);
        assert!(view.processes[0].truncated);
        assert!(view.truncated);
    }

    #[test]
    fn the_process_view_omits_the_child_pid() {
        let view = terminal_process_view(&metadata());
        // The daemon's metadata carries the pid; the view must not, so
        // the machine detail never reaches the renderer.
        assert!(!format!("{view:?}").contains("4242"));
        assert_eq!(view.exit_code, None);
        assert_eq!(view.exit_signal, None);
    }

    #[test]
    fn the_attach_route_carries_the_attachment_and_its_first_chunk() {
        let view = map_terminal_attach(CoreResponse::InteractiveProcessAttached {
            attachment_id: "attachment-1".to_owned(),
            handle: "handle-1".to_owned(),
            chunk: chunk(),
            resync: None,
        })
        .expect("the attach answer maps");
        assert_eq!(view.attachment_id, "attachment-1");
        assert_eq!(view.handle, "handle-1");
        assert_eq!(view.chunk.data_b64, "aGVsbG8=");
        assert_eq!(view.chunk.next_seq, 5);
        assert!(!view.chunk.gap);
        assert_eq!(view.resync_reason, None);
    }

    #[test]
    fn the_attach_route_reports_a_resync_instead_of_an_empty_attach() {
        let view = map_terminal_attach(CoreResponse::InteractiveProcessResyncRequired {
            attachment_id: "attachment-1".to_owned(),
            resync: resync(),
        })
        .expect("the resync answer maps");
        assert_eq!(view.resync_reason.as_deref(), Some("HandleGone"));
        assert_eq!(view.handle, "handle-1");
        // The resync carries no bytes, but it does carry the positions the
        // renderer resets its grid to.
        assert!(view.chunk.data_b64.is_empty());
        assert!(view.chunk.gap);
        assert_eq!(view.chunk.from_seq, 3);
        assert_eq!(view.chunk.next_seq, 11);
    }

    #[test]
    fn the_resume_route_carries_the_chunk() {
        let view = map_terminal_resume(CoreResponse::InteractiveProcessResumed {
            attachment_id: "attachment-1".to_owned(),
            chunk: chunk(),
        })
        .expect("the resume answer maps");
        assert_eq!(view.handle, "handle-1");
        assert_eq!(view.from_seq, 0);
        assert_eq!(view.next_seq, 5);
        assert_eq!(view.data_b64, "aGVsbG8=");
    }

    #[test]
    fn the_input_route_carries_the_accepted_byte_count() {
        let view = map_terminal_input(CoreResponse::InteractiveProcessInputAccepted {
            attachment_id: "attachment-1".to_owned(),
            bytes_accepted: 7,
        })
        .expect("the input answer maps");
        assert_eq!(view.attachment_id, "attachment-1");
        assert_eq!(view.bytes_accepted, 7);
    }

    #[test]
    fn the_resize_route_carries_the_applied_size() {
        let view = map_terminal_resize(CoreResponse::InteractiveProcessResized {
            attachment_id: "attachment-1".to_owned(),
            cols: 120,
            rows: 40,
        })
        .expect("the resize answer maps");
        assert_eq!(view.attachment_id, "attachment-1");
        assert_eq!(view.cols, 120);
        assert_eq!(view.rows, 40);
    }

    #[test]
    fn the_detach_route_carries_the_released_attachment() {
        let view = map_terminal_detach(CoreResponse::InteractiveProcessDetached {
            attachment_id: "attachment-1".to_owned(),
        })
        .expect("the detach answer maps");
        assert_eq!(view.attachment_id, "attachment-1");
    }

    #[test]
    fn an_unexpected_answer_is_a_typed_error_not_a_default_view() {
        let wrong = CoreResponse::Json {
            data: serde_json::json!({ "processes": [] }),
        };
        assert!(map_terminal_list(wrong.clone()).is_err());
        assert!(map_terminal_attach(wrong.clone()).is_err());
        assert!(map_terminal_resume(wrong.clone()).is_err());
        assert!(map_terminal_input(wrong.clone()).is_err());
        assert!(map_terminal_resize(wrong.clone()).is_err());
        assert!(map_terminal_detach(wrong).is_err());
    }

    #[test]
    fn a_daemon_error_carries_its_code() {
        let error = CoreResponse::Error {
            code: "interactive_unknown_handle".to_owned(),
            message: "unknown handle".to_owned(),
        };
        let failure = map_terminal_attach(error).expect_err("an error answer is a failure");
        assert_eq!(failure, "interactive_unknown_handle: unknown handle");
    }

    #[test]
    fn the_input_request_encodes_the_callers_bytes() {
        let request = terminal_input_request("attachment-1", b"hi\n");
        let CoreRequest::InteractiveProcessInput {
            attachment_id,
            data_b64,
        } = request
        else {
            panic!("the input route sends an input request");
        };
        assert_eq!(attachment_id, "attachment-1");
        assert_eq!(data_b64, "aGkK");
    }

    #[test]
    fn the_base64_encoder_pads_like_the_standard_alphabet() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn the_base64_decoder_round_trips_the_encoder() {
        for payload in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foobar",
            b"a\x00\xff\xfe binary",
        ] {
            let encoded = encode_base64(payload);
            assert_eq!(decode_base64(&encoded).as_deref(), Some(payload));
        }
    }

    #[test]
    fn the_base64_decoder_refuses_what_the_daemon_would_refuse() {
        // A length that is not a whole group, a symbol outside the
        // alphabet, and padding in the wrong place.
        assert_eq!(decode_base64("aGk"), None);
        assert_eq!(decode_base64("aGk*"), None);
        assert_eq!(decode_base64("a=kK"), None);
        assert_eq!(decode_base64("=GkK"), None);
    }

    #[test]
    fn the_encoded_input_is_decoded_to_the_callers_bytes() {
        let data = TerminalInputData::Encoded("aGkK".to_owned());
        let TerminalInputData::Encoded(encoded) = &data else {
            panic!("the encoded form is carried as encoded");
        };
        assert_eq!(decode_base64(encoded).as_deref(), Some(&b"hi\n"[..]));
    }

    #[test]
    fn the_input_argument_accepts_both_wire_forms() {
        let encoded: TerminalInputData =
            serde_json::from_str("\"aGkK\"").expect("a base64 string is accepted");
        assert_eq!(encoded, TerminalInputData::Encoded("aGkK".to_owned()));
        let raw: TerminalInputData =
            serde_json::from_str("[104, 105]").expect("a byte array is accepted");
        assert_eq!(raw, TerminalInputData::Bytes(vec![104, 105]));
    }

    #[test]
    fn the_read_requests_ask_for_a_bounded_chunk() {
        let CoreRequest::InteractiveProcessAttach { max_bytes, .. } =
            terminal_attach_request("handle-1")
        else {
            panic!("the attach route sends an attach request");
        };
        assert_eq!(max_bytes, Some(TERMINAL_CHUNK_BYTES));
        let CoreRequest::InteractiveProcessResume {
            from_seq,
            max_bytes,
            ..
        } = terminal_resume_request("attachment-1", 7)
        else {
            panic!("the resume route sends a resume request");
        };
        assert_eq!(from_seq, 7);
        assert_eq!(max_bytes, Some(TERMINAL_CHUNK_BYTES));
    }

    #[test]
    fn the_resync_reason_names_are_the_daemons_own() {
        assert_eq!(
            resync_reason_name(InteractiveResyncReason::HistoryExpired),
            "HistoryExpired"
        );
        assert_eq!(
            resync_reason_name(InteractiveResyncReason::CursorAhead),
            "CursorAhead"
        );
        assert_eq!(
            resync_reason_name(InteractiveResyncReason::HandleGone),
            "HandleGone"
        );
        assert_eq!(
            resync_reason_name(InteractiveResyncReason::VersionMismatch),
            "VersionMismatch"
        );
    }
}
