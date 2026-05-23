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

## Architecture Review Session (2026-05-23)

This session reviewed the consolidated plan and verified each item against the codebase using subagents. Key findings:

### Real Bugs Requiring Fixes

1. **FocusManager pop_dialog() index bug** (HIGH priority)
   - File: `src/tui/components/component/focus.rs:33-46`
   - Bug: `idx_rev = stack.len() - 1 - idx` computes reverse index, then removes `idx_rev` instead of `idx`
   - Fix: Change `self.stack.remove(idx_rev)` to `self.stack.remove(idx)`

2. **snapshot restore() missing path validation** (HIGH priority)
   - File: `src/snapshot/mod.rs:267-292`
   - Bug: `restore()` does not validate paths escape project_root, but `restore_to_path()` does
   - Fix: Add canonicalize check similar to `restore_to_path()` at lines 305-333

### Documentation Gaps Identified

The following documentation items remain pending (see `plans/plan.md` for full list):

1. **`list_skill_resources()` function**: Undocumented in skills skill
2. **`FORMAT_V2_PREFIX` constant**: `"v2:"` not named explicitly in docs
3. **Tool modules undocumented**: teams.rs (5 TeamTools), lsp.rs (11 operations), formatter.rs
4. **Resilience half_open fields**: `half_open_start_time` and `max_half_open_duration` not documented
5. **Snapshot capture flow**: Two-phase capture (before/after tool execution) needs clearer doc
6. **LSP request_id**: Docs show `AtomicI64` but actual is `AtomicU64`
7. **SSE methods**: `connect_sse()`, `connect_sse_stream()`, `take_sse_events()` undocumented
8. **IdeServer::run_socket()**: Async socket method not documented
9. **Histogram 1000-element limit**: VecDeque cap not documented

### Items Already Verified Fixed (No Action Needed)

- Wave 0 docs (DOC-1 to DOC-6): All fixed in prior session
- TTS speaking type shows `Mutex<AtomicBool>` correctly
- UiState fields (`dirty_regions`, `render_panic_count`, `last_render_error`) documented
- fuzzy_match vs fuzzy_score clarification added
- Worktree error handling documented
- PROVIDER_NOT_FOUND error code in exec.md
- Handle_paste default method documented

### Parallelization Guidance

The remaining 13 items can be parallelized across 6 agents:
- Agent A: BUG-6 (FocusManager bug - HIGH)
- Agent B: DOC-8, DOC-9 (new skill files)
- Agent C: DOC-11 (snapshot restore path validation - HIGH)
- Agent D: DOC-10, DOC-13, DOC-15, DOC-16 (architecture corrections)
- Agent E: DOC-12, DOC-17, DOC-18 (skills docs)
- Agent F: DOC-20, DOC-25 (final docs)

### Verification Before Acting

When a plan file claims something is a bug:
1. Read the actual source code at the referenced location
2. Verify the bug exists before marking it as such
3. Many "bugs" from reviews were actually already fixed in prior sessions