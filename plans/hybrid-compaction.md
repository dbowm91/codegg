# Hybrid Compaction Implementation Plan

## Goal

Implement a configurable compaction system for codegg that supports three operating modes:

1. `programmatic`: deterministic-only compaction with no model call.
2. `agent`: model-only semantic compaction, broadly equivalent to the current LLM summarization path but made more structured and configurable.
3. `hybrid`: deterministic extraction and reduction first, followed by optional model-based semantic checkpointing over the reduced state.

The default user experience should remain simple. If the user does not configure a dedicated compaction model, compaction should use the current active model. Users may optionally set a dedicated compaction model for cheaper or higher-fidelity checkpointing.

This plan is written for incremental implementation. Preserve existing behavior until the new path is tested. Do not attempt a large rewrite in one pass.

## Current State

The current compaction implementation is centered in `src/agent/compaction.rs`.

Current public strategy enum:

```rust
pub enum CompactionStrategy {
    TruncateToolOutputs,
    SummarizeOldTurns,
    DropMiddleMessages,
}
```

Current behavior:

- `ContextTracker` estimates token pressure and decides whether compaction is needed.
- `prune_tool_outputs()` does deterministic truncation of long tool outputs.
- `truncate_tool_outputs()` truncates tool messages to about 500 chars.
- `drop_middle_messages()` keeps two messages from the beginning and two from the end.
- `summarize_old_turns()` calls `llm_summarize()` and then keeps the last four messages.
- `llm_summarize()` summarizes the first 20 non-system messages into one paragraph with `max_tokens: Some(500)`.
- `auto_compact_async()` defaults to `gpt-4o-mini` if no compaction model is provided.
- `AgentLoop::compact_if_needed()` is the integration point.
- `ContextFrame` already exists but is only partially populated.

Important invariants already documented in `src/agent/compaction.rs` must remain true:

- No orphan `Message::Tool`.
- No assistant tool-call message without required tool results unless the serializer can safely handle missing results.
- Tool-call and tool-result order must be preserved.
- Tool result truncation must preserve `tool_call_id` unchanged.
- Multi-tool assistant messages must preserve all IDs and ordering.

## Design Principles

Compaction should be treated as state reconstruction, not as generic summarization.

The target is not merely to reduce tokens. The target is to preserve enough executable and semantic state that the next agent step can continue safely: user goal, constraints, active task, touched files, commands run, tests, failures, decisions, failed approaches, unresolved questions, and next actions.

The programmatic layer should remove bulk. The model layer should resolve ambiguity.

Do not ask a model to summarize megabytes of raw transcript if the harness can deterministically reduce it first. Tool outputs, test runs, diffs, file paths, command metadata, and repeated messages should be compacted by code. The model should only receive a reduced event ledger or context frame and should fill semantic fields that code cannot infer reliably.

Every model-generated claim should be either low-stakes prose or traceable to evidence. Prefer evidence IDs over unsupported narrative.

## Configuration

Extend `CompactionConfig` in `src/config/schema.rs` while preserving backward compatibility.

Current struct:

```rust
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CompactionConfig {
    pub enabled: Option<bool>,
    pub auto: Option<bool>,
    pub prune: Option<bool>,
    pub max_tokens: Option<usize>,
    pub threshold: Option<f64>,
    pub reserved: Option<usize>,
    pub summarize_model: Option<String>,
}
```

Proposed extension:

```rust
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(default)]
pub struct CompactionConfig {
    pub enabled: Option<bool>,
    pub auto: Option<bool>,

    // New high-level controls.
    pub mode: Option<CompactionModeConfig>,
    pub policy: Option<CompactionPolicyConfig>,

    // Existing controls.
    pub prune: Option<bool>,
    pub max_tokens: Option<usize>,
    pub threshold: Option<f64>,
    pub reserved: Option<usize>,

    // Existing field retained as compatibility alias.
    pub summarize_model: Option<String>,

    // Preferred new field. If unset, fall back to summarize_model, then active model.
    pub model: Option<String>,

    // New budgets.
    pub max_tool_output_tokens: Option<usize>,
    pub max_summary_tokens: Option<usize>,
    pub max_events: Option<usize>,
    pub keep_recent_messages: Option<usize>,

    // New safety/quality controls.
    pub validate: Option<bool>,
    pub preserve_evidence: Option<bool>,
    pub inject_context_frame: Option<bool>,
}
```

Add string-backed enums. Use serde `rename_all = "snake_case"`.

```rust
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompactionModeConfig {
    Programmatic,
    Agent,
    Hybrid,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompactionPolicyConfig {
    Conservative,
    Balanced,
    Cheap,
    Emergency,
    LosslessDebug,
}
```

Recommended defaults:

- `enabled`: existing behavior/default.
- `auto`: existing behavior/default.
- `mode`: `hybrid` once hybrid implementation is stable. During migration, default may be `agent` or legacy auto behavior behind a compatibility flag.
- `policy`: `balanced`.
- `validate`: true.
- `preserve_evidence`: true.
- `inject_context_frame`: true.
- `model`: none, meaning active model.

Example configs:

Default hybrid using active model:

```json
{
  "compaction": {
    "enabled": true,
    "auto": true,
    "mode": "hybrid",
    "policy": "balanced",
    "threshold": 0.60,
    "reserved": 16000,
    "prune": true,
    "validate": true,
    "preserve_evidence": true
  }
}
```

Dedicated cheap compaction model:

```json
{
  "compaction": {
    "enabled": true,
    "auto": true,
    "mode": "hybrid",
    "policy": "cheap",
    "model": "google/gemini-2.5-flash-lite",
    "threshold": 0.55,
    "reserved": 16000,
    "prune": true,
    "validate": true
  }
}
```

Programmatic-only:

```json
{
  "compaction": {
    "enabled": true,
    "auto": true,
    "mode": "programmatic",
    "policy": "balanced",
    "threshold": 0.55,
    "prune": true,
    "validate": true
  }
}
```

Agent-only:

```json
{
  "compaction": {
    "enabled": true,
    "auto": true,
    "mode": "agent",
    "policy": "conservative",
    "model": "anthropic/claude-haiku-4.5",
    "threshold": 0.65,
    "validate": true
  }
}
```

## Model Resolution

Change compaction model resolution so that the active model is the default.

Current problematic behavior is in `auto_compact_async()`:

```rust
let model = model.unwrap_or("gpt-4o-mini");
```

Replace this with caller-side resolution. `auto_compact_async()` should not pick a global default model. It should receive an already-resolved model name when it needs a model. If no model is available and mode requires an agent call, fall back safely to programmatic/emergency compaction.

Resolution order:

1. `config.compaction.model`
2. `config.compaction.summarize_model`
3. current active request model
4. `execution_policy.model`
5. `config.model`
6. no model available; do not call provider

In `AgentLoop::compact_if_needed()`, compute:

```rust
let active_model = model_profile.model.as_str(); // or exec_policy.model where accessible
let compaction_model = self.config.compaction.as_ref()
    .and_then(|c| c.model.as_deref().or(c.summarize_model.as_deref()))
    .unwrap_or(active_model);
```

If `ResolvedModelProfile` does not expose `model`, use the request model or `ExecutionPolicy.model`. Avoid hard-coded provider/model names in the compaction module.

## Proposed Module Layout

Keep the current file initially, but prefer splitting once the new code grows.

Suggested layout:

```text
src/agent/compaction/
  mod.rs
  config.rs
  tracker.rs
  invariants.rs
  programmatic.rs
  semantic.rs
  hybrid.rs
  frame.rs
  validate.rs
  legacy.rs
```

Incremental approach:

1. Keep existing `src/agent/compaction.rs` compiling.
2. Add new types and helper functions in the same file or a nested module.
3. Once stable, move legacy functions into `legacy.rs`.
4. Keep compatibility wrappers so old tests continue to pass.

## Core Types

Add internal runtime enums separate from config enums if desired.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionMode {
    Programmatic,
    Agent,
    Hybrid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionPolicy {
    Conservative,
    Balanced,
    Cheap,
    Emergency,
    LosslessDebug,
}
```

Add a normalized settings struct:

```rust
#[derive(Debug, Clone)]
pub struct ResolvedCompactionConfig {
    pub enabled: bool,
    pub auto: bool,
    pub mode: CompactionMode,
    pub policy: CompactionPolicy,
    pub prune: bool,
    pub context_limit: usize,
    pub threshold: f64,
    pub reserved_tokens: usize,
    pub max_tool_output_tokens: usize,
    pub max_summary_tokens: usize,
    pub max_events: usize,
    pub keep_recent_messages: usize,
    pub validate: bool,
    pub preserve_evidence: bool,
    pub inject_context_frame: bool,
    pub compaction_model: Option<String>,
}
```

Suggested policy defaults:

```rust
impl ResolvedCompactionConfig {
    pub fn from_config(
        config: &Config,
        context_limit: usize,
        threshold: f64,
        active_model: Option<&str>,
    ) -> Self {
        // Merge config.compaction with defaults.
        // Resolve model according to the model resolution order above.
    }
}
```

Add compaction input/output types:

```rust
pub struct CompactionInput<'a> {
    pub messages: &'a [Message],
    pub session_id: &'a str,
    pub current_tokens: usize,
    pub active_model: Option<&'a str>,
    pub config: ResolvedCompactionConfig,
}

pub struct CompactionOutput {
    pub messages: Vec<Message>,
    pub frame: Option<ContextFrame>,
    pub diagnostics: Vec<CompactionDiagnostic>,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

#[derive(Debug, Clone)]
pub struct CompactionDiagnostic {
    pub level: CompactionDiagnosticLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub enum CompactionDiagnosticLevel {
    Debug,
    Info,
    Warn,
    Error,
}
```

Evidence references:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRef {
    pub id: String,
    pub kind: EvidenceKind,
    pub summary: String,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    TestRun,
    FilePath,
    Command,
    Diff,
    SecurityFinding,
    Todo,
}
```

Programmatic reduced state:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgrammaticCompactionState {
    pub frame: ContextFrame,
    pub evidence: Vec<EvidenceRef>,
    pub retained_message_indices: Vec<usize>,
    pub diagnostics: Vec<CompactionDiagnostic>,
}
```

## ContextFrame Expansion

`ContextFrame` already has useful fields:

```rust
pub struct ContextFrame {
    pub user_goal: Option<String>,
    pub current_task: Option<String>,
    pub constraints: Vec<String>,
    pub decisions: Vec<String>,
    pub touched_files: Vec<String>,
    pub commands_run: Vec<String>,
    pub test_results: Vec<String>,
    pub unresolved_errors: Vec<String>,
    pub security_findings: Vec<String>,
    pub next_steps: Vec<String>,
}
```

Keep this struct for now. Do not block on designing a perfect schema.

Populate fields as follows:

- `user_goal`: already from `original_user_prompt`.
- `current_task`: already from in-progress todo.
- `next_steps`: already from pending todos.
- `security_findings`: already from recent security findings.
- `constraints`: extract from user messages and optionally semantic checkpoint.
- `decisions`: extract from assistant/user messages and semantic checkpoint.
- `touched_files`: extract from tool calls, tool outputs, and optionally git/diff tools.
- `commands_run`: extract from bash/tool call arguments and tool result metadata.
- `test_results`: extract from command outputs indicating test runs.
- `unresolved_errors`: extract from failed commands, compiler diagnostics, test failures, provider/tool errors.

Add a new method to render a more compact and stable control block:

```rust
impl ContextFrame {
    pub fn to_compaction_control_text(&self) -> String {
        // Similar to to_control_text(), but prefer stable headings and avoid overly chatty phrasing.
    }
}
```

Keep `to_control_text()` for compatibility.

## Programmatic Reducers

Implement deterministic reducers before model summarization.

### Reducer 1: Message Indexing

Assign local evidence IDs to messages in the current compaction pass.

Example IDs:

- `msg_0001`
- `tool_0007`
- `cmd_0009`
- `test_0012`

Do not require persistent global IDs in the first implementation. Later, integrate with `EventStore` if useful.

Function:

```rust
fn build_evidence_index(messages: &[Message]) -> Vec<EvidenceRef>
```

### Reducer 2: Tool Pair Analysis

Build a map of assistant tool calls to tool results.

```rust
struct ToolPair<'a> {
    assistant_index: usize,
    tool_index: Option<usize>,
    tool_call_id: String,
    tool_name: String,
    arguments: serde_json::Value,
    result: Option<&'a str>,
}

fn collect_tool_pairs(messages: &[Message]) -> Vec<ToolPair<'_>>
```

Use this both for extraction and invariant validation.

### Reducer 3: Tool Output Pruning

Replace the current blunt `prune_tool_outputs()` with a richer variant. Keep the existing function for compatibility and have it call the new implementation if practical.

Desired behavior:

- Preserve `tool_call_id` unchanged.
- Preserve first N chars/lines and last N chars/lines.
- Preserve lines that look like errors, warnings, failed tests, panics, stack traces, compiler diagnostics, or file paths.
- Include original token estimate.
- Include content hash.
- Include hint that the original can be reread if appropriate.

Function:

```rust
pub fn prune_tool_outputs_rich(
    messages: &[Message],
    max_tokens_per_output: usize,
    policy: CompactionPolicy,
) -> Vec<Message>
```

Generated text example:

```text
[Tool output compacted]
original_tokens_estimate: 18423
content_hash: sha256:...
kept: first 80 lines, last 40 lines, 12 salient lines

--- first lines ---
...

--- salient lines ---
error[E0425]: cannot find value `foo` in this scope
failures: compaction_preserves_tool_pairs

--- last lines ---
...
```

### Reducer 4: Command Extraction

Extract shell commands and structured command-like tool calls.

For `bash` tool calls, parse arguments for `command`, `cmd`, or equivalent fields.

Function:

```rust
fn extract_commands(tool_pairs: &[ToolPair<'_>]) -> Vec<String>
```

Include only recent/high-salience commands in `ContextFrame.commands_run`.

High-salience commands:

- nonzero exit commands,
- test commands,
- build/check/lint commands,
- git status/diff commands,
- commands mentioned in user/assistant messages.

### Reducer 5: File Path Extraction

Extract paths from:

- tool call arguments,
- tool outputs,
- assistant text,
- user text,
- compiler/test output.

Use conservative regexes. Canonicalize paths when possible.

Function:

```rust
fn extract_file_paths(messages: &[Message], tool_pairs: &[ToolPair<'_>]) -> Vec<String>
```

Rules:

- Prefer relative repo paths when current working directory is known.
- Normalize `./src/../src/lib.rs` to `src/lib.rs`.
- Deduplicate.
- Exclude obvious URLs unless needed.
- Exclude very short false positives.

### Reducer 6: Test and Error Extraction

Extract test results and unresolved errors from tool outputs.

Recognize common patterns:

- `cargo test`, `cargo check`, `cargo clippy`
- Rust compiler `error[E...]`, `warning:`, `panicked at`, `failures:`
- `pytest`, `FAILED`, `ERROR`, `assert`, traceback
- generic exit status failures

Function:

```rust
fn extract_test_and_error_state(tool_pairs: &[ToolPair<'_>]) -> (Vec<String>, Vec<String>)
```

Keep this intentionally heuristic in v1. Better partial extraction is useful even if not exhaustive.

### Reducer 7: Constraint Extraction

Extract durable user constraints with conservative rules.

Search user messages for phrases such as:

- `must`
- `do not`
- `don't`
- `avoid`
- `only`
- `prefer`
- `default`
- `should`
- `should not`
- `must not`
- `unless`
- `keep`
- `preserve`
- `configurable`

Function:

```rust
fn extract_user_constraints(messages: &[Message]) -> Vec<String>
```

This should not try to be too clever. Preserve short sentence-level snippets and let hybrid semantic checkpoint refine them.

### Reducer 8: Message Retention Selection

Select retained raw messages after programmatic extraction.

Inputs:

- policy,
- keep recent count,
- message salience scores,
- tool-pair constraints.

Function:

```rust
fn select_retained_messages(
    messages: &[Message],
    state: &ProgrammaticCompactionState,
    policy: CompactionPolicy,
    keep_recent_messages: usize,
) -> Vec<usize>
```

Always retain:

- system messages,
- recent messages according to policy,
- assistant tool-call messages paired with retained tool results,
- tool results paired with retained assistant tool calls,
- user messages containing durable constraints if compacted state does not already preserve them.

Prefer dropping or compacting:

- old assistant prose that is restated in the context frame,
- repeated tool outputs,
- old successful command output,
- old file reads where current repo state can be reread.

## Semantic Compaction

Create a new semantic checkpoint function. This is separate from legacy `llm_summarize()`.

```rust
pub async fn semantic_checkpoint(
    reduced: &ProgrammaticCompactionState,
    retained_messages: &[Message],
    provider: &dyn Provider,
    model: &str,
    max_summary_tokens: usize,
) -> Result<ContextFrame, AppError>
```

Prompt requirements:

- Do not ask for a free-form paragraph.
- Ask for structured JSON or stable markdown headings.
- Ask the model to fill only semantic fields: constraints, decisions, failed approaches if added later, unresolved questions if added later, next actions.
- Instruct it not to invent file paths, test names, or commands.
- Instruct it to preserve exact user constraints when available.
- Instruct it to use evidence IDs where possible.

Minimal JSON target:

```json
{
  "constraints": ["..."],
  "decisions": ["..."],
  "unresolved_errors": ["..."],
  "next_steps": ["..."]
}
```

If `response_format` support is not uniformly available, parse best-effort JSON from text. If parsing fails, return an error and fall back to programmatic state.

Semantic prompt sketch:

```text
You are updating compact session state for a coding agent. Use only the provided reduced ledger and retained messages. Do not invent file paths, commands, tests, or decisions. Preserve exact user constraints when possible. Return JSON only.

Fields:
- constraints: durable user constraints and implementation requirements.
- decisions: durable architectural or implementation decisions already made.
- unresolved_errors: current failures or blockers that still matter.
- next_steps: immediate next actions for the coding agent.

Reduced ledger:
...

Retained recent messages:
...
```

Request settings:

- temperature: `0.0` or very low.
- max_tokens: from `config.compaction.max_summary_tokens`, default around `800`-`1200`.
- no high reasoning budget by default.

For `agent` mode, call a similar function but with a larger raw-message input. Still prefer structured output over the legacy single paragraph.

## Hybrid Engine

Add a central engine:

```rust
pub async fn compact_with_policy(
    input: CompactionInput<'_>,
    provider: Option<&dyn Provider>,
) -> Result<CompactionOutput, AppError>
```

Pseudo-flow:

```rust
pub async fn compact_with_policy(
    input: CompactionInput<'_>,
    provider: Option<&dyn Provider>,
) -> Result<CompactionOutput, AppError> {
    let tokens_before = estimate_tokens_for_messages(input.messages, input.active_model);

    let programmatic = build_programmatic_state(input.messages, &input.config);

    let mut messages = match input.config.mode {
        CompactionMode::Programmatic => {
            compile_programmatic_messages(input.messages, &programmatic, &input.config)
        }
        CompactionMode::Agent => {
            compact_agent_only(input, provider).await?
        }
        CompactionMode::Hybrid => {
            let mut frame = programmatic.frame.clone();
            if let (Some(provider), Some(model)) = (provider, input.config.compaction_model.as_deref()) {
                match semantic_checkpoint(&programmatic, input.messages, provider, model, input.config.max_summary_tokens).await {
                    Ok(semantic_frame) => merge_frames(&mut frame, semantic_frame),
                    Err(err) => {
                        // Diagnostic only; fall back to programmatic.
                    }
                }
            }
            compile_hybrid_messages(input.messages, &programmatic, frame, &input.config)
        }
    };

    if input.config.validate {
        if let Err(err) = validate_message_invariants(&messages) {
            // Fall back to conservative/emergency preservation, not invalid messages.
            messages = emergency_pair_safe_compaction(input.messages, &input.config);
        }
    }

    let tokens_after = estimate_tokens_for_messages(&messages, input.active_model);

    Ok(CompactionOutput {
        messages,
        frame: Some(programmatic.frame),
        diagnostics,
        tokens_before,
        tokens_after,
    })
}
```

Do not make the first implementation perfect. The important part is to isolate the engine and make modes explicit.

## Message Compilation

Add helper functions that turn programmatic/semantic state into final messages.

Programmatic mode:

- Keep system messages.
- Insert a compact system/control message containing `ContextFrame`.
- Keep selected recent messages.
- Keep valid tool-call/result pairs.
- Use rich-pruned tool outputs for retained tool messages.

Hybrid mode:

- Same as programmatic, but merge in semantic checkpoint fields.

Agent mode:

- Keep system messages.
- Insert structured model-generated summary/control message.
- Keep selected recent messages.

Important: avoid stacking many `Message::System` summaries over time. Either replace previous compaction summary messages or mark them in a recognizable way and remove older ones.

Recommended marker:

```text
[codegg compacted session state]
...
[/codegg compacted session state]
```

Before inserting a new compaction state message, remove old messages containing this marker.

## Invariant Validation

Add explicit invariant validation.

```rust
pub fn validate_message_invariants(messages: &[Message]) -> Result<(), CompactionInvariantError>
```

Validation rules:

- For each assistant message with tool calls, every tool call ID must be followed by a corresponding `Message::Tool` before the next unrelated assistant/user/system boundary unless current provider contract allows missing tool results.
- No `Message::Tool` should appear without a previously retained matching assistant tool call.
- Multi-tool assistant messages must retain all associated tool results.
- Tool result order should match assistant tool call order.

If this detects invalid output:

1. Log warning.
2. Add diagnostic.
3. Fall back to `emergency_pair_safe_compaction()`.
4. Revalidate fallback.
5. If fallback still fails, preserve original messages rather than sending invalid history.

There is already `harden_history()` in `src/agent/loop.rs`. Do not rely only on this as validation. It repairs missing tool results by inserting placeholders, but compaction should avoid creating invalid histories in the first place.

## AgentLoop Integration

Refactor `AgentLoop::compact_if_needed()` minimally.

Current flow:

1. Read `auto`, `prune`, `reserved`.
2. If overflow, prune tool outputs.
3. If tracker needs compaction, run plugin hook.
4. If auto, call `auto_compact_async()`.
5. Else drop middle.
6. Recalculate tokens.
7. Inject context frame.
8. Inject todo reminder.
9. Publish event.

New flow:

1. Resolve active model and compaction config.
2. If disabled, return.
3. If overflow, run rich programmatic pruning first.
4. If tracker does not need compaction after pruning, return.
5. Dispatch existing `SessionCompacting` hook with expanded input including mode/policy/model.
6. If hook blocks, return.
7. Call `compact_with_policy()`.
8. Replace messages with output messages.
9. Recalculate tracker.
10. Inject todo reminder only if not already included in frame.
11. Publish event with before/after tokens and possibly mode/policy later.

Keep existing plugin hook behavior compatible. Existing hook input includes messages, context limit, current tokens, and strategy. Add fields rather than removing existing fields:

```json
{
  "messages": [...],
  "context_limit": 128000,
  "current_tokens": 90000,
  "strategy": "auto_compact",
  "mode": "hybrid",
  "policy": "balanced",
  "compaction_model": "provider/model",
  "reserved_tokens": 16000
}
```

## Event Bus

Current event:

```rust
AppEvent::CompactionTriggered {
    session_id,
    tokens_before,
    tokens_after,
}
```

Do not block implementation on changing this enum. Later, consider adding:

```rust
mode: Option<String>,
policy: Option<String>,
model: Option<String>,
diagnostics: Vec<String>,
```

For now, log mode/policy/model via tracing.

## Compatibility Strategy

Do not remove these existing functions immediately:

- `compact_messages()`
- `compact_messages_sync()`
- `compact_messages_async()`
- `auto_compact()`
- `auto_compact_sync()`
- `auto_compact_async()`
- `llm_summarize()`
- `prune_tool_outputs()`

Instead:

- Keep tests passing.
- Introduce new functions alongside them.
- Route `AgentLoop::compact_if_needed()` to the new engine behind config.
- If `mode` is unset and no new config is present, preserve current behavior for one release.
- Once stable, make `hybrid/balanced` the default.

## Test Plan

Add tests in `tests/compaction.rs` or the existing module tests.

### Config Tests

- Parse `mode: "hybrid"`.
- Parse `mode: "programmatic"`.
- Parse `mode: "agent"`.
- Parse `policy: "balanced"`.
- Parse old `summarize_model` without `model`.
- Confirm `model` overrides `summarize_model`.
- Confirm missing model resolves to active model.

### Model Resolution Tests

Given active model `openai/gpt-5.5`:

- no compaction model -> `openai/gpt-5.5`
- `summarize_model = "openai/gpt-5-mini"` -> `openai/gpt-5-mini`
- `model = "google/gemini-flash"` and `summarize_model = "openai/gpt-5-mini"` -> `google/gemini-flash`

### Invariant Tests

- Assistant with one tool call and one tool result survives compaction.
- Assistant with multiple tool calls and multiple tool results survives compaction.
- Tool result without matching assistant is detected.
- Assistant tool call without result is detected.
- Emergency fallback does not create orphan tool results.

### Programmatic Reducer Tests

- Long tool output is compacted with content hash and salient error lines.
- Bash command is extracted from tool call arguments.
- Rust compiler error is extracted into unresolved errors.
- Cargo test failure is extracted into test results.
- File paths are deduplicated and normalized.
- User constraints are extracted from messages containing `must`, `do not`, `prefer`, etc.

### Hybrid Tests

Use a mock provider that returns structured JSON.

- Hybrid mode calls provider after programmatic extraction.
- Hybrid mode merges semantic frame into programmatic frame.
- Hybrid mode falls back to programmatic if provider errors.
- Hybrid mode falls back to programmatic if JSON parsing fails.
- Hybrid mode validates final messages.

### Agent-Only Tests

- Agent mode calls provider.
- Agent mode does not use programmatic semantic checkpoint path except invariant-preserving message compilation.
- Agent mode falls back safely if provider unavailable.

### Regression Tests

- Existing `auto_compact()` tests pass.
- Existing `prune_tool_outputs()` tests pass.
- Existing `ContextTracker` tests pass.
- Compaction still publishes `CompactionTriggered`.
- ContextFrame injection still occurs if configured.

## Implementation Phases

### Phase 1: Model resolution and config plumbing

Files likely touched:

- `src/config/schema.rs`
- `src/agent/compaction.rs`
- `src/agent/loop.rs`

Tasks:

1. Add config enums and new fields to `CompactionConfig`.
2. Add parsing tests.
3. Add `ResolvedCompactionConfig`.
4. Change compaction model resolution to default to active model.
5. Remove or bypass hard-coded `gpt-4o-mini` fallback.
6. Preserve old behavior when mode is unset.

Acceptance criteria:

- Code compiles.
- Old configs still parse.
- Existing compaction tests pass.
- New model resolution tests pass.

### Phase 2: Invariant validator

Files likely touched:

- `src/agent/compaction.rs` or `src/agent/compaction/invariants.rs`

Tasks:

1. Implement `collect_tool_pairs()`.
2. Implement `validate_message_invariants()`.
3. Add tests for valid and invalid tool histories.
4. Call validator after existing compaction paths when `validate` is enabled.

Acceptance criteria:

- Invalid histories are detected.
- Valid multi-tool histories pass.
- No provider-facing invalid messages are produced by new paths.

### Phase 3: Programmatic reducers

Files likely touched:

- `src/agent/compaction.rs` or new `programmatic.rs`
- `src/agent/context_frame.rs`

Tasks:

1. Implement evidence indexing.
2. Implement rich tool pruning.
3. Implement command extraction.
4. Implement file path extraction.
5. Implement test/error extraction.
6. Implement constraint extraction.
7. Implement retained-message selection.
8. Populate `ContextFrame` from reducers.

Acceptance criteria:

- Programmatic mode performs no provider call.
- Programmatic mode reduces token bulk.
- Programmatic mode preserves tool-pair invariants.
- ContextFrame fields are meaningfully populated.

### Phase 4: Semantic checkpoint

Files likely touched:

- `src/agent/compaction.rs` or new `semantic.rs`

Tasks:

1. Implement `semantic_checkpoint()`.
2. Use structured JSON prompt.
3. Parse response robustly.
4. Add low-temperature request settings.
5. Add mock-provider tests.
6. Fall back to programmatic state on error.

Acceptance criteria:

- Hybrid mode can call a model over reduced state.
- Model output is parsed and merged.
- Bad model output does not break compaction.

### Phase 5: Engine integration

Files likely touched:

- `src/agent/compaction.rs`
- `src/agent/loop.rs`

Tasks:

1. Add `compact_with_policy()`.
2. Refactor `AgentLoop::compact_if_needed()` to use it when `mode` is set.
3. Keep legacy path for unset mode if desired.
4. Add tracing diagnostics.
5. Ensure todo/context frame injection is not duplicated.

Acceptance criteria:

- `programmatic`, `agent`, and `hybrid` modes work.
- Dedicated compaction model works.
- Active model default works.
- Existing hook behavior remains compatible.

### Phase 6: Cleanup and docs

Files likely touched:

- `architecture/compaction.md`
- `.codegg/skills/compaction/SKILL.md`
- `.opencode/skills/compaction/SKILL.md`
- config examples/docs if present

Tasks:

1. Update architecture docs.
2. Document config modes and policies.
3. Document active-model default.
4. Document fallback behavior.
5. Mention that `summarize_model` is retained as compatibility alias.

Acceptance criteria:

- Docs match implementation.
- Another model or contributor can configure and test each mode.

## Fallback Behavior

Fallback rules must be explicit.

If provider is unavailable:

- `programmatic`: unaffected.
- `hybrid`: use programmatic-only output and add diagnostic.
- `agent`: fall back to emergency/programmatic output and add diagnostic.

If semantic output fails to parse:

- Do not use it.
- Use programmatic frame.
- Add diagnostic.

If validation fails:

- Use `emergency_pair_safe_compaction()`.
- If that also fails, preserve original messages.

If compaction does not reduce tokens enough:

- Apply stricter policy once: `balanced -> emergency`, `conservative -> balanced`, `cheap -> emergency`.
- Do not loop indefinitely.

## Emergency Pair-Safe Compaction

Implement a safer replacement for naive `drop_middle_messages()`.

The current `drop_middle_messages()` keeps two messages from each side. That can break tool-call/result relationships depending on message shape.

New emergency strategy should operate on message groups rather than individual messages.

Group types:

- system message group,
- plain user message group,
- plain assistant message group,
- assistant tool-call message plus all corresponding tool result messages.

Algorithm:

1. Convert messages into groups.
2. Always keep all system groups.
3. Keep first N non-system groups and last N non-system groups.
4. Ensure any kept tool-call group contains all required tool results.
5. Insert compacted state marker if available.
6. Validate.

Function:

```rust
fn emergency_pair_safe_compaction(
    messages: &[Message],
    config: &ResolvedCompactionConfig,
) -> Vec<Message>
```

## Important Non-Goals For First Implementation

Do not implement a full persistent event-sourced memory system in this change.

Do not require tree-sitter or rust-analyzer integration in v1.

Do not require perfect command/test parsing in v1.

Do not remove existing legacy compaction APIs in v1.

Do not change provider serializers unless validation reveals an existing bug.

Do not make compaction depend on a specific provider or model.

## Later Enhancements

After the first implementation is stable, consider:

- Store evidence in SQLite via `EventStore` with persistent IDs.
- Add git diff summarization as a deterministic reducer.
- Add tree-sitter symbol extraction.
- Add rust-analyzer/rustdoc JSON extraction for Rust projects.
- Add per-agent compaction policies.
- Add per-model compaction defaults in model profiles.
- Add UI display of compaction mode, model, and before/after tokens.
- Add usage accounting tag for compaction calls.
- Add benchmark harness using recorded sessions.

## Benchmark Harness

A simple benchmark should use real codegg sessions.

For each recorded session:

1. Run baseline without compaction until threshold.
2. Run programmatic compaction.
3. Run agent compaction.
4. Run hybrid compaction.
5. Resume task from compacted context.
6. Score whether the agent preserves goal, constraints, touched files, failing tests, failed approaches, and next action.

Suggested metrics:

- token reduction ratio,
- compaction provider cost,
- invalid tool-history rate,
- JSON parse failure rate,
- false deleted constraint count,
- stale error count,
- task continuation success,
- test pass/fail after resume.

## Final Acceptance Criteria

The implementation is complete when:

- Users can select `programmatic`, `agent`, or `hybrid` compaction.
- Users can select a policy such as `balanced`, `cheap`, or `conservative`.
- If no compaction model is configured, the active model is used.
- If a compaction model is configured, that model is used for semantic checkpointing.
- Programmatic mode performs no model calls.
- Hybrid mode reduces transcript/tool bulk before any model call.
- Agent mode remains available for users who want pure model summarization.
- Tool-call/result invariants are validated after compaction.
- Compaction failure falls back safely instead of producing invalid provider messages.
- ContextFrame is populated with meaningful deterministic state.
- Existing tests pass and new mode/config/invariant tests are added.
