# Memory Module Improvements Plan

## Date: 2026-05-22

## Bugs Fixed

### 1. Negation Scoring Documentation Mismatch
- **Issue**: Documentation says negation adds "+8 points", but code subtracts 3.0
- **Fix**: Update architecture/memory.md to reflect actual behavior (negation lowers importance)

### 2. Missing `/memory` Commands
- **Issue**: Docs claim `/memory search`, `/memory list`, `/memory consolidate` exist
- **Reality**: These are NOT in CommandRegistry
- **Fix**: Add memory commands to the command registry

## Improvements Implemented

### 1. On-Demand Memory Loading
- Added `MemoryStore::get_summary()` that loads first 25KB of index
- Topic files loaded lazily when needed
- Hard cap at 200 lines for startup loading

### 2. During-Session Memory Writes
- Added `MemoryStore::remember()` for mid-session memory creation
- `MemoryStore::update()` for modifying existing memories
- `MemoryStore::forget()` for deleting memories

### 3. Git-Aware Project Scoping
- Memory namespace uses git worktree root, not just current directory
- All worktrees of same repo share memory

### 4. Command Implementation
- `/memory` - Show memory dashboard
- `/memory search <query>` - Search memories
- `/memory list [namespace]` - List memories by namespace
- `/memory remember <text>` - Save a memory mid-session
- `/memory forget <id>` - Delete a memory
- `/memory consolidate` - Force session consolidation

## Files Modified

- `src/tui/command.rs` - Added memory commands
- `src/memory/mod.rs` - Added new methods, fixed indexing
- `architecture/memory.md` - Updated documentation
- `.opencode/skills/memory/SKILL.md` - Updated skill documentation
- `AGENTS.md` - Updated memory section reference