# codegg `/goal` Implementation Plan

**Status**: FULLY COMPLETE (verified 2026-06-02)

| Item | Status | Location |
|------|--------|----------|
| Data model | **DONE** | `src/goal/model.rs` |
| SQLite migration (v16) | **DONE** | `src/session/schema.rs` |
| GoalStore | **DONE** | `src/goal/store.rs` |
| Checkpoint artifact | **DONE** | `src/goal/checkpoint.rs` |
| Render helpers | **DONE** | `src/goal/render.rs` |
| Protocol variants | **DONE** | `src/protocol/core.rs:175-207` |
| Core request handlers | **DONE** | `src/core/mod.rs:761-1120` |
| Goal context injection | **DONE** | TurnSubmit handler |
| Goal tools | **DONE** | `src/goal/tool.rs` |
| TUI `/goal` command | **DONE** | `src/tui/command.rs:175` |
| CLI goal support | **DEFERRED** | After TUI path |
| Subagent propagation | **DEFERRED** | Phase 2 |
| Autonomous `/goal run` | **DEFERRED** | Out of scope first pass |

## Purpose

Implement Codex-style long-running goal support in codegg as a durable, session-scoped objective system. This is not a prompt macro. The implementation should persist a structured active goal, inject a compact goal context into model turns, expose a narrow model-facing goal tool, support TUI slash commands, and maintain a small checkpoint artifact that lets smaller models continue long tasks without carrying the full original conversation in context.

This plan is written for implementation by a smaller coding model. Prefer straightforward, low-risk Rust changes over broad redesign. Do not attempt a full autonomous long-horizon agent loop in the first implementation. The first version should be human-driven: the user sets or loads a goal, the runtime injects the active goal context on subsequent turns, and the model can update progress through a tool.

## Repository facts this plan assumes

The current repository is a single Rust package named `codegg`. It already depends on `tokio`, `serde`, `serde_json`, `sqlx` with SQLite, `chrono`, `uuid`, `ulid`, `ratatui`, and `crossterm`. Do not add new dependencies unless absolutely necessary.

The library root currently exposes many top-level modules in `src/lib.rs`, including `agent`, `core`, `protocol`, `session`, `storage`, `tool`, `tui`, `research`, and `model_profile`. Add the new goal subsystem as `pub mod goal;`.

Persistent application storage is SQLite-backed at `.codegg/sessions.db`, created by `src/storage/mod.rs`, and migrations are currently centralized in `src/session/schema.rs`. Add a schema migration rather than creating a separate ad hoc database.

The TUI already has a slash command registry in `src/tui/command.rs`, with commands such as `/compact`, `/checkpoint`, `/tasks`, `/memory`, and `/sessions`. Add `/goal` and aliases/subcommands through the existing command path rather than building a separate command interpreter.

The core boundary already has `CoreRequest`, `CoreResponse`, and `CoreEvent` in `src/protocol/core.rs`, and the in-process core implementation in `src/core/mod.rs` matches on `CoreRequest`. Extend this boundary so goal functionality works through in-process and stdio core transports.

The agent loop is in `src/agent/loop.rs`. The core currently builds the system prompt, appends learned memory context, constructs an `AgentLoop`, and calls `agent_loop.run(request)`. The goal implementation should inject compact goal context at this same core-layer prompt assembly seam in phase 2.

The default tool registry is in `src/tool/mod.rs`, where tools implement the `Tool` trait and are registered in `ToolRegistry::with_defaults()`. Add a goal tool module and register it only when a database pool/session context is available, not as a useless default tool with no store.

The repository already has `.codegg/research` artifact behavior in the `research` command. Goal checkpoint artifacts should follow this local `.codegg/...` pattern.

## Desired user-facing behavior

The user should be able to type the following in the TUI:

```text
/goal set Implement model-specific prompt profiles for provider families
/goal show
/goal pause
/goal resume
/goal clear
/goal done
/goal checkpoint
/goal from-file docs/plans/deep-research-agent.md
```

The implementation may initially support only these exact forms. Do not overbuild a complex command parser.

The first version should not auto-run multiple model turns. The active goal should affect the next normal user prompt and “continue” prompts by injecting a compact context block into the system prompt. Later autonomous execution can be implemented as a separate `/goal run --max-turns N` feature, but it is explicitly out of scope for the first pass.

The model should have access to narrow goal tools:

```text
goal_get
goal_update_progress
goal_request_completion
```

The model must not be able to pause, clear, cancel, or overwrite arbitrary goal fields. Those are user/runtime controls only.

## Architecture

Implement a new `goal` module:

```text
src/goal/
  mod.rs
  model.rs
  store.rs
  checkpoint.rs
  render.rs
  tool.rs
```

The module responsibilities are:

`model.rs`: serializable goal data types.

`store.rs`: SQLite-backed `GoalStore`.

`checkpoint.rs`: local `.codegg/goals/<goal-id>.checkpoint.md` artifact creation/update/read helpers.

`render.rs`: compact active-goal context rendering for model injection and human-readable status rendering for `/goal show`.

`tool.rs`: model-facing tool implementations.

`mod.rs`: public exports.

Add to `src/lib.rs`:

```rust
pub mod goal;
```

## Data model

Create the following types in `src/goal/model.rs`.

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    AwaitingUser,
    BudgetLimited,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalBudget {
    pub max_turns: Option<i64>,
    pub max_model_tokens: Option<i64>,
    pub max_tool_calls: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalUsage {
    pub turns_used: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub session_id: String,
    pub project_id: String,
    pub title: String,
    pub objective: String,
    pub status: GoalStatus,

    pub plan_path: Option<String>,
    pub checkpoint_path: Option<String>,

    pub current_phase: Option<String>,
    pub progress_summary: String,
    pub next_action: Option<String>,
    pub completion_criteria: Vec<String>,
    pub open_questions: Vec<String>,

    pub budget: GoalBudget,
    pub usage: GoalUsage,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GoalProgressUpdate {
    pub current_phase: Option<String>,
    pub progress_summary: Option<String>,
    pub next_action: Option<String>,
    pub completed_items: Vec<String>,
    pub remaining_items: Vec<String>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub evidence: String,
    pub files_changed: Vec<String>,
    pub tests_run: Vec<String>,
    pub remaining_risks: Vec<String>,
}
```

Use `uuid::Uuid::new_v4().to_string()` for IDs. Do not introduce a new ID crate.

Use `chrono::Utc::now()` for timestamps.

Keep `project_id` as the project directory string used elsewhere in session storage. This lets `/goal` list active goals by project later.

## SQLite migration

Modify `src/session/schema.rs`.

Increase the migration chain from version 15 to version 16.

In `migrate(pool)`, add:

```rust
if current_version < 16 {
    migrate_and_record(pool, 16).await?;
}
```

In the `match version` inside `migrate_and_record`, add:

```rust
16 => migrate_v16(pool).await?,
```

Add:

```rust
async fn migrate_v16(pool: &SqlitePool) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS goal (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            objective TEXT NOT NULL,
            status TEXT NOT NULL,

            plan_path TEXT,
            checkpoint_path TEXT,

            current_phase TEXT,
            progress_summary TEXT NOT NULL DEFAULT '',
            next_action TEXT,
            completion_criteria TEXT NOT NULL DEFAULT '[]',
            open_questions TEXT NOT NULL DEFAULT '[]',

            budget TEXT NOT NULL DEFAULT '{}',
            usage TEXT NOT NULL DEFAULT '{}',

            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            started_at INTEGER,
            completed_at INTEGER,

            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS goal_session_status_idx ON goal(session_id, status)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    sqlx::query("CREATE INDEX IF NOT EXISTS goal_project_status_idx ON goal(project_id, status)")
        .execute(pool)
        .await
        .map_err(|e| StorageError::Migration(e.to_string()))?;

    Ok(())
}
```

Timestamps in the table should be milliseconds since epoch, matching the existing session style. Convert between `DateTime<Utc>` and integer milliseconds inside `GoalStore`.

Only one goal should be active per session. SQLite partial indexes are available, but to avoid compatibility surprises, enforce this in store methods by marking any existing active/awaiting/budget-limited goal for the same session as `paused` before inserting a new active goal.

## GoalStore

Implement `src/goal/store.rs` as a concrete SQLite store:

```rust
#[derive(Clone)]
pub struct GoalStore {
    pool: sqlx::SqlitePool,
}
```

Required methods:

```rust
impl GoalStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self;

    pub async fn create_active(
        &self,
        session_id: &str,
        project_id: &str,
        title: String,
        objective: String,
        plan_path: Option<String>,
        checkpoint_path: Option<String>,
        completion_criteria: Vec<String>,
    ) -> Result<Goal, StorageError>;

    pub async fn active_for_session(&self, session_id: &str) -> Result<Option<Goal>, StorageError>;

    pub async fn get(&self, id: &str) -> Result<Option<Goal>, StorageError>;

    pub async fn update_status(
        &self,
        goal_id: &str,
        status: GoalStatus,
    ) -> Result<Option<Goal>, StorageError>;

    pub async fn clear_active_for_session(&self, session_id: &str) -> Result<(), StorageError>;

    pub async fn update_progress(
        &self,
        goal_id: &str,
        update: GoalProgressUpdate,
    ) -> Result<Option<Goal>, StorageError>;

    pub async fn increment_usage(
        &self,
        goal_id: &str,
        input_tokens: i64,
        output_tokens: i64,
        tool_calls: i64,
    ) -> Result<(), StorageError>;
}
```

Implementation rules:

Use `serde_json` for `completion_criteria`, `open_questions`, `budget`, and `usage`.

Use helper functions for timestamp conversion.

When `create_active` is called, pause any existing `active`, `awaiting_user`, or `budget_limited` goal for the same session before inserting the new goal.

When `update_progress` is called, replace fields only if provided. If `completed_items` or `remaining_items` are nonempty, append a short generated line to `progress_summary`; do not try to create a full task-management system.

If status is set to `Complete`, set `completed_at`.

## Checkpoint artifact

Implement `src/goal/checkpoint.rs`.

Checkpoint root:

```text
<project_dir>/.codegg/goals/
```

Checkpoint file:

```text
<project_dir>/.codegg/goals/<goal-id>.checkpoint.md
```

Required helpers:

```rust
pub fn goal_artifact_dir(project_dir: impl AsRef<Path>) -> PathBuf;

pub async fn create_checkpoint_file(
    project_dir: impl AsRef<Path>,
    goal: &Goal,
    plan_excerpt: Option<&str>,
) -> Result<PathBuf, AppError>;

pub async fn read_checkpoint_excerpt(
    path: impl AsRef<Path>,
    max_chars: usize,
) -> Result<Option<String>, AppError>;

pub async fn append_checkpoint_update(
    path: impl AsRef<Path>,
    update: &GoalProgressUpdate,
) -> Result<(), AppError>;
```

Initial checkpoint template:

```markdown
# Goal Checkpoint

## Objective

<objective>

## Plan Source

<plan_path or "none">

## Current Phase

Not started.

## Completed

None yet.

## In Progress

None yet.

## Remaining

<completion criteria or "Unspecified. Derive from objective and plan.">

## Decisions

None recorded.

## Known Issues

None recorded.

## Open Questions

None recorded.

## Next Action

Inspect the repository and identify the first concrete implementation step.

## Plan Excerpt

<first 4000 chars of plan file if available>
```

Do not constantly rewrite the whole checkpoint in phase 1. Appending a small update block is sufficient.

## Rendered active goal context

Implement `src/goal/render.rs`.

Required function:

```rust
pub fn render_goal_context(goal: &Goal, checkpoint_excerpt: Option<&str>) -> String
```

Output should be compact and deterministic:

```text
## Active Codegg Goal

Objective:
...

Status: active
Current phase: ...
Progress:
...
Next action:
...

Completion criteria:
1. ...
2. ...

Open questions:
- ...

Checkpoint excerpt:
...
```

Also add:

```rust
pub fn render_goal_status(goal: &Goal) -> String
```

for `/goal show`.

Hard limit: `render_goal_context` should cap checkpoint excerpt at 4000 characters and the total block should ideally stay under 6000 characters. This is control context, not a full plan dump.

## Protocol changes

Modify `src/protocol/core.rs`.

Add `Goal` variants to `CoreRequest`:

```rust
GoalSet {
    session_id: String,
    project_id: String,
    objective: String,
},
GoalFromFile {
    session_id: String,
    project_id: String,
    path: String,
},
GoalShow {
    session_id: String,
},
GoalPause {
    session_id: String,
},
GoalResume {
    session_id: String,
},
GoalClear {
    session_id: String,
},
GoalDone {
    session_id: String,
},
GoalCheckpoint {
    session_id: String,
    project_id: String,
},
```

Use `CoreResponse::Json { data }` for all goal responses to avoid larger protocol churn.

Add a `CoreEvent::GoalUpdated` only if useful for UI updates. Minimal implementation can skip this and use request responses plus toasts.

## Core implementation

Modify `src/core/mod.rs`.

In `InprocCoreClient::request`, add match arms for each new `CoreRequest::Goal...` variant.

Each arm requires a database pool. If `self.pool` is `None`, return:

```rust
CoreResponse::Error {
    code: "missing_pool".to_string(),
    message: "Core client missing database pool".to_string(),
}
```

Goal command behavior:

`GoalSet`:
1. Create a `GoalStore`.
2. Derive `title` from the first nonempty line of objective, truncated to 80 chars.
3. Use default completion criteria:
   - `Implementation satisfies the stated objective.`
   - `Relevant tests or checks have been run, or skipped with justification.`
   - `Checkpoint/progress state is updated.`
4. Call `create_active`.
5. Create checkpoint file in `<project_id>/.codegg/goals/`.
6. Update the goal row with `checkpoint_path` if needed.
7. Return JSON with status, id, title, checkpoint_path.

`GoalFromFile`:
1. Resolve `path` relative to `project_id` if it is not absolute.
2. Read the file.
3. Use first Markdown heading as title if available; otherwise file stem.
4. Objective should be a short string: `Follow implementation plan from <path>`.
5. Completion criteria should include:
   - `All phases in the plan file that are in scope are completed.`
   - `Tests/checks specified in the plan have been run.`
   - `Goal checkpoint is updated with completed/remaining work.`
6. Create active goal with `plan_path`.
7. Create checkpoint with a plan excerpt.
8. Return JSON.

`GoalShow`:
1. Return active goal if present.
2. Include rendered status and checkpoint excerpt if available.

`GoalPause`:
1. Active goal -> status `Paused`.
2. Return JSON.

`GoalResume`:
1. Find the most recent paused goal for the session. Add `latest_paused_for_session` to `GoalStore` if needed.
2. Set to `Active`.
3. Return JSON.

`GoalClear`:
1. Mark active goal as `Cancelled`.
2. Return ack JSON.

`GoalDone`:
1. Mark active goal as `Complete`.
2. Return JSON.

`GoalCheckpoint`:
1. If active goal has no checkpoint file, create one.
2. If one exists, append a timestamped update using the goal’s current progress fields.
3. Return JSON.

## Inject active goal context into model turns

Modify the `CoreRequest::TurnSubmit` arm in `src/core/mod.rs`.

Currently the core loads config, creates provider, builds a tool registry, adds the task tool conditionally, builds memory context, loads the agent prompt, appends memory context, builds an `AgentLoop`, and constructs `ChatRequest`.

Insert goal behavior after memory context and before creating the `ChatRequest`.

Pseudo-code:

```rust
let goal_context = if let Some(pool) = self.pool.clone() {
    let goal_store = crate::goal::GoalStore::new(pool.clone());
    match goal_store.active_for_session(&session_id).await {
        Ok(Some(goal)) if goal.status == crate::goal::GoalStatus::Active => {
            let checkpoint_excerpt = if let Some(path) = goal.checkpoint_path.as_deref() {
                crate::goal::read_checkpoint_excerpt(path, 4000).await.ok().flatten()
            } else {
                None
            };
            crate::goal::render_goal_context(&goal, checkpoint_excerpt.as_deref())
        }
        _ => String::new(),
    }
} else {
    String::new()
};

system.push_str(&memory_context);
system.push_str(&goal_context);
```

Also register goal tools in the tool registry when `self.pool` is available:

```rust
if let Some(pool) = self.pool.clone() {
    tool_registry.register(crate::goal::tool::GoalGetTool::new(pool.clone(), session_id.clone()));
    tool_registry.register(crate::goal::tool::GoalUpdateProgressTool::new(pool.clone(), session_id.clone()));
    tool_registry.register(crate::goal::tool::GoalRequestCompletionTool::new(pool, session_id.clone()));
}
```

Goal tools must be session-scoped. The model should not pass arbitrary session IDs.

## Model-facing goal tools

Implement in `src/goal/tool.rs`.

All tools implement the existing `crate::tool::Tool` trait.

### `goal_get`

No parameters.

Returns JSON string:

```json
{
  "active": true,
  "goal": { "id": "...", "title": "...", "objective": "...", "status": "active" },
  "checkpoint_excerpt": "..."
}
```

If no active goal:

```json
{ "active": false }
```

### `goal_update_progress`

Parameters:

```json
{
  "type": "object",
  "properties": {
    "current_phase": { "type": "string" },
    "progress_summary": { "type": "string" },
    "next_action": { "type": "string" },
    "completed_items": { "type": "array", "items": { "type": "string" } },
    "remaining_items": { "type": "array", "items": { "type": "string" } },
    "open_questions": { "type": "array", "items": { "type": "string" } }
  }
}
```

Behavior:
1. Load active goal for the tool’s session.
2. Update goal progress.
3. Append a checkpoint update if `checkpoint_path` exists.
4. Return the updated compact goal JSON.

### `goal_request_completion`

Parameters:

```json
{
  "type": "object",
  "required": ["evidence"],
  "properties": {
    "evidence": { "type": "string" },
    "files_changed": { "type": "array", "items": { "type": "string" } },
    "tests_run": { "type": "array", "items": { "type": "string" } },
    "remaining_risks": { "type": "array", "items": { "type": "string" } }
  }
}
```

Phase 1 behavior:
1. Do not auto-complete if `tests_run` is empty and `remaining_risks` does not explicitly justify skipped tests.
2. If evidence exists and either tests were run or skipped with justification, mark the goal complete.
3. Return JSON with accepted/rejected and reasons.

Keep this conservative. The model can still tell the user what remains.

## TUI slash command integration

Add `/goal` to `src/tui/command.rs` registry:

```rust
Command::new("/goal", CommandCategory::Session, None)
    .with_description("Manage active long-running goal"),
```

Find the existing handler that processes commands like `/compact`, `/loop`, `/tasks`, `/checkpoint`, and `/memory-*`.

Add handler logic for:

```text
/goal set <objective>
/goal from-file <path>
/goal show
/goal pause
/goal resume
/goal clear
/goal done
/goal checkpoint
```

All goal commands require a local session. Reuse existing session-creation behavior if available; otherwise show a toast: `No active session`.

The handler should send `CoreRequest::Goal...` through `app.core_client`.

Expected UX:
- Success: toast with concise status, e.g. `Goal active: <title>`.
- Show: add a system/info message or open a simple text dialog with rendered status.
- Errors: toast with the core error message.

If there is no existing generic info-message API, use toasts for phase 1 and defer a dedicated goal dialog.

## CLI support

Optional later phase:

```text
codegg goal show --session <id>
codegg goal set --session <id> "..."
```

Do not implement this before the TUI path.

## Interaction with `/compact`

The goal context should be injected after normal compaction. The compaction process should not need to preserve the initial goal-setting message, because the active goal is stored in SQLite and checkpointed in `.codegg/goals`.

If there is a compaction prompt/template, update it to say:

```text
Do not summarize the active goal from chat history as if it were the only source of truth. The active goal is supplied separately by the runtime.
```

If this is difficult to locate, skip for phase 1.

## Interaction with subagents

Do not fully integrate goal propagation into subagents in phase 1.

Minimal behavior:
- Parent/main agent sees active goal context.
- Subagents spawned by the task tool do not automatically get the full active goal unless their prompt includes it.

Phase 2:
- When spawning a subagent from an active-goal session, append a very small parent-goal context to the subagent task prompt:
  ```text
  Parent goal: <title/objective>
  Subtask scope: respond only to the assigned task; do not try to complete the entire parent goal.
  ```
- Store subagent results as ordinary results. The parent agent should call `goal_update_progress` if useful.

## Interaction with background tasks

Do not automatically run `/goal` through existing `/loop` or background scheduler in phase 1.

A future feature can support:

```text
/goal run --max-turns 5
```

but this should wait until goal state, prompt injection, and completion gating are stable.

## Testing plan

Add unit tests where feasible.

### Store tests

Create tests for `GoalStore` using an in-memory SQLite pool or temp database.

Required cases:
1. Create active goal.
2. Fetch active goal by session.
3. Creating a second active goal pauses the first.
4. Pause/resume/clear/done status transitions.
5. Progress update changes `current_phase`, `progress_summary`, `next_action`, and `open_questions`.
6. Usage increments persist.

Make sure migrations run before tests. Reuse the existing storage/session migration helpers if possible.

### Render tests

Add tests for `render_goal_context`:
1. Includes objective, phase, progress, next action, criteria.
2. Caps long checkpoint excerpt.
3. Handles empty optional fields gracefully.

### Checkpoint tests

Use `tempfile`:
1. `create_checkpoint_file` creates `.codegg/goals`.
2. File contains objective and plan source.
3. `append_checkpoint_update` appends an update block.
4. `read_checkpoint_excerpt` respects max char length.

### Tool tests

If tool tests are straightforward:
1. `goal_get` returns inactive JSON when no goal.
2. `goal_update_progress` updates active goal.
3. `goal_request_completion` rejects empty evidence.
4. `goal_request_completion` accepts evidence plus tests.

### Manual TUI smoke tests

Run:

```bash
cargo test
cargo build
codegg
```

Then in TUI:

```text
/goal set Implement a tiny test change
/goal show
continue
/goal checkpoint
/goal pause
/goal resume
/goal done
```

Expected:
- No crash.
- Goal persists after restarting `codegg -c`.
- Active goal context affects the next model turn.
- Checkpoint appears under `.codegg/goals/`.

## Acceptance criteria

Implementation is complete when:

1. `cargo test` passes.
2. `cargo build` passes.
3. `/goal set ...` creates a persistent active goal in SQLite.
4. `/goal show` displays active goal status.
5. `/goal pause`, `/goal resume`, `/goal clear`, and `/goal done` work.
6. `/goal from-file <path>` creates a goal linked to a plan file and creates a checkpoint artifact.
7. Active goal context is injected into normal model turns through the core path.
8. `goal_get`, `goal_update_progress`, and `goal_request_completion` are available to the model during active sessions.
9. Goal tool mutations are session-scoped and cannot mutate arbitrary sessions.
10. The implementation works through the core boundary rather than only in TUI local state.

## Non-goals for the first implementation

Do not implement autonomous multi-turn `/goal run`.

Do not implement model-driven goal creation unless the user explicitly asks through normal chat and the model calls an appropriate tool in a later version.

Do not implement a complex planner/checklist UI.

Do not implement vector memory for goals.

Do not modify provider APIs.

Do not refactor TUI/core separation while doing this. Use the existing `CoreClient` boundary.

Do not make checkpoint artifacts part of committed project files by default. They should live under `.codegg/goals/` and be treated as local runtime state.

## Suggested implementation order for MiMo v2.5

1. Add `src/goal/model.rs`, `src/goal/mod.rs`, and export `pub mod goal`.
2. Add migration v16.
3. Add `GoalStore` and store tests.
4. Add checkpoint helpers and render helpers with tests.
5. Add protocol request variants.
6. Add core request handlers for `/goal` operations.
7. Add active goal context injection in `CoreRequest::TurnSubmit`.
8. Add goal tools and register them in the core turn path when `self.pool` exists.
9. Add `/goal` registry entry.
10. Find existing slash-command execution handler and wire `/goal` subcommands to `CoreRequest` calls.
11. Run `cargo fmt`, `cargo test`, and `cargo build`.
12. Manually smoke-test TUI persistence and prompt injection.

## Implementation notes and pitfalls

The most likely compile-risk area is ownership/lifetime handling inside `CoreRequest::TurnSubmit`. Keep goal loading before `tokio::spawn`, while still inside the async request handler. Build a final `system` string before moving the `AgentLoop` into the spawned task.

Do not use `ToolRegistry::with_defaults()` to register goal tools globally unless the tools can operate without a session. They cannot. Register session-scoped goal tools in the core turn path, similar to how the task tool is conditionally registered with the subagent pool.

Keep checkpoint reads small. Do not inject entire plan files. `/goal from-file` should store the plan path and include only a short excerpt in the checkpoint.

Use `CoreResponse::Json` to avoid larger protocol churn.

Use status strings serialized with `serde(rename_all = "snake_case")`, but store them as strings in SQLite. Be consistent when querying status.

When in doubt, keep the first implementation conservative and explicit. The goal system should preserve user intent and continuation state; it should not independently decide to run indefinitely.
