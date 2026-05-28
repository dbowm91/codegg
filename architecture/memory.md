# Memory Module

The `memory` module provides persistent memory for session-to-session learning.

## Bug Fixes (2026-05-22)

- **Negation scoring corrected**: Negation patterns ("don't use", "never use") now use `base_score + negation_modifier` instead of just `negation_modifier`. "don't use eval" now scores 5.0 (was 0.0).
- **access_count tracking**: `get()` now increments `access_count` when retrieving a memory.
- **Topic matching fix**: `consolidate_session()` now strips title prefixes ("Preference: ", "Convention: ", etc.) before comparing topics, ensuring correct superseding behavior.

## Overview

**Location**: `src/memory/`

**Key Responsibilities**:
- Store memories across sessions with file-based persistence
- Namespace-based organization (`user/preferences`, `project/{hash}/conventions`)
- Importance scoring and pattern-based consolidation
- Memory superseding to prevent unbounded growth
- During-session memory creation for immediate learning

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
    pub fn set_auto_save(&self, enabled: bool)  // Enable/disable auto-save
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

## Storage

Memories stored as Markdown files with YAML frontmatter. File operations use `flock()` for cross-process synchronization:

```
~/.config/codegg/memory/
├── user/
│   └── preferences/
│       └── MEMORY.md
└── project/
    └── {project_hash}/
        └── MEMORY.md
```

### File Locking

The `flock_lock()` and `flock_unlock()` functions provide advisory locking for memory file operations:
- `flock_lock()` - Acquires exclusive lock (LOCK_EX) before file operations
- `flock_unlock()` - Releases lock (LOCK_UN) after operations complete
- Non-blocking variants available (returns error if lock unavailable)

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
| `project/{hash}` | Project-specific conventions |

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
| "I always X" | 12 |
| "don't use Y" | 8 base + -3 modifier = **5** |
| "never use Y" | 10 base + -3 modifier = **7** |
| "use X instead" | 9 |
| "([^ ]+) is deprecated" | 7 |
| "we use X" | 8 |
| "our X follows Y" | 9 |
| Coding convention match | 4-6 |
| Deprecation notice | 7 |
| Negation modifier | -3 (added to base, not replacement) |

Final score = base + frequency_bonus. Only memories with score >= 8.0 are stored.

**Negation scoring**: When a negation pattern is detected ("don't", "never", "not"), the negation_modifier (-3.0) is ADDED to the base score, not used as a replacement. This ensures negations still have meaningful scores but rank lower than positive preferences.

### Superseding

When a new memory on the same topic has higher importance, the old one gets `superseded_by` set (linked, not deleted).

Max 20 active memories per namespace (soft per-consolidation limit -- `consolidate_session()` processes at most 20 scored candidates via `.take(20)`, but the total count can temporarily exceed 20 when adding individual memories).

### Eviction Policy

When the namespace reaches 20 memories, new memories are still processed but old memories on the same topic get superseded rather than evicted outright. Memories are **not automatically deleted** - they are marked as superseded via the `superseded_by` field when a newer memory on the same topic has higher importance.

To reduce memory count, use `/memory-forget <id>` to manually delete superseded memories.

### consolidate_session Limitations

The `consolidate_session()` function has the following limitations:

1. **Text-only pattern detection**: Only `PartData::Text` parts are analyzed for patterns. Tool call outputs (binary/image data, file contents) are not processed.

2. **No automatic deletion**: When at the 20-memory limit, new memories can still be added and older ones superseded, but the total count may exceed 20 temporarily until cleanup occurs.

3. **Score threshold**: Only patterns scoring >= 8.0 are stored as memories.

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
| `/memory` | Show memory dashboard with counts and recent memories |
| `/memory-search <query>` | Search stored memories |
| `/memory-list [namespace]` | List memories by namespace. If no namespace given, shows memories from both `user/preferences` AND `project/{hash}` namespaces |
| `/memory-remember <text>` | Remember something mid-session |
| `/memory-forget <id>` | Delete a specific memory by ID |
| `/memory-consolidate` | Extract patterns from current session |

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