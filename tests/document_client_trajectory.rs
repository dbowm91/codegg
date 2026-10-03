//! Deterministic frontend-neutral qualification for the shared document replica.
use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use codegg_client::{DocumentController, DocumentState, DocumentTransport};
use codegg_document::{TextEdit, TextTransaction};
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::document::DocumentSnapshotDto;
use tokio::sync::Mutex;

struct ScriptedTransport(Mutex<VecDeque<CoreResponse>>);

#[async_trait]
impl DocumentTransport for ScriptedTransport {
    async fn request(&self, _request: CoreRequest) -> Result<CoreResponse, String> {
        self.0
            .lock()
            .await
            .pop_front()
            .ok_or_else(|| "unexpected request".to_owned())
    }
}

fn snapshot(text: &str, revision: u64, dirty: bool, writer: bool) -> CoreResponse {
    CoreResponse::DocumentSnapshot {
        snapshot: DocumentSnapshotDto {
            document_id: "doc-1".into(),
            project_id: "project-1".into(),
            workspace_id: "workspace-1".into(),
            relative_path: "src/lib.rs".into(),
            revision,
            text: text.into(),
            dirty,
            conflicted: false,
            writer,
            disk_base_digest: "digest".into(),
        },
        writer_lease: writer.then(|| "lease-1".into()),
        lsp_degraded: false,
    }
}

#[tokio::test]
async fn writer_replica_flushes_conflicts_and_explicit_reload_preserves_authority() {
    let writer_transport = Arc::new(ScriptedTransport(Mutex::new(VecDeque::from([
        snapshot("a", 1, false, true),
        CoreResponse::DocumentChanged {
            revision: 2,
            lsp_degraded: false,
        },
        CoreResponse::DocumentSaved {
            revision: 2,
            disk_base_digest: "saved".into(),
            lsp_degraded: false,
        },
        CoreResponse::DocumentChanged {
            revision: 3,
            lsp_degraded: false,
        },
        CoreResponse::Error {
            code: "document_disk_conflict".into(),
            message: "disk changed".into(),
        },
        CoreResponse::DocumentReloaded {
            revision: 4,
            lsp_degraded: false,
        },
        snapshot("external", 4, false, true),
    ]))));
    let writer = Arc::new(DocumentController::new(writer_transport));
    writer
        .open(
            "project-1".into(),
            "workspace-1".into(),
            "src/lib.rs".into(),
            true,
        )
        .await
        .unwrap();
    writer
        .apply_local(TextTransaction::new(vec![TextEdit::new(1..1, "b")]))
        .unwrap();
    assert_eq!(writer.snapshot().await.unwrap().0.to_string(), "ab");
    writer.flush().await.unwrap();
    writer.save().await.unwrap();

    writer
        .apply_local(TextTransaction::new(vec![TextEdit::new(2..2, "!")]))
        .unwrap();
    assert!(writer.save().await.is_err());
    assert_eq!(writer.state().await, DocumentState::Conflict);
    assert_eq!(writer.snapshot().await.unwrap().0.to_string(), "ab!");
    writer.reload_from_disk().await.unwrap();
    assert_eq!(writer.snapshot().await.unwrap().0.to_string(), "external");
    assert_eq!(writer.state().await, DocumentState::Synced);
}

#[tokio::test]
async fn read_only_observer_polls_metadata_without_owning_text_edits() {
    let observer_transport = Arc::new(ScriptedTransport(Mutex::new(VecDeque::from([
        snapshot("canonical", 7, true, false),
        CoreResponse::DocumentStatus {
            document_id: "doc-1".into(),
            revision: 8,
            dirty: true,
            conflicted: false,
            writer: false,
        },
        snapshot("new canonical", 8, true, false),
    ]))));
    let observer = Arc::new(DocumentController::new(observer_transport));
    observer
        .open(
            "project-1".into(),
            "workspace-1".into(),
            "src/lib.rs".into(),
            false,
        )
        .await
        .unwrap();
    assert_eq!(observer.state().await, DocumentState::ReadOnly);
    assert_eq!(
        observer.poll_status().await.unwrap(),
        (8, true, false, false)
    );
    observer.resync().await.unwrap();
    assert_eq!(
        observer.snapshot().await.unwrap().0.to_string(),
        "new canonical"
    );
    assert_eq!(observer.state().await, DocumentState::ReadOnly);
    assert!(observer
        .apply_local(TextTransaction::new(vec![TextEdit::new(0..0, "x")]))
        .is_err());
}
