# Memory Module

The `memory` module provides persistent memory for session-to-session
learning: storing user preferences, coding conventions, and project
decisions across sessions.

## Purpose

Extract and persist meaningful patterns from conversation history so
the agent can learn user preferences and project conventions without
re-discovering them each session.

## Where It Lives

- **Core types & store**: `crates/codegg-core/src/memory/mod.rs`
- **Pattern detection**: `crates/codegg-core/src/memory/patterns.rs`
- **TUI commands**: `src/tui/commands/` (slash commands like
  `/memory`, `/memory-remember`, etc.)

This is a **core crate** module — no UI, server, or plugin
dependencies.

## How It Works

### Storage

Memories are stored as Markdown files with YAML frontmatter under
`~/.config/codegg/memory/`. File operations use `flock()` advisory
locking for cross-process safety. Auto-save is enabled by default.

```
~/.config/codegg/memory/
├── user/
│   └── preferences/
│       └── MEMORY.md
└── project/
    └── {sha256_namespace}/
        └── MEMORY.md
```

### Namespace System

- `user/preferences` — user-specific preferences
- `project/{sha256_hash}` — project-specific conventions

Project namespaces use domain-separated full SHA-256 digest (not
truncated). The domain separator is
`b"codegg-memory-namespace-v1\0"` (`:15`). Legacy MD5-derived
namespaces are migrated idempotently via
`migrate_project_namespace()`.

### Consolidation Flow

1. `consolidate_session()` receives session messages
2. `PatternDetector` scans only `PartData::Text` parts (not tool
   outputs or binary data)
3. Matches are scored; final = base + frequency_bonus
4. Top 20 candidates (score >= 8.0) are stored
5. Existing memories on the same topic are superseded (linked, not
   deleted) if the new memory has higher importance

### Retrieval

`get_memory_summary(namespace, max_memories)` returns a Markdown
summary of the top N non-superseded memories sorted by importance,
formatted as `- [id] title`. This is injected into the system prompt
at session start.

## Key Types & APIs

### Memory (`crates/codegg-core/src/memory/mod.rs:34`)

```rust
pub struct Memory {
    pub id: String,
    pub namespace: String,
    pub title: Option<String>,
    pub content: String,
    pub uri: Option<String>,
    pub created_at: i64,       // millis since epoch
    pub updated_at: i64,
    pub access_count: i64,
    pub importance: f64,       // 0.0–1.0, derived from scoring
    pub superseded_by: Option<String>,
}
```

### MemoryStore (`:70`)

```rust
pub struct MemoryStore {
    root: PathBuf,
    memories: Mutex<HashMap<String, Memory>>,  // parking_lot::Mutex
    auto_save: Mutex<bool>,
}
```

Key methods:

| Method | Line | Description |
|--------|------|-------------|
| `new()` | :99 | Create with auto_save=true |
| `with_auto_save(bool)` | :103 | Create with configurable auto_save |
| `add(Memory)` | :180 | Insert, auto-saves if enabled |
| `get(id)` | :196 | Retrieve by ID, increments access_count |
| `list(namespace)` | :206 | List all memories in a namespace |
| `search(query)` | :259 | Case-insensitive content search |
| `delete(id)` | :269 | Remove by ID, auto-saves if enabled |
| `save()` | :386 | Persist to disk with flock |
| `migrate_project_namespace(identity)` | :220 | Migrate legacy MD5 namespace |
| `consolidate_session(messages, identity)` | :279 | Extract patterns from session |
| `get_memory_summary(ns, max)` | :355 | Markdown summary for prompt injection |

### PatternDetector (`patterns.rs:40`)

```rust
pub struct PatternDetector {
    preference_patterns: Vec<PreferencePattern>,
    convention_patterns: Vec<ConventionPattern>,
}
```

`PatternType` variants: `UserPreference`, `CodingConvention`,
`Deprecation`, `NamingPattern`, `Architecture`, `ToolPreference`.

`ScoredMemory` (:269) wraps a scored match with `to_memory(namespace)`
to convert to a `Memory`.

## Scoring System

| Signal | Base Score |
|--------|-----------|
| "I prefer X" | 10 |
| "I always X" | 12 |
| "don't use Y" | 8 (base) + -3 (negation) = **5** |
| "never use Y" | 10 (base) + -3 (negation) = **7** |
| "use X instead" | 9 |
| "X is deprecated" | 7 |
| "we use X" | 8 |
| "our X follows Y" | 9 |
| Naming convention (snake_case, etc.) | 5 |
| Architecture (barrel file, etc.) | 4–6 |
| Tool preference (mock, linter) | 4–5 |

Final score = average base + (frequency - 1) * 2.0.
Only memories with score >= 8.0 are stored.

**Negation scoring**: The negation_modifier (-3.0) is **added** to the
base score, not used as a replacement. "don't use eval" → 8 + (-3) = 5.

## Configuration Surface

```json
{
  "experimental": {
    "memory_auto_consolidate": true
  }
}
```

Auto-consolidation runs on `AgentFinished` when enabled.

## TUI Commands

| Command | Description |
|---------|-------------|
| `/memory` | Dashboard with counts and recent memories |
| `/memory-search <query>` | Search stored memories |
| `/memory-list [namespace]` | List by namespace (both if omitted) |
| `/memory-remember <text>` | Remember something mid-session |
| `/memory-forget <id>` | Delete a specific memory |
| `/memory-consolidate` | Extract patterns from current session |

## Invariants & Gotchas

- Only `PartData::Text` parts are analyzed — tool outputs, images,
  and binary data are ignored by pattern detection.
- `get()` increments `access_count` in-memory; persistence depends on
  auto_save being enabled.
- The 20-memory limit is a per-consolidation soft cap (`.take(20)`),
  not a hard namespace limit. Individual `add()` calls can exceed 20.
- Superseded memories are not deleted — they are linked via
  `superseded_by`. Use `/memory-forget` to clean up.
- `is_safe_namespace()` rejects `..`, empty components, and backslash
  paths to prevent directory traversal.
- `Memory::new()` defaults `importance` to 0.5; pattern-detected
  memories use `score / 20.0` clamped to 1.0.

## Testing

```bash
cargo test -p codegg-core -- memory
```

## Related Docs

- [agent.md](agent.md) — memory injection into system prompt
- [config.md](config.md) — experimental config options
