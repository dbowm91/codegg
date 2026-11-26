//! M001 (ADR-0065): a workspace `DocumentOpen` returns document authority.
//!
//! These pin the *response deadline*, not a wall clock. The daemon in this
//! fixture has no language-server service, so a publication that the open
//! awaited would have answered `Unavailable` — it is the only outcome that
//! path can produce. The open answers `Pending` instead, which is reachable
//! only when the response did not await publication at all.
//!
//! Nothing here claims a live server, a measured 800-line open, or that a
//! warm open is fast. A warm open that is still slow is a repository-lock
//! wait in `refresh_external_disk`, which this milestone deliberately kept
//! on the response path.

use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreRequest, CoreResponse, DocumentLspSync};
use codegg::protocol::document::DocumentScope;

/// A daemon with one registered workspace and one seeded file.
struct Fixture {
    daemon: Arc<CoreDaemon>,
    workspace_id: String,
    _root: std::path::PathBuf,
}

impl Fixture {
    async fn build(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "codegg-m001-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create the workspace root");
        std::fs::write(root.join("main.rs"), "fn main() {}\n").expect("seed the file");
        // No pool: `verify_project_workspace` has no catalog to check, which
        // is the documented pool-less behaviour. No `lsp_service`: every
        // publication attempt in this file resolves to `Unavailable`.
        let daemon = Arc::new(CoreDaemon::with_deps(CoreRuntimeDeps::new(
            None, None, None,
        )));
        let workspace = daemon
            .workspaces
            .get_or_register(&root)
            .await
            .expect("register the workspace");
        Self {
            daemon,
            workspace_id: workspace.id.to_string(),
            _root: root,
        }
    }

    async fn call(&self, request: CoreRequest) -> CoreResponse {
        let request_id = format!("req-{}", uuid::Uuid::new_v4().simple());
        Box::pin(
            self.daemon
                .handle_request(codegg::core::new_request(request_id, request)),
        )
        .await
        .expect("the daemon request succeeds at the transport layer")
    }

    async fn open(&self) -> CoreResponse {
        self.call(CoreRequest::DocumentOpen {
            project_id: "project-m001".to_owned(),
            workspace_id: self.workspace_id.clone(),
            relative_path: "main.rs".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await
    }
}

#[tokio::test]
async fn a_workspace_open_returns_before_language_server_publication() {
    let fixture = Fixture::build("pending").await;
    let response = fixture.open().await;
    let (snapshot, writer_lease, lsp_degraded, lsp_sync) = match response {
        CoreResponse::DocumentSnapshot {
            snapshot,
            writer_lease,
            lsp_degraded,
            lsp_sync,
        } => (snapshot, writer_lease, lsp_degraded, lsp_sync),
        other => panic!("expected an opened document, got {other:?}"),
    };

    assert_eq!(
        lsp_sync,
        DocumentLspSync::Pending,
        "the open response must say publication has not finished; an \
         awaited publication with no service answers Unavailable"
    );
    assert!(
        !lsp_degraded,
        "a publication that has not finished is not a degraded server"
    );
    assert!(
        !lsp_sync.is_synced(),
        "an interactive open must never claim the server has the revision"
    );

    // Document authority still ran: the text is the daemon's snapshot.
    assert_eq!(snapshot.text, "fn main() {}\n");
    assert_eq!(snapshot.relative_path, "main.rs");
    assert!(
        writer_lease.is_none(),
        "a read-only open takes no writer lease"
    );
}

#[tokio::test]
async fn an_awaited_snapshot_read_still_reports_the_publication_outcome() {
    let fixture = Fixture::build("awaited").await;
    let opened = fixture.open().await;
    let document_id = match opened {
        CoreResponse::DocumentSnapshot { snapshot, .. } => snapshot.document_id,
        other => panic!("expected an opened document, got {other:?}"),
    };

    let response = fixture
        .call(CoreRequest::DocumentSnapshotGet {
            project_id: "project-m001".to_owned(),
            document_id,
        })
        .await;
    let (lsp_sync, lsp_degraded) = match response {
        CoreResponse::DocumentSnapshot {
            lsp_sync,
            lsp_degraded,
            ..
        } => (lsp_sync, lsp_degraded),
        other => panic!("expected a snapshot, got {other:?}"),
    };

    // This arm is not the interactive open: ADR-0065 did not make it
    // asynchronous, so it still awaits and still reports the real outcome.
    assert_eq!(
        lsp_sync,
        DocumentLspSync::Unavailable,
        "the awaited read must report the outcome, not Pending"
    );
    assert!(
        lsp_degraded,
        "no server can serve this document, which `lsp_degraded` reports"
    );
}

#[tokio::test]
async fn a_document_created_before_publication_keeps_its_awaited_answer() {
    let fixture = Fixture::build("create").await;
    let response = fixture
        .call(CoreRequest::DocumentCreate {
            project_id: "project-m001".to_owned(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "notes.txt".to_owned(),
            initial_text: "hello\n".to_owned(),
            scope: DocumentScope::Workspace,
        })
        .await;
    let lsp_sync = match response {
        CoreResponse::DocumentSnapshot { lsp_sync, .. } => lsp_sync,
        other => panic!("expected the created document, got {other:?}"),
    };
    assert_eq!(
        lsp_sync,
        DocumentLspSync::Unavailable,
        "create is not the interactive open; it keeps the awaited outcome"
    );
}
