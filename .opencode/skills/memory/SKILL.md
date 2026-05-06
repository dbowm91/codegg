---
name: memory
description: Persistent memory system for session-to-session learning in opencode-rs
version: 1.0.0
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
    pub namespace: String,  // "user/preferences", "project/conventions"
    pub title: Option<String>,
    pub content: String,
    pub uri: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub access_count: i64,
    pub importance: f64,
    pub superseded_by: Option<String>,
}
```

## File Structure

```
~/.config/opencode/memory/
├── user/
│   ├── MEMORY.md           # Index (200 line limit)
│   ├── preferences.md      # User preferences
│   └── patterns.md         # Code patterns
└── projects/
    └── {project_hash}/
        ├── MEMORY.md
        ├── conventions/     # Project conventions
        └── decisions/       # Architectural decisions
```

## Usage

```rust
use crate::memory::{MemoryStore, Memory};

// Create store
let store = MemoryStore::new()?;

// Add memory
let memory = Memory::new("user/preferences", "I prefer concise code over verbose code");
store.add(memory);

// List memories
let prefs = store.list("user/preferences");

// Search
let results = store.search("concise");
```

## Integration Points

- Memory injected into system prompt (compact summary from MEMORY.md)
- Consolidation runs after session end
- Search available via `/memory search <query>` command