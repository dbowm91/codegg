//! Content-addressed immutable storage for program source and IR.
//!
//! Content is stored by SHA-256 digest; the same content always maps
//! to the same key. Every load verifies digest and length to detect
//! tampering or corruption.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Errors from the content-addressed store.
#[derive(Debug, Error)]
pub enum ContentStoreError {
    #[error("content not found: {0}")]
    NotFound(String),

    #[error("digest mismatch: expected {expected}, got {actual}")]
    DigestMismatch { expected: String, actual: String },

    #[error("length mismatch: expected {expected}, got {actual}")]
    LengthMismatch { expected: u64, actual: u64 },

    #[error("content too large: {0} bytes (max {1})")]
    TooLarge(u64, u64),

    #[error("storage I/O error: {0}")]
    Io(String),
}

/// A content-addressed store for immutable byte blobs.
#[async_trait]
pub trait ContentAddressedStore: Send + Sync {
    /// Store content and return its digest and byte length.
    async fn put(
        &self,
        namespace: &str,
        content: &[u8],
    ) -> Result<(String, u64), ContentStoreError>;

    /// Load content by digest. Verifies digest and length on load.
    async fn get(
        &self,
        namespace: &str,
        digest: &str,
        expected_length: u64,
    ) -> Result<Vec<u8>, ContentStoreError>;

    /// Check whether content with the given digest exists.
    async fn contains(&self, namespace: &str, digest: &str) -> Result<bool, ContentStoreError>;

    /// Remove unreferenced content (best-effort; active content is
    /// never removed).
    async fn gc(
        &self,
        namespace: &str,
        retained_digests: &std::collections::HashSet<String>,
    ) -> Result<u64, ContentStoreError>;
}

/// Compute SHA-256 hex digest of content.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// In-memory content-addressed store for tests and lightweight use.
pub struct InMemoryContentStore {
    inner: Arc<tokio::sync::RwLock<InMemoryInner>>,
    max_entry_bytes: u64,
}

struct InMemoryInner {
    /// namespace -> digest -> (content, byte_length)
    data: HashMap<String, HashMap<String, (Vec<u8>, u64)>>,
}

impl InMemoryContentStore {
    pub fn new() -> Self {
        Self::with_max_entry_bytes(10 * 1024 * 1024)
    }

    pub fn with_max_entry_bytes(max_entry_bytes: u64) -> Self {
        Self {
            inner: Arc::new(tokio::sync::RwLock::new(InMemoryInner {
                data: HashMap::new(),
            })),
            max_entry_bytes,
        }
    }
}

impl Default for InMemoryContentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContentAddressedStore for InMemoryContentStore {
    async fn put(
        &self,
        namespace: &str,
        content: &[u8],
    ) -> Result<(String, u64), ContentStoreError> {
        let byte_length = content.len() as u64;
        if byte_length > self.max_entry_bytes {
            return Err(ContentStoreError::TooLarge(
                byte_length,
                self.max_entry_bytes,
            ));
        }
        let digest = sha256_hex(content);
        let mut inner = self.inner.write().await;
        let ns = inner.data.entry(namespace.to_string()).or_default();
        ns.insert(digest.clone(), (content.to_vec(), byte_length));
        Ok((digest, byte_length))
    }

    async fn get(
        &self,
        namespace: &str,
        digest: &str,
        expected_length: u64,
    ) -> Result<Vec<u8>, ContentStoreError> {
        let inner = self.inner.read().await;
        let ns = inner
            .data
            .get(namespace)
            .ok_or_else(|| ContentStoreError::NotFound(namespace.to_string()))?;
        let (content, byte_length) = ns
            .get(digest)
            .ok_or_else(|| ContentStoreError::NotFound(digest.to_string()))?;

        if *byte_length != expected_length {
            return Err(ContentStoreError::LengthMismatch {
                expected: expected_length,
                actual: *byte_length,
            });
        }

        let actual_digest = sha256_hex(content);
        if actual_digest != digest {
            return Err(ContentStoreError::DigestMismatch {
                expected: digest.to_string(),
                actual: actual_digest,
            });
        }

        Ok(content.clone())
    }

    async fn contains(&self, namespace: &str, digest: &str) -> Result<bool, ContentStoreError> {
        let inner = self.inner.read().await;
        Ok(inner
            .data
            .get(namespace)
            .map(|ns| ns.contains_key(digest))
            .unwrap_or(false))
    }

    async fn gc(
        &self,
        namespace: &str,
        retained_digests: &std::collections::HashSet<String>,
    ) -> Result<u64, ContentStoreError> {
        let mut inner = self.inner.write().await;
        if let Some(ns) = inner.data.get_mut(namespace) {
            let before = ns.len() as u64;
            ns.retain(|digest, _| retained_digests.contains(digest));
            return Ok(before - ns.len() as u64);
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get_roundtrip() {
        let store = InMemoryContentStore::new();
        let content = b"hello world";
        let (digest, len) = store.put("src", content).await.unwrap();
        assert_eq!(len, 11);
        let loaded = store.get("src", &digest, 11).await.unwrap();
        assert_eq!(loaded, content);
    }

    #[tokio::test]
    async fn digest_mismatch_detected() {
        let store = InMemoryContentStore::new();
        let (digest, _len) = store.put("src", b"data").await.unwrap();
        // Tamper with expected length
        let result = store.get("src", &digest, 999).await;
        assert!(matches!(
            result,
            Err(ContentStoreError::LengthMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn not_found_returns_error() {
        let store = InMemoryContentStore::new();
        let result = store.get("src", "nonexistent", 0).await;
        assert!(matches!(result, Err(ContentStoreError::NotFound(_))));
    }

    #[tokio::test]
    async fn contains_works() {
        let store = InMemoryContentStore::new();
        assert!(!store.contains("src", "abc").await.unwrap());
        let (digest, _) = store.put("src", b"test").await.unwrap();
        assert!(store.contains("src", &digest).await.unwrap());
    }

    #[tokio::test]
    async fn namespaces_are_isolated() {
        let store = InMemoryContentStore::new();
        let (digest, _) = store.put("src", b"same").await.unwrap();
        assert!(store.contains("src", &digest).await.unwrap());
        assert!(!store.contains("ir", &digest).await.unwrap());
    }

    #[tokio::test]
    async fn gc_removes_unretained() {
        let store = InMemoryContentStore::new();
        let (d1, _) = store.put("src", b"keep").await.unwrap();
        let (d2, _) = store.put("src", b"drop").await.unwrap();
        let mut retained = std::collections::HashSet::new();
        retained.insert(d1.clone());
        let removed = store.gc("src", &retained).await.unwrap();
        assert_eq!(removed, 1);
        assert!(store.contains("src", &d1).await.unwrap());
        assert!(!store.contains("src", &d2).await.unwrap());
    }

    #[tokio::test]
    async fn too_large_rejected() {
        let store = InMemoryContentStore::with_max_entry_bytes(10);
        let result = store.put("src", &[0u8; 11]).await;
        assert!(matches!(result, Err(ContentStoreError::TooLarge(11, 10))));
    }

    #[test]
    fn sha256_hex_deterministic() {
        let a = sha256_hex(b"hello");
        let b = sha256_hex(b"hello");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }
}
