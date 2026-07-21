#![cfg(feature = "server")]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::routing::get;
use axum::Router;
use codegg::config::schema::Config;
use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::mcp::McpService;
use codegg::protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope};
use codegg::protocol::frames::{ClientCapabilities, ClientHello, ClientKind, CoreFrame};
use codegg::protocol::projection::replay::{
    ProjectionStreamKind, ProjectionSubscriptionId, ProjectionSubscriptionRequest,
};
use codegg::protocol::tui::TuiMessage;
use codegg::server::ws::{handle_core_ws, handle_tui};
use codegg::server::{ServerState, WsRateLimiter};
use codegg_core::projection_replay::seam::ProjectionBindingContext;
use futures::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Client = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn spawn_server() -> (SocketAddr, Arc<CoreDaemon>, tokio::task::JoinHandle<()>) {
    std::env::set_var("CODEGG_SERVER_AUTH_DISABLED", "1");

    let pool = common::projection_replay::test_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None, None));
    let state = ServerState {
        pool,
        mcp_service: Arc::new(tokio::sync::RwLock::new(McpService::new())),
        config: Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(256, 60)),
        daemon: Some(Arc::clone(&daemon)),
    };
    let router = Router::new()
        .route("/core", get(handle_core_ws))
        .route("/tui", get(handle_tui))
        .with_state(state);
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("test server address");
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("test server");
    });
    (address, daemon, task)
}

async fn connect(address: SocketAddr, path: &str) -> Client {
    connect_async(format!("ws://{address}{path}"))
        .await
        .expect("connect websocket")
        .0
}

async fn send_json<T: serde::Serialize>(client: &mut Client, value: &T) {
    client
        .send(Message::Text(
            serde_json::to_string(value)
                .expect("serialize test frame")
                .into(),
        ))
        .await
        .expect("send websocket frame");
}

async fn recv_json<T: serde::de::DeserializeOwned>(client: &mut Client) -> Option<T> {
    loop {
        let message = timeout(Duration::from_millis(750), client.next())
            .await
            .ok()??
            .ok()?;
        match message {
            Message::Text(text) => return serde_json::from_str(&text).ok(),
            Message::Close(_) => return None,
            _ => {}
        }
    }
}

fn client_capabilities(session_projection: bool) -> ClientCapabilities {
    ClientCapabilities {
        visual_notifications: false,
        desktop_notifications: false,
        audio: false,
        tts: false,
        multi_session_view: false,
        plugin_ui_dialog: false,
        plugin_ui_toast: false,
        plugin_ui_panel: false,
        plugin_ui_status_item: false,
        plugin_ui_table: false,
        plugin_ui_markdown: false,
        plugin_ui_code: false,
        plugin_ui_progress: false,
        workspace_registration: false,
        project_catalog: false,
        session_projection,
    }
}

fn raw_client_capabilities() -> ClientCapabilities {
    client_capabilities(false)
}

fn projection_client_capabilities() -> ClientCapabilities {
    client_capabilities(true)
}

async fn core_handshake(client: &mut Client, session_id: &str) {
    send_json(
        client,
        &CoreFrame::ClientHello(ClientHello {
            client_name: format!("m006-{session_id}"),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: raw_client_capabilities(),
        }),
    )
    .await;
    let hello: CoreFrame = recv_json(client).await.expect("ServerHello");
    assert!(matches!(hello, CoreFrame::ServerHello(_)));

    send_json(
        client,
        &CoreFrame::Subscribe {
            client_id: String::new(),
            session_id: Some(session_id.to_string()),
            from_event_seq: Some(0),
        },
    )
    .await;
    // Ping is a protocol barrier: the receive loop processes frames in order,
    // so Pong proves that the connection-local filter is installed.
    send_json(client, &CoreFrame::Ping).await;
    loop {
        let frame: CoreFrame = recv_json(client).await.expect("Pong");
        if matches!(frame, CoreFrame::Pong) {
            break;
        }
    }
}

async fn core_projection_handshake(client: &mut Client, client_name: &str) {
    send_json(
        client,
        &CoreFrame::ClientHello(ClientHello {
            client_name: client_name.to_string(),
            client_kind: ClientKind::Automation,
            protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
            capabilities: projection_client_capabilities(),
        }),
    )
    .await;
    let hello: CoreFrame = recv_json(client).await.expect("ServerHello");
    assert!(matches!(hello, CoreFrame::ServerHello(_)));
}

fn project_subscription_request(project_id: &str) -> ProjectionSubscriptionRequest {
    ProjectionSubscriptionRequest {
        scope: ProjectionStreamKind::Project,
        scope_id: project_id.to_string(),
        cursor: None,
        projection_version: 1,
    }
}

async fn projection_event(daemon: &CoreDaemon, project_id: &str, session_id: &str, turn_id: &str) {
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("SQLite-backed daemon has projection seam");
    let envelope = EventEnvelope {
        protocol_version: codegg::protocol::core::PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1,
        session_id: Some(session_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        payload: CoreEvent::TurnStarted {
            session_id: session_id.to_string(),
            turn_id: turn_id.to_string(),
        },
    };
    let context = ProjectionBindingContext {
        session_id: Some(session_id.to_string()),
        project_id: Some(project_id.to_string()),
        workspace_id: None,
        binding_revision: 1,
    };
    let outcome = seam
        .service()
        .publish_from_core_with_context(&envelope, &context)
        .await
        .expect("publish projection event");
    assert!(matches!(
        outcome,
        codegg_core::projection_replay::service::PublishOutcome::Published { .. }
    ));
}

async fn next_core_event(client: &mut Client) -> Option<EventEnvelope<CoreEvent>> {
    loop {
        let frame: CoreFrame = recv_json(client).await?;
        if let CoreFrame::Event(event) = frame {
            return Some(event);
        }
    }
}

async fn next_core_projection_event(client: &mut Client) -> Option<ProjectionSubscriptionId> {
    loop {
        let frame: CoreFrame = recv_json(client).await?;
        if let CoreFrame::Event(event) = frame {
            if let CoreEvent::ProjectionStreamEvent {
                subscription_id, ..
            } = event.payload
            {
                return Some(subscription_id);
            }
        }
    }
}

async fn core_subscribe(
    client: &mut Client,
    request_id: &str,
    project_id: &str,
) -> ProjectionSubscriptionId {
    send_json(
        client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionSubscribe {
                request: project_subscription_request(project_id),
            },
        )),
    )
    .await;
    loop {
        let frame: CoreFrame = recv_json(client).await.expect("projection response");
        if let CoreFrame::Response {
            request_id: response_id,
            response,
        } = frame
        {
            if response_id == request_id {
                return match *response {
                    CoreResponse::ProjectionSubscribed {
                        subscription_id, ..
                    } => subscription_id,
                    other => panic!("expected ProjectionSubscribed, got {other:?}"),
                };
            }
        }
    }
}

async fn core_reject_foreign_unsubscribe(
    client: &mut Client,
    subscription_id: ProjectionSubscriptionId,
) {
    let request_id = "foreign-unsubscribe";
    send_json(
        client,
        &CoreFrame::Request(new_request(
            request_id.to_string(),
            CoreRequest::ProjectionUnsubscribe { subscription_id },
        )),
    )
    .await;
    loop {
        let frame: CoreFrame = recv_json(client)
            .await
            .expect("foreign unsubscribe response");
        if let CoreFrame::Response {
            request_id: response_id,
            response,
        } = frame
        {
            if response_id == request_id {
                assert!(matches!(
                    *response,
                    CoreResponse::Error {
                        code,
                        ..
                    } if code == "projection_subscription_not_owned"
                ));
                return;
            }
        }
    }
}

async fn tui_raw_handshake(client: &mut Client, session_id: &str) {
    send_json(
        client,
        &TuiMessage::SessionInfo {
            id: session_id.to_string(),
            model: "test-model".to_string(),
        },
    )
    .await;
    send_json(
        client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities {
                min_version: 99,
                max_version: 99,
                supports_incremental_events: true,
                supports_unknown_fields: true,
            },
        },
    )
    .await;
    let ack: TuiMessage = recv_json(client).await.expect("projection capability ack");
    assert!(matches!(
        ack,
        TuiMessage::ProjectionCapabilitiesAck {
            accepted: false,
            ..
        }
    ));
}

async fn next_tui_event(client: &mut Client) -> Option<TuiMessage> {
    loop {
        let message: TuiMessage = recv_json(client).await?;
        if let TuiMessage::EventEnvelope { payload, .. } = message {
            return Some(*payload);
        }
    }
}

async fn next_tui_projection_event(client: &mut Client) -> Option<ProjectionSubscriptionId> {
    loop {
        let message: TuiMessage = recv_json(client).await?;
        if let TuiMessage::ProjectionEvent {
            subscription_id, ..
        } = message
        {
            return Some(subscription_id);
        }
    }
}

async fn tui_subscribe(client: &mut Client, project_id: &str) -> ProjectionSubscriptionId {
    send_json(
        client,
        &TuiMessage::ProjectionSubscribe {
            request: project_subscription_request(project_id),
        },
    )
    .await;
    loop {
        let message: TuiMessage = recv_json(client).await.expect("projection snapshot");
        if let TuiMessage::ProjectionSnapshot {
            subscription_id, ..
        } = message
        {
            return subscription_id;
        }
    }
}

async fn tui_reject_foreign_unsubscribe(
    client: &mut Client,
    subscription_id: ProjectionSubscriptionId,
) {
    send_json(
        client,
        &TuiMessage::ProjectionUnsubscribe { subscription_id },
    )
    .await;
    loop {
        let message: TuiMessage = recv_json(client).await.expect("foreign unsubscribe result");
        if let TuiMessage::ProjectionUnsubscribeResult {
            accepted, reason, ..
        } = message
        {
            assert!(!accepted);
            assert_eq!(reason.as_deref(), Some("projection_subscription_not_owned"));
            return;
        }
    }
}

fn text_event(session_id: &str, delta: &str) -> CoreEvent {
    CoreEvent::TurnTextDelta {
        session_id: session_id.to_string(),
        turn_id: "turn-m006".to_string(),
        delta: delta.to_string(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_projection_delivery_is_ordered_and_connection_owned() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/core").await;
    let mut client_b = connect(address, "/core").await;
    core_projection_handshake(&mut client_a, "projection-a").await;
    core_projection_handshake(&mut client_b, "projection-b").await;

    let sub_a = core_subscribe(&mut client_a, "subscribe-a", "project-a").await;
    let sub_b = core_subscribe(&mut client_b, "subscribe-b", "project-b").await;
    core_reject_foreign_unsubscribe(&mut client_b, sub_a.clone()).await;

    projection_event(&daemon, "project-a", "session-a", "turn-a").await;
    assert_eq!(
        next_core_projection_event(&mut client_a).await,
        Some(sub_a.clone())
    );
    assert!(
        timeout(
            Duration::from_millis(250),
            next_core_projection_event(&mut client_b)
        )
        .await
        .is_err(),
        "client B received client A's projection event"
    );

    projection_event(&daemon, "project-b", "session-b", "turn-b").await;
    assert_eq!(next_core_projection_event(&mut client_b).await, Some(sub_b));
    server.abort();
}

#[test]
fn real_tui_projection_delivery_is_ordered_and_connection_owned() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_stack_size(16 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("projection test runtime")
        .block_on(async {
            let (address, daemon, server) = spawn_server().await;
            let mut client_a = connect(address, "/tui").await;
            let mut client_b = connect(address, "/tui").await;
            send_json(
                &mut client_a,
                &TuiMessage::ProjectionCapabilities {
                    capabilities:
                        codegg::protocol::projection::caps::ProjectionCapabilities::default(),
                },
            )
            .await;
            send_json(
                &mut client_b,
                &TuiMessage::ProjectionCapabilities {
                    capabilities:
                        codegg::protocol::projection::caps::ProjectionCapabilities::default(),
                },
            )
            .await;
            assert!(matches!(
                recv_json::<TuiMessage>(&mut client_a).await,
                Some(TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. })
            ));
            assert!(matches!(
                recv_json::<TuiMessage>(&mut client_b).await,
                Some(TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. })
            ));

            let sub_a = tui_subscribe(&mut client_a, "project-a").await;
            let sub_b = tui_subscribe(&mut client_b, "project-b").await;
            tui_reject_foreign_unsubscribe(&mut client_b, sub_a.clone()).await;

            projection_event(&daemon, "project-a", "session-a", "turn-a").await;
            assert_eq!(
                next_tui_projection_event(&mut client_a).await,
                Some(sub_a.clone())
            );
            assert!(
                timeout(
                    Duration::from_millis(250),
                    next_tui_projection_event(&mut client_b)
                )
                .await
                .is_err(),
                "client B received client A's projection event"
            );

            projection_event(&daemon, "project-b", "session-b", "turn-b").await;
            assert_eq!(next_tui_projection_event(&mut client_b).await, Some(sub_b));
            server.abort();
        });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_core_clients_keep_raw_sessions_isolated() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/core").await;
    let mut client_b = connect(address, "/core").await;
    core_handshake(&mut client_a, "session-a").await;
    core_handshake(&mut client_b, "session-b").await;

    daemon
        .event_log
        .publish(
            Some("session-a".to_string()),
            None,
            text_event("session-a", "a"),
        )
        .await;
    let event_a = next_core_event(&mut client_a)
        .await
        .expect("client A event");
    assert_eq!(event_a.session_id.as_deref(), Some("session-a"));

    let foreign_for_b = timeout(Duration::from_millis(250), next_core_event(&mut client_b)).await;
    assert!(
        foreign_for_b.is_err(),
        "client B received session A traffic"
    );

    daemon
        .event_log
        .publish(
            Some("session-b".to_string()),
            None,
            text_event("session-b", "b"),
        )
        .await;
    let event_b = next_core_event(&mut client_b)
        .await
        .expect("client B event");
    assert_eq!(event_b.session_id.as_deref(), Some("session-b"));

    let foreign_for_a = timeout(Duration::from_millis(250), next_core_event(&mut client_a)).await;
    assert!(
        foreign_for_a.is_err(),
        "client A received session B traffic"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_tui_clients_keep_raw_sessions_isolated() {
    let (address, daemon, server) = spawn_server().await;
    let mut client_a = connect(address, "/tui").await;
    let mut client_b = connect(address, "/tui").await;
    tui_raw_handshake(&mut client_a, "session-a").await;
    tui_raw_handshake(&mut client_b, "session-b").await;

    daemon
        .event_log
        .publish(
            Some("session-a".to_string()),
            None,
            text_event("session-a", "a"),
        )
        .await;
    assert!(matches!(
        next_tui_event(&mut client_a).await,
        Some(TuiMessage::TextDelta { delta }) if delta == "a"
    ));
    assert!(
        timeout(Duration::from_millis(250), next_tui_event(&mut client_b))
            .await
            .is_err(),
        "client B received session A traffic"
    );

    daemon
        .event_log
        .publish(
            Some("session-b".to_string()),
            None,
            text_event("session-b", "b"),
        )
        .await;
    assert!(matches!(
        next_tui_event(&mut client_b).await,
        Some(TuiMessage::TextDelta { delta }) if delta == "b"
    ));
    assert!(
        timeout(Duration::from_millis(250), next_tui_event(&mut client_a))
            .await
            .is_err(),
        "client A received session B traffic"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_tui_projection_primary_suppresses_raw_session_events() {
    let (address, daemon, server) = spawn_server().await;
    let mut client = connect(address, "/tui").await;
    send_json(
        &mut client,
        &TuiMessage::SessionInfo {
            id: "session-primary".to_string(),
            model: "test-model".to_string(),
        },
    )
    .await;
    send_json(
        &mut client,
        &TuiMessage::ProjectionCapabilities {
            capabilities: codegg::protocol::projection::caps::ProjectionCapabilities::default(),
        },
    )
    .await;
    let ack: TuiMessage = recv_json(&mut client)
        .await
        .expect("accepted capability ack");
    assert!(matches!(
        ack,
        TuiMessage::ProjectionCapabilitiesAck { accepted: true, .. }
    ));

    daemon
        .event_log
        .publish(
            Some("session-primary".to_string()),
            None,
            text_event("session-primary", "must-not-be-raw"),
        )
        .await;
    assert!(
        timeout(Duration::from_millis(250), next_tui_event(&mut client))
            .await
            .is_err(),
        "projection-primary client received raw session traffic"
    );
    server.abort();
}
