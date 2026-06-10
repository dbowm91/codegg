use std::collections::HashMap;

use async_trait::async_trait;
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    ToolResult,
    CommandOutput,
    ReadResult,
    Diff,
    TestOutput,
    WebFetch,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextArtifact {
    pub handle: String,
    pub session_id: String,
    pub turn_index: usize,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub kind: ArtifactKind,
    pub created_at_ms: i64,
    pub content_hash: String,
    pub redacted_content: String,
    pub raw_bytes_len: usize,
    pub estimated_tokens: usize,
}

#[async_trait]
pub trait ContextArtifactStore: Send + Sync {
    async fn put(&self, artifact: ContextArtifact) -> anyhow::Result<()>;
    async fn get(&self, handle: &str) -> anyhow::Result<Option<ContextArtifact>>;
    async fn list_recent(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ContextArtifact>>;
}

pub struct InMemoryArtifactStore {
    inner: tokio::sync::RwLock<HashMap<String, ContextArtifact>>,
}

impl InMemoryArtifactStore {
    pub fn new() -> Self {
        Self {
            inner: tokio::sync::RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextArtifactStore for InMemoryArtifactStore {
    async fn put(&self, artifact: ContextArtifact) -> anyhow::Result<()> {
        let mut map = self.inner.write().await;
        map.insert(artifact.handle.clone(), artifact);
        Ok(())
    }

    async fn get(&self, handle: &str) -> anyhow::Result<Option<ContextArtifact>> {
        let map = self.inner.read().await;
        Ok(map.get(handle).cloned())
    }

    async fn list_recent(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ContextArtifact>> {
        let map = self.inner.read().await;
        let mut artifacts: Vec<ContextArtifact> = map
            .values()
            .filter(|a| a.session_id == session_id)
            .cloned()
            .collect();
        artifacts.sort_by_key(|b| std::cmp::Reverse(b.created_at_ms));
        artifacts.truncate(limit);
        Ok(artifacts)
    }
}

pub fn build_handle(session_id: &str, turn_index: usize, tool_call_id: &str) -> String {
    format!("ctx://tool/{session_id}/{turn_index}/{tool_call_id}")
}

pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    let word_count = text.split_whitespace().count();
    if word_count > 0 {
        ((word_count as f64) * 1.3).ceil() as usize
    } else {
        text.len() / 4
    }
}

pub fn compute_content_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// Stable full SHA-256 hex (64 lowercase chars) over arbitrary input bytes.
/// This is the common primitive for cache-aware context hashes (content, block ids, tool defs).
pub fn stable_hash_hex(input: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(input.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::handle::ContextHandle;

    #[test]
    fn test_build_handle_checked_valid() {
        let handle = ContextHandle::build_tool("sess123", 5, "call_abc").unwrap();
        assert_eq!(handle, "ctx://tool/sess123/5/call_abc");
    }

    #[test]
    fn test_build_handle_checked_special_chars() {
        let handle = ContextHandle::build_tool("s-1", 0, "c").unwrap();
        assert_eq!(handle, "ctx://tool/s-1/0/c");
    }

    #[test]
    fn test_build_handle_rejects_whitespace_in_session() {
        let err = ContextHandle::build_tool("s 1", 0, "c1").unwrap_err();
        assert!(matches!(
            err,
            crate::context::handle::ContextHandleError::UnsafeSegment {
                field: "session_id",
                ..
            }
        ));
    }

    #[test]
    fn test_build_handle_rejects_slash_in_tool_call_id() {
        let err = ContextHandle::build_tool("s1", 0, "c/1").unwrap_err();
        assert!(matches!(
            err,
            crate::context::handle::ContextHandleError::UnsafeSegment {
                field: "tool_call_id",
                ..
            }
        ));
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_whitespace_only() {
        assert_eq!(estimate_tokens("   "), 0);
    }

    #[test]
    fn test_estimate_tokens_single_word() {
        assert_eq!(estimate_tokens("hello"), 2);
    }

    #[test]
    fn test_estimate_tokens_multiple_words() {
        let tokens = estimate_tokens("hello world foo bar");
        assert!((5..=7).contains(&tokens));
    }

    #[test]
    fn test_estimate_tokens_long_text() {
        let text = "the quick brown fox jumps over the lazy dog";
        let tokens = estimate_tokens(text);
        assert!((10..=15).contains(&tokens));
    }

    #[test]
    fn test_compute_content_hash_deterministic() {
        let h1 = compute_content_hash("hello");
        let h2 = compute_content_hash("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_content_hash_different() {
        let h1 = compute_content_hash("hello");
        let h2 = compute_content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_stable_hash_hex_stable_across_calls() {
        let h1 = stable_hash_hex("hello world");
        let h2 = stable_hash_hex("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn test_stable_hash_hex_known_sha256_vector() {
        // SHA-256("hello") must match this exact lowercase hex (64 chars)
        let h = stable_hash_hex("hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn test_compute_content_hash_changes_with_text() {
        let h1 = compute_content_hash("foo bar baz");
        let h2 = compute_content_hash("different text here");
        assert_ne!(h1, h2);
        assert_eq!(h1.len(), 64);
        assert_eq!(h2.len(), 64);
    }

    #[test]
    fn test_artifact_kind_serde() {
        let kind = ArtifactKind::ToolResult;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"tool_result\"");
        let deserialized: ArtifactKind = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, ArtifactKind::ToolResult);
    }

    #[test]
    fn test_artifact_kind_all_variants() {
        let variants = vec![
            ArtifactKind::ToolResult,
            ArtifactKind::CommandOutput,
            ArtifactKind::ReadResult,
            ArtifactKind::Diff,
            ArtifactKind::TestOutput,
            ArtifactKind::WebFetch,
            ArtifactKind::Image,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let back: ArtifactKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn test_context_artifact_roundtrip() {
        let artifact = ContextArtifact {
            handle: "ctx://tool/s1/0/c1".into(),
            session_id: "s1".into(),
            turn_index: 0,
            tool_call_id: Some("c1".into()),
            tool_name: Some("bash".into()),
            kind: ArtifactKind::ToolResult,
            created_at_ms: 1000,
            content_hash: "abc123".into(),
            redacted_content: "output".into(),
            raw_bytes_len: 6,
            estimated_tokens: 2,
        };
        let json = serde_json::to_string(&artifact).unwrap();
        let back: ContextArtifact = serde_json::from_str(&json).unwrap();
        assert_eq!(back.handle, artifact.handle);
        assert_eq!(back.session_id, artifact.session_id);
        assert_eq!(back.kind, artifact.kind);
    }

    #[tokio::test]
    async fn test_in_memory_store_put_and_get() {
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
        let got = store.get("ctx://tool/s1/0/c1").await.unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().handle, "ctx://tool/s1/0/c1");
    }

    #[tokio::test]
    async fn test_in_memory_store_get_missing() {
        let store = InMemoryArtifactStore::new();
        let got = store.get("nonexistent").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn test_in_memory_store_list_recent() {
        let store = InMemoryArtifactStore::new();
        for i in 0..5 {
            let artifact = ContextArtifact {
                handle: format!("ctx://tool/s1/{i}/c{i}"),
                session_id: "s1".into(),
                turn_index: i,
                tool_call_id: Some(format!("c{i}")),
                tool_name: Some("bash".into()),
                kind: ArtifactKind::ToolResult,
                created_at_ms: (i as i64) * 1000,
                content_hash: "abc".into(),
                redacted_content: "output".into(),
                raw_bytes_len: 6,
                estimated_tokens: 2,
            };
            store.put(artifact).await.unwrap();
        }
        let results = store.list_recent("s1", 3).await.unwrap();
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].turn_index, 4);
        assert_eq!(results[1].turn_index, 3);
        assert_eq!(results[2].turn_index, 2);
    }

    #[tokio::test]
    async fn test_in_memory_store_list_recent_filters_session() {
        let store = InMemoryArtifactStore::new();
        store
            .put(ContextArtifact {
                handle: "ctx://tool/s1/0/c0".into(),
                session_id: "s1".into(),
                turn_index: 0,
                tool_call_id: Some("c0".into()),
                tool_name: None,
                kind: ArtifactKind::ToolResult,
                created_at_ms: 1000,
                content_hash: "abc".into(),
                redacted_content: String::new(),
                raw_bytes_len: 0,
                estimated_tokens: 0,
            })
            .await
            .unwrap();
        store
            .put(ContextArtifact {
                handle: "ctx://tool/s2/0/c0".into(),
                session_id: "s2".into(),
                turn_index: 0,
                tool_call_id: Some("c0".into()),
                tool_name: None,
                kind: ArtifactKind::ToolResult,
                created_at_ms: 2000,
                content_hash: "abc".into(),
                redacted_content: String::new(),
                raw_bytes_len: 0,
                estimated_tokens: 0,
            })
            .await
            .unwrap();
        let results = store.list_recent("s1", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].session_id, "s1");
    }

    #[tokio::test]
    async fn test_in_memory_store_overwrite() {
        let store = InMemoryArtifactStore::new();
        store
            .put(ContextArtifact {
                handle: "ctx://tool/s1/0/c0".into(),
                session_id: "s1".into(),
                turn_index: 0,
                tool_call_id: Some("c0".into()),
                tool_name: Some("bash".into()),
                kind: ArtifactKind::ToolResult,
                created_at_ms: 1000,
                content_hash: "abc".into(),
                redacted_content: "old".into(),
                raw_bytes_len: 3,
                estimated_tokens: 1,
            })
            .await
            .unwrap();
        store
            .put(ContextArtifact {
                handle: "ctx://tool/s1/0/c0".into(),
                session_id: "s1".into(),
                turn_index: 0,
                tool_call_id: Some("c0".into()),
                tool_name: Some("bash".into()),
                kind: ArtifactKind::ToolResult,
                created_at_ms: 2000,
                content_hash: "def".into(),
                redacted_content: "new".into(),
                raw_bytes_len: 3,
                estimated_tokens: 1,
            })
            .await
            .unwrap();
        let got = store.get("ctx://tool/s1/0/c0").await.unwrap().unwrap();
        assert_eq!(got.redacted_content, "new");
    }

    // --- Phase 3: Failing mock store ---

    struct FailingStore;

    #[async_trait]
    impl ContextArtifactStore for FailingStore {
        async fn put(&self, _artifact: ContextArtifact) -> anyhow::Result<()> {
            anyhow::bail!("simulated store failure")
        }
        async fn get(&self, _handle: &str) -> anyhow::Result<Option<ContextArtifact>> {
            anyhow::bail!("simulated store failure")
        }
        async fn list_recent(
            &self,
            _session_id: &str,
            _limit: usize,
        ) -> anyhow::Result<Vec<ContextArtifact>> {
            anyhow::bail!("simulated store failure")
        }
    }

    #[tokio::test]
    async fn test_failing_store_returns_error() {
        let store = FailingStore;
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
        let result = store.put(artifact).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("simulated store failure"));
    }

    #[test]
    fn test_estimate_tokens_paragraph() {
        let text = "This is a longer piece of text that should be used to test the token estimation heuristic. It has many words and should produce a reasonable token count.";
        let tokens = estimate_tokens(text);
        assert!((20..=40).contains(&tokens));
    }
}
