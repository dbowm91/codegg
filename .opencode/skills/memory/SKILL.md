---
name: memory
description: Persistent memory system for session-to-session learning in opencode-rs
version: 1.1.0
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

## File Structure

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

## Commands

### `/memory search <query>`
Search stored memories for a specific term.
```
/memory search snake_case
/memory search "I prefer"
```

### `/memory list`
List all stored memories.

### `/memory consolidate`
Extract patterns from the current session and store them as memories.
Uses rule-based pattern detection to identify:
- User preferences ("I prefer X", "don't use Y")
- Coding conventions (naming patterns, file organization)
- Deprecation notices
- Tool preferences

Patterns are scored based on:
- Base importance (explicit statements = higher)
- Frequency bonus (repeated mentions)
- Cross-turn confirmation

Only patterns scoring >= 8.0 are stored. Old memories on the same topic are superseded (linked, not deleted).

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
| Negation/deprecation ("don't use Y") | -3 |
| Repeated occurrence | +2 each |
| Coding convention match | 5 |
| Deprecation notice | 7 |

Final score = base + frequency_bonus. Memories with score < 8.0 are discarded.

## Memory Lifecycle

1. **Creation**: New memories are created by consolidation or manual add
2. **Superseding**: When a new memory on the same topic has higher importance, the old one gets `superseded_by` set
3. **Retrieval**: Superseded memories are excluded from search results but preserved for audit trail
4. **Pruning**: Max 20 active memories per namespace

## Usage

```rust
use crate::memory::{MemoryStore, Memory};

// Create store
let store = MemoryStore::new()?;

// Add memory manually
let memory = Memory::new("user/preferences", "I prefer concise code");
store.add(memory);

// List memories in namespace
let prefs = store.list("user/preferences");

// Search memories
let results = store.search("concise");

// Consolidate session messages
let new_memories = store.consolidate_session(&messages, "project_hash");
```

## Integration Points

- Memory injected into system prompt (compact summary of top memories)
- Auto-consolidation runs after session end (when enabled)
- Manual consolidation via `/memory consolidate` command
- Search available via `/memory search <query>` command
