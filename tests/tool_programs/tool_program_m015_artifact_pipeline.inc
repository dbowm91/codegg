//! M015 canonical Tool Program artifact production and verification.

use codegg::context::{ContextArtifactStore, FileArtifactStore, InMemoryArtifactStore};
use codegg::tool::tool_program_result::{
    persist_program_artifact, resolve_program_artifact, ToolProgramResultStore,
};
use codegg_core::tool_program::{ProgramResult, ProgramStatus, ProgramValue};
use std::sync::Arc;

#[tokio::test(flavor = "current_thread")]
async fn canonical_artifact_round_trip_verifies_handle_and_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store: Arc<dyn ContextArtifactStore> = Arc::new(FileArtifactStore::new(temp.path()));
    let reference = persist_program_artifact(
        store.clone(),
        "session",
        "call-1",
        "read",
        br#"{"content":"real broker output"}"#,
    )
    .await
    .unwrap();
    let artifact = resolve_program_artifact(store, &reference).await.unwrap();
    assert_eq!(artifact.handle, reference.handle);
    assert_eq!(
        format!("sha256:{}", artifact.content_hash),
        reference.digest
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tampered_or_missing_artifact_fails_closed() {
    let store: Arc<dyn ContextArtifactStore> = Arc::new(InMemoryArtifactStore::new());
    let mut reference =
        persist_program_artifact(store.clone(), "session", "call-2", "read", b"content")
            .await
            .unwrap();
    reference.digest = "sha256:tampered".into();
    assert!(resolve_program_artifact(store, &reference).await.is_err());
}

#[test]
fn result_store_does_not_fabricate_large_output_handles() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let result = ProgramResult {
        status: ProgramStatus::Completed,
        output: Some(ProgramValue::String("x".repeat(150 * 1024))),
        error_message: None,
        failure_class: None,
        steps_used: 1,
        bytes_used: 150 * 1024,
        calls_completed: 0,
        calls_total: 0,
        iterations_used: 0,
    };
    let record = store
        .persist(
            "tp-large",
            "attempt-1",
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .unwrap();
    assert!(
        record.output_artifact.is_none(),
        "the result store must never fabricate a ctx:// handle"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn canonical_store_failure_is_propagated() {
    struct FailingStore;
    #[async_trait::async_trait]
    impl ContextArtifactStore for FailingStore {
        async fn put(&self, _: codegg::context::ContextArtifact) -> anyhow::Result<()> {
            anyhow::bail!("injected artifact persistence failure")
        }
        async fn get(&self, _: &str) -> anyhow::Result<Option<codegg::context::ContextArtifact>> {
            Ok(None)
        }
        async fn list_recent(
            &self,
            _: &str,
            _: usize,
        ) -> anyhow::Result<Vec<codegg::context::ContextArtifact>> {
            Ok(vec![])
        }
    }

    let error = persist_program_artifact(
        Arc::new(FailingStore),
        "session",
        "output",
        "tool_program",
        b"large output",
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("injected artifact"));
}
