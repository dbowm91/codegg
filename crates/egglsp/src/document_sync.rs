//! Open document tracking for LSP file synchronization.
//!
//! Maintains authoritative state of currently open documents per client
//! key. Used for document replay after server restart and for health
//! snapshots. The registry reflects successful service-level
//! open/update/save/close operations.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use url::Url;

/// Snapshot of a single open document's state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum OpenDocumentSource {
    #[default]
    Disk,
    Managed,
}

/// Snapshot of a single open document's state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenDocumentSnapshot {
    pub uri: Url,
    pub language_id: String,
    pub version: i32,
    pub text: String,
    pub dirty: bool,
    /// Which layer owns synchronized text. Managed text must not be refreshed
    /// from disk by disk-oriented semantic helpers.
    #[serde(default)]
    pub source: OpenDocumentSource,
    /// Set before sending managed text to the server and cleared only after
    /// didOpen/didChange succeeds. Semantic helpers retry stale mirrors.
    #[serde(default)]
    pub sync_stale: bool,
}

/// Authoritative registry of open documents per client key.
#[derive(Debug, Clone, Default)]
pub struct OpenDocumentRegistry {
    /// client_key -> (uri -> snapshot)
    documents: Arc<RwLock<HashMap<String, HashMap<Url, OpenDocumentSnapshot>>>>,
}

impl OpenDocumentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a document open event.
    pub async fn open(
        &self,
        client_key: &str,
        uri: Url,
        language_id: impl Into<String>,
        version: i32,
        text: impl Into<String>,
    ) {
        let lang = language_id.into();
        let txt = text.into();
        let mut docs = self.documents.write().await;
        docs.entry(client_key.to_string()).or_default().insert(
            uri.clone(),
            OpenDocumentSnapshot {
                uri,
                language_id: lang,
                version,
                text: txt,
                dirty: false,
                source: OpenDocumentSource::Disk,
                sync_stale: false,
            },
        );
    }

    /// Mark a successfully opened or updated registry entry editor-managed.
    pub async fn mark_managed(&self, client_key: &str, uri: &Url) {
        let mut docs = self.documents.write().await;
        if let Some(entry) = docs.get_mut(client_key).and_then(|m| m.get_mut(uri)) {
            entry.source = OpenDocumentSource::Managed;
        }
    }

    pub async fn open_managed(
        &self,
        client_key: &str,
        uri: Url,
        language_id: impl Into<String>,
        version: i32,
        text: impl Into<String>,
        dirty: bool,
    ) {
        let mut docs = self.documents.write().await;
        docs.entry(client_key.to_string()).or_default().insert(
            uri.clone(),
            OpenDocumentSnapshot {
                uri,
                language_id: language_id.into(),
                version,
                text: text.into(),
                dirty,
                source: OpenDocumentSource::Managed,
                sync_stale: true,
            },
        );
    }

    pub async fn change_managed(
        &self,
        client_key: &str,
        uri: &Url,
        version: i32,
        text: impl Into<String>,
        dirty: bool,
    ) {
        let mut docs = self.documents.write().await;
        if let Some(entry) = docs.get_mut(client_key).and_then(|m| m.get_mut(uri)) {
            entry.version = version;
            entry.text = text.into();
            entry.dirty = dirty;
            entry.source = OpenDocumentSource::Managed;
            entry.sync_stale = true;
        }
    }

    pub async fn managed_snapshot(
        &self,
        client_key: &str,
        uri: &Url,
    ) -> Option<OpenDocumentSnapshot> {
        let docs = self.documents.read().await;
        docs.get(client_key)
            .and_then(|m| m.get(uri))
            .filter(|entry| entry.source == OpenDocumentSource::Managed)
            .cloned()
    }

    pub async fn set_sync_stale(&self, client_key: &str, uri: &Url, stale: bool) {
        let mut docs = self.documents.write().await;
        if let Some(entry) = docs.get_mut(client_key).and_then(|m| m.get_mut(uri)) {
            if entry.source == OpenDocumentSource::Managed {
                entry.sync_stale = stale;
            }
        }
    }

    pub async fn source(&self, client_key: &str, uri: &Url) -> Option<OpenDocumentSource> {
        let docs = self.documents.read().await;
        docs.get(client_key)
            .and_then(|m| m.get(uri))
            .map(|entry| entry.source)
    }

    pub async fn is_managed_dirty(&self, client_key: &str, uri: &Url) -> bool {
        let docs = self.documents.read().await;
        docs.get(client_key)
            .and_then(|m| m.get(uri))
            .is_some_and(|entry| entry.source == OpenDocumentSource::Managed && entry.dirty)
    }

    /// Record a document change.
    pub async fn change(&self, client_key: &str, uri: &Url, version: i32, text: impl Into<String>) {
        let mut docs = self.documents.write().await;
        if let Some(entry) = docs.get_mut(client_key).and_then(|m| m.get_mut(uri)) {
            entry.version = version;
            entry.text = text.into();
            entry.dirty = true;
        }
    }

    /// Record a document save.
    pub async fn save(&self, client_key: &str, uri: &Url) {
        let mut docs = self.documents.write().await;
        if let Some(entry) = docs.get_mut(client_key).and_then(|m| m.get_mut(uri)) {
            entry.dirty = false;
        }
    }

    /// Record a document close.
    pub async fn close(&self, client_key: &str, uri: &Url) {
        let mut docs = self.documents.write().await;
        if let Some(client_docs) = docs.get_mut(client_key) {
            client_docs.remove(uri);
        }
    }

    /// Get all currently open documents for a client key.
    pub async fn open_documents(&self, client_key: &str) -> Vec<OpenDocumentSnapshot> {
        let docs = self.documents.read().await;
        docs.get(client_key)
            .map(|m| m.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Get the count of open documents for a client key.
    pub async fn document_count(&self, client_key: &str) -> usize {
        let docs = self.documents.read().await;
        docs.get(client_key).map(|m| m.len()).unwrap_or(0)
    }

    /// Clear all documents for a client key (e.g. on terminal failure).
    pub async fn clear_client(&self, client_key: &str) {
        let mut docs = self.documents.write().await;
        docs.remove(client_key);
    }

    /// Clear all documents across all clients.
    pub async fn clear_all(&self) {
        let mut docs = self.documents.write().await;
        docs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri(path: &str) -> Url {
        Url::parse(&format!("file://{path}")).unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn open_and_retrieve() {
        let reg = OpenDocumentRegistry::new();
        let uri = test_uri("/tmp/test.py");
        reg.open("client1", uri.clone(), "python", 1, "print('hello')")
            .await;
        let docs = reg.open_documents("client1").await;
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].language_id, "python");
        assert_eq!(docs[0].version, 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn managed_source_survives_change_save_and_replay_snapshot() {
        let reg = OpenDocumentRegistry::new();
        let uri = test_uri("/tmp/managed.py");
        reg.open("client", uri.clone(), "py", 1, "unsaved").await;
        assert_eq!(
            reg.source("client", &uri).await,
            Some(OpenDocumentSource::Disk)
        );
        reg.mark_managed("client", &uri).await;
        reg.change("client", &uri, 2, "new unsaved").await;
        reg.save("client", &uri).await;
        let replay = reg.open_documents("client").await;
        assert_eq!(replay[0].source, OpenDocumentSource::Managed);
        assert_eq!(replay[0].text, "new unsaved");
        assert!(!replay[0].dirty);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn change_updates_text_and_version() {
        let reg = OpenDocumentRegistry::new();
        let uri = test_uri("/tmp/test.py");
        reg.open("c", uri.clone(), "python", 1, "v1").await;
        reg.change("c", &uri, 2, "v2").await;
        let docs = reg.open_documents("c").await;
        assert_eq!(docs[0].text, "v2");
        assert_eq!(docs[0].version, 2);
        assert!(docs[0].dirty);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn save_clears_dirty() {
        let reg = OpenDocumentRegistry::new();
        let uri = test_uri("/tmp/test.py");
        reg.open("c", uri.clone(), "python", 1, "v1").await;
        reg.change("c", &uri, 2, "v2").await;
        reg.save("c", &uri).await;
        let docs = reg.open_documents("c").await;
        assert!(!docs[0].dirty);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn close_removes_document() {
        let reg = OpenDocumentRegistry::new();
        let uri = test_uri("/tmp/test.py");
        reg.open("c", uri.clone(), "python", 1, "v1").await;
        assert_eq!(reg.document_count("c").await, 1);
        reg.close("c", &uri).await;
        assert_eq!(reg.document_count("c").await, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clear_client_removes_all_for_key() {
        let reg = OpenDocumentRegistry::new();
        let u1 = test_uri("/tmp/a.py");
        let u2 = test_uri("/tmp/b.py");
        reg.open("c", u1, "python", 1, "a").await;
        reg.open("c", u2, "python", 1, "b").await;
        assert_eq!(reg.document_count("c").await, 2);
        reg.clear_client("c").await;
        assert_eq!(reg.document_count("c").await, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clients_are_independent() {
        let reg = OpenDocumentRegistry::new();
        let uri = test_uri("/tmp/test.py");
        reg.open("c1", uri.clone(), "python", 1, "v1").await;
        reg.open("c2", uri, "python", 1, "v2").await;
        assert_eq!(reg.document_count("c1").await, 1);
        assert_eq!(reg.document_count("c2").await, 1);
        reg.clear_client("c1").await;
        assert_eq!(reg.document_count("c1").await, 0);
        assert_eq!(reg.document_count("c2").await, 1);
    }
}
