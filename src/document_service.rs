//! Process-local daemon authority for open editor documents.
//!
//! Text is owned by `codegg-document`; this service owns scoped identity,
//! attachments, the single-writer lease, bounds, and disk-base metadata.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Weak};

use codegg_document::{DocumentBuffer, DocumentLimits, TextEdit, TextTransaction};
use codegg_protocol::document::{DocumentSnapshotDto, DocumentTransactionDto};
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

const MAX_DOCUMENTS: usize = 128;
const MAX_DIRTY_ORPHANS: usize = 64;
const MAX_RESIDENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_ATTACHMENTS: usize = 32;
const MAX_DEDUPE: usize = 128;

#[derive(Debug, thiserror::Error)]
pub enum DocumentServiceError {
    #[error("document not found")]
    NotFound,
    #[error("invalid relative document path")]
    InvalidPath,
    #[error("document is not valid UTF-8 text")]
    InvalidText,
    #[error("document exceeds the configured size limit")]
    ResourceLimit,
    #[error("document already has an active writer")]
    WriterBusy,
    #[error("writer lease is stale")]
    StaleLease,
    #[error("base document revision is stale")]
    StaleRevision,
    #[error("change id was reused with a different payload")]
    ChangeCollision,
    #[error("document save/reload is not enabled until checked disk mutation is installed")]
    SaveNotReady,
    #[error("document service lock was poisoned")]
    LockPoisoned,
    #[error(transparent)]
    Text(#[from] codegg_document::DocumentError),
    #[error("document I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone)]
struct DocumentKey {
    project_id: String,
    workspace_id: String,
    relative_path: String,
}

struct OpenDocument {
    id: String,
    key: DocumentKey,
    workspace_root: PathBuf,
    buffer: DocumentBuffer,
    base_digest: String,
    dirty: bool,
    conflicted: bool,
    readers: HashMap<String, String>,
    writer: Option<(String, String)>,
    recent: HashMap<String, (String, u64)>,
    operation: Arc<Mutex<()>>,
}

impl OpenDocument {
    fn snapshot(&self, client: &str) -> DocumentSnapshotDto {
        DocumentSnapshotDto {
            document_id: self.id.clone(),
            project_id: self.key.project_id.clone(),
            workspace_id: self.key.workspace_id.clone(),
            relative_path: self.key.relative_path.clone(),
            revision: self.buffer.revision().get(),
            text: self.buffer.snapshot().to_string(),
            dirty: self.dirty,
            conflicted: self.conflicted,
            writer: self
                .writer
                .as_ref()
                .is_some_and(|(owner, _)| owner == client),
            disk_base_digest: self.base_digest.clone(),
        }
    }
}

#[derive(Default)]
pub struct DocumentService {
    by_id: DashMap<String, Arc<Mutex<OpenDocument>>>,
    by_key: DashMap<String, String>,
    key_locks: DashMap<String, Weak<Mutex<()>>>,
}

/// Holds per-document operation gates while an external mutation checks and
/// updates disk. This prevents an accepted editor transaction from racing
/// through the clean-buffer preview check.
pub struct DocumentOperationGuards {
    _guards: Vec<tokio::sync::OwnedMutexGuard<()>>,
}

impl DocumentService {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn open(
        &self,
        project_id: String,
        workspace_id: String,
        root: &Path,
        relative_path: &str,
        client_id: &str,
        writable: bool,
    ) -> Result<(DocumentSnapshotDto, Option<String>), DocumentServiceError> {
        let relative = validate_relative(relative_path)?;
        let key_string = format!("{project_id}\0{workspace_id}\0{}", relative.display());
        let lock = self.key_lock(&key_string)?;
        let _guard = lock.lock().await;
        if let Some(id) = self.by_key.get(&key_string).map(|id| id.value().clone()) {
            let Some(entry) = self.by_id.get(&id).map(|doc| doc.value().clone()) else {
                self.by_key.remove(&key_string);
                return Err(DocumentServiceError::NotFound);
            };
            let mut doc = entry.lock().await;
            attach(&mut doc, client_id, writable)?;
            let lease = doc
                .writer
                .as_ref()
                .filter(|(owner, _)| owner == client_id)
                .map(|(_, lease)| lease.clone());
            return Ok((doc.snapshot(client_id), lease));
        }
        if self.by_id.len() >= MAX_DOCUMENTS {
            return Err(DocumentServiceError::ResourceLimit);
        }
        if self.resident_bytes().await > MAX_RESIDENT_BYTES {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let canonical_root = tokio::fs::canonicalize(root).await?;
        let target = canonical_root.join(&relative);
        reject_symlink_components(&canonical_root, &target)?;
        let canonical_target = tokio::fs::canonicalize(&target).await?;
        if !canonical_target.starts_with(&canonical_root) || !canonical_target.is_file() {
            return Err(DocumentServiceError::InvalidPath);
        }
        if tokio::fs::metadata(&canonical_target).await?.len()
            > codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES as u64
        {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let bytes = tokio::fs::read(&canonical_target).await?;
        if bytes.len() > codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES {
            return Err(DocumentServiceError::ResourceLimit);
        }
        if self.resident_bytes().await.saturating_add(bytes.len()) > MAX_RESIDENT_BYTES {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let text =
            String::from_utf8(bytes.clone()).map_err(|_| DocumentServiceError::InvalidText)?;
        let id = format!("doc_{}", uuid::Uuid::new_v4().simple());
        let base_digest = digest(&bytes);
        let mut doc = OpenDocument {
            id: id.clone(),
            key: DocumentKey {
                project_id,
                workspace_id,
                relative_path: relative.to_string_lossy().into_owned(),
            },
            workspace_root: canonical_root,
            buffer: DocumentBuffer::new(text),
            base_digest,
            dirty: false,
            conflicted: false,
            readers: HashMap::new(),
            writer: None,
            recent: HashMap::new(),
            operation: Arc::new(Mutex::new(())),
        };
        attach(&mut doc, client_id, writable)?;
        let lease = doc.writer.as_ref().map(|(_, lease)| lease.clone());
        let snapshot = doc.snapshot(client_id);
        self.by_id.insert(id.clone(), Arc::new(Mutex::new(doc)));
        self.by_key.insert(key_string, id);
        Ok((snapshot, lease))
    }

    pub async fn snapshot(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
    ) -> Result<DocumentSnapshotDto, DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc.readers.contains_key(client) {
            return Err(DocumentServiceError::NotFound);
        }
        Ok(doc.snapshot(client))
    }

    pub async fn status(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
    ) -> Result<(u64, bool, bool, bool), DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let doc = entry.lock().await;
        if doc.key.project_id != project_id || !doc.readers.contains_key(client) {
            return Err(DocumentServiceError::NotFound);
        }
        Ok((
            doc.buffer.revision().get(),
            doc.dirty,
            doc.conflicted,
            doc.writer
                .as_ref()
                .is_some_and(|(owner, _)| owner == client),
        ))
    }

    pub async fn acquire_writer(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
    ) -> Result<String, DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let mut doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc.readers.contains_key(client) {
            return Err(DocumentServiceError::NotFound);
        }
        if let Some((owner, lease)) = &doc.writer {
            return if owner == client {
                Ok(lease.clone())
            } else {
                Err(DocumentServiceError::WriterBusy)
            };
        }
        let lease = uuid::Uuid::new_v4().to_string();
        doc.writer = Some((client.to_owned(), lease.clone()));
        Ok(lease)
    }

    pub async fn detach_document(&self, id: &str, project_id: &str, client: &str) -> bool {
        if let Some(entry) = self.by_id.get(id).map(|doc| doc.value().clone()) {
            let mut doc = entry.lock().await;
            if doc.key.project_id != project_id {
                return false;
            }
            doc.readers.remove(client);
            if doc
                .writer
                .as_ref()
                .is_some_and(|(owner, _)| owner == client)
            {
                doc.writer = None;
            }
            let evict = !doc.dirty && doc.readers.is_empty() && doc.writer.is_none();
            if evict {
                let key = format!(
                    "{}\0{}\0{}",
                    doc.key.project_id, doc.key.workspace_id, doc.key.relative_path
                );
                drop(doc);
                self.by_id.remove(id);
                self.by_key.remove(&key);
                return true;
            }
            return false;
        }
        false
    }

    pub async fn change(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
        lease: &str,
        base_revision: u64,
        change_id: &str,
        dto: DocumentTransactionDto,
    ) -> Result<u64, DocumentServiceError> {
        if change_id.is_empty() || change_id.len() > 128 {
            return Err(DocumentServiceError::ChangeCollision);
        }
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let operation = entry.lock().await.operation.clone();
        let _operation = operation.lock_owned().await;
        let mut doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc
            .writer
            .as_ref()
            .is_some_and(|(owner, token)| owner == client && token == lease)
        {
            return Err(DocumentServiceError::StaleLease);
        }
        let payload =
            serde_json::to_vec(&dto).map_err(|_| DocumentServiceError::ChangeCollision)?;
        let change_digest = digest(&payload);
        if let Some((old_digest, revision)) = doc.recent.get(change_id) {
            return if old_digest == &change_digest {
                Ok(*revision)
            } else {
                Err(DocumentServiceError::ChangeCollision)
            };
        }
        if doc.buffer.revision().get() != base_revision {
            return Err(DocumentServiceError::StaleRevision);
        }
        if !doc.dirty
            && !dto.edits.is_empty()
            && self.dirty_document_count_excluding(id).await >= MAX_DIRTY_ORPHANS
        {
            return Err(DocumentServiceError::ResourceLimit);
        }
        if dto.edits.len() > codegg_protocol::document::MAX_DOCUMENT_EDITS {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let inserted = dto
            .edits
            .iter()
            .map(|edit| edit.insert.len())
            .sum::<usize>();
        let removed = dto
            .edits
            .iter()
            .map(|edit| edit.range.end.saturating_sub(edit.range.start))
            .sum::<usize>();
        let old_len = doc.buffer.snapshot().len_bytes();
        let projected = old_len.saturating_sub(removed).saturating_add(inserted);
        if self
            .resident_bytes_excluding(id)
            .await
            .saturating_add(projected)
            > MAX_RESIDENT_BYTES
        {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let edits = dto
            .edits
            .into_iter()
            .map(|edit| TextEdit::new(edit.range.start..edit.range.end, edit.insert))
            .collect();
        let applied = doc.buffer.apply(
            &TextTransaction::new(edits),
            DocumentLimits {
                max_document_bytes: codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES,
                max_edits: codegg_protocol::document::MAX_DOCUMENT_EDITS,
                max_inserted_bytes: codegg_protocol::document::MAX_DOCUMENT_INSERT_BYTES,
            },
        )?;
        doc.dirty |= applied.revision.get() != base_revision;
        let revision = applied.revision.get();
        if doc.recent.len() >= MAX_DEDUPE {
            if let Some(oldest) = doc.recent.keys().next().cloned() {
                doc.recent.remove(&oldest);
            }
        }
        doc.recent
            .insert(change_id.to_owned(), (change_digest, revision));
        Ok(revision)
    }

    pub async fn save_or_reload_not_ready(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
        lease: &str,
    ) -> Result<(), DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc
            .writer
            .as_ref()
            .is_some_and(|(owner, token)| owner == client && token == lease)
        {
            return Err(DocumentServiceError::StaleLease);
        }
        Err(DocumentServiceError::SaveNotReady)
    }

    /// Lock changes, saves, and reloads for one document across disk I/O.
    pub async fn operation_lock(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
        lease: &str,
    ) -> Result<Arc<Mutex<()>>, DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc
            .writer
            .as_ref()
            .is_some_and(|(owner, token)| owner == client && token == lease)
        {
            return Err(DocumentServiceError::StaleLease);
        }
        Ok(doc.operation.clone())
    }

    pub async fn lock_clean_paths(
        &self,
        workspace_id: &str,
        relative_paths: &[String],
    ) -> Result<DocumentOperationGuards, String> {
        let paths: std::collections::HashSet<&str> =
            relative_paths.iter().map(String::as_str).collect();
        let documents = self
            .by_id
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        for (id, entry) in documents {
            let doc = entry.lock().await;
            if doc.key.workspace_id == workspace_id
                && paths.contains(doc.key.relative_path.as_str())
            {
                candidates.push((id, entry.clone(), doc.operation.clone()));
            }
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        let mut guards = Vec::with_capacity(candidates.len());
        for (_, _, operation) in &candidates {
            guards.push(operation.clone().lock_owned().await);
        }
        for (id, entry, _) in &candidates {
            if entry.lock().await.dirty {
                return Err(format!(
                    "{id} has unsaved editor changes; save and regenerate the preview"
                ));
            }
        }
        Ok(DocumentOperationGuards { _guards: guards })
    }

    pub async fn capture_save(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
        lease: &str,
        expected_revision: u64,
    ) -> Result<DocumentSaveSnapshot, DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc
            .writer
            .as_ref()
            .is_some_and(|(owner, token)| owner == client && token == lease)
        {
            return Err(DocumentServiceError::StaleLease);
        }
        if doc.buffer.revision().get() != expected_revision {
            return Err(DocumentServiceError::StaleRevision);
        }
        Ok(DocumentSaveSnapshot {
            document_id: doc.id.clone(),
            project_id: doc.key.project_id.clone(),
            workspace_id: doc.key.workspace_id.clone(),
            path: doc.workspace_root.join(&doc.key.relative_path),
            relative_path: doc.key.relative_path.clone(),
            revision: expected_revision,
            expected_disk_digest: doc.base_digest.clone(),
            text: doc.buffer.snapshot().to_string(),
        })
    }

    pub async fn commit_save(
        &self,
        id: &str,
        project_id: &str,
        saved_revision: u64,
        new_disk_digest: String,
    ) -> Result<bool, DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let mut doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        doc.base_digest = new_disk_digest;
        let saved_current_revision = doc.buffer.revision().get() == saved_revision;
        doc.dirty = !saved_current_revision;
        doc.conflicted = false;
        Ok(saved_current_revision)
    }

    pub async fn mark_disk_conflict(
        &self,
        id: &str,
        project_id: &str,
        actual_disk_digest: &str,
    ) -> Result<u64, DocumentServiceError> {
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let mut doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        doc.conflicted = actual_disk_digest != doc.base_digest;
        Ok(doc.buffer.revision().get())
    }

    /// Reconcile a disk change at an explicit snapshot boundary. A clean
    /// buffer advances revision to the verified disk text; a dirty buffer is
    /// preserved and marked conflicted.
    pub async fn refresh_clean_from_disk(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
        expected_revision: u64,
        text: String,
        disk_digest: String,
    ) -> Result<Option<u64>, DocumentServiceError> {
        if text.len() > codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let mut doc = entry.lock().await;
        if doc.key.project_id != project_id || !doc.readers.contains_key(client) {
            return Err(DocumentServiceError::NotFound);
        }
        if doc.buffer.revision().get() != expected_revision {
            return Err(DocumentServiceError::StaleRevision);
        }
        if disk_digest == doc.base_digest {
            return Ok(None);
        }
        if doc.dirty {
            doc.conflicted = true;
            return Ok(None);
        }
        let snapshot = doc.buffer.replace_text(text)?;
        doc.base_digest = disk_digest;
        doc.dirty = false;
        doc.conflicted = false;
        Ok(Some(snapshot.revision().get()))
    }

    pub async fn reload_verified(
        &self,
        id: &str,
        project_id: &str,
        client: &str,
        lease: &str,
        expected_revision: u64,
        text: String,
        disk_digest: String,
    ) -> Result<u64, DocumentServiceError> {
        if text.len() > codegg_protocol::document::MAX_DOCUMENT_TEXT_BYTES {
            return Err(DocumentServiceError::ResourceLimit);
        }
        let entry = self
            .by_id
            .get(id)
            .map(|doc| doc.value().clone())
            .ok_or(DocumentServiceError::NotFound)?;
        let mut doc = entry.lock().await;
        if doc.key.project_id != project_id {
            return Err(DocumentServiceError::NotFound);
        }
        if !doc
            .writer
            .as_ref()
            .is_some_and(|(owner, token)| owner == client && token == lease)
        {
            return Err(DocumentServiceError::StaleLease);
        }
        if doc.buffer.revision().get() != expected_revision {
            return Err(DocumentServiceError::StaleRevision);
        }
        let snapshot = doc.buffer.replace_text(text)?;
        doc.base_digest = disk_digest;
        doc.dirty = false;
        doc.conflicted = false;
        Ok(snapshot.revision().get())
    }

    pub async fn detach_client(&self, client_id: &str) {
        let documents = self
            .by_id
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        for entry in documents {
            let mut doc = entry.lock().await;
            doc.readers.remove(client_id);
            if doc
                .writer
                .as_ref()
                .is_some_and(|(owner, _)| owner == client_id)
            {
                doc.writer = None;
            }
        }
        self.evict_clean_unattached().await;
    }

    async fn evict_clean_unattached(&self) {
        let mut remove = Vec::new();
        for item in self.by_id.iter() {
            let doc = item.value().lock().await;
            if !doc.dirty && doc.readers.is_empty() && doc.writer.is_none() {
                remove.push((item.key().clone(), doc.key.clone()));
            }
        }
        for (id, key) in remove {
            self.by_id.remove(&id);
            self.by_key.remove(&format!(
                "{}\0{}\0{}",
                key.project_id, key.workspace_id, key.relative_path
            ));
        }
    }

    async fn dirty_document_count_excluding(&self, excluded: &str) -> usize {
        let documents = self
            .by_id
            .iter()
            .filter(|entry| entry.key() != excluded)
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        let mut count = 0;
        for entry in documents {
            let doc = entry.lock().await;
            count += usize::from(doc.dirty);
        }
        count
    }

    async fn resident_bytes(&self) -> usize {
        self.resident_bytes_excluding("").await
    }

    async fn resident_bytes_excluding(&self, excluded: &str) -> usize {
        let documents = self
            .by_id
            .iter()
            .filter(|entry| entry.key() != excluded)
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        let mut bytes = 0usize;
        for entry in documents {
            bytes = bytes.saturating_add(entry.lock().await.buffer.snapshot().len_bytes());
        }
        bytes
    }

    fn key_lock(&self, key: &str) -> Result<Arc<Mutex<()>>, DocumentServiceError> {
        if self.key_locks.len() >= MAX_DOCUMENTS * 2 {
            self.key_locks.retain(|_, weak| weak.strong_count() > 0);
            if self.key_locks.len() >= MAX_DOCUMENTS * 2 && !self.key_locks.contains_key(key) {
                return Err(DocumentServiceError::ResourceLimit);
            }
        }
        match self.key_locks.entry(key.to_owned()) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                if let Some(lock) = entry.get().upgrade() {
                    Ok(lock)
                } else {
                    let lock = Arc::new(Mutex::new(()));
                    entry.insert(Arc::downgrade(&lock));
                    Ok(lock)
                }
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let lock = Arc::new(Mutex::new(()));
                entry.insert(Arc::downgrade(&lock));
                Ok(lock)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocumentSaveSnapshot {
    pub document_id: String,
    pub project_id: String,
    pub workspace_id: String,
    pub path: PathBuf,
    pub relative_path: String,
    pub revision: u64,
    pub expected_disk_digest: String,
    pub text: String,
}

fn attach(
    doc: &mut OpenDocument,
    client: &str,
    writable: bool,
) -> Result<(), DocumentServiceError> {
    if doc.readers.len() >= MAX_ATTACHMENTS {
        return Err(DocumentServiceError::ResourceLimit);
    }
    let attachment = doc
        .readers
        .entry(client.to_owned())
        .or_insert_with(|| uuid::Uuid::new_v4().to_string())
        .clone();
    let _ = attachment;
    if writable {
        match &doc.writer {
            Some((owner, _)) if owner != client => return Err(DocumentServiceError::WriterBusy),
            Some((_, lease)) => {
                doc.writer = Some((client.to_owned(), lease.clone()));
            }
            None => doc.writer = Some((client.to_owned(), uuid::Uuid::new_v4().to_string())),
        }
    }
    Ok(())
}

fn validate_relative(raw: &str) -> Result<PathBuf, DocumentServiceError> {
    let path = Path::new(raw);
    if raw.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(DocumentServiceError::InvalidPath);
    }
    Ok(path.to_path_buf())
}

fn reject_symlink_components(root: &Path, target: &Path) -> Result<(), DocumentServiceError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| DocumentServiceError::InvalidPath)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if std::fs::symlink_metadata(&current)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(DocumentServiceError::InvalidPath);
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_protocol::document::{DocumentTextEditDto, DocumentTextRangeDto};

    #[tokio::test]
    async fn one_writer_dedupe_stale_revision_and_disconnect_reacquire() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("main.rs"), "fn main() {}\n")
            .await
            .unwrap();
        let service = DocumentService::new();
        let (writer, lease) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "main.rs",
                "client-a",
                true,
            )
            .await
            .unwrap();
        let lease = lease.unwrap();
        let (observer, _) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "main.rs",
                "client-b",
                false,
            )
            .await
            .unwrap();
        assert_eq!(writer.document_id, observer.document_id);
        assert!(matches!(
            service
                .open(
                    "p".into(),
                    "w".into(),
                    temp.path(),
                    "main.rs",
                    "client-b",
                    true
                )
                .await,
            Err(DocumentServiceError::WriterBusy)
        ));
        let tx = DocumentTransactionDto {
            edits: vec![DocumentTextEditDto {
                range: DocumentTextRangeDto { start: 13, end: 13 },
                insert: "// hi\n".into(),
            }],
        };
        let revision = service
            .change(
                &writer.document_id,
                "p",
                "client-a",
                &lease,
                0,
                "change-1",
                tx.clone(),
            )
            .await
            .unwrap();
        assert_eq!(revision, 1);
        assert!(matches!(
            service
                .change(
                    &writer.document_id,
                    "p",
                    "client-a",
                    &lease,
                    0,
                    "stale",
                    tx.clone()
                )
                .await,
            Err(DocumentServiceError::StaleRevision)
        ));
        assert_eq!(
            service
                .change(
                    &writer.document_id,
                    "p",
                    "client-a",
                    &lease,
                    0,
                    "change-1",
                    tx.clone()
                )
                .await
                .unwrap(),
            1
        );
        assert!(matches!(
            service
                .snapshot(&writer.document_id, "other-project", "client-b")
                .await,
            Err(DocumentServiceError::NotFound)
        ));
        assert!(matches!(
            service
                .change(
                    &writer.document_id,
                    "p",
                    "client-a",
                    &lease,
                    0,
                    "change-1",
                    DocumentTransactionDto::default()
                )
                .await,
            Err(DocumentServiceError::ChangeCollision)
        ));
        assert_eq!(
            service
                .snapshot(&writer.document_id, "p", "client-b")
                .await
                .unwrap()
                .revision,
            1
        );
        assert_eq!(
            service
                .status(&writer.document_id, "p", "client-b")
                .await
                .unwrap(),
            (1, true, false, false)
        );
        service.detach_client("client-a").await;
        let (reopened, new_lease) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "main.rs",
                "client-c",
                true,
            )
            .await
            .unwrap();
        assert_eq!(reopened.revision, 1);
        assert!(reopened.dirty);
        assert_ne!(new_lease.as_deref(), Some(lease.as_str()));
        assert!(matches!(
            service
                .change(
                    &reopened.document_id,
                    "p",
                    "client-a",
                    &lease,
                    1,
                    "old-lease",
                    tx
                )
                .await,
            Err(DocumentServiceError::StaleLease)
        ));
        assert_eq!(
            service
                .snapshot(&reopened.document_id, "p", "client-c")
                .await
                .unwrap()
                .text,
            "fn main() {}\n// hi\n"
        );
    }

    #[test]
    fn relative_path_policy_rejects_escape_and_absolute_paths() {
        assert!(validate_relative("../secret").is_err());
        assert!(validate_relative("/tmp/secret").is_err());
        assert!(validate_relative("src/main.rs").is_ok());
    }

    #[tokio::test]
    async fn concurrent_first_opens_converge_on_one_document() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("main.rs"), "fn main() {}\n")
            .await
            .unwrap();
        let service = Arc::new(DocumentService::new());
        let first = service.open("p".into(), "w".into(), temp.path(), "main.rs", "one", false);
        let second = service.open("p".into(), "w".into(), temp.path(), "main.rs", "two", false);
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap().0.document_id, second.unwrap().0.document_id);
        assert_eq!(service.by_id.len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn open_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        tokio::fs::write(outside.path().join("secret.txt"), "secret")
            .await
            .unwrap();
        symlink(
            outside.path().join("secret.txt"),
            root.path().join("alias.txt"),
        )
        .unwrap();
        let service = DocumentService::new();
        assert!(matches!(
            service
                .open(
                    "p".into(),
                    "w".into(),
                    root.path(),
                    "alias.txt",
                    "client",
                    false
                )
                .await,
            Err(DocumentServiceError::InvalidPath)
        ));
    }

    #[tokio::test]
    async fn save_commit_revalidates_revision_and_reload_advances_generation() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("main.rs"), "disk")
            .await
            .unwrap();
        let service = DocumentService::new();
        let (opened, _lease) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "main.rs",
                "writer",
                false,
            )
            .await
            .unwrap();
        let lease = service
            .acquire_writer(&opened.document_id, "p", "writer")
            .await
            .unwrap();
        let captured = service
            .capture_save(&opened.document_id, "p", "writer", &lease, 0)
            .await
            .unwrap();
        assert_eq!(captured.text, "disk");
        let tx = DocumentTransactionDto {
            edits: vec![DocumentTextEditDto {
                range: DocumentTextRangeDto { start: 4, end: 4 },
                insert: "!".into(),
            }],
        };
        assert_eq!(
            service
                .change(
                    &opened.document_id,
                    "p",
                    "writer",
                    &lease,
                    0,
                    "save-race",
                    tx
                )
                .await
                .unwrap(),
            1
        );
        assert!(!service
            .commit_save(&opened.document_id, "p", 0, "new-digest".into())
            .await
            .unwrap());
        assert_eq!(
            service
                .status(&opened.document_id, "p", "writer")
                .await
                .unwrap(),
            (1, true, false, true)
        );
        service
            .mark_disk_conflict(&opened.document_id, "p", "external-digest")
            .await
            .unwrap();
        assert_eq!(
            service
                .status(&opened.document_id, "p", "writer")
                .await
                .unwrap(),
            (1, true, true, true)
        );
        assert_eq!(
            service
                .reload_verified(
                    &opened.document_id,
                    "p",
                    "writer",
                    &lease,
                    1,
                    "external".into(),
                    "external-digest".into()
                )
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            service
                .status(&opened.document_id, "p", "writer")
                .await
                .unwrap(),
            (2, false, false, true)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn external_disk_change_refreshes_clean_text_and_preserves_dirty_text_as_conflict() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("clean.rs"), "clean")
            .await
            .unwrap();
        tokio::fs::write(temp.path().join("dirty.rs"), "dirty")
            .await
            .unwrap();
        let service = DocumentService::new();
        let (clean, _) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "clean.rs",
                "reader",
                false,
            )
            .await
            .unwrap();
        let refreshed = service
            .refresh_clean_from_disk(
                &clean.document_id,
                "p",
                "reader",
                0,
                "outside".into(),
                "external-clean".into(),
            )
            .await
            .unwrap();
        assert_eq!(refreshed, Some(1));
        let clean_snapshot = service
            .snapshot(&clean.document_id, "p", "reader")
            .await
            .unwrap();
        assert_eq!(clean_snapshot.text, "outside");
        assert_eq!(clean_snapshot.revision, 1);
        assert!(!clean_snapshot.dirty && !clean_snapshot.conflicted);
        let clean_guard = service
            .lock_clean_paths("w", &["clean.rs".into()])
            .await
            .expect("clean preview target should be guardable");
        drop(clean_guard);

        let (dirty, _) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "dirty.rs",
                "writer",
                false,
            )
            .await
            .unwrap();
        let lease = service
            .acquire_writer(&dirty.document_id, "p", "writer")
            .await
            .unwrap();
        service
            .change(
                &dirty.document_id,
                "p",
                "writer",
                &lease,
                0,
                "edit",
                DocumentTransactionDto {
                    edits: vec![DocumentTextEditDto {
                        range: DocumentTextRangeDto { start: 5, end: 5 },
                        insert: " local".into(),
                    }],
                },
            )
            .await
            .unwrap();
        assert!(service
            .lock_clean_paths("w", &["dirty.rs".into()])
            .await
            .is_err());
        assert_eq!(
            service
                .refresh_clean_from_disk(
                    &dirty.document_id,
                    "p",
                    "writer",
                    1,
                    "outside".into(),
                    "external-dirty".into()
                )
                .await
                .unwrap(),
            None
        );
        let dirty_snapshot = service
            .snapshot(&dirty.document_id, "p", "writer")
            .await
            .unwrap();
        assert_eq!(dirty_snapshot.text, "dirty local");
        assert!(dirty_snapshot.dirty && dirty_snapshot.conflicted);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accepted_change_waits_behind_checked_save_operation_gate() {
        let temp = tempfile::tempdir().unwrap();
        tokio::fs::write(temp.path().join("main.rs"), "base")
            .await
            .unwrap();
        let service = Arc::new(DocumentService::new());
        let (opened, _) = service
            .open(
                "p".into(),
                "w".into(),
                temp.path(),
                "main.rs",
                "writer",
                false,
            )
            .await
            .unwrap();
        let lease = service
            .acquire_writer(&opened.document_id, "p", "writer")
            .await
            .unwrap();
        let operation = service
            .operation_lock(&opened.document_id, "p", "writer", &lease)
            .await
            .unwrap();
        let save_guard = operation.lock_owned().await;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let task_service = service.clone();
        let task_id = opened.document_id.clone();
        let task_lease = lease.clone();
        let change = tokio::spawn(async move {
            let _ = started_tx.send(());
            task_service
                .change(
                    &task_id,
                    "p",
                    "writer",
                    &task_lease,
                    0,
                    "racing-change",
                    DocumentTransactionDto {
                        edits: vec![DocumentTextEditDto {
                            range: DocumentTextRangeDto { start: 4, end: 4 },
                            insert: "!".into(),
                        }],
                    },
                )
                .await
        });
        started_rx.await.unwrap();
        tokio::task::yield_now().await;
        assert!(
            !change.is_finished(),
            "change crossed the save operation gate"
        );
        drop(save_guard);
        assert_eq!(change.await.unwrap().unwrap(), 1);
    }
}
