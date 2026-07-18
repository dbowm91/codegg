use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope, PROTOCOL_VERSION,
};
use codegg::protocol::provider::{
    ConnectionRotateChange, ConnectionRotateStatusDto, PurgeOutcome, SecretInput, SecretInputRef,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Default)]
struct FakeLifecycleDaemon {
    operations: Mutex<Vec<String>>,
}

#[async_trait]
impl CoreClient for FakeLifecycleDaemon {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::ConnectionRotateSecretStage { request_id, secret } => {
                assert!(!format!("{secret:?}").contains(secret.expose()));
                self.operations
                    .lock()
                    .unwrap()
                    .push("secret_stage".to_string());
                Ok(CoreResponse::ConnectionRotateSecretStaged {
                    request_id,
                    secret: SecretInputRef::new("rot-secret-test").unwrap(),
                })
            }
            CoreRequest::ConnectionRotateBegin {
                request_id,
                secret,
                change: ConnectionRotateChange::CredentialOnly,
                ..
            } => {
                assert_eq!(secret.handle, "rot-secret-test");
                self.operations.lock().unwrap().push("rotate".to_string());
                Ok(CoreResponse::ConnectionRotateStatus {
                    result: ConnectionRotateStatusDto {
                        request_id,
                        connection_id: "connection-1".to_string(),
                        state: "committed".to_string(),
                        new_revision: Some(2),
                        catalog_revision: Some("catalog-2".to_string()),
                        error_code: None,
                    },
                })
            }
            CoreRequest::ConnectionDisable { .. } => {
                self.operations.lock().unwrap().push("disable".to_string());
                Ok(CoreResponse::Ack)
            }
            CoreRequest::ConnectionDelete { .. } => {
                self.operations.lock().unwrap().push("delete".to_string());
                Ok(CoreResponse::Ack)
            }
            CoreRequest::ConnectionRestore { .. } => {
                self.operations.lock().unwrap().push("restore".to_string());
                Ok(CoreResponse::Ack)
            }
            CoreRequest::ConnectionPurge { .. } => {
                self.operations.lock().unwrap().push("purge".to_string());
                Ok(CoreResponse::ConnectionPurge {
                    outcome: PurgeOutcome::Purged,
                })
            }
            other => panic!("unexpected lifecycle request: {other:?}"),
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::unbounded_channel();
        rx
    }
}

fn request(payload: CoreRequest) -> RequestEnvelope<CoreRequest> {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: "test-request".to_string(),
        payload,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn fake_daemon_exercises_secret_stage_rotation_and_lifecycle_sequence() {
    let daemon = Arc::new(FakeLifecycleDaemon::default());
    let staged = daemon
        .request(request(CoreRequest::ConnectionRotateSecretStage {
            request_id: "rotation-1".to_string(),
            secret: SecretInput::new("not-in-a-rotation-handle").unwrap(),
        }))
        .await
        .unwrap();
    let secret = match staged {
        CoreResponse::ConnectionRotateSecretStaged { secret, .. } => secret,
        other => panic!("expected staged secret, got {other:?}"),
    };
    let rotated = daemon
        .request(request(CoreRequest::ConnectionRotateBegin {
            request_id: "rotation-1".to_string(),
            connection_id: "connection-1".to_string(),
            expected_revision: 1,
            change: ConnectionRotateChange::CredentialOnly,
            secret,
        }))
        .await
        .unwrap();
    assert!(matches!(
        rotated,
        CoreResponse::ConnectionRotateStatus { result }
            if result.state == "committed" && result.new_revision == Some(2)
    ));

    for payload in [
        CoreRequest::ConnectionDisable {
            connection_id: "connection-1".to_string(),
            expected_revision: 2,
        },
        CoreRequest::ConnectionDelete {
            connection_id: "connection-1".to_string(),
            expected_revision: 3,
        },
        CoreRequest::ConnectionRestore {
            connection_id: "connection-1".to_string(),
            expected_revision: 4,
        },
        CoreRequest::ConnectionPurge {
            connection_id: "connection-1".to_string(),
            expected_revision: 5,
        },
    ] {
        let response = daemon.request(request(payload)).await.unwrap();
        assert!(!matches!(response, CoreResponse::Error { .. }));
    }

    assert_eq!(
        daemon.operations.lock().unwrap().as_slice(),
        [
            "secret_stage",
            "rotate",
            "disable",
            "delete",
            "restore",
            "purge"
        ]
    );
}
