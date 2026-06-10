use async_trait::async_trait;
use serde_json::Value;

use super::handle::{clamp_to_char_boundary, ContextHandle};

pub struct ContextReadTool {
    store: std::sync::Arc<dyn crate::context::ContextArtifactStore>,
    session_id: String,
}

impl ContextReadTool {
    pub fn new(
        store: std::sync::Arc<dyn crate::context::ContextArtifactStore>,
        session_id: String,
    ) -> Self {
        Self { store, session_id }
    }
}

#[async_trait]
impl crate::tool::Tool for ContextReadTool {
    fn name(&self) -> &str {
        "context_read"
    }

    fn description(&self) -> &str {
        "Read the full content of a stored context artifact by its ctx:// handle. \
         Use this to recover full tool output that was compressed in the model transcript."
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "handle": {
                    "type": "string",
                    "description": "The ctx:// handle of the artifact to read (e.g. ctx://tool/session_id/turn_index/tool_call_id)"
                },
                "offset": {
                    "type": "integer",
                    "default": 0,
                    "description": "Byte offset to start reading from"
                },
                "max_bytes": {
                    "type": "integer",
                    "default": 20000,
                    "description": "Maximum bytes to return"
                }
            },
            "required": ["handle"]
        })
    }

    fn category(&self) -> crate::tool::ToolCategory {
        crate::tool::ToolCategory::ReadOnly
    }

    async fn execute(&self, input: Value) -> Result<String, crate::error::ToolError> {
        let handle_str = input
            .get("handle")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::error::ToolError::Format("missing 'handle' parameter".into()))?;

        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        let max_bytes = input
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(20000) as usize;

        // Parse the handle with the typed parser
        let parsed = ContextHandle::parse(handle_str)
            .map_err(|e| crate::error::ToolError::Format(format!("invalid handle: {e}")))?;

        // Exact session match, not substring
        if !parsed.same_session(&self.session_id) {
            return Err(crate::error::ToolError::Permission(
                "cross-session artifact access not permitted".into(),
            ));
        }

        let artifact = self
            .store
            .get(handle_str)
            .await
            .map_err(|e| crate::error::ToolError::Execution(format!("store error: {e}")))?;

        match artifact {
            Some(artifact) => {
                let content = &artifact.redacted_content;
                let total_len = content.len();

                if offset >= total_len {
                    return Ok(format!(
                        "Artifact handle: {}\nTool: {}\nStatus: empty or fully consumed\nTotal bytes: {total_len}",
                        handle_str,
                        artifact.tool_name.as_deref().unwrap_or("unknown"),
                    ));
                }

                // Clamp offsets to valid UTF-8 boundaries
                let start = clamp_to_char_boundary(content, offset);
                let raw_end = (start + max_bytes).min(total_len);
                let end = clamp_to_char_boundary(content, raw_end);
                let slice = &content[start..end];
                let truncated = end < total_len;

                let mut result = format!(
                    "Artifact handle: {}\nTool: {}\nKind: {:?}\nBytes: {total_len} total, {start}-{end} shown\n",
                    handle_str,
                    artifact.tool_name.as_deref().unwrap_or("unknown"),
                    artifact.kind,
                );

                result.push_str("---\n");
                result.push_str(slice);

                if truncated {
                    result.push_str(&format!(
                        "\n---\n[truncated at {end}/{total_len} bytes. Use offset={end} to continue.]"
                    ));
                }

                Ok(result)
            }
            None => Err(crate::error::ToolError::NotFound(format!(
                "no artifact found for handle: {handle_str}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        ArtifactKind, ContextArtifact, ContextArtifactStore, InMemoryArtifactStore,
    };
    use crate::tool::Tool;
    use std::sync::Arc;

    fn make_store_with_artifact(
        handle: &str,
        session_id: &str,
        content: &str,
    ) -> Arc<dyn ContextArtifactStore> {
        let store = Arc::new(InMemoryArtifactStore::new());
        let artifact = ContextArtifact {
            handle: handle.into(),
            session_id: session_id.into(),
            turn_index: 0,
            tool_call_id: Some("c1".into()),
            tool_name: Some("bash".into()),
            kind: ArtifactKind::ToolResult,
            created_at_ms: 1000,
            content_hash: "abc".into(),
            redacted_content: content.into(),
            raw_bytes_len: content.len(),
            estimated_tokens: 10,
        };
        // Synchronously block on the async put - this is fine in sync test context
        futures::executor::block_on(store.put(artifact)).unwrap();
        store
    }

    #[tokio::test]
    async fn test_context_read_success() {
        let store = make_store_with_artifact("ctx://tool/s1/0/c1", "s1", "hello world");
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/s1/0/c1"});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("hello world"));
        assert!(result.contains("ctx://tool/s1/0/c1"));
        assert!(result.contains("Tool: bash"));
    }

    #[tokio::test]
    async fn test_context_read_missing_handle() {
        let store = Arc::new(InMemoryArtifactStore::new());
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing 'handle'"));
    }

    #[tokio::test]
    async fn test_context_read_invalid_handle_prefix() {
        let store = Arc::new(InMemoryArtifactStore::new());
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "invalid://handle"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid handle"));
    }

    #[tokio::test]
    async fn test_context_read_cross_session_denied() {
        let store = make_store_with_artifact("ctx://tool/other/0/c1", "other", "secret");
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/other/0/c1"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cross-session"));
    }

    #[tokio::test]
    async fn test_context_read_cross_session_substring_denied() {
        // "s1" must not match a handle with session "not-s1"
        let store = make_store_with_artifact("ctx://tool/not-s1/0/c1", "not-s1", "secret");
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/not-s1/0/c1"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cross-session"));
    }

    #[tokio::test]
    async fn test_context_read_not_found() {
        let store = Arc::new(InMemoryArtifactStore::new());
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/s1/0/nonexistent"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no artifact found"));
    }

    #[tokio::test]
    async fn test_context_read_truncation() {
        let long_content = "x".repeat(50000);
        let store = make_store_with_artifact("ctx://tool/s1/0/c1", "s1", &long_content);
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/s1/0/c1", "max_bytes": 100});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("truncated"));
        assert!(result.contains("100/50000"));
    }

    #[tokio::test]
    async fn test_context_read_offset() {
        let store = make_store_with_artifact("ctx://tool/s1/0/c1", "s1", "abcdef");
        let tool = ContextReadTool::new(store, "s1".into());

        let input =
            serde_json::json!({"handle": "ctx://tool/s1/0/c1", "offset": 2, "max_bytes": 3});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("cde"));
        assert!(result.contains("2-5 shown"));
    }

    #[tokio::test]
    async fn test_context_read_offset_beyond_content() {
        let store = make_store_with_artifact("ctx://tool/s1/0/c1", "s1", "short");
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/s1/0/c1", "offset": 100});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("empty or fully consumed"));
    }

    #[tokio::test]
    async fn test_context_read_non_ascii_no_panic() {
        // Chinese characters + emoji: each char is multi-byte
        let content = "你好世界🚀🎉";
        let store = make_store_with_artifact("ctx://tool/s1/0/c1", "s1", content);
        let tool = ContextReadTool::new(store, "s1".into());

        // Offset in the middle of a multi-byte char should not panic
        let input =
            serde_json::json!({"handle": "ctx://tool/s1/0/c1", "offset": 1, "max_bytes": 10});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("---"));
    }

    #[tokio::test]
    async fn test_context_read_non_ascii_truncation_hint() {
        let content = "日本語テスト".repeat(100);
        let store = make_store_with_artifact("ctx://tool/s1/0/c1", "s1", &content);
        let tool = ContextReadTool::new(store, "s1".into());

        let input = serde_json::json!({"handle": "ctx://tool/s1/0/c1", "max_bytes": 50});
        let result = tool.execute(input).await.unwrap();
        assert!(result.contains("truncated"));
        // Continuation offset should be on a char boundary
        assert!(result.contains("Use offset="));
    }

    #[test]
    fn test_context_read_tool_metadata() {
        let store = Arc::new(InMemoryArtifactStore::new());
        let tool = ContextReadTool::new(store, "s1".into());

        assert_eq!(tool.name(), "context_read");
        assert!(tool.description().contains("ctx://"));
        assert_eq!(tool.category(), crate::tool::ToolCategory::ReadOnly);

        let params = tool.parameters();
        let props = params.get("properties").unwrap();
        assert!(props.get("handle").is_some());
        assert!(props.get("offset").is_some());
        assert!(props.get("max_bytes").is_some());
    }

    #[tokio::test]
    async fn test_context_read_malformed_handle_rejected_before_store() {
        let store = Arc::new(InMemoryArtifactStore::new());
        let tool = ContextReadTool::new(store, "s1".into());

        // Missing turn_index
        let input = serde_json::json!({"handle": "ctx://tool/s1/c1"});
        let result = tool.execute(input).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid handle"));
    }
}
