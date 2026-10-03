//! Session projection driver tests against a scripted fake daemon.
//!
//! Each test runs a fake daemon over a Unix socket speaking the real
//! `CoreFrame` wire protocol. The daemon scripts cover M004 §16: normal
//! capability/subscribe/live flow, forced transport lag, duplicates,
//! sequence gaps, subscription mismatch, snapshot and bare resync,
//! replay continuation, ack cadence, unsubscribe, reconnect cursor
//! retention, closed transport, unsupported/denied daemons, and artifact
//! handle bounds.

#![cfg(unix)]

use std::time::Duration;

use codegg_client::{
    DriverConfig, DriverError, DriverSnapshotView, DriverState, FrontendDescriptor,
    LocalSocketClient, SessionProjectionDriver,
};
use codegg_protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, PROTOCOL_VERSION,
};
use codegg_protocol::frames::{
    ClientCapabilities, ClientKind, CoreFrame, ServerCapabilities, ServerHello,
};
use codegg_protocol::projection::caps::PROJECTION_PROTOCOL_VERSION;
use codegg_protocol::projection::consumer::HeadlessProjectionConsumer;
use codegg_protocol::projection::event::{ProjectionEnvelope, ProjectionEvent};
use codegg_protocol::projection::fixtures::{
    active_turn_event_script, completed_snapshot, idle_snapshot, FIXTURE_PROJECT_ID,
    FIXTURE_SESSION_ID, FIXTURE_WORKSPACE_ID,
};
use codegg_protocol::projection::reducer::ReducerEventInput;
use codegg_protocol::projection::replay::{
    ArtifactHandleKind, ProjectionArtifactHandleDto, ProjectionCursor, ProjectionReplayBatch,
    ProjectionResyncReason, ProjectionSnapshotBundle, ProjectionStreamDescriptor,
    ProjectionStreamId, ProjectionStreamKind, ProjectionSubscriptionId,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn gui_capabilities() -> ClientCapabilities {
    ClientCapabilities {
        visual_notifications: false,
        desktop_notifications: true,
        audio: false,
        tts: false,
        multi_session_view: true,
        plugin_ui_dialog: false,
        plugin_ui_toast: false,
        plugin_ui_panel: false,
        plugin_ui_status_item: false,
        plugin_ui_table: false,
        plugin_ui_markdown: false,
        plugin_ui_code: false,
        plugin_ui_progress: false,
        workspace_registration: true,
        project_catalog: true,
        session_projection: true,
    }
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        event_replay: true,
        session_management: true,
        permission_routing: true,
        workspace_registration: true,
        workspace_snapshots: true,
        durable_jobs: true,
        durable_schedules: true,
        identity_aware_context: true,
        project_catalog: true,
        session_projection: true,
    }
}

fn descriptor() -> ProjectionStreamDescriptor {
    ProjectionStreamDescriptor {
        stream_id: ProjectionStreamId::new("test-stream-1").unwrap(),
        kind: ProjectionStreamKind::Session,
        project_id: FIXTURE_PROJECT_ID.into(),
        workspace_id: Some(FIXTURE_WORKSPACE_ID.into()),
        session_id: Some(FIXTURE_SESSION_ID.into()),
        projection_version: PROJECTION_PROTOCOL_VERSION,
        retention_floor_seq: 0,
        high_water_seq: 0,
        latest_checkpoint_seq: None,
    }
}

fn caps_response() -> CoreResponse {
    CoreResponse::ProjectionCapabilitiesResponse {
        supported: true,
        projection_version: PROJECTION_PROTOCOL_VERSION,
        max_events_per_batch: 512,
        max_event_bytes: 64 * 1024,
        max_subscriptions_per_client: 32,
        max_subscriptions_per_daemon: 256,
        retention_session_max_events: 20_000,
        retention_project_max_events: 50_000,
    }
}

fn subscribed_response(sub_id: &str) -> CoreResponse {
    let descriptor = descriptor();
    CoreResponse::ProjectionSubscribed {
        subscription_id: ProjectionSubscriptionId::new(sub_id),
        descriptor: descriptor.clone(),
        snapshot: ProjectionSnapshotBundle::One {
            snapshot: Box::new(idle_snapshot()),
        },
        cursor: ProjectionCursor {
            stream_id: descriptor.stream_id.clone(),
            event_seq: 0,
            projection_version: PROJECTION_PROTOCOL_VERSION,
        },
        retention_floor_seq: 0,
    }
}

fn script_envelope(input: &ReducerEventInput) -> ProjectionEnvelope {
    ProjectionEnvelope {
        protocol_version: input.protocol_version,
        event_seq: input.event_seq,
        timestamp_ms: input.timestamp_ms,
        session_id: input.session_id.clone(),
        turn_id: input.turn_id.clone(),
        scope: codegg_protocol::projection::event::ProjectionStreamScope::Session,
        payload: input.payload.clone(),
    }
}

fn diagnostic_envelope(seq: u64) -> ProjectionEnvelope {
    ProjectionEnvelope::session_event(
        seq,
        seq as i64,
        FIXTURE_SESSION_ID,
        None,
        ProjectionEvent::Diagnostic {
            code: "test-flood".into(),
            message: "flood".into(),
        },
    )
}

fn stream_frame(sub_id: &str, transport_seq: u64, envelope: ProjectionEnvelope) -> CoreFrame {
    CoreFrame::Event(EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: transport_seq,
        timestamp_ms: 0,
        session_id: None,
        turn_id: None,
        payload: CoreEvent::ProjectionStreamEvent {
            subscription_id: ProjectionSubscriptionId::new(sub_id),
            stream_id: descriptor().stream_id.clone(),
            envelope,
        },
    })
}

fn replay_response(
    sub_id: &str,
    events: Vec<ProjectionEnvelope>,
    start_seq: u64,
    end_seq: u64,
) -> CoreResponse {
    CoreResponse::ProjectionReplay {
        subscription_id: Some(ProjectionSubscriptionId::new(sub_id)),
        batch: ProjectionReplayBatch {
            descriptor: descriptor(),
            events,
            snapshot: None,
            replay_start_seq: start_seq,
            replay_end_seq: end_seq,
            current_high_water: end_seq,
            truncation_flag: false,
            next_cursor: None,
        },
    }
}

type WriteHalf = tokio::net::unix::OwnedWriteHalf;

async fn send_frame(write: &mut WriteHalf, frame: &CoreFrame) {
    write
        .write_all(format!("{}\n", serde_json::to_string(frame).unwrap()).as_bytes())
        .await
        .expect("send frame");
    write.flush().await.expect("flush frame");
}

async fn read_line(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> String {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
        .await
        .expect("read timeout")
        .expect("read line");
    line
}

/// Read the next client `CoreRequest`, skipping transport-level frames
/// such as the automatic post-hello `Subscribe`.
async fn next_request(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> (String, CoreRequest) {
    loop {
        let line = read_line(reader).await;
        match serde_json::from_str::<CoreFrame>(line.trim()).expect("decode client frame") {
            CoreFrame::Request(request) => return (request.request_id, request.payload),
            _ => continue,
        }
    }
}

async fn send_response(write: &mut WriteHalf, request_id: String, response: CoreResponse) {
    send_frame(
        write,
        &CoreFrame::Response {
            request_id,
            response: Box::new(response),
        },
    )
    .await;
}

/// Perform the hello exchange and consume the automatic post-hello
/// transport subscribe. Returns when the daemon side is ready for the
/// scripted dialogue.
async fn hello_exchange(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    write: &mut WriteHalf,
) {
    let line = read_line(reader).await;
    assert!(
        matches!(
            serde_json::from_str::<CoreFrame>(line.trim()).expect("decode hello"),
            CoreFrame::ClientHello(_)
        ),
        "expected ClientHello"
    );
    send_frame(
        write,
        &CoreFrame::ServerHello(ServerHello {
            daemon_id: "fake-daemon".into(),
            protocol_version: PROTOCOL_VERSION,
            server_capabilities: server_capabilities(),
            client_id: "driver-client-id".into(),
        }),
    )
    .await;
    // Consume the automatic `CoreFrame::Subscribe` the client reader
    // sends after the hello.
    loop {
        let line = read_line(reader).await;
        if matches!(
            serde_json::from_str::<CoreFrame>(line.trim()).expect("decode post-hello"),
            CoreFrame::Subscribe { .. }
        ) {
            break;
        }
    }
}

async fn bind() -> (String, UnixListener) {
    // Short socket name: macOS temp dirs are long and SUN_LEN is 104.
    let short = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let socket = std::env::temp_dir().join(format!("cgd-{short}.sock"));
    let listener = UnixListener::bind(&socket).expect("bind fake daemon");
    (format!("unix://{}", socket.display()), listener)
}

async fn connect_client(endpoint: &str) -> LocalSocketClient {
    LocalSocketClient::connect(
        endpoint.to_string(),
        FrontendDescriptor::new("codegg-driver-test", ClientKind::Gui, gui_capabilities()),
    )
    .await
    .expect("connect driver client")
}

async fn wait_for(
    driver: &SessionProjectionDriver,
    want: &str,
    timeout: Duration,
    mut pred: impl FnMut(&DriverSnapshotView) -> bool,
) -> DriverSnapshotView {
    let start = std::time::Instant::now();
    loop {
        let view = driver.current();
        if pred(&view) {
            return view;
        }
        if start.elapsed() > timeout {
            panic!(
                "timed out waiting for {want}; last state={:?} cursor={:?}",
                view.state, view.cursor_seq
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn test_config(ack_every: usize) -> DriverConfig {
    DriverConfig {
        ack_every,
        max_resume_rounds: 8,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_applies_live_events_and_acks_cadence() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        // Two live public events: TurnStarted(1) + MessageAppended(2).
        let script = active_turn_event_script();
        for (transport_seq, input) in script.iter().take(2).enumerate() {
            send_frame(
                &mut write,
                &stream_frame("sub-1", transport_seq as u64 + 1, script_envelope(input)),
            )
            .await;
        }

        // ack_every=2: exactly one ack after the second applied event.
        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionAck { ack } = payload else {
            panic!("expected ProjectionAck, got {payload:?}");
        };
        assert_eq!(ack.subscription_id.as_str(), "sub-1");
        assert_eq!(ack.cursor.event_seq, 2);
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionAckAccepted {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
                last_acked_seq: 2,
                lag_count: 0,
            },
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(2),
    )
    .await
    .expect("attach");
    let view = wait_for(&driver, "cursor 2", Duration::from_secs(10), |view| {
        view.state == DriverState::Attached && view.cursor_seq == Some(2)
    })
    .await;
    assert_eq!(view.subscription_id.as_deref(), Some("sub-1"));
    let snapshot = view.snapshot.expect("snapshot installed");
    assert!(snapshot.active_turn.is_some());
    assert_eq!(
        snapshot
            .active_turn
            .as_ref()
            .expect("active turn")
            .messages
            .len(),
        1
    );
    let stopped = driver.stop().await.expect("stop");
    assert_eq!(stopped.cursor_seq(), Some(2));
    assert!(stopped.subscription_released());
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_lag_forces_resync_and_converges() {
    // The flood must exceed the combined broadcast (256) + forwarder
    // mpsc (256) slack or no lag occurs and the driver converges live
    // without ever resyncing. 1200 events guarantee forwarder lag while
    // every processed event costs a delayed ack roundtrip.
    const FLOOD_END: u64 = 1201;
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        // Probe event proves the loop is draining before the flood, so
        // the flood produces genuine broadcast lag (not pre-spawn loss).
        send_frame(
            &mut write,
            &stream_frame("sub-1", 1, diagnostic_envelope(1)),
        )
        .await;
        // ack_every=1: the probe ack proves liveness.
        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionAck { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionAckAccepted {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
                last_acked_seq: 1,
                lag_count: 0,
            },
        )
        .await;

        // Flood far beyond the broadcast + mpsc bounds while every
        // processed event costs a (delayed) ack roundtrip.
        for seq in 2..=FLOOD_END {
            send_frame(
                &mut write,
                &stream_frame("sub-1", seq, diagnostic_envelope(seq)),
            )
            .await;
        }

        let mut resume_seen = false;
        loop {
            let (id, payload) = next_request(&mut reader).await;
            match payload {
                CoreRequest::ProjectionAck { .. } => {
                    // Slow the loop down so lag is deterministic.
                    tokio::time::sleep(Duration::from_millis(2)).await;
                    send_response(
                        &mut write,
                        id,
                        CoreResponse::ProjectionAckAccepted {
                            subscription_id: ProjectionSubscriptionId::new("sub-1"),
                            last_acked_seq: 0,
                            lag_count: 0,
                        },
                    )
                    .await;
                }
                CoreRequest::ProjectionResume { cursor, .. } => {
                    resume_seen = true;
                    let from = cursor.event_seq.saturating_add(1);
                    let events: Vec<_> = (from..=FLOOD_END).map(diagnostic_envelope).collect();
                    send_response(
                        &mut write,
                        id,
                        replay_response("sub-1", events, from, FLOOD_END),
                    )
                    .await;
                }
                CoreRequest::ProjectionUnsubscribe { .. } => {
                    send_response(
                        &mut write,
                        id,
                        CoreResponse::ProjectionUnsubscribed {
                            subscription_id: ProjectionSubscriptionId::new("sub-1"),
                        },
                    )
                    .await;
                    break;
                }
                other => panic!("unexpected request during flood: {other:?}"),
            }
        }
        assert!(resume_seen, "driver never resumed after lag");
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1),
    )
    .await
    .expect("attach");
    // The authoritative replay converges the full flood: no post-gap
    // event is ever applied as contiguous live delivery.
    let view = wait_for(
        &driver,
        "flood convergence",
        Duration::from_secs(30),
        |view| view.state == DriverState::Attached && view.cursor_seq == Some(FLOOD_END),
    )
    .await;
    assert_eq!(view.subscription_id.as_deref(), Some("sub-1"));
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_events_are_idempotent() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        let script = active_turn_event_script();
        let first = stream_frame("sub-1", 1, script_envelope(&script[0]));
        send_frame(&mut write, &first).await;
        // Redeliver the identical envelope: harmless, no resync.
        send_frame(&mut write, &first).await;

        // The only further traffic must be the stop unsubscribe: any
        // resume here would mean the duplicate was treated as loss.
        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionUnsubscribe { subscription_id } = payload else {
            panic!("duplicate triggered traffic: {payload:?}");
        };
        assert_eq!(subscription_id.as_str(), "sub-1");
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    wait_for(&driver, "cursor 1", Duration::from_secs(10), |view| {
        view.state == DriverState::Attached && view.cursor_seq == Some(1)
    })
    .await;
    // Settle window: a spurious resume would fail the server assertion.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(driver.current().cursor_seq, Some(1));
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequence_gap_forces_resume_replay() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        // Skip seq 1 entirely: the gap must force resync, and the
        // reducer must never see the out-of-order payload.
        let script = active_turn_event_script();
        send_frame(
            &mut write,
            &stream_frame("sub-1", 2, script_envelope(&script[1])),
        )
        .await;

        // Resume must carry the retained cursor (still 0).
        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionResume { cursor, .. } = payload else {
            panic!("expected ProjectionResume, got {payload:?}");
        };
        assert_eq!(cursor.event_seq, 0);
        // Delay so the Resyncing presentation is observable.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let events = script.iter().take(2).map(script_envelope).collect();
        send_response(&mut write, id, replay_response("sub-1", events, 1, 2)).await;

        // Replay installed seq 1-2; the redelivered live seq 2 is now a
        // harmless duplicate. Only the stop unsubscribe follows.
        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    wait_for(
        &driver,
        "resyncing presentation",
        Duration::from_secs(10),
        |view| view.state == DriverState::Resyncing,
    )
    .await;
    let view = wait_for(&driver, "cursor 2", Duration::from_secs(10), |view| {
        view.state == DriverState::Attached && view.cursor_seq == Some(2)
    })
    .await;
    assert!(view.snapshot.expect("snapshot").active_turn.is_some());
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mismatched_subscription_events_are_rejected() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        let script = active_turn_event_script();
        // Foreign subscription id: must be ignored wholesale.
        send_frame(
            &mut write,
            &stream_frame("sub-stale", 9, script_envelope(&script[0])),
        )
        .await;
        // Correct subscription but foreign stream session: the consumer
        // detects the stream mismatch and resyncs rather than corrupting
        // state.
        let mut foreign = script_envelope(&script[0]);
        foreign.session_id = Some("other-session".into());
        send_frame(&mut write, &stream_frame("sub-1", 10, foreign)).await;

        // The stream mismatch triggers exactly one cursor resume.
        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionResume { cursor, .. } = payload else {
            panic!("expected ProjectionResume, got {payload:?}");
        };
        assert_eq!(cursor.event_seq, 0);
        send_response(&mut write, id, replay_response("sub-1", vec![], 0, 0)).await;

        // Now the genuine live event applies cleanly on the same
        // subscription, proving the driver stayed healthy.
        send_frame(
            &mut write,
            &stream_frame("sub-1", 11, script_envelope(&script[0])),
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    let view = wait_for(&driver, "cursor 1", Duration::from_secs(10), |view| {
        view.state == DriverState::Attached && view.cursor_seq == Some(1)
    })
    .await;
    assert_eq!(view.subscription_id.as_deref(), Some("sub-1"));
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snapshot_resync_atomically_replaces_presentation() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        // Jump the stream: the daemon answers the resume with an
        // authoritative snapshot at seq 10.
        send_frame(
            &mut write,
            &stream_frame("sub-1", 2, diagnostic_envelope(5)),
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionResume { .. }));
        let mut authoritative = completed_snapshot();
        authoritative.event_seq = 10;
        let descriptor = descriptor();
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionResyncRequired {
                subscription_id: Some(ProjectionSubscriptionId::new("sub-1")),
                reason: ProjectionResyncReason::HistoryGap,
                descriptor: Some(descriptor.clone()),
                requested_cursor: None,
                snapshot: Some(ProjectionSnapshotBundle::One {
                    snapshot: Box::new(authoritative),
                }),
            },
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    let view = wait_for(
        &driver,
        "snapshot cursor 10",
        Duration::from_secs(10),
        |view| view.state == DriverState::Attached && view.cursor_seq == Some(10),
    )
    .await;
    let snapshot = view.snapshot.expect("authoritative snapshot");
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.runs[0].status, "completed");
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bare_resync_falls_back_to_fresh_subscribe() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        send_frame(
            &mut write,
            &stream_frame("sub-1", 2, diagnostic_envelope(5)),
        )
        .await;

        // Resume is answered with a bare resync (no snapshot): the
        // driver must fall back to a fresh subscribe, never spin on the
        // dead cursor.
        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionResume { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionResyncRequired {
                subscription_id: None,
                reason: ProjectionResyncReason::HistoryExpired,
                descriptor: None,
                requested_cursor: None,
                snapshot: None,
            },
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-2")).await;

        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionUnsubscribe { subscription_id } = payload else {
            panic!("expected unsubscribe, got {payload:?}");
        };
        assert_eq!(subscription_id.as_str(), "sub-2");
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-2"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    let view = wait_for(
        &driver,
        "fresh subscription sub-2",
        Duration::from_secs(10),
        |view| {
            view.state == DriverState::Attached && view.subscription_id.as_deref() == Some("sub-2")
        },
    )
    .await;
    assert_eq!(view.cursor_seq, Some(0));
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ack_cadence_sends_periodic_acknowledgements() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        // Five contiguous diagnostic events with ack_every=2: exactly
        // two acks (after seq 2 and seq 4), cursors strictly increasing.
        for seq in 1..=5u64 {
            send_frame(
                &mut write,
                &stream_frame("sub-1", seq, diagnostic_envelope(seq)),
            )
            .await;
        }

        let mut acked = Vec::new();
        for _ in 0..2 {
            let (id, payload) = next_request(&mut reader).await;
            let CoreRequest::ProjectionAck { ack } = payload else {
                panic!("expected ProjectionAck, got {payload:?}");
            };
            acked.push(ack.cursor.event_seq);
            send_response(
                &mut write,
                id,
                CoreResponse::ProjectionAckAccepted {
                    subscription_id: ProjectionSubscriptionId::new("sub-1"),
                    last_acked_seq: ack.cursor.event_seq,
                    lag_count: 0,
                },
            )
            .await;
        }
        assert_eq!(acked, vec![2, 4]);

        // The fifth event is processed but never acked; the next frame
        // must be the stop unsubscribe.
        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(2),
    )
    .await
    .expect("attach");
    wait_for(&driver, "cursor 5", Duration::from_secs(10), |view| {
        view.state == DriverState::Attached && view.cursor_seq == Some(5)
    })
    .await;
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_unsubscribes_and_reports_disconnected() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        let script = active_turn_event_script();
        send_frame(
            &mut write,
            &stream_frame("sub-1", 1, script_envelope(&script[0])),
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionUnsubscribe { subscription_id } = payload else {
            panic!("expected unsubscribe, got {payload:?}");
        };
        assert_eq!(subscription_id.as_str(), "sub-1");
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-1"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    wait_for(&driver, "cursor 1", Duration::from_secs(10), |view| {
        view.cursor_seq == Some(1)
    })
    .await;
    let stopped = driver.stop().await.expect("stop");
    assert_eq!(stopped.session_id(), FIXTURE_SESSION_ID);
    assert_eq!(stopped.cursor_seq(), Some(1));
    assert!(stopped.subscription_released());
    assert!(stopped.last_snapshot().is_some());
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resume_retains_cursor_with_new_subscription() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        // First connection: attach and advance to seq 3.
        let (stream, _) = listener.accept().await.expect("accept 1");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-A")).await;

        let script = active_turn_event_script();
        for (transport_seq, input) in script.iter().enumerate() {
            send_frame(
                &mut write,
                &stream_frame("sub-A", transport_seq as u64 + 1, script_envelope(input)),
            )
            .await;
        }

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-A"),
            },
        )
        .await;
        drop(write);

        // Second connection: the resume must carry cursor 3 and receive
        // a fresh subscription id, never a reuse of sub-A.
        let (stream, _) = listener.accept().await.expect("accept 2");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionResume { cursor, .. } = payload else {
            panic!("expected ProjectionResume, got {payload:?}");
        };
        assert_eq!(cursor.event_seq, 3);
        // Already at the high water: empty replay, no continuation.
        send_response(&mut write, id, replay_response("sub-B", vec![], 3, 3)).await;

        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionUnsubscribe { subscription_id } = payload else {
            panic!("expected unsubscribe, got {payload:?}");
        };
        assert_eq!(subscription_id.as_str(), "sub-B");
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-B"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    wait_for(&driver, "cursor 3", Duration::from_secs(10), |view| {
        view.cursor_seq == Some(3)
    })
    .await;
    assert_eq!(driver.generation(), 1);
    let stopped = driver.stop().await.expect("stop");
    assert_eq!(stopped.cursor_seq(), Some(3));
    assert!(stopped.subscription_released());

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::resume_with_config(client, stopped, test_config(1000))
        .await
        .expect("resume");
    assert_eq!(driver.generation(), 2);
    let view = wait_for(&driver, "resumed attach", Duration::from_secs(10), |view| {
        view.state == DriverState::Attached && view.subscription_id.as_deref() == Some("sub-B")
    })
    .await;
    assert_eq!(view.cursor_seq, Some(3));
    // The retained pre-resume snapshot survived the generation change.
    assert!(view.snapshot.expect("snapshot").active_turn.is_some());
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closed_transport_publishes_disconnected() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-1")).await;

        // Peer death: no unsubscribe handshake, just a dead socket.
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect("attach");
    wait_for(&driver, "disconnected", Duration::from_secs(10), |view| {
        view.state == DriverState::Disconnected
    })
    .await;
    let stopped = driver.stop().await.expect("stop after close");
    assert!(stopped.subscription_released());
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unsupported_daemon_fails_attach_closed() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionCapabilitiesResponse {
                supported: false,
                projection_version: PROJECTION_PROTOCOL_VERSION,
                max_events_per_batch: 0,
                max_event_bytes: 0,
                max_subscriptions_per_client: 0,
                max_subscriptions_per_daemon: 0,
                retention_session_max_events: 0,
                retention_project_max_events: 0,
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let error = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect_err("unsupported daemon must fail attach");
    assert!(matches!(error, DriverError::Unsupported));
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn denied_subscribe_fails_attach_closed() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::Error {
                code: "project_not_found".into(),
                message: "no such project".into(),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let error = SessionProjectionDriver::attach_with_config(
        client,
        FIXTURE_SESSION_ID.into(),
        test_config(1000),
    )
    .await
    .expect_err("denied subscribe must fail attach");
    assert!(matches!(error, DriverError::Denied(code) if code == "project_not_found"));
    server.await.expect("server task");
}

#[test]
fn artifact_read_bounds_match_driver_constructors() {
    // The driver builds artifact reads exclusively through these exact
    // consumer constructors; pin their bounds here.
    let mut consumer = HeadlessProjectionConsumer::new();
    assert!(consumer.artifact_read_request("missing", 0, None).is_err());
    consumer
        .accept_response(&caps_response())
        .expect("capabilities");
    consumer
        .accept_response(&subscribed_response("sub-1"))
        .expect("subscribed");
    assert!(matches!(
        consumer.artifact_read_request("missing", 0, None),
        Err(codegg_protocol::projection::consumer::HeadlessConsumerError::UnsafeArtifactHandle)
    ));
    consumer
        .accept_artifact_handles(vec![ProjectionArtifactHandleDto {
            handle_id: "handle-1".into(),
            kind: ArtifactHandleKind::ToolOutput,
            project_id: FIXTURE_PROJECT_ID.into(),
            source_record_id: "tool-1".into(),
            content_type: "text/plain".into(),
            total_bytes: Some(128),
            created_at: 0,
            expires_at: None,
            revision: 1,
            public_summary: None,
        }])
        .expect("handles");
    assert!(matches!(
        consumer.artifact_read_request("handle-1", 10, Some(5)),
        Err(codegg_protocol::projection::consumer::HeadlessConsumerError::InvalidArtifactRange)
    ));
    let request = consumer
        .artifact_read_request("handle-1", 0, None)
        .expect("bounded read");
    let CoreRequest::ProjectionArtifactRead { project_id, .. } = request else {
        panic!("expected artifact read, got {request:?}");
    };
    assert_eq!(project_id, FIXTURE_PROJECT_ID);
    assert_eq!(
        consumer.artifact_handles().len(),
        1,
        "artifact catalogue stays bounded"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn artifact_excerpt_round_trip_returns_validated_outcome() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-art")).await;

        // The excerpt refreshes the authorized registry first.
        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionArtifactList { project_id } = payload else {
            panic!("expected ProjectionArtifactList, got {payload:?}");
        };
        assert_eq!(project_id, FIXTURE_PROJECT_ID);
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionArtifactList {
                handles: vec![ProjectionArtifactHandleDto {
                    handle_id: "handle-1".into(),
                    kind: ArtifactHandleKind::ToolOutput,
                    project_id: FIXTURE_PROJECT_ID.into(),
                    source_record_id: "tool-1".into(),
                    content_type: "text/plain".into(),
                    total_bytes: Some(128),
                    created_at: 0,
                    expires_at: None,
                    revision: 7,
                    public_summary: None,
                }],
            },
        )
        .await;

        // The read carries the registry revision and a bounded window;
        // the handle is opaque (no path anywhere).
        let (id, payload) = next_request(&mut reader).await;
        let CoreRequest::ProjectionArtifactRead {
            request,
            project_id,
            ..
        } = payload
        else {
            panic!("expected ProjectionArtifactRead, got {payload:?}");
        };
        assert_eq!(request.handle_id, "handle-1");
        assert_eq!(request.expected_revision, 7);
        assert_eq!(request.start, 0);
        assert_eq!(request.end, Some(64 * 1024));
        assert_eq!(project_id, FIXTURE_PROJECT_ID);
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionArtifactRead {
                outcome: codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::Ok(
                    codegg_protocol::projection::replay::ProjectionArtifactReadResponse {
                        handle_id: "handle-1".into(),
                        revision: 7,
                        start: 0,
                        end: 13,
                        content_type: "text/plain".into(),
                        content: "hello excerpt".into(),
                        redacted: false,
                        truncated: true,
                        note: None,
                    },
                ),
            },
        )
        .await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionUnsubscribed {
                subscription_id: ProjectionSubscriptionId::new("sub-art"),
            },
        )
        .await;
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach(client, FIXTURE_SESSION_ID.into())
        .await
        .expect("attach");
    let outcome = driver
        .artifact_excerpt(FIXTURE_PROJECT_ID, "handle-1", 0, None)
        .await
        .expect("excerpt");
    let codegg_protocol::projection::replay::ProjectionArtifactReadOutcome::Ok(excerpt) = outcome
    else {
        panic!("expected Ok excerpt, got {outcome:?}");
    };
    assert_eq!(excerpt.content, "hello excerpt");
    assert!(excerpt.truncated);
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn artifact_excerpt_unknown_handle_sends_no_read() {
    let (endpoint, listener) = bind().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        hello_exchange(&mut reader, &mut write).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionCapabilities));
        send_response(&mut write, id, caps_response()).await;

        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(payload, CoreRequest::ProjectionSubscribe { .. }));
        send_response(&mut write, id, subscribed_response("sub-art")).await;

        // Empty registry: the consumer rejects the unknown handle
        // before any read request is built.
        let (id, payload) = next_request(&mut reader).await;
        assert!(matches!(
            payload,
            CoreRequest::ProjectionArtifactList { .. }
        ));
        send_response(
            &mut write,
            id,
            CoreResponse::ProjectionArtifactList { handles: vec![] },
        )
        .await;

        // No ProjectionArtifactRead may follow a rejected handle. The
        // only frame allowed in this window is the stop-time
        // unsubscribe; decode instead of asserting silence.
        let mut line = String::new();
        let late =
            tokio::time::timeout(Duration::from_millis(300), reader.read_line(&mut line)).await;
        let unsubscribed_early = match late {
            Err(_) => false,
            Ok(_) => match serde_json::from_str::<CoreFrame>(line.trim()) {
                Ok(CoreFrame::Request(request))
                    if matches!(request.payload, CoreRequest::ProjectionUnsubscribe { .. }) =>
                {
                    send_response(
                        &mut write,
                        request.request_id,
                        CoreResponse::ProjectionUnsubscribed {
                            subscription_id: ProjectionSubscriptionId::new("sub-art"),
                        },
                    )
                    .await;
                    true
                }
                Ok(frame) => panic!("unexpected frame after rejected excerpt: {frame:?}"),
                Err(error) => panic!("decode late frame: {error}"),
            },
        };

        if !unsubscribed_early {
            let (id, payload) = next_request(&mut reader).await;
            assert!(matches!(payload, CoreRequest::ProjectionUnsubscribe { .. }));
            send_response(
                &mut write,
                id,
                CoreResponse::ProjectionUnsubscribed {
                    subscription_id: ProjectionSubscriptionId::new("sub-art"),
                },
            )
            .await;
        }
    });

    let client = connect_client(&endpoint).await;
    let driver = SessionProjectionDriver::attach(client, FIXTURE_SESSION_ID.into())
        .await
        .expect("attach");
    let error = driver
        .artifact_excerpt(FIXTURE_PROJECT_ID, "handle-evil", 0, None)
        .await
        .expect_err("unknown handle fails closed");
    assert!(
        matches!(error, DriverError::Consumer(_)),
        "unexpected error: {error:?}"
    );
    driver.stop().await.expect("stop");
    server.await.expect("server task");
}
