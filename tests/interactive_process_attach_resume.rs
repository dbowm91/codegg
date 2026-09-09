//! Integration fixtures for M002: bounded attach/resume protocol.
//!
//! These tests drive the daemon-owned [`InteractiveProcessProtocol`]
//! (attachment registry, transport-derived ownership, cursor/resync)
//! over the real supported-host M001 PTY engine, plus the `CoreDaemon`
//! dispatch arms. They are the M002 acceptance matrix: create/list,
//! attach/detach, input/resize/terminate/remove, spoofed IDs, two-client
//! policy, disconnect survival, explicit kill, sequence/lag/resync,
//! saturation bounds, writer failure, shutdown/restart gone-handles, and
//! unknown-capability compatibility.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use codegg::core::daemon::CoreDaemon;
use codegg::interactive_process::{InteractiveProcessService, SessionState};
use codegg::interactive_process_attach::{InteractiveAuthority, InteractiveProcessProtocol};
use codegg::protocol::core::{CoreRequest, CoreResponse, RequestEnvelope};
use codegg::protocol::interactive_process::{
    InteractiveOutputChunk, InteractiveProcessCapabilities, InteractiveProcessCreateRequest,
    InteractiveResyncReason, INTERACTIVE_PROCESS_PROTOCOL_VERSION,
};
use codegg::scheduler::admission::AdmissionController;
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

async fn wait_for_seq(
    protocol: &InteractiveProcessProtocol,
    client: &str,
    attachment: &str,
    predicate: impl Fn(u64) -> bool,
) -> u64 {
    let started = Instant::now();
    loop {
        let seq = match protocol.resume(client, attachment, 0, Some(1024)).await {
            CoreResponse::InteractiveProcessResumed { chunk, .. } => chunk.next_seq,
            CoreResponse::InteractiveProcessResyncRequired { resync, .. } => resync.next_seq,
            other => panic!("unexpected resume while polling: {other:?}"),
        };
        if predicate(seq) {
            return seq;
        }
        if started.elapsed() > POLL_TIMEOUT {
            panic!("timed out waiting for output sequence");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn wait_for_exit(
    service: &InteractiveProcessService,
    handle_raw: &str,
) -> codegg::interactive_process::SessionSnapshot {
    use codegg::interactive_process::InteractiveHandle;
    let handle = InteractiveHandle::parse(handle_raw).expect("handle");
    let started = Instant::now();
    loop {
        let snapshot = service.snapshot(&handle).await.expect("snapshot");
        if matches!(snapshot.state, SessionState::Exited) {
            return snapshot;
        }
        if started.elapsed() > POLL_TIMEOUT {
            panic!("timed out waiting for session exit");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn expect_error_code(response: CoreResponse) -> String {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_resume_full_lifecycle_over_cat() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    // Create returns a handle with no attachment yet.
    let handle = match protocol
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, metadata } => {
            assert_eq!(handle, metadata.handle);
            assert_eq!(metadata.workspace_id, workspace_id);
            assert_eq!(metadata.command, "cat");
            handle
        }
        other => panic!("expected created, got {other:?}"),
    };

    // List observes the new process metadata (no output bytes).
    match protocol.list("client-a", None, None).await {
        CoreResponse::InteractiveProcessList { processes, .. } => {
            assert!(processes.iter().any(|entry| entry.handle == handle));
        }
        other => panic!("expected list, got {other:?}"),
    }

    // Attach from the beginning of the stream.
    let attachment = match protocol.attach("client-a", &handle, Some(0), None).await {
        CoreResponse::InteractiveProcessAttached {
            attachment_id,
            handle: attached,
            chunk,
            resync,
        } => {
            assert_eq!(attached, handle);
            assert_eq!(chunk.from_seq, 0);
            assert!(resync.is_none());
            attachment_id
        }
        other => panic!("expected attached, got {other:?}"),
    };

    // Input through the attachment round-trips through `cat`.
    match protocol
        .input("client-a", &attachment, &B64.encode(b"attach-round-trip\n"))
        .await
    {
        CoreResponse::InteractiveProcessInputAccepted { bytes_accepted, .. } => {
            assert_eq!(bytes_accepted, "attach-round-trip\n".len());
        }
        other => panic!("expected accepted, got {other:?}"),
    }
    wait_for_seq(&protocol, "client-a", &attachment, |seq| seq > 0).await;
    match protocol.resume("client-a", &attachment, 0, None).await {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => {
            assert!(chunk_text(&chunk).contains("attach-round-trip"));
        }
        other => panic!("expected resumed, got {other:?}"),
    }

    // Resize through the attachment is accepted.
    match protocol.resize("client-a", &attachment, 100, 30).await {
        CoreResponse::InteractiveProcessResized { cols, rows, .. } => {
            assert_eq!((cols, rows), (100, 30));
        }
        other => panic!("expected resized, got {other:?}"),
    }

    // Explicit terminate kills; the attachment survives for post-mortem reads.
    match protocol
        .terminate("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessTerminated {
            handle: terminated, ..
        } => {
            assert_eq!(terminated, handle);
        }
        other => panic!("expected terminated, got {other:?}"),
    }
    match protocol.resume("client-a", &attachment, 0, None).await {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => {
            assert!(chunk_text(&chunk).contains("attach-round-trip"));
        }
        CoreResponse::InteractiveProcessResyncRequired { .. } => {}
        other => panic!("expected post-mortem read, got {other:?}"),
    }

    // Remove frees the handle; re-attach reports the typed gone handle.
    match protocol
        .remove("client-a", &local("client-a"), &attachment)
        .await
    {
        CoreResponse::InteractiveProcessRemoved {
            handle: removed, ..
        } => {
            assert_eq!(removed, handle);
        }
        other => panic!("expected removed, got {other:?}"),
    }
    assert_eq!(
        expect_error_code(protocol.attach("client-a", &handle, None, None).await),
        "interactive_handle_gone"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spoofed_attachment_ids_match_unknown_ids() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let handle = match protocol
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };

    // Client B replaying A's attachment id learns nothing: same code as a
    // random id, and the process is untouched (still running, no input).
    let spoofed = protocol
        .input("client-b", &attachment, &B64.encode(b"spoofed\n"))
        .await;
    let random = protocol
        .input(
            "client-b",
            "00000000-0000-0000-0000-000000000000",
            &B64.encode(b"spoofed\n"),
        )
        .await;
    assert_eq!(expect_error_code(spoofed), "interactive_attachment_gone");
    assert_eq!(expect_error_code(random), "interactive_attachment_gone");
    assert_eq!(
        expect_error_code(protocol.detach("client-b", &attachment).await),
        "interactive_attachment_gone"
    );
    assert_eq!(
        expect_error_code(protocol.resize("client-b", &attachment, 80, 24).await),
        "interactive_attachment_gone"
    );
    assert_eq!(
        expect_error_code(
            protocol
                .terminate("client-b", &local("client-b"), &attachment)
                .await
        ),
        "interactive_attachment_gone"
    );
    // Forged handles are typed gone, never routed anywhere.
    assert_eq!(
        expect_error_code(
            protocol
                .attach(
                    "client-b",
                    "ffffffff-ffff-ffff-ffff-ffffffffffff",
                    None,
                    None
                )
                .await
        ),
        "interactive_handle_gone"
    );
    assert_eq!(
        expect_error_code(protocol.attach("client-b", "../escape", None, None).await),
        "interactive_handle_gone"
    );

    // Owner still drives its own process; cleanup.
    match protocol
        .input("client-a", &attachment, &B64.encode(b"owner\n"))
        .await
    {
        CoreResponse::InteractiveProcessInputAccepted { .. } => {}
        other => panic!("owner input must succeed, got {other:?}"),
    }
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
async fn remote_terminate_requires_the_semantic_capability() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let handle = match protocol
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    // Even the owning connection cannot terminate over a remote transport
    // without the plugged semantic capability; the process survives.
    assert_eq!(
        expect_error_code(
            protocol
                .terminate(
                    "client-a",
                    &InteractiveAuthority::remote("client-a"),
                    &attachment
                )
                .await
        ),
        "interactive_not_authorized"
    );
    // The seam grants it explicitly with no wire change.
    match protocol
        .terminate(
            "client-a",
            &InteractiveAuthority::remote("client-a").with_terminate_capability(),
            &attachment,
        )
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
async fn two_clients_share_a_process_with_independent_cursors() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let mut create = create_cat(&workspace_id);
    create.argv = vec![
        "sh".to_string(),
        "-c".to_string(),
        "echo shared-marker; sleep 30".to_string(),
    ];
    let handle = match protocol.create("client-a", &ctx, &create).await {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attach_a = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    let attach_b = match protocol.attach("client-b", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    assert_ne!(attach_a, attach_b);

    // Client A drains the stream; client B still resumes everything from 0.
    wait_for_seq(&protocol, "client-a", &attach_a, |seq| seq > 0).await;
    match protocol.resume("client-a", &attach_a, 0, None).await {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => {
            assert!(chunk_text(&chunk).contains("shared-marker"));
        }
        other => panic!("expected resumed, got {other:?}"),
    }
    match protocol.resume("client-b", &attach_b, 0, None).await {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => {
            assert!(chunk_text(&chunk).contains("shared-marker"));
        }
        other => panic!("expected resumed, got {other:?}"),
    }

    // A detaching does not disturb B's attachment or the process.
    match protocol.detach("client-a", &attach_a).await {
        CoreResponse::InteractiveProcessDetached { .. } => {}
        other => panic!("expected detached, got {other:?}"),
    }
    match protocol.resume("client-b", &attach_b, 0, None).await {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => {
            assert!(chunk_text(&chunk).contains("shared-marker"));
        }
        other => panic!("expected resumed, got {other:?}"),
    }

    match protocol
        .terminate("client-b", &local("client-b"), &attach_b)
        .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    match protocol
        .remove("client-b", &local("client-b"), &attach_b)
        .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disconnect_drops_attachments_without_killing() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let handle = match protocol
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    match protocol
        .input("client-a", &attachment, &B64.encode(b"before-disconnect\n"))
        .await
    {
        CoreResponse::InteractiveProcessInputAccepted { .. } => {}
        other => panic!("expected accepted, got {other:?}"),
    }
    wait_for_seq(&protocol, "client-a", &attachment, |seq| seq > 0).await;

    // Connection close releases the attachment only.
    assert_eq!(protocol.handle_disconnect("client-a"), 1);
    assert!(protocol.attachments().is_empty());
    assert_eq!(
        expect_error_code(protocol.resume("client-a", &attachment, 0, None).await),
        "interactive_attachment_gone"
    );

    // The process survived: re-attach reads the earlier output.
    match protocol.attach("client-a", &handle, Some(0), None).await {
        CoreResponse::InteractiveProcessAttached { chunk, resync, .. } => {
            assert!(resync.is_none());
            assert!(chunk_text(&chunk).contains("before-disconnect"));
        }
        other => panic!("expected reattach, got {other:?}"),
    }
    let reattached = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    match protocol
        .terminate("client-a", &local("client-a"), &reattached)
        .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    match protocol
        .remove("client-a", &local("client-a"), &reattached)
        .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn saturation_reports_typed_resync_with_cursors() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    // 4 KiB ring with ~30 KiB of output guarantees truncation.
    let mut create = InteractiveProcessCreateRequest {
        workspace_id,
        argv: vec![
            "sh".to_string(),
            "-c".to_string(),
            "yes z | head -c 30000; echo saturated".to_string(),
        ],
        cwd: None,
        env_overrides: Vec::new(),
        cols: Some(80),
        rows: Some(24),
        scrollback_bytes: Some(4096),
    };
    let handle = match protocol.create("client-a", &ctx, &create).await {
        CoreResponse::InteractiveProcessCreated { handle, metadata } => {
            assert!(metadata.truncated || metadata.total_bytes < 30000);
            handle
        }
        other => panic!("expected created, got {other:?}"),
    };
    create.argv = vec!["true".to_string()];
    let snapshot = wait_for_exit(protocol.service(), &handle).await;
    assert!(snapshot.total_bytes >= 30000);
    assert!(snapshot.truncated);

    // Attach from the start of history: oldest retained bytes plus a typed
    // HistoryExpired resync carrying both cursors.
    let attachment = match protocol.attach("client-a", &handle, Some(0), None).await {
        CoreResponse::InteractiveProcessAttached {
            attachment_id,
            chunk,
            resync,
            ..
        } => {
            assert!(chunk.gap);
            let resync = resync.expect("gap must carry resync");
            assert_eq!(resync.reason, InteractiveResyncReason::HistoryExpired);
            assert!(resync.base_seq > 0);
            assert_eq!(resync.next_seq, snapshot.total_bytes);
            attachment_id
        }
        other => panic!("expected attached with resync, got {other:?}"),
    };

    // Resume from the start of history is likewise a typed resync.
    match protocol.resume("client-a", &attachment, 0, None).await {
        CoreResponse::InteractiveProcessResyncRequired { resync, .. } => {
            assert_eq!(resync.reason, InteractiveResyncReason::HistoryExpired);
            assert!(resync.snapshot.is_some());
        }
        other => panic!("expected resync, got {other:?}"),
    }

    // Resume from the live cursor returns the tail incrementally.
    let next = snapshot.total_bytes;
    match protocol.resume("client-a", &attachment, next, None).await {
        CoreResponse::InteractiveProcessResumed { chunk, .. } => {
            assert_eq!(chunk.next_seq, next);
            assert!(!chunk.gap);
        }
        other => panic!("expected resumed, got {other:?}"),
    }

    // A cursor ahead of production is a typed CursorAhead, not silent lag.
    match protocol
        .resume("client-a", &attachment, next + 5000, None)
        .await
    {
        CoreResponse::InteractiveProcessResyncRequired { resync, .. } => {
            assert_eq!(resync.reason, InteractiveResyncReason::CursorAhead);
            assert_eq!(resync.next_seq, next);
        }
        other => panic!("expected cursor-ahead resync, got {other:?}"),
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
async fn chunk_and_input_bounds_are_rejected_before_side_effects() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let handle = match protocol
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };

    // Oversized reads are rejected without touching the ring.
    assert_eq!(
        expect_error_code(
            protocol
                .resume("client-a", &attachment, 0, Some(256 * 1024 + 1))
                .await
        ),
        "interactive_read_too_large"
    );
    assert_eq!(
        expect_error_code(protocol.attach("client-a", &handle, None, Some(0)).await),
        "interactive_read_too_large"
    );
    // Oversized input is rejected before any PTY write.
    let big = B64.encode(vec![b'x'; 32 * 1024 + 1]);
    assert_eq!(
        expect_error_code(protocol.input("client-a", &attachment, &big).await),
        "interactive_input_too_large"
    );
    assert_eq!(
        expect_error_code(
            protocol
                .input("client-a", &attachment, "!!!not-base64!!!")
                .await
        ),
        "interactive_invalid_request"
    );
    // Invalid sizes never reach the PTY.
    assert_eq!(
        expect_error_code(protocol.resize("client-a", &attachment, 0, 24).await),
        "interactive_invalid_request"
    );

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
async fn writer_failure_after_exit_is_typed() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let mut create = create_cat(&workspace_id);
    create.argv = vec!["true".to_string()];
    let handle = match protocol.create("client-a", &ctx, &create).await {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    wait_for_exit(protocol.service(), &handle).await;

    // Input and resize after exit surface the terminal state.
    assert_eq!(
        expect_error_code(
            protocol
                .input("client-a", &attachment, &B64.encode(b"late\n"))
                .await
        ),
        "interactive_not_running"
    );
    assert_eq!(
        expect_error_code(protocol.resize("client-a", &attachment, 80, 24).await),
        "interactive_not_running"
    );
    // Terminate on an exited process converges instead of failing.
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
async fn removed_handles_resume_as_typed_gone() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let handle = match protocol
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attach_a = match protocol.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    let attach_b = match protocol.attach("client-b", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };

    // B removes the handle (dropping only B's attachment). A's surviving
    // attachment resumes as typed HandleGone with no snapshot.
    match protocol
        .remove("client-b", &local("client-b"), &attach_b)
        .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
    match protocol.resume("client-a", &attach_a, 0, None).await {
        CoreResponse::InteractiveProcessResyncRequired { resync, .. } => {
            assert_eq!(resync.reason, InteractiveResyncReason::HandleGone);
            assert_eq!(resync.handle, handle);
            assert!(resync.snapshot.is_none());
        }
        other => panic!("expected gone resync, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_invalidates_ephemeral_handles() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let before = protocol();

    let handle = match before
        .create("client-a", &ctx, &create_cat(&workspace_id))
        .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };

    // A restarted daemon (fresh ephemeral state) answers the old handle
    // with the typed gone response and lists nothing, never another
    // process.
    let after = protocol();
    assert_eq!(
        expect_error_code(after.attach("client-a", &handle, None, None).await),
        "interactive_handle_gone"
    );
    match after.list("client-a", None, None).await {
        CoreResponse::InteractiveProcessList {
            processes,
            truncated,
        } => {
            assert!(processes.is_empty());
            assert!(!truncated);
        }
        other => panic!("expected empty list, got {other:?}"),
    }

    match before.attach("client-a", &handle, None, None).await {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => {
            match before
                .terminate("client-a", &local("client-a"), &attachment_id)
                .await
            {
                CoreResponse::InteractiveProcessTerminated { .. } => {}
                other => panic!("expected terminated, got {other:?}"),
            }
            match before
                .remove("client-a", &local("client-a"), &attachment_id)
                .await
            {
                CoreResponse::InteractiveProcessRemoved { .. } => {}
                other => panic!("expected removed, got {other:?}"),
            }
        }
        other => panic!("expected attached, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_rejects_new_spawns_with_typed_error() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    protocol.service().shutdown().await;
    assert_eq!(
        expect_error_code(
            protocol
                .create("client-a", &ctx, &create_cat(&workspace_id))
                .await
        ),
        "interactive_shutting_down"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_capability_degrades_without_blocking() {
    let protocol = InteractiveProcessProtocol::new(test_admission(2));

    // Disjoint ranges answer `supported: false`, never an error.
    let future = InteractiveProcessCapabilities {
        min_version: INTERACTIVE_PROCESS_PROTOCOL_VERSION + 1,
        max_version: INTERACTIVE_PROCESS_PROTOCOL_VERSION + 2,
        ..Default::default()
    };
    match protocol.capabilities(&future) {
        CoreResponse::InteractiveProcessCapabilitiesResponse { supported, .. } => {
            assert!(!supported);
        }
        other => panic!("expected capabilities, got {other:?}"),
    }
    match protocol.capabilities(&InteractiveProcessCapabilities::current()) {
        CoreResponse::InteractiveProcessCapabilitiesResponse {
            supported,
            protocol_version,
            ..
        } => {
            assert!(supported);
            assert_eq!(protocol_version, INTERACTIVE_PROCESS_PROTOCOL_VERSION);
        }
        other => panic!("expected capabilities, got {other:?}"),
    }

    // A legacy request envelope without any new field still decodes.
    let legacy: CoreRequest =
        serde_json::from_str(r#"{"type":"session_create","directory":"/tmp/legacy","title":null}"#)
            .expect("legacy decodes");
    assert!(matches!(legacy, CoreRequest::SessionCreate { .. }));

    // New operations round-trip through the envelope.
    let envelope = RequestEnvelope {
        protocol_version: 2,
        request_id: "interactive-1".to_string(),
        payload: CoreRequest::InteractiveProcessAttach {
            handle: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            from_seq: Some(0),
            max_bytes: None,
        },
    };
    let json = serde_json::to_string(&envelope).expect("serialize");
    assert!(json.contains("interactive_process_attach"));
    let decoded: RequestEnvelope<CoreRequest> = serde_json::from_str(&json).expect("decode");
    assert!(matches!(
        decoded.payload,
        CoreRequest::InteractiveProcessAttach { .. }
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_create_never_consumes_admission_or_handles() {
    let dir = tempfile::tempdir().expect("workspace");
    let ctx = test_context(dir.path()).await;
    let workspace_id = ctx.workspace_id.as_str().to_string();
    let protocol = protocol();

    let mut bad = create_cat(&workspace_id);
    bad.argv.clear();
    assert_eq!(
        expect_error_code(protocol.create("client-a", &ctx, &bad).await),
        "interactive_invalid_request"
    );
    assert_eq!(protocol.service().session_count(), 0);
}

fn daemon_envelope(payload: CoreRequest) -> RequestEnvelope<CoreRequest> {
    RequestEnvelope {
        protocol_version: 2,
        request_id: format!("interactive-{}", uuid::Uuid::new_v4()),
        payload,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_session_create_fixture_still_decodes() {
    let request: CoreRequest =
        serde_json::from_str(r#"{"type":"session_create","directory":"/tmp/legacy","title":null}"#)
            .expect("legacy decodes");
    assert!(matches!(request, CoreRequest::SessionCreate { .. }));
}

/// Stack probe: the interactive dispatch path (capabilities + list on an
/// empty daemon) must fit a default 2 MiB multi-thread worker stack. The
/// dispatch match is documented near its limit, so this pins the M002
/// addition against pushing worker threads over the edge.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interactive_daemon_worker_stack_probe() {
    let daemon = CoreDaemon::new(None, None, None);
    match daemon
        .handle_request_for_client(
            daemon_envelope(CoreRequest::InteractiveProcessCapabilities),
            "probe-client",
        )
        .await
        .expect("capabilities")
    {
        CoreResponse::InteractiveProcessCapabilitiesResponse { supported, .. } => {
            assert!(supported);
        }
        other => panic!("expected capabilities, got {other:?}"),
    }
    match daemon
        .handle_request_for_client(
            daemon_envelope(CoreRequest::InteractiveProcessList {
                workspace_id: None,
                limit: None,
            }),
            "probe-client",
        )
        .await
        .expect("list")
    {
        CoreResponse::InteractiveProcessList { processes, .. } => {
            assert!(processes.is_empty());
        }
        other => panic!("expected list, got {other:?}"),
    }
}
/// End-to-end daemon dispatch needs worker stacks above the default: the
/// dispatch future is documented near the stack limit (see the 16 MiB
/// precedent in `tests/projection_transport_real.rs`), so daemon tests
/// below run on an explicit big-stack runtime instead of the default
/// `#[tokio::test]` threads.
fn interactive_test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("interactive test runtime")
}

async fn daemon_with_workspace() -> (CoreDaemon, String) {
    let dir = tempfile::tempdir().expect("workspace");
    // Leak the directory guard for the life of the test process: the
    // daemon resolves workspace roots on demand, and per-test cleanup
    // races PTY shutdown. The OS reclaims the temp dir on exit.
    let dir = Box::leak(Box::new(dir));
    let daemon = CoreDaemon::new(None, None, None);
    let workspace = daemon
        .workspaces
        .get_or_register(dir.path())
        .await
        .expect("register workspace");
    (daemon, workspace.id.as_str().to_string())
}

async fn daemon_request(daemon: &CoreDaemon, client: &str, payload: CoreRequest) -> CoreResponse {
    daemon
        .handle_request_for_client(daemon_envelope(payload), client)
        .await
        .expect("daemon request")
}

async fn daemon_spawn_cat(daemon: &CoreDaemon, workspace_id: &str) -> (String, String) {
    let handle = match daemon_request(
        daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessCreate {
            request: create_cat(workspace_id),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessCreated { handle, .. } => handle,
        other => panic!("expected created, got {other:?}"),
    };
    let attachment = match daemon_request(
        daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessAttach {
            handle: handle.clone(),
            from_seq: Some(0),
            max_bytes: None,
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessAttached { attachment_id, .. } => attachment_id,
        other => panic!("expected attached, got {other:?}"),
    };
    (handle, attachment)
}

async fn daemon_capabilities_create_and_unknown_workspace_body() {
    let (daemon, workspace_id) = daemon_with_workspace().await;
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessCapabilities,
    )
    .await
    {
        CoreResponse::InteractiveProcessCapabilitiesResponse { supported, .. } => {
            assert!(supported);
        }
        other => panic!("expected capabilities, got {other:?}"),
    }
    let (handle, attachment) = daemon_spawn_cat(&daemon, &workspace_id).await;
    assert!(!handle.is_empty() && !attachment.is_empty());
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessCreate {
            request: create_cat("00000000-0000-0000-0000-000000000000"),
        },
    )
    .await
    {
        CoreResponse::Error { code, .. } => assert_eq!(code, "interactive_invalid_request"),
        other => panic!("expected error, got {other:?}"),
    }
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessTerminate {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessRemove {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
}

#[test]
fn daemon_capabilities_create_and_unknown_workspace() {
    let runtime = interactive_test_runtime();
    runtime.block_on(async {
        tokio::spawn(daemon_capabilities_create_and_unknown_workspace_body())
            .await
            .expect("interactive daemon flow stays on a big-stack worker");
    });
}

async fn daemon_attach_input_resume_and_cross_client_denial_body() {
    let (daemon, workspace_id) = daemon_with_workspace().await;
    let (handle, attachment) = daemon_spawn_cat(&daemon, &workspace_id).await;
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessInput {
            attachment_id: attachment.clone(),
            data_b64: B64.encode(b"daemon-round-trip\n"),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessInputAccepted { .. } => {}
        other => panic!("expected accepted, got {other:?}"),
    }
    let started = Instant::now();
    loop {
        let text = match daemon_request(
            &daemon,
            "daemon-client",
            CoreRequest::InteractiveProcessResume {
                attachment_id: attachment.clone(),
                from_seq: 0,
                max_bytes: None,
            },
        )
        .await
        {
            CoreResponse::InteractiveProcessResumed { chunk, .. } => chunk_text(&chunk),
            CoreResponse::InteractiveProcessResyncRequired { .. } => String::new(),
            other => panic!("expected resumed, got {other:?}"),
        };
        if text.contains("daemon-round-trip") {
            break;
        }
        if started.elapsed() > POLL_TIMEOUT {
            panic!("timed out waiting for daemon-routed output");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // Another connection cannot steer the first client's attachment: the
    // payload never names the client, so ownership is transport-derived.
    match daemon_request(
        &daemon,
        "other-client",
        CoreRequest::InteractiveProcessTerminate {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::Error { code, .. } => assert_eq!(code, "interactive_attachment_gone"),
        other => panic!("expected gone, got {other:?}"),
    }
    assert_eq!(handle.len(), 36);
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessTerminate {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessRemove {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
}

#[test]
fn daemon_attach_input_resume_and_cross_client_denial() {
    let runtime = interactive_test_runtime();
    runtime.block_on(async {
        tokio::spawn(daemon_attach_input_resume_and_cross_client_denial_body())
            .await
            .expect("interactive daemon flow stays on a big-stack worker");
    });
}

async fn daemon_terminate_publishes_exit_event_body() {
    let (daemon, workspace_id) = daemon_with_workspace().await;
    let (handle, attachment) = daemon_spawn_cat(&daemon, &workspace_id).await;
    let mut events = daemon.subscribe();
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessTerminate {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessTerminated { .. } => {}
        other => panic!("expected terminated, got {other:?}"),
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let envelope = events.recv().await.expect("event");
            match &envelope.payload {
                codegg::protocol::core::CoreEvent::InteractiveProcessExited {
                    handle: exited,
                    ..
                } => {
                    assert_eq!(exited, &handle);
                    break;
                }
                _ => continue,
            }
        }
    })
    .await
    .expect("exit event must be published");
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessRemove {
            attachment_id: attachment.clone(),
        },
    )
    .await
    {
        CoreResponse::InteractiveProcessRemoved { .. } => {}
        other => panic!("expected removed, got {other:?}"),
    }
}

#[test]
fn daemon_terminate_publishes_exit_event() {
    let runtime = interactive_test_runtime();
    runtime.block_on(async {
        tokio::spawn(daemon_terminate_publishes_exit_event_body())
            .await
            .expect("interactive daemon flow stays on a big-stack worker");
    });
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_workspace_registration_and_capabilities() {
    let (daemon, _workspace_id) = daemon_with_workspace().await;
    match daemon_request(
        &daemon,
        "daemon-client",
        CoreRequest::InteractiveProcessCapabilities,
    )
    .await
    {
        CoreResponse::InteractiveProcessCapabilitiesResponse { supported, .. } => {
            assert!(supported);
        }
        other => panic!("expected capabilities, got {other:?}"),
    }
}
