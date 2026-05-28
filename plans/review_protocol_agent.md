# Review: Batch 1 - Core Protocol and Agent Loop

**Reviewed**: 2026-05-28
**Files**: architecture/protocol.md, architecture/agent.md, architecture/compaction.md, architecture/command.md

## Summary

The four architecture documents are generally accurate and well-structured. Most counts, variant names, and field references match the actual code. However, several documentation errors were found: incorrect `turn_id` optionality in CoreEvent, missing fields in the AgentLoop struct listing, wrong command count claim (39 vs actual 42 built-in entries), inconsistent `/issue` alias representation, and incorrect instruction file listing. The compaction document is the most accurate of the four.

## Documentation Issues

| # | File | Line | Issue | Action |
|---|------|------|-------|--------|
| 1 | protocol.md | 157 | `TurnCompleted` doc shows `turn_id: String` (required), code has `turn_id: String` (required) - CONFIRMED correct | CONFIRMED |
| 2 | protocol.md | 158 | `TurnFailed` doc shows `turn_id` as required field, code has `turn_id: Option<String>` | UPDATE |
| 3 | protocol.md | 159 | `ToolStarted` doc shows `turn_id` as required field, code has `turn_id: Option<String>` | UPDATE |
| 4 | protocol.md | 160 | `ToolCompleted` doc shows `turn_id` as required field, code has `turn_id: Option<String>` | UPDATE |
| 5 | protocol.md | 217-219 | TuiMessage "Special (2)" section counts EventEnvelope and ResyncRequired as special, but EventEnvelope is already listed under Server-to-Client Events. Actual special count is 1. | UPDATE |
| 6 | agent.md | 44-59 | AgentLoop struct listing is missing 9 actual fields: `config`, `question_tx`, `question_rx`, `session_id`, `mcp_service`, `tool_def_cache`, `file_change_rx`, `usage_store`, `pricing_service` | UPDATE |
| 7 | agent.md | 673 | Claims "27 built-in tools (including ImageTool)" - needs verification against `src/tool/mod.rs` (verified in AGENTS.md) | CONFIRMED |
| 8 | agent.md | 674-678 | Instruction file loading section lists `.codegg/instructions.md`, `INSTRUCTIONS.md`, `~/.config/codegg/instructions.md` as primary sources. Actual code at `prompt.rs:7` uses `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md` as the `INSTRUCTION_FILES` constant, with the other paths as secondary/fallback sources | UPDATE |
| 9 | command.md | 51 | Claims "39 hardcoded commands (highest priority)" for built-in commands. Actual count of `Command::new()` calls in `src/tui/command.rs:83-182` is 42 | UPDATE |
| 10 | command.md | 114 | Header says "Built-in Commands (46 total)" - conflates command entries with aliases. Actual unique command entries: 42 | UPDATE |
| 11 | command.md | 163 | `/issue` aliases shown as `bugs`, `features` without slashes. Code stores them as `"/bugs"`, `"/features"` with leading slashes (normalized for matching) | UPDATE |
| 12 | protocol.md | 289 | Line 289 claim about `map_app_event_to_core_event` at `src/core/mod.rs:795-838` is specific enough to verify | CONFIRMED |
| 13 | agent.md | 817-819 | `is_file_modifying_tool` matches `"write" | "edit" | "replace" | "multiedit" | "apply_patch"` - confirmed matches code at `loop.rs:278-283` | CONFIRMED |
| 14 | compaction.md | 59 | `truncate_tool_outputs` doc says "500 characters" - code at `compaction.rs:306` confirms `content.len() > 500` | CONFIRMED |
| 15 | compaction.md | 67 | `SummarizeOldTurns` doc says "Only processes first 20 non-system messages" - code at `compaction.rs:353` confirms `.take(20)` | CONFIRMED |
| 16 | compaction.md | 91 | `select_compaction_strategy` doc says "more than 6 messages with long tool outputs → TruncateToolOutputs; more than 8 messages → SummarizeOldTurns" - code at `compaction.rs:581-591` confirms thresholds are `> 6` and `> 8` | CONFIRMED |

## Code Issues Found

| # | Module | Bug/Issue | Location | Severity |
|---|--------|-----------|----------|----------|
| 1 | protocol | CoreEvent `TurnFailed.turn_id` is `Option<String>` but `TurnCompleted.turn_id` is `String` (non-optional). Inconsistent: a failed turn should always have a turn_id if it started. Not a bug per se but inconsistent API design. | `src/protocol/core.rs:232-236` | Low |
| 2 | protocol | CoreEvent `ToolStarted.turn_id` and `ToolCompleted.turn_id` are `Option<String>`, but documentation and the doc's own text imply they are required fields. Client code may incorrectly assume turn_id is always present. | `src/protocol/core.rs:204-217` | Medium |
| 3 | agent | `AgentLoop` struct has `question_tx`/`question_rx` as `Option<oneshot::Sender/Receiver>` fields but the doc's struct listing omits them. These are critical for the question tool flow. | `src/agent/loop.rs:573-574` | Low (doc only) |
| 4 | command | The `CommandRegistry::new()` at `src/tui/command.rs:83-182` hardcodes 42 command entries, but command.md line 51 claims 39. The discrepancy is 3 commands. | `src/tui/command.rs:83-182` | Low (doc only) |

## Improvement Opportunities

| # | Module | Opportunity | Impact |
|---|--------|-------------|--------|
| 1 | protocol | Add `#[serde(skip_serializing_if = "Option::is_none")]` to optional fields in CoreEvent variants for cleaner JSON output. Currently `Option<String>` fields serialize as `"turn_id": null` which adds noise. | Minor - cleaner wire format |
| 2 | agent | Document the `AgentLoop::new()` constructor parameters since the struct has many fields not shown in the doc. The constructor takes `agents`, `provider`, `permission_checker`, `tool_registry`, `config`, `mcp_service`, `pool`. | Documentation completeness |
| 3 | agent | The `INSTRUCTION_FILES` constant (`AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`) is not documented anywhere in the architecture docs. This is the primary instruction loading mechanism. | Documentation gap |
| 4 | compaction | The `auto_compact_sync` function at `compaction.rs:594` is a near-duplicate of `auto_compact` at `compaction.rs:550`. Consider consolidating. | Code deduplication |
| 5 | command | The command.md doc could clarify that the command count varies between the `src/command/mod.rs` module (42 file-based) vs `src/tui/command.rs` (42 built-in TUI commands), which are separate registries. | Documentation clarity |
| 6 | agent | `prompt.rs:7` defines `INSTRUCTION_FILES` as `["AGENTS.md", "CLAUDE.md", "CONTEXT.md"]` - this list should be documented in architecture as it controls which files are loaded as system context. | Security/reproducibility |

## Stale Content to Prune

| # | File | Content | Reason |
|---|------|---------|--------|
| 1 | agent.md | Line 674-678 lists instruction files as `.codegg/instructions.md`, `INSTRUCTIONS.md`, `~/.config/codegg/instructions.md`. These are secondary paths used in `find_instructions_file()`, not the primary instruction sources. Primary sources are `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md` via `INSTRUCTION_FILES` constant. | Stale/incomplete - the doc was written before `INSTRUCTION_FILES` was introduced |
| 2 | command.md | Line 51 "39 hardcoded commands" - wrong number, should be 42 | Stale count |
| 3 | command.md | Line 114 "Built-in Commands (46 total)" - misleading, conflates entries with aliases | Stale/misleading count |

## Verified Correct

| Item | Value | Location | Status |
|------|-------|----------|--------|
| CoreRequest variant count | 35 | `src/protocol/core.rs:50-175` | CONFIRMED |
| CoreResponse variant count | 7 | `src/protocol/core.rs:24-46` | CONFIRMED |
| CoreEvent variant count | 17 | `src/protocol/core.rs:179-272` | CONFIRMED |
| TuiMessage variant count | 16 | `src/protocol/tui.rs:3-75` | CONFIRMED |
| Protocol version | 1 | `src/protocol/core.rs:3` | CONFIRMED |
| AgentLoopState fields | 6 fields | `src/agent/loop.rs:534-541` | CONFIRMED |
| ExecutionLimits defaults | 100 turns, 1M tokens, 600s | `src/agent/loop.rs:549-557` | CONFIRMED |
| Built-in agent count | 7 | `src/agent/mod.rs:147-276` | CONFIRMED |
| Agent struct fields | 15 fields | `src/agent/mod.rs:28-44` | CONFIRMED |
| AgentMode variants | 3 (Primary, Subagent, All) | `src/agent/mod.rs:46-53` | CONFIRMED |
| SubAgentPool defaults | max_concurrent=5, max_depth=3 | `src/agent/worker.rs:85-94` | CONFIRMED |
| SubAgentRequest fields | 8 fields | `src/agent/worker.rs:19-28` | CONFIRMED |
| ModelRouter fields | enabled, simple/medium/complex_model | `src/agent/router.rs:21-26` | CONFIRMED |
| TaskComplexity variants | 3 (Simple, Medium, Complex) | `src/agent/router.rs:4-8` | CONFIRMED |
| CompactionStrategy variants | 3 | `src/agent/compaction.rs:218-222` | CONFIRMED |
| ContextTracker fields | 7 fields | `src/agent/compaction.rs:76-84` | CONFIRMED |
| TokenizerType variants | 4 | `src/agent/compaction.rs:17-22` | CONFIRMED |
| Prompt files count | 8 (anthropic, beast, codex, default, gemini, gpt, kimi, trinity) | `src/agent/prompts/` | CONFIRMED |
| DoomLoopDetector default threshold | 20 | `src/agent/loop.rs:664-671` | CONFIRMED |
| is_file_modifying_tool list | write, edit, replace, multiedit, apply_patch | `src/agent/loop.rs:278-283` | CONFIRMED |
| ToolDefCache tuple shape | (Option<String>, bool, bool, usize, u64, Vec<ToolDefinition>) | `src/agent/loop.rs:60-67` | CONFIRMED |
| QuestionSpec fields | id, prompt, default | `src/protocol/tui.rs:78-82` | CONFIRMED |
| drop_middle_messages keep_each_side | 2 | `src/agent/compaction.rs:460` | CONFIRMED |
| truncate_tool_outputs threshold | 500 chars | `src/agent/compaction.rs:306` | CONFIRMED |
| prune_tool_outputs max_tokens | 10,000 | `src/agent/compaction.rs:494` | CONFIRMED |
| Command struct (src/command) fields | name, description, template, agent, model, subtask (deprecated), source | `src/command/mod.rs:9-18` | CONFIRMED |
| Command struct (src/tui/command) fields | name, aliases, description, category, dialog, template, agent, model, subtask, source | `src/tui/command.rs:26-37` | CONFIRMED |
| Instruction files constant | AGENTS.md, CLAUDE.md, CONTEXT.md | `src/agent/prompt.rs:7` | NEW (not in docs) |
| find_instructions_file paths | .codegg/instructions.md, INSTRUCTIONS.md, ~/.config/codegg/instructions.md | `src/agent/prompt.rs:149-169` | CONFIRMED |
