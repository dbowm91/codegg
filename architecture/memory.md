# Memory Module

The `memory` module provides persistent memory for session-to-session learning.

## Overview

**Location**: `src/memory/`

**Key Responsibilities**:
- Store memories across sessions
- Namespace-based organization
- Importance scoring
- File-based storage

## Key Types

### Memory

```rust
pub struct Memory {
    pub id: String,
    pub namespace: String,
    pub content: String,
    pub importance: u8,  // 1-10
    pub created_at: DateTime<Utc>,
    pub accessed_at: DateTime<Utc>,
}
```

### MemoryStore

```rust
pub struct MemoryStore {
    path: PathBuf,
}

impl MemoryStore {
    pub fn new(namespace: &str) -> Result<Self>;
    pub fn add(&self, memory: &Memory) -> Result<()>;
    pub fn get(&self, id: &str) -> Result<Option<Memory>>;
    pub fn list(&self, namespace: &str) -> Result<Vec<Memory>>;
    pub fn search(&self, query: &str) -> Result<Vec<Memory>>;
    pub fn delete(&self, id: &str) -> Result<()>;
}
```

## Storage

Memories stored as YAML files:

```
~/.config/codegg/memory/
├── namespace1/
│   ├── memory1.yaml
│   └── memory2.yaml
└── namespace2/
    └── memory3.yaml
```

### Memory YAML Format

```yaml
id: "uuid"
namespace: "user_preferences"
content: "User prefers short explanations"
importance: 7
created_at: "2025-01-15T10:30:00Z"
accessed_at: "2025-01-15T10:30:00Z"
```

## Namespace Usage

Namespaces provide isolation:

| Namespace | Content |
|-----------|---------|
| `user` | User preferences |
| `project` | Project-specific knowledge |
| `session` | Session-specific memories |
| `global` | Global knowledge |

## Retrieval

Memories retrieved during session start and used to build context:

```rust
pub async fn build_session_context(store: &MemoryStore) -> Result<String> {
    let memories = store.list("global")?;
    let user_prefs = store.list("user")?;
    // Combine into context string
}
```

## See Also

- [agent.md](agent.md) - Uses memory for context
