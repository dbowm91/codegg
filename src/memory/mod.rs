//! Persistent Memory System
//!
//! Session-to-session learning storing and retrieving context across sessions.

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
}

pub struct MemoryStore {
    root: PathBuf,
    memories: Mutex<HashMap<String, Memory>>,
    auto_save: Mutex<bool>,
}

fn is_safe_namespace(namespace: &str) -> bool {
    if namespace.is_empty() || namespace.contains('/') || namespace.contains('\\') {
        return false;
    }
    if namespace == "." || namespace == ".." {
        return false;
    }
    true
}

impl MemoryStore {
    pub fn new() -> std::io::Result<Self> {
        Self::with_auto_save(false)
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

        let _ = store.load_all();

        Ok(store)
    }

    pub fn set_auto_save(&self, enabled: bool) {
        *self.auto_save.lock() = enabled;
    }

    fn load_all(&mut self) -> std::io::Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let namespace = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("user")
                    .to_string();

                if !is_safe_namespace(&namespace) {
                    continue;
                }

                let index_path = path.join("MEMORY.md");
                if index_path.exists() {
                    let content = fs::read_to_string(&index_path)?;
                    if !content.is_empty() {
                        let memory = Memory::new(namespace.clone(), content);
                        self.memories.lock().insert(memory.id.clone(), memory);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn add(&self, memory: Memory) -> Option<Memory> {
        let result = self.memories.lock().insert(memory.id.clone(), memory);
        if *self.auto_save.lock() {
            let _ = self.save();
        }
        result
    }

    pub fn get(&self, id: &str) -> Option<std::sync::MutexGuard<'_, Memory>> {
        let guard = self.memories.lock();
        if guard.contains_key(id) {
            Some(guard)
        } else {
            None
        }
    }

    pub fn list(&self, namespace: &str) -> Vec<std::sync::MutexGuard<'_, Memory>> {
        self.memories
            .lock()
            .values()
            .filter(|m| m.namespace == namespace)
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn search(&self, query: &str) -> Vec<std::sync::MutexGuard<'_, Memory>> {
        let query_lower = query.to_lowercase();
        self.memories
            .lock()
            .values()
            .filter(|m| m.content.to_lowercase().contains(&query_lower))
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn delete(&self, id: &str) -> Option<Memory> {
        let result = self.memories.lock().remove(id);
        if *self.auto_save.lock() {
            let _ = self.save();
        }
        result
    }

    pub fn save(&self) -> std::io::Result<()> {
        let lock_path = self.root.join(".lock");
        let lock_file = fs::OpenOptions::new()
            .create(true)
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
            by_namespace
                .entry(memory.namespace.clone())
                .or_default()
                .push(memory);
        }

        for (namespace, memories) in by_namespace.iter() {
            if !is_safe_namespace(namespace) {
                continue;
            }

            let namespace_dir = self.root.join(namespace);
            fs::create_dir_all(&namespace_dir)?;

            let index_path = namespace_dir.join("MEMORY.md");
            let temp_path = namespace_dir.join("MEMORY.md.tmp");

            let mut content = String::new();

            for memory in memories {
                content.push_str(&format!(
                    "---\nid: {}\ntitle: {:?}\nimportance: {:.2}\naccess_count: {}\n---\n{}\n\n",
                    memory.id,
                    memory.title.as_deref().unwrap_or(""),
                    memory.importance,
                    memory.access_count,
                    memory.content
                ));
            }

            fs::write(&temp_path, &content)?;
            fs::rename(&temp_path, &index_path)?;
        }

        Ok(())
    }
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
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn flock_unlock(file: &fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
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