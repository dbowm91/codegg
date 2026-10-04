//! Deterministic frontend-neutral qualification for the shared document replica.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use codegg_client::{DocumentController, DocumentState, DocumentTransport};
use codegg_document::{TextEdit, TextTransaction};
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::document::DocumentSnapshotDto;
use tokio::sync::Mutex;

struct ScriptedTransport(Mutex<VecDeque<CoreResponse>>);

struct SaveRaceTransport {
    text: Mutex<String>,
    revision: Mutex<u64>,
    dirty: AtomicBool,
    first_save: AtomicBool,
    save_entered: tokio::sync::Notify,
    release_save: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
}

#[async_trait]
impl DocumentTransport for SaveRaceTransport {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        match request {
            CoreRequest::DocumentOpen { .. } => Ok(snapshot(
                &self.text.lock().await,
                *self.revision.lock().await,
                self.dirty.load(Ordering::Acquire),
                true,
            )),
            CoreRequest::DocumentChange { transaction, .. } => {
                let mut text = self.text.lock().await;
                for edit in transaction.edits.iter().rev() {
                    text.replace_range(edit.range.start..edit.range.end, &edit.insert);
                }
                let mut revision = self.revision.lock().await;
                *revision += 1;
                self.dirty.store(true, Ordering::Release);
                Ok(CoreResponse::DocumentChanged {
                    revision: *revision,
                    lsp_degraded: false,
                })
            }
            CoreRequest::DocumentSave { .. } => {
                if !self.first_save.swap(true, Ordering::AcqRel) {
                    self.save_entered.notify_one();
                    if let Some(release) = self.release_save.lock().await.take() {
                        release.await.map_err(|error| error.to_string())?;
                    }
                }
                self.dirty.store(false, Ordering::Release);
                Ok(CoreResponse::DocumentSaved {
                    revision: *self.revision.lock().await,
                    disk_base_digest: "saved".into(),
                    lsp_degraded: false,
                })
            }
            CoreRequest::DocumentClose { .. } => Ok(CoreResponse::Ack),
            _ => Err("unexpected trajectory request".into()),
        }
    }
}

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
        CoreResponse::Ack,
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
    observer.close().await.unwrap();
    assert_eq!(observer.state().await, DocumentState::Closed);
}

#[tokio::test]
async fn tui_kind_controller_keeps_edit_through_save_and_reopen() {
    let (release, gate) = tokio::sync::oneshot::channel();
    let transport = Arc::new(SaveRaceTransport {
        text: Mutex::new("a".into()),
        revision: Mutex::new(0),
        dirty: AtomicBool::new(false),
        first_save: AtomicBool::new(false),
        save_entered: tokio::sync::Notify::new(),
        release_save: Mutex::new(Some(gate)),
    });
    let writer = Arc::new(DocumentController::new(transport.clone()));
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
    writer.flush().await.unwrap();

    let save_writer = writer.clone();
    let save = tokio::spawn(async move { save_writer.save().await });
    transport.save_entered.notified().await;
    writer
        .apply_local(TextTransaction::new(vec![TextEdit::new(2..2, "!")]))
        .unwrap();
    release.send(()).unwrap();
    save.await.unwrap().unwrap();
    assert_eq!(writer.snapshot().await.unwrap().0.to_string(), "ab!");
    assert!(matches!(
        writer.state().await,
        DocumentState::DirtyLocal | DocumentState::Flushing
    ));
    writer.flush().await.unwrap();
    writer.save().await.unwrap();
    assert_eq!(*transport.text.lock().await, "ab!");
    writer.close().await.unwrap();
    writer
        .open(
            "project-1".into(),
            "workspace-1".into(),
            "src/lib.rs".into(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(writer.snapshot().await.unwrap().0.to_string(), "ab!");
}
