---
name: memory
description: Persistent memory system for session-to-session learning in opencode-rs
version: 1.3.0
tags:
  - memory
  - persistence
  - context
  - learning
---

# Memory System Guide

This skill covers the persistent memory system for session-to-session learning.

## Overview

The memory system stores and retrieves context across sessions:
- User preferences learned over time
- Code patterns user prefers
- Project-specific conventions
- Architectural decisions

## Data Model

```rust
pub struct Memory {
    pub id: String,
    pub namespace: String,  // "user/preferences", "project/{hash}/conventions"
    pub title: Option<String>,
    pub content: String,
    pub uri: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub access_count: i64,
    pub importance: f64,      // 0.0-1.0, derived from scoring
    pub superseded_by: Option<String>,  // Links to newer memory on same topic
}
```

## MemoryStore API

```rust
impl MemoryStore {
    pub fn new() -> std::io::Result<Self>
    pub fn with_auto_save(auto_save: bool) -> std::io::Result<Self>
    pub fn set_auto_save(&self, enabled: bool)
    pub fn add(&self, memory: Memory) -> Option<Memory>
    pub fn get(&self, id: &str) -> Option<Memory>  // Increments access_count
    pub fn list(&self, namespace: &str) -> Vec<Memory>
    pub fn search(&self, query: &str) -> Vec<Memory>
    pub fn delete(&self, id: &str) -> Option<Memory>
    pub fn save(&self) -> std::io::Result<()>
    pub fn consolidate_session(&self, messages: &[Message], project_hash: &str) -> Vec<Memory>
    pub fn get_memory_summary(&self, namespace: &str, max_memories: usize) -> String
}
```

## File Structure

```
~/.config/codegg/memory/
├── user/
│   └── preferences/
│       └── MEMORY.md
└── project/
    └── {project_hash}/
        └── conventions/
            └── MEMORY.md
```

## Commands

### `/memory`
Show memory dashboard with counts and recent memories.

### `/memory-search <query>`
Search stored memories for a specific term.
```
/memory-search snake_case
/memory-search "I prefer"
```

### `/memory-list [namespace]`
List memories by namespace (defaults to user/preferences and project).

### `/memory-remember <text>`
Remember something mid-session. Creates a new memory with the given text.
```
/memory-remember I prefer concise code
/memory-remember Always run tests before committing
```

### `/memory-forget <id>`
Delete a specific memory by ID (use `/memory` to see IDs).

### `/memory-consolidate`
Extract patterns from the current session and store them as memories.
Uses rule-based pattern detection to identify:
- User preferences ("I prefer X", "don't use Y")
- Coding conventions (naming patterns, file organization)
- Deprecation notices
- Tool preferences

## Configuration

Enable auto-consolidation on session end via `opencode.jsonc`:

```json
{
  "experimental": {
    "memory_auto_consolidate": true
  }
}
```

When enabled, the system automatically runs consolidation after each completed session.

## Importance Scoring

| Signal | Points |
|--------|--------|
| Explicit preference ("I prefer X") | 10 |
| "I always X" | 12 |
| Negation/deprecation ("don't use Y") | 8 base + -3 modifier = 5 final |
| "never use X" | 10 base + -3 modifier = 7 final |
| "use X instead" | 9 |
| "([^ ]+) is deprecated" | 7 |
| "we use X" | 8 |
| "our X follows Y" | 9 |
| Coding convention match | 4-6 |
| Deprecation notice | 7 |
| Linter/Eslint/clippy/ruff | 5 |
| mock() usage | 4 |

Final score = base + frequency_bonus. Memories with score < 8.0 are discarded.

**Negation scoring**: When a negation pattern is detected ("don't", "never", "not"), the base score has the negation_modifier (-3.0) added to it, not replacing the base. So "don't use eval" = 8.0 + (-3.0) = 5.0.

## Pattern Types

| Type | Description |
|------|-------------|
| UserPreference | Explicit preferences ("I prefer X") |
| CodingConvention | Code organization patterns |
| Deprecation | Deprecation notices |
| NamingPattern | snake_case, camelCase, etc. |
| Architecture | Barrel files, index exports |
| ToolPreference | Linter, mock, tool usage |

## Memory Lifecycle

1. **Creation**: New memories are created by consolidation or manual `/memory-remember`
2. **Superseding**: When a new memory on the same topic has higher importance, the old one gets `superseded_by` set
3. **Retrieval**: `get()` increments access_count; superseded memories excluded from results
4. **Pruning**: Max 20 active memories per namespace

## Topic Matching

When consolidating, topics are matched by comparing the matched text against existing memory titles with prefixes stripped:
- "Preference: ", "Convention: ", "Naming: ", "Architecture: ", "Deprecated: ", "Tool: " prefixes are removed before comparison

This ensures "snake_case" from "I prefer snake_case" correctly matches an existing "Preference: snake_case" memory.

## Usage

```rust
use crate::memory::{MemoryStore, Memory};

// Create store
let store = MemoryStore::new()?;

// Add memory manually
let memory = Memory::new("user/preferences", "I prefer concise code");
store.add(memory);

// Get memory (increments access_count)
if let Some(memory) = store.get(&id) {
    println!("Accessed {} times", memory.access_count);
}

// List memories in namespace
let prefs = store.list("user/preferences");

// Search memories
let results = store.search("concise");

// Consolidate session messages
let new_memories = store.consolidate_session(&messages, "project_hash");
```

## Integration Points

- Memory injected into system prompt at session start (top memories from `user/preferences`)
- Auto-consolidation runs after session end (when enabled)
- Manual consolidation via `/memory-consolidate` command
- Search available via `/memory-search <query>` command
- Mid-session memory via `/memory-remember <text>`

## See Also

- [architecture/memory.md](../../architecture/memory.md) - Module architecture documentation
- [src/memory/mod.rs](../../../src/memory/mod.rs) - MemoryStore implementation
- [src/memory/patterns.rs](../../../src/memory/patterns.rs) - PatternDetector implementation