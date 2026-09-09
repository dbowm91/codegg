//! Project Collaboration M002 — TUI project chat and observer input routing.
//!
//! TUI-side projection proof: daemon-owned `chat.v1` state renders as
//! bounded per-project chat with send/reply/edit/redact/read/composing
//! flows, paging/resync cursors, unread markers, composing expiry,
//! authorization-identical unavailable rendering, and observer-mode
//! insert input routed to chat with zero turn steering/control.
//!
//! The fake core below records every `CoreRequest` variant it receives so
//! the observer test can prove only `Chat*` requests are ever issued on
//! the observer insert path.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    ChatCapabilitiesDto, ChatChannelDto, ChatComposingDto, ChatMessageDto, CoreEvent, CoreRequest,
    CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::tui::app::TuiCommand;
use tokio::sync::mpsc;

// ── Fake chat core ───────────────────────────────────────────────────────

#[derive(Default)]
struct FakeChatClient {
    /// Recorded request kinds (e.g. `"chat_send"`, `"turn_submit"`).
    seen: Mutex<Vec<String>>,
    advertise: bool,
    unauthorized: Mutex<Vec<String>>,
    channels: Mutex<HashMap<String, ChatChannelDto>>,
    messages: Mutex<HashMap<String, Vec<ChatMessageDto>>>,
    composing: Mutex<HashMap<String, Vec<ChatComposingDto>>>,
    read_markers: Mutex<HashMap<String, u64>>,
    next_seq: Mutex<HashMap<String, u64>>,
}

impl FakeChatClient {
    fn supporting() -> Self {
        Self {
            advertise: true,
            ..Default::default()
        }
    }

    fn capabilities(&self) -> ChatCapabilitiesDto {
        ChatCapabilitiesDto {
            supported: self.advertise,
            protocol_version: 1,
            max_body_bytes: 8192,
            max_page_limit: 100,
            max_mentions: 16,
            max_references: 8,
            composing_ttl_secs: 30,
            retention_max_messages: 1000,
            actions_supported: true,
            max_action_title_bytes: 512,
            max_action_prompt_bytes: 8192,
        }
    }

    fn record(&self, kind: &str) {
        self.seen.lock().unwrap().push(kind.to_string());
    }

    fn kinds(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn channel_for(&self, project_id: &str) -> ChatChannelDto {
        let mut channels = self.channels.lock().unwrap();
        channels
            .entry(project_id.to_string())
            .or_insert_with(|| ChatChannelDto {
                channel_id: format!("ch-{project_id}"),
                project_id: project_id.to_string(),
                name: "general".to_string(),
                created_by: "alice".to_string(),
                created_at_ms: 1,
            })
            .clone()
    }

    fn channel_project(&self, channel_id: &str) -> Option<String> {
        self.channels
            .lock()
            .unwrap()
            .values()
            .find(|c| c.channel_id == channel_id)
            .map(|c| c.project_id.clone())
    }

    fn denied(&self, project_id: &str) -> bool {
        self.unauthorized
            .lock()
            .unwrap()
            .contains(&project_id.to_string())
    }
}

#[async_trait]
impl CoreClient for FakeChatClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::ChatCapabilities => {
                self.record("chat_capabilities");
                Ok(CoreResponse::ChatCapabilities {
                    capabilities: self.capabilities(),
                })
            }
            CoreRequest::ChatChannelEnsure { project_id, name } => {
                self.record("chat_channel_ensure");
                if self.denied(&project_id) {
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                }
                let mut channel = self.channel_for(&project_id);
                if let Some(name) = name {
                    channel.name = name;
                }
                Ok(CoreResponse::ChatChannel { channel })
            }
            CoreRequest::ChatChannelList { project_id, .. } => {
                self.record("chat_channel_list");
                if self.denied(&project_id) {
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                }
                let channel = self.channel_for(&project_id);
                Ok(CoreResponse::ChatChannelList {
                    channels: vec![channel],
                    truncated: false,
                })
            }
            CoreRequest::ChatHistory {
                channel_id,
                from_seq,
                limit,
            } => {
                self.record("chat_history");
                let Some(project_id) = self.channel_project(&channel_id) else {
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                };
                if self.denied(&project_id) {
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                }
                let from = from_seq.unwrap_or(0);
                let stored = self.messages.lock().unwrap();
                let all: Vec<ChatMessageDto> = stored
                    .get(&channel_id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| m.seq >= from)
                    .take(limit.unwrap_or(100) as usize)
                    .collect();
                let next_cursor = all.last().map(|m| m.seq + 1).unwrap_or(from);
                Ok(CoreResponse::ChatHistory {
                    channel_id,
                    messages: all,
                    next_cursor,
                    truncated: false,
                    retention_floor_seq: 0,
                })
            }
            CoreRequest::ChatSend {
                channel_id,
                body,
                reply_to,
                mentions,
                references,
                ..
            } => {
                self.record("chat_send");
                let Some(project_id) = self.channel_project(&channel_id) else {
                    // First send ensures the channel implicitly in this
                    // fake (the real daemon resolves channel→project
                    // server-side and fails closed on unknown channels).
                    // Derive the project from the cached channels only;
                    // unknown channels deny like the daemon.
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                };
                if self.denied(&project_id) {
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                }
                let mut seqs = self.next_seq.lock().unwrap();
                let seq = seqs.entry(channel_id.clone()).or_insert(1);
                let assigned = *seq;
                *seq += 1;
                let message = ChatMessageDto {
                    message_id: format!("m-{channel_id}-{assigned}"),
                    channel_id: channel_id.clone(),
                    project_id,
                    seq: assigned,
                    author_principal: "test-user".to_string(),
                    author_agent: None,
                    body,
                    reply_to,
                    thread_root: None,
                    mentions,
                    references,
                    revision: 0,
                    redacted: false,
                    created_at_ms: 1,
                    edited_at_ms: None,
                };
                self.messages
                    .lock()
                    .unwrap()
                    .entry(channel_id)
                    .or_default()
                    .push(message.clone());
                Ok(CoreResponse::ChatMessage {
                    message,
                    duplicate: false,
                })
            }
            CoreRequest::ChatEdit {
                channel_id,
                message_id,
                new_body,
                ..
            } => {
                self.record("chat_edit");
                let mut stored = self.messages.lock().unwrap();
                let entry = stored.entry(channel_id).or_default();
                match entry.iter_mut().find(|m| m.message_id == message_id) {
                    Some(slot) => {
                        slot.body = new_body;
                        slot.revision += 1;
                        slot.edited_at_ms = Some(2);
                        Ok(CoreResponse::ChatMessage {
                            message: slot.clone(),
                            duplicate: false,
                        })
                    }
                    None => Ok(CoreResponse::Error {
                        code: "chat_channel_not_found".to_string(),
                        message: "unknown message".to_string(),
                    }),
                }
            }
            CoreRequest::ChatRedact {
                channel_id,
                message_id,
                ..
            } => {
                self.record("chat_redact");
                let mut stored = self.messages.lock().unwrap();
                let entry = stored.entry(channel_id).or_default();
                match entry.iter_mut().find(|m| m.message_id == message_id) {
                    Some(slot) => {
                        slot.body = "[REDACTED]".to_string();
                        slot.redacted = true;
                        slot.revision += 1;
                        Ok(CoreResponse::ChatMessage {
                            message: slot.clone(),
                            duplicate: false,
                        })
                    }
                    None => Ok(CoreResponse::Error {
                        code: "chat_channel_not_found".to_string(),
                        message: "unknown message".to_string(),
                    }),
                }
            }
            CoreRequest::ChatReadSet {
                channel_id,
                last_read_seq,
            } => {
                self.record("chat_read_set");
                self.read_markers
                    .lock()
                    .unwrap()
                    .insert(channel_id.clone(), last_read_seq);
                Ok(CoreResponse::ChatReadMarker {
                    channel_id,
                    last_read_seq,
                    updated_at_ms: 3,
                })
            }
            CoreRequest::ChatReadGet { channel_id } => {
                self.record("chat_read_get");
                let seq = self
                    .read_markers
                    .lock()
                    .unwrap()
                    .get(&channel_id)
                    .copied()
                    .unwrap_or(0);
                Ok(CoreResponse::ChatReadMarker {
                    channel_id,
                    last_read_seq: seq,
                    updated_at_ms: 3,
                })
            }
            CoreRequest::ChatComposingSet {
                channel_id,
                composing,
            } => {
                self.record("chat_composing_set");
                let mut map = self.composing.lock().unwrap();
                let entry = map.entry(channel_id.clone()).or_default();
                if composing {
                    entry.push(ChatComposingDto {
                        channel_id,
                        principal_id: "test-user".to_string(),
                        client_id: "c1".to_string(),
                        expires_at_ms: 1_000_000,
                    });
                } else {
                    entry.clear();
                }
                Ok(CoreResponse::ChatComposing {
                    channel_id: String::new(),
                    composing: Vec::new(),
                })
            }
            CoreRequest::ChatComposingList { channel_id } => {
                self.record("chat_composing_list");
                let list = self
                    .composing
                    .lock()
                    .unwrap()
                    .get(&channel_id)
                    .cloned()
                    .unwrap_or_default();
                Ok(CoreResponse::ChatComposing {
                    channel_id,
                    composing: list,
                })
            }
            CoreRequest::ChatSync {
                channel_id,
                from_seq,
                limit,
            } => {
                self.record("chat_sync");
                let stored = self.messages.lock().unwrap();
                let all: Vec<ChatMessageDto> = stored
                    .get(&channel_id)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|m| m.seq >= from_seq)
                    .take(limit.unwrap_or(100) as usize)
                    .collect();
                let next_cursor = all.last().map(|m| m.seq + 1).unwrap_or(from_seq);
                Ok(CoreResponse::ChatSync {
                    channel_id,
                    messages: all,
                    next_cursor,
                    resync_required: false,
                    retention_floor_seq: 0,
                })
            }
            _ => {
                self.record("non_chat_request");
                Ok(CoreResponse::Error {
                    code: "unsupported".to_string(),
                    message: "unsupported in fake".to_string(),
                })
            }
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn app_with_project(project_id: &str) -> codegg::tui::app::App {
    let mut app = codegg::tui::app::App::new_for_testing("/tmp/m002-chat".to_string());
    if let Some(tab) = app.project_tabs.active_mut() {
        tab.project_id = Some(project_id.to_string());
        tab.workspace_id = Some("ws-1".to_string());
    }
    app
}

fn wire_fake(
    app: &mut codegg::tui::app::App,
    fake: Arc<FakeChatClient>,
) -> mpsc::Receiver<TuiCommand> {
    app.set_core_client(fake);
    let (tx, rx) = mpsc::channel(16);
    app.tui_cmd_tx = Some(tx);
    rx
}

async fn recv_cmd(rx: &mut mpsc::Receiver<TuiCommand>) -> TuiCommand {
    tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .expect("completion arrives")
        .expect("channel open")
}

fn test_message(project: &str, channel: &str, seq: u64, id: &str) -> ChatMessageDto {
    ChatMessageDto {
        message_id: id.to_string(),
        channel_id: channel.to_string(),
        project_id: project.to_string(),
        seq,
        author_principal: "alice".to_string(),
        author_agent: None,
        body: format!("body {seq}"),
        reply_to: None,
        thread_root: None,
        mentions: Vec::new(),
        references: Vec::new(),
        revision: 0,
        redacted: false,
        created_at_ms: 1,
        edited_at_ms: None,
    }
}

// ── Multi-project routing ────────────────────────────────────────────────

#[test]
fn multi_project_chat_windows_stay_isolated() {
    let mut app = app_with_project("proj-a");
    let second_id = codegg::tui::app::state::ProjectTabId::new();
    let mut second = codegg::tui::app::state::ProjectTabState::empty(second_id.clone(), "b".into());
    second.project_id = Some("proj-b".to_string());
    second.workspace_id = Some("ws-b".to_string());
    app.project_tabs.add_tab(second);

    let epoch = app.chat.reconnect_epoch;
    for (project, channel, body) in [("proj-a", "ch-a", "hello a"), ("proj-b", "ch-b", "hello b")] {
        let ensure_req = app.chat.begin_ensure(project).unwrap();
        assert!(app.chat.apply_channel_ensured(
            ensure_req,
            project,
            &ChatChannelDto {
                channel_id: channel.to_string(),
                project_id: project.to_string(),
                name: "general".to_string(),
                created_by: "alice".to_string(),
                created_at_ms: 1,
            },
            epoch
        ));
        let hist_req = app.chat.begin_history(project, channel).unwrap();
        assert!(app.chat.apply_history(
            hist_req,
            project,
            channel,
            vec![test_message(project, channel, 1, &format!("m-{project}"))],
            2,
            false,
            0,
            epoch
        ));
        let _ = body;
    }
    assert_eq!(app.chat.get("proj-a").unwrap().messages[0].body, "body 1");
    assert_eq!(
        app.chat.get("proj-a").unwrap().messages[0].project_id,
        "proj-a"
    );
    assert_eq!(
        app.chat.get("proj-b").unwrap().messages[0].project_id,
        "proj-b"
    );
    // Drafts never cross-route between projects.
    app.chat.set_draft("proj-a", "draft a".to_string());
    assert_eq!(app.chat.draft_for("proj-a"), "draft a");
    assert_eq!(app.chat.draft_for("proj-b"), "");
}

// ── Send / reply / render round-trip ─────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_reply_render_round_trip() {
    let fake = Arc::new(FakeChatClient::supporting());
    let mut app = app_with_project("proj-a");
    let mut rx = wire_fake(&mut app, fake.clone());

    // Send a top-level message mentioning @bob.
    app.send_chat_message("proj-a".to_string(), "hello @bob".to_string(), None);
    let sent = match recv_cmd(&mut rx).await {
        TuiCommand::ChatMessageSent {
            project_id,
            channel_id,
            message,
            draft,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
            ..
        } => {
            assert_eq!(project_id, "proj-a");
            let message = message.expect("send succeeds");
            assert!(error.is_none());
            app.apply_chat_sent(
                project_id,
                channel_id,
                Some(message.clone()),
                false,
                draft,
                None,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
            message
        }
        other => panic!("expected ChatMessageSent, got {other:?}"),
    };
    assert_eq!(sent.body, "hello @bob");
    assert_eq!(sent.mentions, vec!["bob".to_string()]);
    let entry = app.chat.get("proj-a").unwrap();
    assert_eq!(entry.messages.len(), 1);
    assert_eq!(app.chat.draft_for("proj-a"), "");

    // Reply to it; the reply linkage survives the round-trip.
    app.send_chat_message(
        "proj-a".to_string(),
        "ack".to_string(),
        Some(sent.message_id.clone()),
    );
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatMessageSent {
            project_id,
            channel_id,
            message,
            draft,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
            ..
        } => {
            let reply = message.expect("reply succeeds");
            assert_eq!(reply.reply_to.as_deref(), Some(sent.message_id.as_str()));
            assert!(error.is_none());
            app.apply_chat_sent(
                project_id,
                channel_id,
                Some(reply),
                false,
                draft,
                None,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("expected ChatMessageSent, got {other:?}"),
    }
    let entry = app.chat.get("proj-a").unwrap();
    assert_eq!(entry.messages.len(), 2);
    // Panel renders both messages with reply linkage.
    let lines = app.chat.panel_lines("proj-a", 0);
    assert!(lines.iter().any(|l| l.contains("hello @bob")));
    assert!(lines.iter().any(|l| l.contains("reply to")));
    // Only chat requests were issued.
    for kind in fake.kinds() {
        assert!(
            kind.starts_with("chat_"),
            "unexpected non-chat request on send path: {kind}"
        );
    }
}

// ── Paging / resync ──────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_then_sync_converges_and_resync_replaces() {
    let fake = Arc::new(FakeChatClient::supporting());
    // Seed two daemon messages.
    {
        let channel = fake.channel_for("proj-a");
        let mut stored = fake.messages.lock().unwrap();
        stored.insert(
            channel.channel_id.clone(),
            vec![
                test_message("proj-a", &channel.channel_id, 1, "m1"),
                test_message("proj-a", &channel.channel_id, 2, "m2"),
            ],
        );
        *fake.next_seq.lock().unwrap() = HashMap::from([(channel.channel_id, 3)]);
    }
    let mut app = app_with_project("proj-a");
    let mut rx = wire_fake(&mut app, fake.clone());

    app.refresh_chat("proj-a".to_string());
    let (channel_id, cursor) = match recv_cmd(&mut rx).await {
        TuiCommand::ChatHistoryLoaded {
            request_id,
            project_id,
            channel_id,
            messages,
            next_cursor,
            truncated,
            retention_floor_seq,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => {
            assert!(error.is_none());
            app.apply_chat_history(
                request_id,
                project_id,
                channel_id.clone(),
                messages,
                next_cursor,
                truncated,
                retention_floor_seq,
                None,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
            (channel_id, next_cursor)
        }
        other => panic!("expected ChatHistoryLoaded, got {other:?}"),
    };
    assert_eq!(app.chat.get("proj-a").unwrap().messages.len(), 2);

    // Incremental sync from the cached cursor converges with no dupes.
    app.sync_chat("proj-a".to_string());
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatSyncLoaded {
            request_id,
            project_id,
            channel_id: sync_channel,
            messages,
            next_cursor,
            resync_required,
            retention_floor_seq,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => {
            assert_eq!(sync_channel, channel_id);
            assert!(!resync_required);
            assert!(error.is_none());
            app.apply_chat_sync(
                request_id,
                project_id,
                sync_channel,
                messages,
                next_cursor,
                false,
                retention_floor_seq,
                None,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("expected ChatSyncLoaded, got {other:?}"),
    }
    assert_eq!(app.chat.get("proj-a").unwrap().messages.len(), 2);
    assert_eq!(app.chat.get("proj-a").unwrap().next_cursor, cursor);

    // A daemon resync page replaces the window deterministically.
    let epoch = app.chat.reconnect_epoch;
    let req = app.chat.begin_history("proj-a", &channel_id).unwrap();
    assert!(app.chat.apply_sync(
        req,
        "proj-a",
        &channel_id,
        vec![test_message("proj-a", &channel_id, 50, "m50")],
        51,
        true,
        40,
        epoch
    ));
    let entry = app.chat.get("proj-a").unwrap();
    assert_eq!(entry.messages.len(), 1);
    assert_eq!(entry.messages[0].seq, 50);
    assert_eq!(entry.retention_floor_seq, 40);
}

// ── Unread / read ────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unread_badge_clears_on_read() {
    let fake = Arc::new(FakeChatClient::supporting());
    let mut app = app_with_project("proj-a");
    let mut rx = wire_fake(&mut app, fake.clone());

    app.send_chat_message("proj-a".to_string(), "one".to_string(), None);
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatMessageSent {
            project_id,
            channel_id,
            message,
            draft,
            unauthorized,
            unsupported,
            reconnect_epoch,
            ..
        } => app.apply_chat_sent(
            project_id,
            channel_id,
            message,
            false,
            draft,
            None,
            unauthorized,
            unsupported,
            reconnect_epoch,
        ),
        other => panic!("expected ChatMessageSent, got {other:?}"),
    }
    // A live event from another principal arrives; without a known
    // marker there is no unread badge yet (fail closed).
    let live = {
        let entry = app.chat.get("proj-a").unwrap();
        let channel = entry.active_channel_id.clone().unwrap();
        let mut m = test_message("proj-a", &channel, 2, "m-live");
        m.author_principal = "bob".to_string();
        m
    };
    app.apply_chat_event_committed(live);
    assert_eq!(app.chat.get("proj-a").unwrap().unread_count(), 0);

    // Mark read through the daemon; the badge converges.
    app.chat.apply_read_marker(
        "proj-a",
        &app.chat
            .get("proj-a")
            .unwrap()
            .active_channel_id
            .clone()
            .unwrap(),
        1,
    );
    assert_eq!(app.chat.get("proj-a").unwrap().unread_count(), 1);
    let summary = app.chat.header_summary("proj-a").unwrap();
    assert!(
        summary.contains("unread"),
        "summary shows unread: {summary}"
    );

    // Full read via the async path clears the badge.
    app.mark_chat_read("proj-a".to_string());
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatReadMarkerSet {
            project_id,
            channel_id,
            last_read_seq,
            error,
        } => {
            assert!(error.is_none());
            app.apply_chat_read(project_id, channel_id, last_read_seq, None);
        }
        other => panic!("expected ChatReadMarkerSet, got {other:?}"),
    }
    assert_eq!(app.chat.get("proj-a").unwrap().unread_count(), 0);
    assert!(fake.kinds().contains(&"chat_read_set".to_string()));
}

// ── Composing ────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composing_set_lists_and_expires() {
    let fake = Arc::new(FakeChatClient::supporting());
    let mut app = app_with_project("proj-a");
    let mut rx = wire_fake(&mut app, fake.clone());

    // Load the window first so the channel resolves.
    app.refresh_chat("proj-a".to_string());
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatHistoryLoaded {
            request_id,
            project_id,
            channel_id,
            messages,
            next_cursor,
            truncated,
            retention_floor_seq,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => app.apply_chat_history(
            request_id,
            project_id,
            channel_id,
            messages,
            next_cursor,
            truncated,
            retention_floor_seq,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        ),
        other => panic!("expected ChatHistoryLoaded, got {other:?}"),
    }

    // Publish a composing lease, then list it back through the daemon.
    app.set_chat_composing("proj-a".to_string(), true);
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatComposingLoaded {
            project_id,
            channel_id,
            composing,
            error,
            unauthorized,
            unsupported,
            ..
        } => {
            assert!(error.is_none());
            app.apply_chat_composing(
                project_id,
                channel_id,
                composing,
                None,
                unauthorized,
                unsupported,
                0,
            );
        }
        other => panic!("expected ChatComposingLoaded, got {other:?}"),
    }
    assert!(fake.kinds().contains(&"chat_composing_set".to_string()));
    // The daemon lease round-trips into the panel; display filters by
    // expiry (fake leases use a far-future expiry so they show now).
    let lines = app.chat.panel_lines("proj-a", 500_000);
    assert!(lines.iter().any(|l| l.contains("composing")), "{lines:?}");
    let expired = app.chat.panel_lines("proj-a", 2_000_000);
    assert!(!expired.iter().any(|l| l.contains("composing")));
}

// ── Authorization denial ─────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthorized_and_absent_render_identically_with_draft_retained() {
    let fake = Arc::new(FakeChatClient::supporting());
    fake.unauthorized.lock().unwrap().push("secret".to_string());
    let mut app = app_with_project("secret");
    let mut rx = wire_fake(&mut app, fake.clone());

    app.refresh_chat("secret".to_string());
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatHistoryLoaded {
            request_id,
            project_id,
            channel_id,
            messages,
            next_cursor,
            truncated,
            retention_floor_seq,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => {
            assert!(unauthorized);
            app.apply_chat_history(
                request_id,
                project_id,
                channel_id,
                messages,
                next_cursor,
                truncated,
                retention_floor_seq,
                error,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("expected ChatHistoryLoaded, got {other:?}"),
    }
    // Denied content is cleared; panel is the generic unavailable shape.
    assert_eq!(app.chat.get("secret").unwrap().messages.len(), 0);
    assert_eq!(app.chat.header_summary("secret"), None);

    // A failed send retains the editable draft and stores nothing.
    app.send_chat_message("secret".to_string(), "unsent words".to_string(), None);
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatMessageSent {
            project_id,
            channel_id,
            message,
            draft,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
            ..
        } => {
            assert!(message.is_none());
            assert!(unauthorized);
            app.apply_chat_sent(
                project_id,
                channel_id,
                None,
                false,
                draft,
                error,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("expected ChatMessageSent, got {other:?}"),
    }
    assert_eq!(app.chat.draft_for("secret"), "unsent words");
    assert!(app.chat.get("secret").unwrap().messages.is_empty());
    // Absent projects render the identical panel (denials are
    // indistinguishable from absent — same convention as M001).
    let epoch = app.chat.reconnect_epoch;
    let req_absent = app.chat.begin_ensure("absent").unwrap();
    assert!(app.chat.apply_error(
        req_absent,
        "absent",
        "project_not_found: absent".to_string(),
        true,
        false,
        epoch
    ));
    assert_eq!(
        app.chat.panel_lines("secret", 0),
        app.chat.panel_lines("absent", 0)
    );
}

// ── Observer insert routing: zero control ────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn observer_insert_routes_to_chat_with_zero_control_requests() {
    let fake = Arc::new(FakeChatClient::supporting());
    let mut app = app_with_project("proj-chat");
    let mut rx = wire_fake(&mut app, fake.clone());

    // Begin observing another session in the same project.
    let _ = app
        .observer
        .begin_observe("proj-chat", "sess-observed")
        .expect("valid locators");
    assert!(app.observer.blocks_prompt_submit());
    assert!(app.observer.blocks_permission_response());

    // Chat slash commands are allowlisted while observing; every control
    // family stays blocked.
    for allowed in ["/chat", "/chat-send", "/chat-history", "/chat-read"] {
        assert!(
            !app.observer.blocks_command(allowed),
            "chat command should be allowed while observing: {allowed}"
        );
    }
    for blocked in [
        "/turn",
        "turn_submit",
        "turn_steer",
        "turn_cancel",
        "permission_respond",
        "question_respond",
        "/models",
        "/new",
        "/revert",
        "/lsp-preview-apply",
        "/shell-rerun",
        "/terminal-send",
        "/plugin-enable",
    ] {
        assert!(
            app.observer.blocks_command(blocked),
            "control must stay blocked while observing: {blocked}"
        );
    }

    // Bare observer insert-mode text routes to project chat.
    assert!(app.route_observer_text_to_chat("hello from the observer"));
    let completion = recv_cmd(&mut rx).await;
    match completion {
        TuiCommand::ChatMessageSent {
            project_id,
            channel_id,
            message,
            draft,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
            ..
        } => {
            assert_eq!(project_id, "proj-chat");
            assert!(error.is_none());
            app.apply_chat_sent(
                project_id,
                channel_id,
                message,
                false,
                draft,
                None,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("expected ChatMessageSent, got {other:?}"),
    }
    // The message landed in the observed project's chat.
    let entry = app.chat.get("proj-chat").unwrap();
    assert_eq!(entry.messages.len(), 1);
    assert_eq!(entry.messages[0].body, "hello from the observer");

    // Zero-control proof: every core request on this path is a chat
    // request. No turn submit/steer/cancel, no permission/question
    // answer, no shell spawn.
    let kinds = fake.kinds();
    assert!(!kinds.is_empty(), "expected chat requests to be recorded");
    for kind in &kinds {
        assert!(
            kind.starts_with("chat_"),
            "observer insert must emit zero control requests, saw: {kind}"
        );
        assert!(
            !kind.contains("turn")
                && !kind.contains("permission")
                && !kind.contains("question")
                && !kind.contains("cancel")
                && !kind.contains("steer"),
            "observer insert emitted a control request: {kind}"
        );
    }
    // Observation itself is untouched: still read-only, target intact.
    assert!(app.observer.is_observing());
    assert_eq!(app.observer.observed_session_id(), Some("sess-observed"));
    assert!(app.observer.blocks_prompt_submit());
    assert!(app.observer.blocks_permission_response());
}

// ── Reconnect ────────────────────────────────────────────────────────────

#[test]
fn reconnect_resumes_cursor_or_resyncs_window() {
    let mut app = app_with_project("proj-a");
    let epoch = app.chat.reconnect_epoch;
    let ensure_req = app.chat.begin_ensure("proj-a").unwrap();
    assert!(app.chat.apply_channel_ensured(
        ensure_req,
        "proj-a",
        &ChatChannelDto {
            channel_id: "ch-a".to_string(),
            project_id: "proj-a".to_string(),
            name: "general".to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 1,
        },
        epoch
    ));
    let hist_req = app.chat.begin_history("proj-a", "ch-a").unwrap();
    assert!(app.chat.apply_history(
        hist_req,
        "proj-a",
        "ch-a",
        vec![test_message("proj-a", "ch-a", 1, "m1")],
        2,
        false,
        0,
        epoch
    ));
    // In-flight fetch + reconnect: pre-reconnect completion drops, the
    // entry flags resync, and the M001 cursor survives for resume.
    let in_flight = app.chat.begin_history("proj-a", "ch-a").unwrap();
    let new_epoch = app.chat.on_reconnect();
    assert_ne!(epoch, new_epoch);
    assert!(!app.chat.apply_history(
        in_flight,
        "proj-a",
        "ch-a",
        vec![test_message("proj-a", "ch-a", 9, "ghost")],
        10,
        false,
        0,
        epoch
    ));
    let entry = app.chat.get("proj-a").unwrap();
    assert!(entry.needs_resync);
    assert_eq!(entry.next_cursor, 2);
    assert_eq!(entry.messages.len(), 1);
    // Resume from the cached cursor converges.
    let resume = app.chat.begin_history("proj-a", "ch-a").unwrap();
    assert!(app.chat.apply_sync(
        resume,
        "proj-a",
        "ch-a",
        vec![test_message("proj-a", "ch-a", 2, "m2")],
        3,
        false,
        0,
        new_epoch
    ));
    assert!(!app.chat.get("proj-a").unwrap().needs_resync);
}

// ── Focus / keys / bounds ────────────────────────────────────────────────

#[test]
fn chat_panel_opens_with_standard_focus_keys_and_stays_bounded() {
    let mut app = app_with_project("proj-a");
    app.chat.set_capability(true);
    let epoch = app.chat.reconnect_epoch;
    let ensure_req = app.chat.begin_ensure("proj-a").unwrap();
    assert!(app.chat.apply_channel_ensured(
        ensure_req,
        "proj-a",
        &ChatChannelDto {
            channel_id: "ch-a".to_string(),
            project_id: "proj-a".to_string(),
            name: "general".to_string(),
            created_by: "alice".to_string(),
            created_at_ms: 1,
        },
        epoch
    ));
    app.show_chat();
    // Panel opens as the standard scrollable info dialog (j/k scroll,
    // Esc/Enter close — no session state mutated).
    let dialog = app.dialog_state.info_dialog.as_ref().expect("panel open");
    assert_eq!(
        dialog.info_type(),
        codegg::tui::components::dialogs::info::InfoType::ProjectChat
    );
    assert_eq!(app.chat_panel_project.as_deref(), Some("proj-a"));
    assert_eq!(app.open_tab_count(), 1);

    // Bounded history: 110 retained messages keep a 100-row window and
    // the panel renders at most the display bound plus chrome.
    let hist_req = app.chat.begin_history("proj-a", "ch-a").unwrap();
    let many: Vec<ChatMessageDto> = (1..=110)
        .map(|s| test_message("proj-a", "ch-a", s, &format!("m{s}")))
        .collect();
    assert!(app
        .chat
        .apply_history(hist_req, "proj-a", "ch-a", many, 111, true, 5, epoch));
    assert_eq!(app.chat.get("proj-a").unwrap().messages.len(), 100);
    let lines = app.chat.panel_lines("proj-a", 0);
    assert!(
        lines.len() <= 100,
        "panel stays bounded, got {}",
        lines.len()
    );
    assert!(lines.iter().any(|l| l.contains("older messages")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_daemon_hides_panel_without_breaking_tabs() {
    let fake = Arc::new(FakeChatClient {
        advertise: false,
        ..Default::default()
    });
    let mut app = app_with_project("proj-a");
    let mut rx = wire_fake(&mut app, fake);
    assert_eq!(app.open_tab_count(), 1);

    app.refresh_chat("proj-a".to_string());
    match recv_cmd(&mut rx).await {
        TuiCommand::ChatHistoryLoaded {
            request_id,
            project_id,
            channel_id,
            messages,
            next_cursor,
            truncated,
            retention_floor_seq,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => {
            assert!(unsupported);
            app.apply_chat_history(
                request_id,
                project_id,
                channel_id,
                messages,
                next_cursor,
                truncated,
                retention_floor_seq,
                error,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("expected ChatHistoryLoaded, got {other:?}"),
    }
    assert!(!app.chat_supported());
    assert_eq!(app.chat.header_summary("proj-a"), None);
    assert!(app
        .chat
        .panel_lines("proj-a", 0)
        .iter()
        .any(|l| l.contains("unavailable")));
    // Local session operation is unchanged: tabs keep working.
    assert_eq!(app.open_tab_count(), 1);
    assert!(app.active_tab().is_some());
}

// ── Edit / redact ────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_redact_round_trip() {
    let fake = Arc::new(FakeChatClient::supporting());
    let mut app = app_with_project("proj-a");
    let mut rx = wire_fake(&mut app, fake.clone());

    app.send_chat_message("proj-a".to_string(), "original".to_string(), None);
    let sent = match recv_cmd(&mut rx).await {
        TuiCommand::ChatMessageSent {
            project_id,
            channel_id,
            message,
            draft,
            unauthorized,
            unsupported,
            reconnect_epoch,
            ..
        } => {
            let message = message.expect("send succeeds");
            app.apply_chat_sent(
                project_id,
                channel_id,
                Some(message.clone()),
                false,
                draft,
                None,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
            message
        }
        other => panic!("expected ChatMessageSent, got {other:?}"),
    };

    // Edit bumps the revision in place.
    let channel = app
        .chat
        .get("proj-a")
        .unwrap()
        .active_channel_id
        .clone()
        .unwrap();
    let edit_req = codegg::core::new_request(
        "edit-1".to_string(),
        CoreRequest::ChatEdit {
            channel_id: channel.clone(),
            message_id: sent.message_id.clone(),
            expected_revision: 0,
            new_body: "edited body".to_string(),
        },
    );
    let edited = match fake.request(edit_req).await.unwrap() {
        CoreResponse::ChatMessage { message, .. } => message,
        other => panic!("expected edited message, got {other:?}"),
    };
    assert_eq!(edited.revision, 1);
    app.apply_chat_event_edited(edited);
    let stored = app
        .chat
        .get("proj-a")
        .unwrap()
        .messages
        .iter()
        .find(|m| m.message_id == sent.message_id)
        .unwrap();
    assert_eq!(stored.body, "edited body");
    let lines = app.chat.panel_lines("proj-a", 0);
    assert!(lines.iter().any(|l| l.contains("(edited r1)")));

    // Redact replaces the body daemon-side; the panel shows the marker.
    app.apply_chat_event_redacted("proj-a".to_string(), channel, sent.message_id.clone(), 2);
    let redacted = app
        .chat
        .get("proj-a")
        .unwrap()
        .messages
        .iter()
        .find(|m| m.message_id == sent.message_id)
        .unwrap();
    assert!(redacted.redacted);
    let lines = app.chat.panel_lines("proj-a", 0);
    assert!(lines.iter().any(|l| l.contains("REDACTED")));
}
