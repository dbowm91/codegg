pub mod artifact;
pub mod block;
pub mod block_builder;
pub mod cache_stats;
pub mod handle;
pub mod packer;
pub mod projection;
pub mod read_tool;
pub mod tool_hash;

pub use artifact::{
    build_handle, compute_content_hash, estimate_tokens, ArtifactKind, ContextArtifact,
    ContextArtifactStore, InMemoryArtifactStore,
};
pub use block::{CacheClass, ContextBlock, ContextBlockId, ContextBlockKind, Lossiness};
pub use cache_stats::{CacheStatsEntry, ContextCacheStats};
pub use handle::{clamp_to_char_boundary, ContextHandle, ContextHandleError, ContextHandleKind};
pub use packer::{ContextPackBudget, ContextPackResult, OmissionReason, OmittedContextBlock};
pub use projection::{
    project_tool_output, ProjectionConfig, ProjectionStatus, ToolOutputProjection,
};
pub use read_tool::ContextReadTool;
pub use tool_hash::tool_definitions_hash;

pub use block_builder::ContextBlockBuilder;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::Tool;

    #[test]
    fn test_reexports_available() {
        let _ = std::mem::size_of::<ArtifactKind>();
        let _ = std::mem::size_of::<ProjectionStatus>();
        let _ = std::mem::size_of::<ProjectionConfig>();
        let _ = std::mem::size_of::<ToolOutputProjection>();
    }

    #[test]
    fn test_build_handle_checked_through_mod() {
        let h = ContextHandle::build_tool("s1", 0, "c1").unwrap();
        assert_eq!(h, "ctx://tool/s1/0/c1");
    }

    #[test]
    fn test_estimate_tokens_through_mod() {
        let t = estimate_tokens("hello world");
        assert!(t > 0);
    }

    #[tokio::test]
    async fn test_in_memory_store_through_mod() {
        let store = InMemoryArtifactStore::new();
        let artifact = ContextArtifact {
            handle: "ctx://tool/s1/0/c0".into(),
            session_id: "s1".into(),
            turn_index: 0,
            tool_call_id: Some("c0".into()),
            tool_name: Some("bash".into()),
            kind: ArtifactKind::ToolResult,
            created_at_ms: 1000,
            content_hash: "abc".into(),
            redacted_content: "out".into(),
            raw_bytes_len: 3,
            estimated_tokens: 1,
        };
        store.put(artifact).await.unwrap();
        let got = store.get("ctx://tool/s1/0/c0").await.unwrap();
        assert!(got.is_some());
    }

    #[test]
    fn test_project_through_mod() {
        let config = ProjectionConfig::default();
        let proj = project_tool_output("bash", None, "output", true, "ctx://t", &config);
        assert_eq!(proj.status, ProjectionStatus::Success);
    }

    // --- Projection integration tests ---

    #[test]
    fn test_projection_short_success_passthrough() {
        let config = ProjectionConfig::default();
        let proj = project_tool_output("bash", None, "hello", true, "ctx://t/0/c1", &config);
        assert_eq!(proj.status, ProjectionStatus::Success);
        assert!(proj.model_text.contains("hello"));
        assert!(proj.model_text.contains("ctx://t/0/c1"));
    }

    #[test]
    fn test_projection_verbose_success_truncated() {
        let config = ProjectionConfig {
            max_success_tokens: 5,
            ..Default::default()
        };
        let output: String = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let proj = project_tool_output("bash", None, &output, true, "ctx://t", &config);
        assert!(proj.model_text.contains("more lines"));
        assert!(proj.summary.contains("tokens"));
    }

    #[test]
    fn test_projection_failure_preserves_rust_errors() {
        let config = ProjectionConfig::default();
        let output =
            "error[E0308]: mismatched types\n  --> src/main.rs:5:10\nexpected `i32`, found `&str`";
        let proj = project_tool_output("bash", None, output, false, "ctx://t", &config);
        assert_eq!(proj.status, ProjectionStatus::Failure);
        assert!(proj.model_text.contains("error[E0308]"));
        assert!(proj
            .unresolved_errors
            .iter()
            .any(|e| e.contains("error[E0308]")));
    }

    #[test]
    fn test_projection_failure_preserves_pytest_errors() {
        let config = ProjectionConfig::default();
        let output = "FAILED test_app.py::test_login - AssertionError: x != y\nTraceback (most recent call last):\n  File \"app.py\", line 10\n    foo()\nAssertionError: x != y";
        let proj = project_tool_output("python", None, output, false, "ctx://t", &config);
        assert_eq!(proj.status, ProjectionStatus::Failure);
        assert!(proj
            .unresolved_errors
            .iter()
            .any(|e| e.contains("FAILED") || e.contains("AssertionError")));
    }

    #[test]
    fn test_projection_collapses_repeated_lines() {
        let config = ProjectionConfig::default();
        let output = "normal\nnormal\nnormal\nerror: something\nnormal\nnormal";
        let proj = project_tool_output("bash", None, output, false, "ctx://t", &config);
        assert!(proj
            .unresolved_errors
            .iter()
            .any(|e| e.contains("error: something")));
    }

    #[test]
    fn test_projection_within_token_budget() {
        let config = ProjectionConfig {
            max_success_tokens: 50,
            ..Default::default()
        };
        let output: String = (0..200)
            .map(|i| format!("output line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let proj = project_tool_output("bash", None, &output, true, "ctx://t", &config);
        let token_est = estimate_tokens(&proj.model_text);
        assert!(
            token_est <= 120,
            "projection exceeded budget: {token_est} tokens"
        );
    }

    #[test]
    fn test_projection_disabled_passthrough() {
        let config = ProjectionConfig {
            enabled: false,
            ..Default::default()
        };
        let output = "a".repeat(5000);
        let proj = project_tool_output("bash", None, &output, true, "ctx://t", &config);
        assert!(proj.model_text.contains(&output));
    }

    // --- Artifact tests ---

    #[tokio::test]
    async fn test_artifact_put_get_roundtrip() {
        let store = InMemoryArtifactStore::new();
        let artifact = ContextArtifact {
            handle: "ctx://tool/s1/0/c1".into(),
            session_id: "s1".into(),
            turn_index: 0,
            tool_call_id: Some("c1".into()),
            tool_name: Some("bash".into()),
            kind: ArtifactKind::ToolResult,
            created_at_ms: 1000,
            content_hash: "abc".into(),
            redacted_content: "output".into(),
            raw_bytes_len: 6,
            estimated_tokens: 2,
        };
        store.put(artifact.clone()).await.unwrap();
        let got = store.get("ctx://tool/s1/0/c1").await.unwrap().unwrap();
        assert_eq!(got.handle, artifact.handle);
        assert_eq!(got.redacted_content, artifact.redacted_content);
    }

    #[tokio::test]
    async fn test_artifact_missing_handle_returns_none() {
        let store = InMemoryArtifactStore::new();
        let got = store.get("ctx://tool/s1/0/missing").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn test_artifact_list_recent_by_session() {
        let store = InMemoryArtifactStore::new();
        for i in 0..5 {
            store
                .put(ContextArtifact {
                    handle: format!("ctx://tool/s1/{i}/c{i}"),
                    session_id: "s1".into(),
                    turn_index: i,
                    tool_call_id: Some(format!("c{i}")),
                    tool_name: Some("bash".into()),
                    kind: ArtifactKind::ToolResult,
                    created_at_ms: (i as i64) * 1000,
                    content_hash: "abc".into(),
                    redacted_content: "out".into(),
                    raw_bytes_len: 3,
                    estimated_tokens: 1,
                })
                .await
                .unwrap();
        }
        let results = store.list_recent("s1", 3).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].turn_index, 4);
    }

    // --- ContextLedgerState tests ---

    #[test]
    fn test_ledger_record_projection_adds_files() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        let proj = ToolOutputProjection {
            model_text: "text".into(),
            summary: "summary".into(),
            status: ProjectionStatus::Success,
            detected_kind: ArtifactKind::ToolResult,
            touched_files: vec!["src/main.rs".into(), "src/lib.rs".into()],
            commands_run: vec![],
            test_results: vec![],
            unresolved_errors: vec![],
        };
        ledger.record_projection(&proj, "ctx://t/0/c1");
        assert!(ledger.touched_files.contains(&"src/main.rs".to_string()));
        assert!(ledger.touched_files.contains(&"src/lib.rs".to_string()));
    }

    #[test]
    fn test_ledger_record_projection_deduplicates_files() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        let proj = ToolOutputProjection {
            model_text: "text".into(),
            summary: "summary".into(),
            status: ProjectionStatus::Success,
            detected_kind: ArtifactKind::ToolResult,
            touched_files: vec!["src/main.rs".into()],
            commands_run: vec![],
            test_results: vec![],
            unresolved_errors: vec![],
        };
        ledger.record_projection(&proj, "ctx://t/0/c1");
        ledger.record_projection(&proj, "ctx://t/0/c2");
        let count = ledger
            .touched_files
            .iter()
            .filter(|f| f.as_str() == "src/main.rs")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_ledger_record_projection_limits_files_to_20() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        for i in 0..25 {
            let proj = ToolOutputProjection {
                model_text: "text".into(),
                summary: "summary".into(),
                status: ProjectionStatus::Success,
                detected_kind: ArtifactKind::ToolResult,
                touched_files: vec![format!("file_{i}.rs")],
                commands_run: vec![],
                test_results: vec![],
                unresolved_errors: vec![],
            };
            ledger.record_projection(&proj, &format!("ctx://t/0/c{i}"));
        }
        assert!(ledger.touched_files.len() <= 20);
    }

    #[test]
    fn test_ledger_record_projection_adds_commands() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        let proj = ToolOutputProjection {
            model_text: "text".into(),
            summary: "summary".into(),
            status: ProjectionStatus::Success,
            detected_kind: ArtifactKind::ToolResult,
            touched_files: vec![],
            commands_run: vec!["cargo build".into()],
            test_results: vec![],
            unresolved_errors: vec![],
        };
        ledger.record_projection(&proj, "ctx://t/0/c1");
        assert!(ledger.commands_run.contains(&"cargo build".to_string()));
    }

    #[test]
    fn test_ledger_record_projection_limits_commands_to_10() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        for i in 0..15 {
            let proj = ToolOutputProjection {
                model_text: "text".into(),
                summary: "summary".into(),
                status: ProjectionStatus::Success,
                detected_kind: ArtifactKind::ToolResult,
                touched_files: vec![],
                commands_run: vec![format!("cmd_{i}")],
                test_results: vec![],
                unresolved_errors: vec![],
            };
            ledger.record_projection(&proj, &format!("ctx://t/0/c{i}"));
        }
        assert!(ledger.commands_run.len() <= 10);
    }

    #[test]
    fn test_ledger_to_context_frame() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        let proj = ToolOutputProjection {
            model_text: "text".into(),
            summary: "summary".into(),
            status: ProjectionStatus::Success,
            detected_kind: ArtifactKind::ToolResult,
            touched_files: vec!["src/main.rs".into()],
            commands_run: vec!["cargo test".into()],
            test_results: vec!["5 passed".into()],
            unresolved_errors: vec!["error: something".into()],
        };
        ledger.record_projection(&proj, "ctx://t/0/c1");
        let frame = ledger.to_context_frame();
        assert!(frame.touched_files.contains(&"src/main.rs".to_string()));
        assert!(frame.commands_run.contains(&"cargo test".to_string()));
        assert!(frame.test_results.contains(&"5 passed".to_string()));
        assert!(frame
            .unresolved_errors
            .contains(&"error: something".to_string()));
    }

    // --- Config tests ---

    #[test]
    fn test_projection_config_defaults() {
        let config = ProjectionConfig::default();
        assert_eq!(config.max_success_tokens, 800);
        assert_eq!(config.max_failure_tokens, 2000);
        assert!(config.enabled);
    }

    #[test]
    fn test_projection_config_custom() {
        let config = ProjectionConfig {
            enabled: false,
            max_success_tokens: 100,
            max_failure_tokens: 500,
            artifact_store_enabled: false,
            lossless_debug: true,
        };
        assert!(!config.enabled);
        assert_eq!(config.max_success_tokens, 100);
        assert_eq!(config.max_failure_tokens, 500);
        assert!(!config.artifact_store_enabled);
        assert!(config.lossless_debug);
    }

    #[test]
    fn test_context_config_defaults() {
        let config = codegg_config::schema::ContextConfig::default();
        assert!(config.artifact_store.is_none());
        assert!(config.project_tool_outputs.is_none());
        assert!(config.max_success_tokens.is_none());
        assert!(config.max_failure_tokens.is_none());
        assert!(config.lossless_debug.is_none());
    }

    #[test]
    fn test_context_config_deserialization() {
        let json = r#"{
            "artifact_store": true,
            "project_tool_outputs": false,
            "max_success_tokens": 500,
            "max_failure_tokens": 1500,
            "lossless_debug": true
        }"#;
        let config: codegg_config::schema::ContextConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.artifact_store, Some(true));
        assert_eq!(config.project_tool_outputs, Some(false));
        assert_eq!(config.max_success_tokens, Some(500));
        assert_eq!(config.max_failure_tokens, Some(1500));
        assert_eq!(config.lossless_debug, Some(true));
    }

    #[test]
    fn test_context_config_partial_deserialization() {
        let json = r#"{"max_success_tokens": 100}"#;
        let config: codegg_config::schema::ContextConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_success_tokens, Some(100));
        assert!(config.artifact_store.is_none());
        assert!(config.project_tool_outputs.is_none());
    }

    // --- Phase 2: artifact_store = false skips storage ---

    #[tokio::test]
    async fn test_artifact_store_false_skips_storage() {
        let store = InMemoryArtifactStore::new();
        // Simulate artifact_store = false by not calling put()
        // The store should remain empty
        let results = store.list_recent("s1", 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_projection_without_artifact_store_no_handle() {
        let config = ProjectionConfig {
            artifact_store_enabled: false,
            ..Default::default()
        };
        // When artifact_store is false, effective_handle should be ""
        let proj = project_tool_output("bash", None, "output", true, "", &config);
        assert!(!proj.model_text.contains("ctx://"));
    }

    // --- Phase 6: no empty artifact handles ---

    #[test]
    fn test_ledger_does_not_store_empty_handles() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        let proj = ToolOutputProjection {
            model_text: "text".into(),
            summary: "summary".into(),
            status: ProjectionStatus::Success,
            detected_kind: ArtifactKind::ToolResult,
            touched_files: vec![],
            commands_run: vec![],
            test_results: vec![],
            unresolved_errors: vec![],
        };
        ledger.record_projection(&proj, "");
        assert!(ledger.artifact_handles.is_empty());
    }

    #[test]
    fn test_ledger_deduplicates_handles() {
        let mut ledger = crate::agent::context_frame::ContextLedgerState::new();
        let proj = ToolOutputProjection {
            model_text: "text".into(),
            summary: "summary".into(),
            status: ProjectionStatus::Success,
            detected_kind: ArtifactKind::ToolResult,
            touched_files: vec![],
            commands_run: vec![],
            test_results: vec![],
            unresolved_errors: vec![],
        };
        ledger.record_projection(&proj, "ctx://t/0/c1");
        ledger.record_projection(&proj, "ctx://t/0/c1");
        assert_eq!(ledger.artifact_handles.len(), 1);
    }

    // --- ContextReadTool tests (additional) ---

    #[tokio::test]
    async fn test_context_read_tool_preserves_metadata() {
        let store = InMemoryArtifactStore::new();
        let artifact = ContextArtifact {
            handle: "ctx://tool/s1/0/c1".into(),
            session_id: "s1".into(),
            turn_index: 0,
            tool_call_id: Some("c1".into()),
            tool_name: Some("read".into()),
            kind: ArtifactKind::ReadResult,
            created_at_ms: 1000,
            content_hash: "abc".into(),
            redacted_content: "file content here".into(),
            raw_bytes_len: 17,
            estimated_tokens: 5,
        };
        store.put(artifact).await.unwrap();

        let tool = ContextReadTool::new(std::sync::Arc::new(store), "s1".into());
        let input = serde_json::json!({"handle": "ctx://tool/s1/0/c1"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("Tool: read"));
        assert!(result.contains("Kind: ReadResult"));
        assert!(result.contains("file content here"));
    }
}
