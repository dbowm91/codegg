//! Persistent Memory System
//!
//! Session-to-session learning storing and retrieving context across sessions.

pub mod habit;
pub mod patterns;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

const PROJECT_NAMESPACE_DOMAIN: &[u8] = b"codegg-memory-namespace-v1\0";

/// Truncate a UTF-8 string without splitting a code point.
pub fn bounded_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

/// Returns the current durable namespace for project-scoped memories.
///
/// The domain separator makes this digest independent from other SHA-256
/// uses in the application and the full digest avoids recreating the old
/// truncated-hash collision space.
pub fn project_namespace(project_identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(PROJECT_NAMESPACE_DOMAIN);
    hasher.update(project_identity.as_bytes());
    format!("project/{:x}", hasher.finalize())
}

/// Returns the legacy MD5 namespace used before the SHA-256 migration.
pub fn legacy_project_namespace(project_identity: &str) -> String {
    format!("project/{:x}", md5::compute(project_identity.as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub namespace: String,
    pub title: Option<String>,
    pub content: String,
    pub uri: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub access_count: i64,
    pub importance: f64,
    pub superseded_by: Option<String>,
}

impl Memory {
    pub fn new(namespace: impl Into<String>, content: impl Into<String>) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            namespace: namespace.into(),
            title: None,
            content: content.into(),
            uri: None,
            created_at: now,
            updated_at: now,
            access_count: 0,
            importance: 0.5,
            superseded_by: None,
        }
    }

    pub fn content(&self) -> &str {
        &self.content
    }
}

pub struct MemoryStore {
    root: PathBuf,
    memories: Mutex<HashMap<String, Memory>>,
    auto_save: Mutex<bool>,
}

/// Host-derived namespaces admitted to model-facing memory reads.  The model
/// can select among these scope kinds, but it cannot supply a namespace or
/// path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryScopeKind {
    User,
    CurrentProject,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReadScope {
    pub current_project: Option<String>,
    pub legacy_project: Option<String>,
}

impl MemoryReadScope {
    pub fn for_project(project_identity: Option<&str>) -> Self {
        Self {
            current_project: project_identity.map(project_namespace),
            legacy_project: project_identity.map(legacy_project_namespace),
        }
    }

    fn namespaces(&self, kind: MemoryScopeKind) -> Vec<(&'static str, &str)> {
        let mut result = Vec::new();
        if matches!(kind, MemoryScopeKind::User | MemoryScopeKind::Both) {
            result.push(("user", "user/preferences"));
        }
        if matches!(
            kind,
            MemoryScopeKind::CurrentProject | MemoryScopeKind::Both
        ) {
            if let Some(namespace) = self.current_project.as_deref() {
                result.push(("current_project", namespace));
            }
            if let Some(namespace) = self.legacy_project.as_deref() {
                result.push(("current_project_legacy", namespace));
            }
        }
        result
    }
}

fn is_safe_namespace(namespace: &str) -> bool {
    if namespace.is_empty() {
        return false;
    }
    if namespace.contains("..") {
        return false;
    }
    for component in namespace.split('/') {
        if component.is_empty() || component == "." {
            return false;
        }
        if component.contains('\\') {
            return false;
        }
    }
    true
}

fn namespace_to_path(namespace: &str) -> PathBuf {
    namespace.split('/').collect()
}

impl MemoryStore {
    pub fn new() -> std::io::Result<Self> {
        Self::with_auto_save(true)
    }

    pub fn with_auto_save(auto_save: bool) -> std::io::Result<Self> {
        let root = dirs::config_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config dir"))?
            .join("codegg")
            .join("memory");

        fs::create_dir_all(&root)?;

        let mut store = Self {
            root,
            memories: Mutex::new(HashMap::new()),
            auto_save: Mutex::new(auto_save),
        };

        if let Err(e) = store.load_all() {
            tracing::warn!(error = %e, "failed to load memories");
        }

        Ok(store)
    }

    pub fn set_auto_save(&self, enabled: bool) {
        *self.auto_save.lock() = enabled;
    }

    fn load_all(&mut self) -> std::io::Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        let root = self.root.clone();
        self.load_recursive(&root, "")
    }

    fn load_recursive(&mut self, dir: &PathBuf, parent_namespace: &str) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if !is_safe_namespace_single_component(dir_name) {
                    continue;
                }

                let namespace = if parent_namespace.is_empty() {
                    dir_name.to_string()
                } else {
                    format!("{}/{}", parent_namespace, dir_name)
                };

                let index_path = path.join("MEMORY.md");
                if index_path.exists() {
                    self.load_memories_from_file(&index_path, &namespace)?;
                }

                self.load_recursive(&path, &namespace)?;
            }
        }
        Ok(())
    }

    fn load_memories_from_file(&self, path: &PathBuf, namespace: &str) -> std::io::Result<()> {
        let content = fs::read_to_string(path)?;
        let memories = parse_memories_file(&content, namespace);

        let mut memories_lock = self.memories.lock();
        for memory in memories {
            if let Some(existing) = memories_lock.get(&memory.id) {
                if existing.updated_at >= memory.updated_at {
                    continue;
                }
            }
            memories_lock.insert(memory.id.clone(), memory);
        }
        Ok(())
    }

    pub fn add(&self, memory: Memory) -> Option<Memory> {
        let result = self.memories.lock().insert(memory.id.clone(), memory);
        if *self.auto_save.lock() {
            if let Err(e) = self.save() {
                tracing::warn!(error = %e, "failed to auto-save memories after add");
            }
        }
        result
    }

    /// Retrieves a memory by ID, incrementing its access_count.
    ///
    /// Note: access_count is incremented in-memory but persistence depends on
    /// the auto_save setting. If auto_save is disabled, the incremented count
    /// will be lost on program exit. Call `save()` explicitly if persistence
    /// is needed without triggering auto_save.
    pub fn get(&self, id: &str) -> Option<Memory> {
        let mut memories = self.memories.lock();
        if let Some(memory) = memories.get_mut(id) {
            memory.access_count += 1;
            Some(memory.clone())
        } else {
            None
        }
    }

    pub fn list(&self, namespace: &str) -> Vec<Memory> {
        self.memories
            .lock()
            .values()
            .filter(|m| m.namespace == namespace)
            .cloned()
            .collect::<Vec<_>>()
    }

    /// Migrate a legacy project namespace into the current namespace.
    ///
    /// The operation is idempotent: existing current records win by ID,
    /// migrated records are rewritten under the current namespace, and the
    /// legacy directory is removed only after the new data has been saved.
    pub fn migrate_project_namespace(&self, project_identity: &str) -> std::io::Result<()> {
        if self.root.as_os_str().is_empty() {
            return Ok(());
        }
        let current_namespace = project_namespace(project_identity);
        let legacy_namespace = legacy_project_namespace(project_identity);
        if current_namespace == legacy_namespace {
            return Ok(());
        }

        let mut migrated = false;
        {
            let mut memories = self.memories.lock();
            let legacy_ids: Vec<String> = memories
                .values()
                .filter(|memory| memory.namespace == legacy_namespace)
                .map(|memory| memory.id.clone())
                .collect();

            for id in legacy_ids {
                if let Some(memory) = memories.get_mut(&id) {
                    if memory.namespace == legacy_namespace {
                        memory.namespace = current_namespace.clone();
                        migrated = true;
                    }
                }
            }
        }

        let legacy_dir = self.root.join(namespace_to_path(&legacy_namespace));
        if migrated {
            self.save()?;
        }
        if migrated && legacy_dir.exists() {
            fs::remove_dir_all(legacy_dir)?;
        }
        Ok(())
    }

    pub fn search(&self, query: &str) -> Vec<Memory> {
        let query_lower = query.to_lowercase();
        self.memories
            .lock()
            .values()
            .filter(|m| m.content.to_lowercase().contains(&query_lower))
            .cloned()
            .collect::<Vec<_>>()
    }

    /// Search only namespaces derived by the host from the current principal
    /// and project binding.  Superseded records are not effective results.
    pub fn search_scoped(
        &self,
        query: &str,
        scope: &MemoryReadScope,
        scope_kind: MemoryScopeKind,
        limit: usize,
    ) -> Vec<(String, Memory)> {
        let query_lower = query.to_lowercase();
        let namespaces = scope.namespaces(scope_kind);
        let mut results: Vec<_> = self
            .memories
            .lock()
            .values()
            .filter(|memory| memory.superseded_by.is_none())
            .filter_map(|memory| {
                let source = namespaces
                    .iter()
                    .find(|(_, namespace)| *namespace == memory.namespace)
                    .map(|(source, _)| *source)?;
                let haystack = format!(
                    "{} {}",
                    memory.title.as_deref().unwrap_or_default(),
                    memory.content
                )
                .to_lowercase();
                haystack
                    .contains(&query_lower)
                    .then(|| (source.to_string(), memory.clone()))
            })
            .collect();
        results.sort_by(|(source_a, a), (source_b, b)| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
                .then_with(|| source_a.cmp(source_b))
                .then_with(|| a.id.cmp(&b.id))
        });
        results.truncate(limit);
        results
    }

    /// Exact scoped lookup.  Namespace membership is checked before the
    /// access counter is incremented, so a foreign id cannot be observed.
    pub fn get_scoped(
        &self,
        id: &str,
        scope: &MemoryReadScope,
        scope_kind: MemoryScopeKind,
    ) -> Option<(String, Memory)> {
        let namespaces = scope.namespaces(scope_kind);
        let source = {
            let memories = self.memories.lock();
            let memory = memories.get(id)?;
            if memory.superseded_by.is_some() {
                return None;
            }
            namespaces
                .iter()
                .find(|(_, namespace)| *namespace == memory.namespace)
                .map(|(source, _)| source.to_string())?
        };
        self.get(id).map(|memory| (source, memory))
    }

    pub fn delete(&self, id: &str) -> Option<Memory> {
        let result = self.memories.lock().remove(id);
        if *self.auto_save.lock() {
            if let Err(e) = self.save() {
                tracing::warn!(error = %e, "failed to auto-save memories after delete");
            }
        }
        result
    }

    pub fn consolidate_session(
        &self,
        messages: &[crate::session::message::Message],
        project_identity: &str,
    ) -> Vec<Memory> {
        use crate::memory::patterns::PatternDetector;

        let detector = PatternDetector::new();
        let all_matches = detector.detect_from_messages(messages);
        let scored = detector.aggregate_and_score(all_matches);

        let namespace = project_namespace(project_identity);

        let existing = self.list(&namespace);
        let existing_by_topic: HashMap<String, &Memory> = existing
            .iter()
            .map(|m| {
                let key = m
                    .title
                    .as_deref()
                    .map(|t| {
                        t.replace("Preference: ", "")
                            .replace("Convention: ", "")
                            .replace("Naming: ", "")
                            .replace("Architecture: ", "")
                            .replace("Deprecated: ", "")
                            .replace("Tool: ", "")
                            .to_lowercase()
                    })
                    .unwrap_or_else(|| m.content.to_lowercase());
                (key, m)
            })
            .collect();

        let mut new_memories = Vec::new();

        for scored_mem in scored.into_iter().take(20) {
            if scored_mem.score < 8.0 {
                continue;
            }

            let topic_key = format!(
                "{}:{}",
                scored_mem.pattern_type,
                scored_mem.matched_text.to_lowercase()
            );

            if let Some(existing_mem) = existing_by_topic.get(&topic_key) {
                if existing_mem.importance > scored_mem.score / 20.0 {
                    continue;
                }

                let mut updated = scored_mem.to_memory(&namespace);
                updated.superseded_by = Some(existing_mem.id.clone());
                self.memories
                    .lock()
                    .insert(updated.id.clone(), updated.clone());
                new_memories.push(updated);
            } else {
                let memory = scored_mem.to_memory(&namespace);
                self.memories
                    .lock()
                    .insert(memory.id.clone(), memory.clone());
                new_memories.push(memory);
            }
        }

        if *self.auto_save.lock() {
            if let Err(e) = self.save() {
                tracing::warn!(error = %e, "failed to auto-save memories after consolidate");
            }
        }

        new_memories
    }

    pub fn get_memory_summary(&self, namespace: &str, max_memories: usize) -> String {
        let memories = self.list(namespace);
        if memories.is_empty() {
            return String::new();
        }

        let mut sorted: Vec<_> = memories
            .into_iter()
            .filter(|m| m.superseded_by.is_none())
            .collect();
        sorted.sort_by(|a, b| {
            b.importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let summary: Vec<_> = sorted
            .into_iter()
            .take(max_memories)
            .map(|m| {
                format!(
                    "- [{}] {}",
                    m.id,
                    m.title.as_deref().unwrap_or("(untitled)")
                )
            })
            .collect();

        format!("## Learned Conventions\n{}\n", summary.join("\n"))
    }

    pub fn save(&self) -> std::io::Result<()> {
        let lock_path = self.root.join(".lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&lock_path)?;
        flock_lock(&lock_file)?;

        let result = self.save_unlocked();

        let _ = flock_unlock(&lock_file);
        result
    }

    fn save_unlocked(&self) -> std::io::Result<()> {
        let memories = self.memories.lock().clone();
        let mut by_namespace: HashMap<String, Vec<&Memory>> = HashMap::new();

        for memory in memories.values() {
            if memory.superseded_by.is_some() {
                continue;
            }
            by_namespace
                .entry(memory.namespace.clone())
                .or_default()
                .push(memory);
        }

        for (namespace, memories) in by_namespace.iter() {
            if !is_safe_namespace(namespace) {
                continue;
            }

            let namespace_dir = self.root.join(namespace_to_path(namespace));
            fs::create_dir_all(&namespace_dir)?;

            let index_path = namespace_dir.join("MEMORY.md");
            let temp_path = namespace_dir.join("MEMORY.md.tmp");

            let mut content = String::new();

            for memory in memories {
                let uri_str = match &memory.uri {
                    Some(u) => format!("\"{}\"", u.replace('"', "\\\"")),
                    None => "null".to_string(),
                };
                let superseded_by_str = match &memory.superseded_by {
                    Some(s) => format!("\"{}\"", s.replace('"', "\\\"")),
                    None => "null".to_string(),
                };

                content.push_str(&format!(
                    "---\nid: {}\ntitle: {:?}\nuri: {}\ncreated_at: {}\nupdated_at: {}\nimportance: {:.2}\naccess_count: {}\nsuperseded_by: {}\n---\n{}\n\n",
                    memory.id,
                    memory.title.as_deref().unwrap_or(""),
                    uri_str,
                    memory.created_at,
                    memory.updated_at,
                    memory.importance,
                    memory.access_count,
                    superseded_by_str,
                    memory.content
                ));
            }

            // Durable temp write (fsync) before rename so a crash
            // cannot leave an empty or truncated index behind.
            let mut file = fs::File::create(&temp_path)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp_path, &index_path)?;
        }

        Ok(())
    }
}

fn is_safe_namespace_single_component(component: &str) -> bool {
    !component.is_empty()
        && !component.contains('/')
        && !component.contains('\\')
        && component != "."
        && component != ".."
}

fn parse_memories_file(content: &str, default_namespace: &str) -> Vec<Memory> {
    let mut memories = Vec::new();
    let content_len = content.len();
    let mut pos = 0;

    while pos < content_len {
        let Some(block_start) = content[pos..].find("---\n").map(|o| pos + o) else {
            break;
        };
        let block_start = block_start + 4;

        let Some(frontmatter_end) = content[block_start..].find("\n---") else {
            break;
        };
        let frontmatter_end = block_start + frontmatter_end;

        let next_delim = content[frontmatter_end..]
            .find("\n\n---\n")
            .map(|o| frontmatter_end + o);
        let content_end = next_delim.unwrap_or(content_len);

        let frontmatter = &content[block_start..frontmatter_end];
        let actual_content = content[frontmatter_end + 5..content_end].trim();

        if let Some(memory) = parse_frontmatter(frontmatter, actual_content, default_namespace) {
            memories.push(memory);
        }

        pos = content_end;
        while pos < content_len && content[pos..].starts_with('\n') {
            pos += 1;
        }
    }

    memories
}

fn parse_frontmatter(frontmatter: &str, content: &str, namespace: &str) -> Option<Memory> {
    let mut id = None;
    let mut title = None;
    let mut uri = None;
    let mut created_at = Utc::now().timestamp_millis();
    let mut updated_at = created_at;
    let mut importance = 0.5;
    let mut access_count = 0;
    let mut superseded_by = None;

    for line in frontmatter.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "id" => id = Some(value.to_string()),
                "title" if !value.is_empty() && value != "\"\"" && value != "null" => {
                    title = Some(value.trim_matches('"').to_string());
                }
                "uri" if !value.is_empty() && value != "null" => {
                    uri = Some(value.trim_matches('"').to_string());
                }
                "created_at" => {
                    if let Ok(ts) = value.parse::<i64>() {
                        created_at = ts;
                    }
                }
                "updated_at" => {
                    if let Ok(ts) = value.parse::<i64>() {
                        updated_at = ts;
                    }
                }
                "importance" => {
                    if let Ok(imp) = value.parse::<f64>() {
                        importance = imp;
                    }
                }
                "access_count" => {
                    if let Ok(count) = value.parse::<i64>() {
                        access_count = count;
                    }
                }
                "superseded_by" if !value.is_empty() && value != "null" => {
                    superseded_by = Some(value.trim_matches('"').to_string());
                }
                _ => {}
            }
        }
    }

    let id = id?;
    Some(Memory {
        id,
        namespace: namespace.to_string(),
        title,
        content: content.to_string(),
        uri,
        created_at,
        updated_at,
        access_count,
        importance,
        superseded_by,
    })
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            memories: Mutex::new(HashMap::new()),
            auto_save: Mutex::new(false),
        }
    }
}

#[cfg(unix)]
fn flock_lock(file: &fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    #[allow(unsafe_code)]
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn flock_unlock(file: &fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    #[allow(unsafe_code)]
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn flock_lock(_file: &fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn flock_unlock(_file: &fs::File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_new() {
        let memory = Memory::new("test-namespace", "Test content");
        assert_eq!(memory.namespace, "test-namespace");
        assert_eq!(memory.content, "Test content");
        assert!(!memory.id.is_empty());
        assert!(memory.title.is_none());
        assert_eq!(memory.access_count, 0);
        assert_eq!(memory.importance, 0.5);
    }

    #[test]
    fn test_memory_with_title() {
        let mut memory = Memory::new("test-namespace", "Test content");
        memory.title = Some("Test Title".to_string());
        assert_eq!(memory.title, Some("Test Title".to_string()));
    }

    #[test]
    fn test_is_safe_namespace() {
        assert!(is_safe_namespace("user"));
        assert!(is_safe_namespace("user/preferences"));
        assert!(is_safe_namespace("project/abc123/conventions"));
        assert!(!is_safe_namespace(""));
        assert!(!is_safe_namespace("."));
        assert!(!is_safe_namespace(".."));
        assert!(!is_safe_namespace("user/../etc"));
        assert!(!is_safe_namespace("user\\preferences"));
    }

    #[test]
    fn test_parse_frontmatter() {
        let frontmatter = "id: test-id\ntitle: \"Test Title\"\nimportance: 0.8\naccess_count: 5";
        let content = "This is the memory content";
        let memory = parse_frontmatter(frontmatter, content, "test-ns").unwrap();

        assert_eq!(memory.id, "test-id");
        assert_eq!(memory.namespace, "test-ns");
        assert_eq!(memory.title, Some("Test Title".to_string()));
        assert_eq!(memory.content, "This is the memory content");
        assert_eq!(memory.importance, 0.8);
        assert_eq!(memory.access_count, 5);
    }

    #[test]
    fn test_parse_memories_file() {
        let content = "---\nid: memory1\ntitle: \"First\"\nimportance: 0.9\naccess_count: 10\n---\nContent of first memory\n\n---\nid: memory2\ntitle: \"Second\"\nimportance: 0.5\naccess_count: 3\n---\nContent of second memory";
        let memories = parse_memories_file(content, "user/test");

        assert_eq!(memories.len(), 2);
        assert_eq!(memories[0].id, "memory1");
        assert_eq!(memories[0].content, "Content of first memory");
        assert_eq!(memories[1].id, "memory2");
        assert_eq!(memories[1].content, "Content of second memory");
    }

    #[test]
    fn project_namespace_is_domain_separated_full_sha256() {
        let current = project_namespace("/workspace/example");
        let repeated = project_namespace("/workspace/example");
        let legacy = legacy_project_namespace("/workspace/example");

        assert_eq!(current, repeated);
        assert!(current.starts_with("project/"));
        assert_eq!(current.len(), "project/".len() + 64);
        assert_eq!(legacy.len(), "project/".len() + 32);
        assert_ne!(current, legacy);
    }

    #[test]
    fn legacy_project_namespace_migrates_idempotently() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = MemoryStore {
            root: temp_dir.path().to_path_buf(),
            memories: Mutex::new(HashMap::new()),
            auto_save: Mutex::new(true),
        };
        let identity = "/workspace/migrate-me";
        let legacy_namespace = legacy_project_namespace(identity);
        let current_namespace = project_namespace(identity);
        let memory = Memory::new(legacy_namespace.clone(), "legacy memory");
        let memory_id = memory.id.clone();

        store.add(memory);
        assert!(temp_dir
            .path()
            .join(namespace_to_path(&legacy_namespace))
            .join("MEMORY.md")
            .exists());

        store.migrate_project_namespace(identity).unwrap();
        store.migrate_project_namespace(identity).unwrap();

        let migrated = store.list(&current_namespace);
        assert_eq!(migrated.len(), 1);
        assert_eq!(migrated[0].id, memory_id);
        assert_eq!(migrated[0].content, "legacy memory");
        assert!(!temp_dir
            .path()
            .join(namespace_to_path(&legacy_namespace))
            .exists());
    }

    #[test]
    fn scoped_reads_include_current_and_legacy_project_but_not_foreign_data() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = MemoryStore {
            root: temp_dir.path().to_path_buf(),
            memories: Mutex::new(HashMap::new()),
            auto_save: Mutex::new(false),
        };
        let scope = MemoryReadScope::for_project(Some("/workspace/current"));
        let current = Memory::new(
            project_namespace("/workspace/current"),
            "current convention",
        );
        let current_id = current.id.clone();
        let legacy = Memory::new(
            legacy_project_namespace("/workspace/current"),
            "legacy convention",
        );
        let foreign = Memory::new(project_namespace("/workspace/other"), "foreign convention");
        let foreign_id = foreign.id.clone();
        store.add(current);
        store.add(legacy);
        store.add(foreign);

        let results = store.search_scoped("convention", &scope, MemoryScopeKind::Both, 8);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, memory)| memory.id != foreign_id));
        assert_eq!(
            store
                .get_scoped(&current_id, &scope, MemoryScopeKind::CurrentProject)
                .unwrap()
                .0,
            "current_project"
        );
    }

    #[test]
    fn scoped_reads_exclude_superseded_records() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = MemoryStore {
            root: temp_dir.path().to_path_buf(),
            memories: Mutex::new(HashMap::new()),
            auto_save: Mutex::new(false),
        };
        let scope = MemoryReadScope::for_project(Some("/workspace/current"));
        let replacement = Memory::new(project_namespace("/workspace/current"), "replacement");
        let mut old = Memory::new(project_namespace("/workspace/current"), "old convention");
        old.superseded_by = Some(replacement.id.clone());
        store.add(old.clone());
        store.add(replacement);
        assert!(store
            .search_scoped("convention", &scope, MemoryScopeKind::CurrentProject, 8)
            .is_empty());
        assert!(store
            .get_scoped(&old.id, &scope, MemoryScopeKind::CurrentProject)
            .is_none());
    }
}
