//! Persistent Memory System
//!
//! Session-to-session learning storing and retrieving context across sessions.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    memories: HashMap<String, Memory>,
}

impl MemoryStore {
    pub fn new() -> std::io::Result<Self> {
        let root = dirs::config_dir()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config dir"))?
            .join("codegg")
            .join("memory");

        std::fs::create_dir_all(&root)?;

        let mut store = Self {
            root,
            memories: HashMap::new(),
        };

        let _ = store.load_all();

        Ok(store)
    }

    fn load_all(&mut self) -> std::io::Result<()> {
        if !self.root.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let namespace = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("user")
                    .to_string();
                let index_path = path.join("MEMORY.md");
                if index_path.exists() {
                    let content = std::fs::read_to_string(&index_path)?;
                    if !content.is_empty() {
                        let memory = Memory::new(namespace.clone(), content);
                        self.memories.insert(memory.id.clone(), memory);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn add(&mut self, memory: Memory) -> Option<Memory> {
        self.memories.insert(memory.id.clone(), memory)
    }

    pub fn get(&self, id: &str) -> Option<&Memory> {
        self.memories.get(id)
    }

    pub fn list(&self, namespace: &str) -> Vec<&Memory> {
        self.memories
            .values()
            .filter(|m| m.namespace == namespace)
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<&Memory> {
        let query_lower = query.to_lowercase();
        self.memories
            .values()
            .filter(|m| m.content.to_lowercase().contains(&query_lower))
            .collect()
    }

    pub fn delete(&mut self, id: &str) -> Option<Memory> {
        self.memories.remove(id)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let mut by_namespace: HashMap<String, Vec<&Memory>> = HashMap::new();

        for memory in self.memories.values() {
            by_namespace
                .entry(memory.namespace.clone())
                .or_default()
                .push(memory);
        }

        for (namespace, memories) in by_namespace.iter() {
            let namespace_dir = self.root.join(namespace);
            std::fs::create_dir_all(&namespace_dir)?;

            let index_path = namespace_dir.join("MEMORY.md");
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

            std::fs::write(&index_path, content)?;
        }

        Ok(())
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new().unwrap_or(Self {
            root: PathBuf::new(),
            memories: HashMap::new(),
        })
    }
}
