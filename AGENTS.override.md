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