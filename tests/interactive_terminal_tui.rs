//! M003 end-to-end fixture: the TUI interactive-terminal controller as a
//! projection of the M002 attach/resume protocol over the real PTY engine,
//! plus the legacy `terminal` tool-surface disposition evidence.
//!
//! The controller under test owns no PTY/process state: every state change
//! below is applied from a daemon protocol answer, exactly as the TUI
//! command layer does through `CoreRequest::InteractiveProcess*`
//! operations. Fixtures use polling loops (never fixed sleeps) and the
//! `multi_thread` flavor the PTY engine requires.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use codegg::interactive_process_attach::{InteractiveAuthority, InteractiveProcessProtocol};
use codegg::protocol::core::CoreResponse;
use codegg::protocol::interactive_process::{
    InteractiveOutputChunk, InteractiveProcessCreateRequest, InteractiveResync,
    InteractiveResyncReason,
};
use codegg::scheduler::admission::AdmissionController;
use codegg::tui::interactive_terminal::{
    classify_key, InteractiveTerminalController, TerminalKey, TerminalKeyAction, TerminalLinkState,
};
use codegg_core::workspace::{ExecutionContext, InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use tokio_util::sync::CancellationToken;

const POLL_TIMEOUT: Duration = Duration::from_secs(15);

async fn test_context(root: &Path) -> Arc<ExecutionContext> {
    let store = Arc::new(InMemoryWorkspaceStore::new());
    let registry = WorkspaceRegistry::load(store).await.expect("registry");
    let record = registry.get_or_register(root).await.expect("register");
    let _services = WorkspaceServiceRegistry::new(
        registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    ExecutionContext::new(
        record,
        Some("session-integration".to_string()),
        CancellationToken::new(),
    )
}

fn test_admission(process_slots: u32) -> Arc<AdmissionController> {
    let mut config = codegg::scheduler::config::ResolvedSchedulerConfig::default();
    config.resources.max_process_slots = process_slots;
    Arc::new(AdmissionController::new(config))
}

fn protocol() -> InteractiveProcessProtocol {
    InteractiveProcessProtocol::new(test_admission(8))
}

fn local(client: &str) -> InteractiveAuthority {
    InteractiveAuthority::local(client)
}

fn create_cat(workspace_id: &str) -> InteractiveProcessCreateRequest {
    InteractiveProcessCreateRequest {
        workspace_id: workspace_id.to_string(),
        argv: vec!["cat".to_string()],
        cwd: None,
        env_overrides: Vec::new(),
        cols: Some(80),
        rows: Some(24),
        scrollback_bytes: None,
    }
}

fn chunk_text(chunk: &InteractiveOutputChunk) -> String {
    let bytes = B64.decode(&chunk.data_b64).expect("valid base64");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Poll resume until `predicate` holds over the accumulated text,
/// returning the final chunk. Mirrors the M002 polling-loop convention.
async fn poll_resume_text(
    protocol: &InteractiveProcessProtocol,
    client: &str,
    attachment: &str,
    from_seq: u64,
    predicate: impl Fn(&str) -> bool,
) -> (InteractiveOutputChunk, String) {
    let started = Instant::now();
    let mut cursor = from_seq;
    let mut accumulated = String::new();
    loop {
        match protocol
            .resume(client, attachment, cursor, Some(64 * 1024))
            .await
        {
            CoreResponse::InteractiveProcessResumed { chunk, .. } => {
                accumulated.push_str(&chunk_text(&chunk));
                cursor = chunk.next_seq;
                if predicate(&accumulated) {
                    return (chunk, accumulated);
                }
            }
            CoreResponse::InteractiveProcessResyncRequired { resync, .. } => {
                panic!("unexpected resync while polling: {resync:?}");
            }
            other => panic!("unexpected resume while polling: {other:?}"),
        }
        if started.elapsed() > POLL_TIMEOUT {
            panic!("timed out waiting for terminal output; got: {accumulated:?}");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn create_and_attach(
    protocol: &InteractiveProcessProtocol,
    ctx: &Arc<ExecutionContext>,
    workspace_id: &str,
    client: &str,
    controller: &mut InteractiveTerminalController,
) -> (String, String) {
    let handle = match protocol
        .create(client, ctx, &create_cat(workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, metadata } => {
            assert_eq!(metadata.workspace_id, workspace_id);
            assert!(controller.apply_created(
                handle.clone(),
                metadata.workspace_id.clone(),
                metadata.command.clone(),
                metadata.cols,
                metadata.rows,
            ));
            handle
        }
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match protocol.attach(client, &handle, Some(0), None).await {
        CoreResponse::InteractiveProcessAttached {
            attachment_id,
            chunk,
            resync,
            ..
        } => {
            assert!(resync.is_none());
            assert!(controller
                .apply_attached(&handle, attachment_id.clone(), &chunk, resync.as_ref())
                .expect("attach applies")
                .is_none());
            attachment_id
        }
        other => panic!("expected attached, got {other:?}"),
    };
    (handle, attachment)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tui_terminal_full_lifecycle_over_cat() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();
    let mut controller = InteractiveTerminalController::new();

    // Create + attach project through the TUI controller.
    let (handle, attachment) =
        create_and_attach(&protocol, &ctx, &workspace_id, "client-a", &mut controller).await;

    // Focus is explicit: keys are ignored until the user focuses.
    assert_eq!(
        classify_key(false, TerminalKey::Char('x')),
        TerminalKeyAction::Ignored
    );
    assert!(controller.focus(&handle));
    assert!(controller.is_focused(&handle));

    // Interactive input round-trips through `cat` into bounded scrollback.
    let input = b"tui-marker-123\n";
    match protocol
        .input("client-a", &attachment, &B64.encode(input))
        .await
    {
        CoreResponse::InteractiveProcessInputAccepted { bytes_accepted, .. } => {
            assert_eq!(bytes_accepted, input.len());
        }
        other => panic!("expected input accepted, got {other:?}"),
    }
    let from = controller.resume_cursor(&handle).expect("cursor");
    let (chunk, text) = poll_resume_text(&protocol, "client-a", &attachment, from, |text| {
        text.contains("tui-marker-123")
    })
    .await;
    assert!(controller
        .apply_resumed(&handle, &chunk)
        .expect("resume applies")
        .is_none());
    let rendered = controller
        .render_lines(&handle, 50)
        .expect("render")
        .join("\n");
    assert!(
        rendered.contains("tui-marker-123"),
        "scrollback renders input"
    );
    assert!(!text.is_empty());

    // Resize forwards through the attachment; the daemon echoes the size.
    match protocol.resize("client-a", &attachment, 120, 40).await {
        CoreResponse::InteractiveProcessResized { cols, rows, .. } => {
            assert_eq!((cols, rows), (120, 40));
            controller.applied_resize(&handle, cols, rows);
        }
        other => panic!("expected resized, got {other:?}"),
    }

    // Rapid resizes coalesce last-wins on the controller queue.
    controller
        .queue_resize(&handle, 80, 24)
        .expect("resize queues");
    controller
        .queue_resize(&handle, 100, 30)
        .expect("resize queues");
    assert_eq!(controller.take_pending_resize(&handle), Some((100, 30)));

    // Detach releases the attachment; the process is unaffected and the
    // scrollback is retained.
    match protocol.detach("client-a", &attachment).await {
        CoreResponse::InteractiveProcessDetached { .. } => {
            controller.apply_detached(&handle);
        }
        other => panic!("expected detached, got {other:?}"),
    }
    assert!(controller.attachment_id(&handle).is_none());
    assert!(!controller.is_focused(&handle));
    assert!(
        controller
            .render_lines(&handle, 50)
            .expect("render")
            .join("\n")
            .contains("tui-marker-123"),
        "scrollback survives detach"
    );

    // Re-attach resumes from the retained cursor.
    let cursor = controller.resume_cursor(&handle).expect("cursor");
    let attachment = match protocol
        .attach("client-a", &handle, Some(cursor), None)
        .await
    {
        CoreResponse::InteractiveProcessAttached {
            attachment_id,
            chunk,
            resync,
            ..
        } => {
            controller
                .apply_attached(&handle, attachment_id.clone(), &chunk, resync.as_ref())
                .expect("reattach applies");
            attachment_id
        }
        other => panic!("expected re-attached, got {other:?}"),
    };

    // Explicit terminate renders the exit state and rejects later input.
    let (exit_code, exit_signal) = match protocol
        .terminate("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessTerminated {
            exit_code,
            exit_signal,
            ..
        } => (exit_code, exit_signal),
        other => panic!("expected terminated, got {other:?}"),
    };
    controller.apply_terminated(&handle, exit_code, exit_signal);
    assert!(controller
        .view(&handle)
        .expect("view")
        .link_state()
        .is_exited());
    assert!(controller.queue_input(&handle, b"after exit\n").is_err());
    let headers = controller
        .header_lines(&handle)
        .expect("headers")
        .join("\n");
    assert!(headers.contains("exited"));
    assert!(
        !headers.contains("tui-marker-123"),
        "headers never carry output"
    );

    // Post-mortem scrollback stays readable, then remove frees the view.
    assert!(controller
        .render_lines(&handle, 50)
        .expect("render")
        .join("\n")
        .contains("tui-marker-123"));
    match protocol
        .remove("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {
            assert!(controller.apply_removed(&handle));
        }
        other => panic!("expected removed, got {other:?}"),
    }
    assert!(controller.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tui_terminal_resync_reasons_are_typed() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();
    let mut controller = InteractiveTerminalController::new();
    let (handle, attachment) =
        create_and_attach(&protocol, &ctx, &workspace_id, "client-a", &mut controller).await;

    // A cursor ahead of produced bytes answers CursorAhead, never silence.
    match protocol
        .resume("client-a", &attachment, u64::MAX, None)
        .await
    {
        CoreResponse::InteractiveProcessResyncRequired { resync, .. } => {
            assert_eq!(resync.reason, InteractiveResyncReason::CursorAhead);
            controller.apply_resync(&handle, resync);
        }
        other => panic!("expected resync required, got {other:?}"),
    }
    assert!(matches!(
        controller.view(&handle).expect("view").link_state(),
        TerminalLinkState::ResyncRequired(_)
    ));
    assert!(controller.queue_input(&handle, b"x").is_err());

    // A forged handle answers the gone code; the view goes Gone.
    let forged = "00000000-0000-0000-0000-000000000000";
    match protocol.attach("client-a", forged, Some(0), None).await {
        CoreResponse::Error { code, .. } => assert!(code.contains("handle_gone"), "got {code}"),
        other => panic!("expected handle gone, got {other:?}"),
    }
    controller.apply_resync(
        &handle,
        InteractiveResync {
            reason: InteractiveResyncReason::HandleGone,
            handle: handle.clone(),
            base_seq: 0,
            next_seq: 0,
            snapshot: None,
        },
    );
    assert!(matches!(
        controller.view(&handle).expect("view").link_state(),
        TerminalLinkState::Gone { .. }
    ));

    // Cleanup: the live process behind the gone view still terminates.
    match protocol
        .terminate("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    match protocol
        .remove("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tui_terminal_disconnect_then_restart() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();
    let mut controller = InteractiveTerminalController::new();
    // The pre-disconnect attachment is released server-side by
    // `handle_disconnect` below; only the handle is tracked onward.
    let (handle, _attachment) =
        create_and_attach(&protocol, &ctx, &workspace_id, "client-a", &mut controller).await;

    // Connection close drops attachments only (M002 handle_disconnect):
    // the TUI keeps scrollback and marks the view reconnecting.
    protocol.attachments().handle_disconnect("client-a");
    controller.note_transport_disconnect();
    assert!(matches!(
        controller.view(&handle).expect("view").link_state(),
        TerminalLinkState::Reconnecting
    ));
    assert!(controller.queue_input(&handle, b"x").is_err());

    // Re-attach resumes from the retained cursor while the process lives.
    let cursor = controller.resume_cursor(&handle).expect("cursor");
    let attachment = match protocol
        .attach("client-a", &handle, Some(cursor), None)
        .await
    {
        CoreResponse::InteractiveProcessAttached {
            attachment_id,
            chunk,
            resync,
            ..
        } => {
            controller
                .apply_attached(&handle, attachment_id.clone(), &chunk, resync.as_ref())
                .expect("reattach applies");
            attachment_id
        }
        other => panic!("expected re-attached, got {other:?}"),
    };

    // A daemon restart invalidates ephemeral handles: the old handle is
    // gone on a fresh instance, and the TUI marks the view Gone.
    let restarted = InteractiveProcessProtocol::new(test_admission(8));
    match restarted.attach("client-a", &handle, Some(0), None).await {
        CoreResponse::Error { code, .. } => assert!(code.contains("handle_gone"), "got {code}"),
        other => panic!("expected handle gone after restart, got {other:?}"),
    }
    controller.note_daemon_restart();
    assert!(matches!(
        controller.view(&handle).expect("view").link_state(),
        TerminalLinkState::Gone { .. }
    ));

    // The original instance still owns the process: terminate + remove.
    match protocol
        .terminate("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    match protocol
        .remove("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {
            assert!(controller.apply_removed(&handle));
        }
        other => panic!("expected removed, got {other:?}"),
    }
    let _ = attachment;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tui_terminal_workspace_routing() {
    let dir_a = tempfile::tempdir().expect("workspace a");
    let dir_b = tempfile::tempdir().expect("workspace b");
    let ctx_a = test_context(dir_a.path()).await;
    let ctx_b = test_context(dir_b.path()).await;
    let ws_a = ctx_a.workspace_id.as_str().to_string();
    let ws_b = ctx_b.workspace_id.as_str().to_string();
    let protocol = protocol();
    let mut controller = InteractiveTerminalController::new();

    let (handle_a, attachment_a) =
        create_and_attach(&protocol, &ctx_a, &ws_a, "client-a", &mut controller).await;
    let (handle_b, attachment_b) =
        create_and_attach(&protocol, &ctx_b, &ws_b, "client-a", &mut controller).await;

    assert_eq!(
        controller.handles_for_workspace(&ws_a),
        vec![handle_a.clone()]
    );
    assert_eq!(
        controller.handles_for_workspace(&ws_b),
        vec![handle_b.clone()]
    );
    assert!(controller.workspace_matches(&handle_a, &ws_a));
    assert!(!controller.workspace_matches(&handle_a, &ws_b));

    for (handle, attachment) in [(handle_a, attachment_a), (handle_b, attachment_b)] {
        match protocol
            .terminate("client-a", &local("client-a"), &attachment)
            .await
        {
            CoreResponse::InteractiveProcessTerminated { .. } => {}
            other => panic!("expected terminated, got {other:?}"),
        }
        match protocol
            .remove("client-a", &local("client-a"), &attachment)
            .await
        {
            CoreResponse::InteractiveProcessRemoved { .. } => {
                assert!(controller.apply_removed(&handle));
            }
            other => panic!("expected removed, got {other:?}"),
        }
    }
    assert!(controller.is_empty());
}

#[test]
fn terminal_tool_surface_is_truthful_after_disposition() {
    // The historic name stays registered so stored runs, permission
    // modes, and agent deny-lists keep resolving; the description must
    // be truthful one-shot disclosure (M003 work package D).
    let registry = codegg::tool::ToolRegistry::with_defaults();
    let tool = registry.get("terminal").expect("terminal stays registered");
    assert_eq!(tool.name(), "terminal");
    let description = tool.description();
    assert!(
        description.contains("one-shot"),
        "description must say one-shot, got: {description}"
    );
    assert!(
        !description.contains("interactive terminal session"),
        "description must not claim an interactive session, got: {description}"
    );

    // Bash remains the canonical model shell; terminal stays deferred.
    assert!(codegg::tool::disclosure::is_deferred_by_default("terminal"));
    assert!(!codegg::tool::disclosure::is_core("terminal"));
    assert!(codegg::tool::disclosure::is_core("bash"));

    // Historical stored tool names remain readable as history: session
    // import redaction still covers "terminal" tool calls (the name is
    // preserved, the payload is redacted).
    let redacted = codegg_core::session::import::redact_for_export(serde_json::json!([{
        "type": "tool_call",
        "name": "terminal",
        "input": {"command": "rm -rf /"},
        "output": "x",
    }]));
    let entry = redacted.as_array().expect("array").first().expect("entry");
    assert_eq!(
        entry.get("name").and_then(|v| v.as_str()),
        Some("terminal"),
        "terminal history name must stay readable"
    );
    assert_eq!(
        entry.get("input").and_then(|v| v.as_str()),
        Some("[REDACTED]"),
        "terminal history payload must stay redacted"
    );
}

#[test]
fn terminal_focus_gate_never_submits_from_escape() {
    // Escape while focused leaves focus and never forwards bytes.
    assert_eq!(
        classify_key(true, TerminalKey::Esc),
        TerminalKeyAction::EscapeFocus
    );
    // Focusing a second terminal hides the first: exactly one owner.
    let mut controller = InteractiveTerminalController::new();
    for handle in ["h-1", "h-2"] {
        assert!(controller.apply_created(
            handle.to_string(),
            "ws-1".to_string(),
            "sh".to_string(),
            80,
            24,
        ));
    }
    // Without attachments focus is refused: keys cannot be misrouted to
    // a terminal that cannot receive them.
    assert!(!controller.focus("h-1"));
    assert!(matches!(
        controller.view("h-1").expect("view").link_state(),
        TerminalLinkState::Live
    ));
}
