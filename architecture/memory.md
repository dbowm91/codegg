# Memory Module

The `memory` module provides persistent memory for session-to-session learning.

## Overview

**Location**: `src/memory/`

**Key Responsibilities**:
- Store memories across sessions with file-based persistence
- Namespace-based organization (`user/preferences`, `project/{hash}/conventions`)
- Importance scoring and pattern-based consolidation
- Memory superseding to prevent unbounded growth

## Key Types

### Memory

```rust
pub struct Memory {
    pub id: String,
    pub namespace: String,           // e.g., "user/preferences", "project/{hash}/conventions"
    pub title: Option<String>,
    pub content: String,
    pub uri: Option<String>,
    pub created_at: i64,             // Unix timestamp in milliseconds
    pub updated_at: i64,
    pub access_count: i64,
    pub importance: f64,             // 0.0-1.0 scale, derived from scoring
    pub superseded_by: Option<String>,  // Links to newer memory on same topic
}
```

### MemoryStore

```rust
pub struct MemoryStore {
    root: PathBuf,
    memories: Mutex<HashMap<String, Memory>>,
    auto_save: Mutex<bool>,
}

impl MemoryStore {
    pub fn new() -> std::io::Result<Self>
    pub fn with_auto_save(auto_save: bool) -> std::io::Result<Self>
    pub fn add(&self, memory: Memory) -> Option<Memory>
    pub fn get(&self, id: &str) -> Option<Memory>
    pub fn list(&self, namespace: &str) -> Vec<Memory>
    pub fn search(&self, query: &str) -> Vec<Memory>
    pub fn delete(&self, id: &str) -> Option<Memory>
    pub fn save(&self) -> std::io::Result<()>
    pub fn consolidate_session(&self, messages: &[Message], project_hash: &str) -> Vec<Memory>
    pub fn get_memory_summary(&self, namespace: &str, max_memories: usize) -> String
}
```

## Storage

Memories stored as Markdown files with YAML frontmatter:

```
~/.config/opencode/memory/
├── user/
│   └── preferences/
│       └── MEMORY.md
└── projects/
    └── {project_hash}/
        └── conventions/
            └── MEMORY.md
```

### Memory File Format

```markdown
---
id: {uuid}
title: "Preference: snake_case"
uri: null
created_at: {timestamp}
updated_at: {timestamp}
importance: 0.65
access_count: 0
superseded_by: null
---
I prefer snake_case for variable names (mentioned 3 times)
```

## Namespace Usage

Namespaces provide isolation and hierarchy:

| Namespace | Content |
|-----------|---------|
| `user/preferences` | User-specific preferences |
| `project/{hash}/conventions` | Project-specific conventions |

## Consolidation System

### Pattern Detection (`src/memory/patterns.rs`)

Rule-based pattern detection identifies:

- User preferences ("I prefer X", "don't use Y")
- Coding conventions (naming patterns, file organization)
- Deprecation notices
- Tool preferences

### Scoring System

| Signal | Points |
|--------|--------|
| Explicit preference ("I prefer X") | 10 |
| Negation/deprecation ("don't use Y") | -3 |
| Repeated occurrence | +2 each |
| Coding convention match | 5 |
| Deprecation notice | 7 |

Final score = base + frequency_bonus. Only memories with score >= 8.0 are stored.

### Superseding

When a new memory on the same topic has higher importance, the old one gets `superseded_by` set (linked, not deleted).

Max 20 active memories per namespace.

## Configuration

Enable auto-consolidation via `opencode.jsonc`:

```json
{
  "experimental": {
    "memory_auto_consolidate": true
  }
}
```

## Retrieval

Memories retrieved during session start and injected into system prompt via `get_memory_summary()`:

```rust
let summary = memory_store.get_memory_summary("user/preferences", 10);
```

## TUI Commands

| Command | Description |
|---------|-------------|
| `/memory search <query>` | Search stored memories |
| `/memory list` | List all stored memories |
| `/memory consolidate` | Extract patterns from current session |

## Auto-Consolidation Flow

```
AgentFinished event (session completed)
    ↓
If experimental.memory_auto_consolidate = true:
    ↓
Load session messages from MessageStore
    ↓
Run PatternDetector on messages
    ↓
Aggregate and score patterns (threshold: >=8.0)
    ↓
Store top 20 memories with superseding
```

## See Also

- [.opencode/skills/memory/SKILL.md](../.opencode/skills/memory/SKILL.md) - User-facing documentation
- [agent.md](agent.md) - Uses memory for context injection
- [config.md](config.md) - Experimental config options