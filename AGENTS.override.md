# AGENTS.override.md

This file documents project-specific conventions that override default AGENTS.md behavior. These conventions apply to all agents working in this repository.

## Nested AGENTS.md Convention

When a subagent works in a subdirectory that contains its own `AGENTS.md`, the subdirectory's AGENTS.md takes precedence over this root file for that subtree. This allows project-specific guidance without modifying the root AGENTS.md.

**Rule**: More specific (deeper path) AGENTS.md overrides less specific (root) AGENTS.md.

## Session-to-Session Continuity

When continuing work from a previous session:

- Reference specific files and line numbers, not just module names
- Note any verification steps that were performed
- Document what was confirmed vs what was not confirmed
- Include the date of last review since code may have changed

## Key Lesson from Module Review Sessions

**Always verify documentation claims against actual code**. Many bugs in review files turned out to be correctly implemented after direct inspection. The act of reviewing often reveals assumptions that were wrong.

When encountering a claim like "Bug X exists in file Y", first read the actual code at that location to confirm before marking it as a bug.

## Architecture Review Findings (2026-05-23)

### Bug: FocusManager pop_dialog() Index Reversal
- **File**: `src/tui/components/component/focus.rs:33-46`
- **Issue**: `pop_dialog()` reverses the removal index before calling `remove()`
- **Detail**: `position()` returns index from front (0 = first), but code computes `idx_rev = stack.len() - 1 - idx` and removes `idx_rev`
- **Example**: stack `[A,B,C,D,E]`, searching for `B` gives `idx=1`, `idx_rev=3`, removes `D` not `B`
- **Fix**: Use `pos` directly instead of `idx_rev` when calling `remove()`

### Snapshot Capture Flow (Two-Phase)
- `capture_snapshot_if_needed()` is called BEFORE tool execution (`loop.rs:1655`)
- `capture_incremental_snapshot_if_needed()` is called AFTER tool execution (`loop.rs:1853`)
- Both gated on same `has_file_modifying` condition
- Pre-execution snapshot drains stale file-change events first

### Documentation Gaps Identified (May 2026 Review)
The following items need documentation in skills/architecture:

1. **`list_skill_resources()` function**: In `src/tool/skill.rs` - scans skill directory for additional resource files
2. **`FORMAT_V2_PREFIX` constant**: `"v2:"` prefix in crypto module used by `config/encryption.rs`
3. **Tool modules undocumented**: teams.rs (5 TeamTools), lsp.rs (11 operations), formatter.rs
4. **Resilience half_open fields**: `half_open_start_time` and `max_half_open_duration` in circuit breaker
5. **fuzzy_match vs fuzzy_score**: `fuzzy_match` returns distance (lower=better), `fuzzy_score` returns similarity (higher=better)
6. **Histogram 1000-element limit**: Metrics cap at 1000 values (FIFO eviction)
7. **Snapshot restore() path validation**: `restore()` lacks path validation but `restore_to_path()` has it
8. **UiState fields**: `dirty_regions`, `render_panic_count`, `last_render_error` undocumented

### Verification Before Acting
When a plan file claims something is a bug:
1. Read the actual source code at the referenced location
2. Verify the bug exists before marking it as such
3. Many "bugs" from reviews were actually already fixed in prior sessions