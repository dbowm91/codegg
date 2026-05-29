# Codegg TUI Refinement Implementation Plan

## Purpose

Improve the codegg TUI from a mostly conversational terminal interface into a structured operational console for long-running coding-agent sessions.

The goal is not to make the terminal UI visually elaborate. The goal is to make agent work inspectable, interruptible, resumable, reviewable, and less context-polluting.

This plan is intended for implementation by a smaller coding model. Favor incremental, low-risk changes over large rewrites. Preserve existing behavior unless explicitly replacing it with a compatible abstraction.

## Current repo assumptions

The repository is currently a single Rust crate named `codegg`.

Known dependencies and capabilities include:

- `ratatui` and `crossterm` for the TUI.
- `ratatui-textarea` for prompt input.
- `tokio` for async runtime and process management.
- `sqlx` with SQLite for persistent session storage.
- `serde` / `serde_json` for structured state.
- `similar` for diff-related functionality.
- Optional `server` feature using `axum` and WebSocket support.
- Existing slash commands, session management, model selection, MCP status, context/status/cost commands, skills, subagents, worktree support, and export/import commands.

Do not begin by splitting the crate into a workspace. The plan should create internal seams that make a future TUI/core split easier, but it should remain compatible with the current single-crate layout.

## Non-goals for this pass

Do not implement a complete desktop/web frontend.

Do not rewrite the entire TUI.

Do not introduce a remote-first protocol such as gRPC as a prerequisite.

Do not replace existing session storage wholesale.

Do not implement full hunk-level patch editing unless the current diff architecture already makes it trivial.

Do not redesign all slash commands at once.

Do not add a large dependency stack unless absolutely necessary.

## Design principle

The TUI should render structured session state, not merely a transcript.

Every significant agent action should produce a compact, inspectable artifact:

- Plan update.
- Tool call summary.
- File change summary.
- Test result summary.
- Permission request.
- Context compaction report.
- Model-routing decision.
- Security/review finding.
- Subagent result.
- Exportable handoff summary.

The transcript may continue to exist, but it must not be the only source of truth.

## Implementation overview

Implement this in five phases:

1. Add typed session events and derived TUI state.
2. Add a task/plan/state panel.
3. Add structured tool-call and permission cards.
4. Add diff/review and test-result views.
5. Add context/session observability and handoff export.

Each phase should compile and preserve existing behavior.

---

# Phase 1: Typed session events and derived TUI state

## Objective

Introduce a typed event layer that can represent important agent and TUI state transitions without forcing all state to be scraped from chat messages.

This is the foundational step. The TUI should eventually render from `SessionEvent` and `TuiSessionState`, while the existing transcript remains available.

## New module candidates

Use names that match the existing codebase style. If a similar module already exists, extend it rather than duplicating.

Suggested files:

- `src/session/events.rs`
- `src/session/state.rs`
- `src/tui/state.rs`
- `src/tui/events.rs`

If the repo already has session or TUI state modules, place these types there.

## Core types

Add a serializable event enum.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SessionEvent {
    GoalSet(GoalSetEvent),
    PlanUpdated(PlanUpdatedEvent),
    PlanItemUpdated(PlanItemUpdatedEvent),
    AgentMessage(AgentMessageEvent),
    UserMessage(UserMessageEvent),
    ToolCallStarted(ToolCallStartedEvent),
    ToolCallFinished(ToolCallFinishedEvent),
    PermissionRequested(PermissionRequestedEvent),
    PermissionResolved(PermissionResolvedEvent),
    FileChanged(FileChangedEvent),
    TestRunStarted(TestRunStartedEvent),
    TestRunFinished(TestRunFinishedEvent),
    ContextCompacted(ContextCompactedEvent),
    ModelRouted(ModelRoutedEvent),
    SubagentStarted(SubagentStartedEvent),
    SubagentFinished(SubagentFinishedEvent),
    FindingRaised(FindingRaisedEvent),
    CheckpointCreated(CheckpointCreatedEvent),
    SessionExported(SessionExportedEvent),
}
```

Use smaller event structs rather than stuffing everything into one enum variant.

Each event should carry:

```rust
pub struct EventMeta {
    pub id: String,
    pub session_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
```

Avoid making event IDs depend on database row IDs. Use `ulid` or `uuid`.

## Minimum event structs

Implement at least:

```rust
pub struct GoalSetEvent {
    pub meta: EventMeta,
    pub goal: String,
}

pub struct PlanUpdatedEvent {
    pub meta: EventMeta,
    pub plan: AgentPlan,
}

pub struct ToolCallStartedEvent {
    pub meta: EventMeta,
    pub call_id: String,
    pub tool_name: String,
    pub command_preview: Option<String>,
    pub cwd: Option<String>,
    pub purpose: Option<String>,
    pub risk: ToolRisk,
}

pub struct ToolCallFinishedEvent {
    pub meta: EventMeta,
    pub call_id: String,
    pub status: ToolCallStatus,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub output_summary: Option<String>,
    pub output_ref: Option<String>,
}

pub struct FileChangedEvent {
    pub meta: EventMeta,
    pub path: String,
    pub change_kind: FileChangeKind,
    pub summary: Option<String>,
}

pub struct ContextCompactedEvent {
    pub meta: EventMeta,
    pub before_tokens: Option<usize>,
    pub after_tokens: Option<usize>,
    pub preserved: Vec<String>,
    pub dropped: Vec<String>,
}

pub struct ModelRoutedEvent {
    pub meta: EventMeta,
    pub role: String,
    pub provider: Option<String>,
    pub model: String,
    pub profile_class: Option<String>,
    pub reason: Option<String>,
}
```

Suggested enums:

```rust
pub enum ToolRisk {
    ReadOnly,
    WorkspaceWrite,
    GitMutation,
    DependencyMutation,
    Network,
    Destructive,
    CredentialAdjacent,
    Unknown,
}

pub enum ToolCallStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Denied,
}

pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

pub enum PlanItemStatus {
    Pending,
    Active,
    Complete,
    Failed,
    Skipped,
    Blocked,
}
```

## Derived state

Add a state object that can be rebuilt from events.

```rust
pub struct TuiSessionState {
    pub goal: Option<String>,
    pub plan: Option<AgentPlan>,
    pub active_tool_calls: Vec<ToolCallSummary>,
    pub recent_tool_calls: Vec<ToolCallSummary>,
    pub changed_files: Vec<ChangedFileSummary>,
    pub test_state: TestState,
    pub context_state: ContextState,
    pub model_state: ModelState,
    pub findings: Vec<FindingSummary>,
    pub subagents: Vec<SubagentSummary>,
}
```

Implement:

```rust
impl TuiSessionState {
    pub fn apply_event(&mut self, event: &SessionEvent);
    pub fn from_events(events: &[SessionEvent]) -> Self;
}
```

The first version does not need perfect coverage. It only needs to support visible state for the following:

- Current goal.
- Current plan.
- Active tool call.
- Recent tool calls.
- Changed files.
- Last test status.
- Last compaction status.
- Active model/provider.

## Persistence

If there is already a session/event table, extend it. Otherwise add a simple event table.

Suggested SQLite table:

```sql
CREATE TABLE IF NOT EXISTS session_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_session_events_session_created
ON session_events(session_id, created_at);
```

Add repository methods:

```rust
async fn append_session_event(&self, event: &SessionEvent) -> Result<()>;
async fn list_session_events(&self, session_id: &str) -> Result<Vec<SessionEvent>>;
```

If migrations already exist, add a migration. If migrations do not exist, add table initialization in the existing database initialization path.

## Acceptance criteria

- The project builds.
- Existing sessions still load.
- A new session can append at least `GoalSet`, `ToolCallStarted`, `ToolCallFinished`, and `FileChanged` events.
- `TuiSessionState::from_events` reconstructs visible state from a list of events.
- Unit tests cover at least:
  - Plan update application.
  - Tool call start/finish lifecycle.
  - Changed file accumulation.
  - Context compaction event application.

---

# Phase 2: Task spine and plan panel

## Objective

Add a compact persistent task panel to the TUI showing the current goal, phase, active plan item, dirty/changed files count, test state, context pressure, model, and active tool.

This should work even if the main transcript is long.

## TUI behavior

Add a right-side panel when terminal width is sufficient.

Suggested responsive behavior:

- If width >= 120 columns, show side panel.
- If width < 120 columns, hide side panel by default and allow toggling it.
- Preserve existing `Ctrl+T` sidebar behavior if present. If `Ctrl+T` already toggles another sidebar, integrate with that instead of stealing the key.

Panel contents:

```text
Goal
  implement model-aware todo prompting

Plan
  ✓ Inspect current prompt builder
  → Add model profile resolver
  · Add tests
  · Update docs

State
  Model: openai/gpt-5.5
  Agent: manager
  Tool: cargo test
  Files: 4 changed
  Tests: stale
  Context: 71%
```

Use Unicode symbols only if the existing TUI already uses them safely. Otherwise use ASCII fallbacks:

```text
[x] complete
[>] active
[ ] pending
[!] failed
[-] skipped
[?] blocked
```

## New slash commands

Add or refine:

```text
/goal <text>
/plan
/plan add <text>
/plan done <index>
/plan skip <index>
/plan block <index>
/plan clear
/state
```

These commands should update structured session state, not only send text to the model.

If `/status` already exists, do not replace it. `/state` may be a compact operational view while `/status` can remain usage/session focused.

## Plan type

Add:

```rust
pub struct AgentPlan {
    pub items: Vec<AgentPlanItem>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub struct AgentPlanItem {
    pub id: String,
    pub text: String,
    pub status: PlanItemStatus,
    pub note: Option<String>,
}
```

The plan should be serializable and persisted through `SessionEvent::PlanUpdated`.

## Model integration

Do not require the model to use this system perfectly.

For now, support user-facing commands and internal harness updates. Later the agent can emit structured plan updates.

If the current code has a planner/todo mechanism, bridge that mechanism into `AgentPlan` rather than replacing it.

## Acceptance criteria

- User can set a goal using `/goal`.
- User can add/update/complete/skip/clear plan items.
- The TUI side panel renders goal and plan state.
- The panel hides or collapses on narrow terminals.
- State survives session reload.
- Existing chat behavior still works.

---

# Phase 3: Structured tool-call and permission cards

## Objective

Render tool calls and permission requests as structured cards in the transcript or work view, with expandable raw output.

The goal is to avoid dumping raw shell/tool output into the main UI unless needed.

## Tool card content

Each tool card should show:

```text
Tool: cargo test
Purpose: validate prompt profile changes
Cwd: /repo/codegg
Risk: workspace-write
Status: failed
Exit: 101
Duration: 14.2s
Summary: 2 prompt snapshot tests failed
```

Raw output should be accessible but collapsed by default. If the existing UI already supports expandable message blocks, reuse that.

## Tool output summarization

Do not require an LLM summary for this phase.

Implement deterministic summaries for common cases:

- Exit code.
- Number of stdout/stderr lines.
- First few significant error lines.
- For `cargo test`, detect failed test lines when possible.
- For `cargo check` / `cargo clippy`, detect compiler error count roughly if present.

Store large raw output separately if existing session storage supports this. Otherwise keep only the summary in the event and continue storing full output wherever it is already stored.

## Permission card content

Permission prompts should show:

```text
Approve command?

cargo update

Risk: dependency mutation
Scope: workspace
Reason: lockfile update requested by agent

[a] allow once
[s] allow session
[e] edit
[d] deny
```

The exact keys may differ based on the existing permission system. Preserve existing safety semantics.

## Tool risk classifier

Add a deterministic classifier for tool risk.

Initial heuristic:

- Read-only:
  - `ls`, `cat`, `rg`, `grep`, `find`, `pwd`, `git diff`, `git status`, `git log`
- Workspace write:
  - file writes, formatters, code generators
- Git mutation:
  - `git add`, `git commit`, `git reset`, `git checkout`, `git switch`, `git merge`, `git rebase`, `git clean`, `git stash`
- Dependency mutation:
  - `cargo update`, `npm install`, `pnpm install`, `uv add`, `pip install`, lockfile-changing operations
- Network:
  - `curl`, `wget`, package manager downloads, remote git fetch/pull
- Destructive:
  - `rm`, `mv` over existing path, `chmod -R`, `chown -R`, `dd`, disk operations
- Credential-adjacent:
  - commands touching `.env`, credentials files, SSH keys, cloud config

Be conservative. Unknown commands should be `Unknown`.

## Acceptance criteria

- Tool calls emit `ToolCallStarted` and `ToolCallFinished` events.
- TUI renders tool calls as compact cards.
- Raw output remains available.
- Permission prompts include risk and purpose when available.
- Tool risk classifier has unit tests for common shell commands.
- Existing tool execution behavior is not broken.

---

# Phase 4: Diff/review and test-result views

## Objective

Add review ergonomics for changed files and test results.

The user should be able to quickly answer:

- What files changed?
- Why did they change?
- What does the diff look like?
- Are tests passing, failing, stale, or not run?
- What should I inspect before accepting the agent’s work?

## Changed files panel

Add a changed-files section to the side panel or a dedicated review view.

Example:

```text
Changed files
  M src/agent/prompt.rs
  A src/model/profile.rs
  M src/config.rs
  M README.md
```

Use git status if inside a git repo. If git is unavailable, use internal file-change events only.

## Review view

Add a TUI mode or dialog for changed files.

Suggested commands:

```text
/review
/diff
/diff <path>
/revert <path>
/tests
/tests last
/tests failed
```

Start with file-level diff navigation. Hunk-level accept/reject is optional and should not be attempted unless the existing codebase already has reliable patch application helpers.

Review view should show:

- File path.
- Change kind.
- Diff.
- Optional rationale if available from `FileChangedEvent.summary`.
- Test status if related.

## Diff generation

Use existing diff functionality if present. The repo already depends on `similar`, so prefer that for in-process textual diffs if no git diff abstraction exists.

If inside a git repo, `git diff -- <path>` is acceptable, but command execution must respect existing sandbox/permission rules.

## Test state

Add:

```rust
pub enum TestState {
    Unknown,
    Stale,
    Running { command: String },
    Passed { command: String, duration_ms: Option<u64> },
    Failed { command: String, duration_ms: Option<u64>, summary: String },
}
```

Emit `TestRunStarted` and `TestRunFinished` events when common test commands are executed through the agent.

Heuristic test commands:

- `cargo test`
- `cargo nextest`
- `npm test`
- `pnpm test`
- `pytest`
- `uv run pytest`
- `go test`
- `zig build test`

For codegg itself, make sure `cargo test` is recognized.

## Stale test detection

Mark tests as stale when a file changes after the last passing test event.

Do not overcomplicate dependency mapping in the first pass. A simple timestamp comparison is enough.

## Acceptance criteria

- `/review` opens changed-file review view or dialog.
- `/diff` shows a repository diff or changed file list.
- `/diff <path>` shows a file-specific diff.
- `/tests` shows current test state.
- Running `cargo test` through the agent updates test state.
- File changes after a passing test mark tests stale.
- Existing F11 IDE diff behavior, if present, is not removed.

---

# Phase 5: Context/session observability and handoff export

## Objective

Make context pressure, compaction, model routing, and exportable handoff artifacts visible.

This phase is important for codegg’s larger goal: reducing context pollution and enabling useful smaller-model handoffs.

## Context inspector

Add or extend `/context`.

The context inspector should show sections like:

```text
Pinned
  current goal
  active plan
  AGENTS.md
  selected skill: code-review

Summarized
  previous exploration
  latest test failure
  reviewer findings

Retrieved
  src/agent/prompt.rs
  src/model/catalog.rs

Excluded
  target/
  large generated files
  raw shell logs
```

If the current context system does not expose all these categories, render what is available and mark unavailable sections as omitted. Do not fabricate context details.

## Context pressure display

Add compact status display:

```text
ctx: 71%
```

If exact token accounting is unavailable, display approximate or unknown:

```text
ctx: unknown
ctx: ~71%
```

Do not imply precision that does not exist.

## Compaction report

When `/compact` runs or automatic compaction occurs, emit a `ContextCompacted` event.

Render a compact report:

```text
Compaction completed
Before: 91k tokens
After: 18k tokens
Preserved: goal, plan, 4 file summaries, latest test failure
Dropped: raw shell logs, resolved errors
```

If before/after token counts are unavailable, still record preserved/dropped categories if known.

## Model routing trace

When model switching or subagent routing occurs, emit `ModelRouted`.

Render compact route cards:

```text
Reviewer spawned: anthropic/claude-sonnet
Profile: workhorse
Reason: patch-level code review, medium complexity
```

If routing is manual, reason can be `user selected`.

## Handoff export

Add:

```text
/export handoff
```

The output should be copied to clipboard if clipboard support is enabled and/or printed to a file if the existing export system supports file output.

Suggested handoff format:

```markdown
# Codegg Handoff

## Goal

...

## Current plan

- [x] ...
- [ ] ...

## Current state

- Model:
- Branch:
- Changed files:
- Tests:
- Context pressure:

## Files changed

...

## Relevant findings

...

## Latest test result

...

## Suggested next steps

...

## Notes for smaller model

- Do not broaden scope.
- Prefer minimal patches.
- Run targeted tests first.
```

This is especially useful for handing work to MiMo 2.5 or another lower-cost model.

## Session search

Optional for this phase, but useful if easy:

```text
/search <query>
```

Search over:

- Transcript messages.
- Event summaries.
- Tool-call summaries.
- File paths.
- Findings.

Do not implement a full fuzzy finder unless one already exists.

## Acceptance criteria

- `/context` shows structured context information where available.
- Context pressure is visible in the status bar or state panel.
- Compaction emits and renders a report.
- Model routing or model switching emits a visible event.
- `/export handoff` produces a concise markdown handoff artifact.
- Handoff artifact includes goal, plan, changed files, tests, findings, and next steps.

---

# TUI mode refinement

## Objective

Avoid overloading the chat transcript by introducing conceptual views.

Do not necessarily build a fully separate mode system if that is too invasive. Start by adding view/dialog commands that approximate modes.

Suggested views:

```text
Chat      existing transcript and prompt
Work      transcript + task spine + active tool
Review    changed files and diffs
Inspect   context/events/session state
```

Minimum viable implementation:

- `/review` opens review view.
- `/context` opens inspect/context view.
- `/state` opens work/session state view.
- Existing default remains chat/work hybrid.

Future implementation can formalize:

```rust
pub enum TuiView {
    Chat,
    Work,
    Review,
    Inspect,
}
```

Acceptance criteria:

- User can enter and exit review/context/state views.
- Existing chat input remains usable.
- Narrow terminals remain usable.

---

# Command palette

## Objective

Unify slash commands, keyboard shortcuts, and future UI commands around named command actions.

This can start as a small registry without replacing all existing commands.

## Suggested type

```rust
pub struct TuiCommand {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub category: CommandCategory,
}

pub enum CommandCategory {
    Session,
    Agent,
    Model,
    Tools,
    Review,
    Context,
    Git,
    Export,
    Tui,
}
```

Add a command palette command:

```text
:
```

or reuse existing Vim command mode if present.

Minimum behavior:

- Search command names and descriptions.
- Execute selected command.
- Show keybinding if one exists.

Do not implement this before Phases 1–2 unless command handling is already centralized.

Acceptance criteria:

- At least the new commands are registered in one place.
- `/help` or command palette can show them.
- Slash command parsing is not made worse.

---

# Safety and permission requirements

Preserve existing sandbox and permission semantics.

Any new actions that mutate files, git state, dependencies, or session storage must either:

- Reuse existing permission checks, or
- Be clearly local and safe, such as updating in-memory TUI view state.

High-risk actions must remain confirmable:

- Revert file.
- Git mutation.
- Dependency update.
- Delete session.
- Delete event history.
- Network access.
- Destructive shell command.

Never hide raw command details from the user when requesting permission.

---

# Testing strategy

Add unit tests for pure state logic first.

Priority tests:

1. `TuiSessionState::apply_event` updates goal.
2. Plan events update active/completed items.
3. Tool call start/finish lifecycle works.
4. File change events populate changed files.
5. Test state becomes stale after file change.
6. Context compaction event updates context state.
7. Tool risk classifier classifies common commands conservatively.
8. Handoff export includes required sections.

Add integration-ish tests only where existing test structure makes it easy.

Do not require snapshot testing unless the repo already uses it.

Run:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features
```

If `--all-features` fails because optional features require unavailable system dependencies, document the failure and run the default feature set.

---

# Suggested implementation order for MiMo 2.5

## Step 1

Inspect the existing source tree.

Find:

- TUI app state.
- Slash command parser.
- Session persistence.
- Tool execution path.
- Permission prompt path.
- Diff/git helpers.
- Context compaction path.
- Model selection/routing path.

Write down the actual file paths before editing.

## Step 2

Add event/state types and tests.

Keep this mostly isolated.

Do not wire every subsystem immediately.

## Step 3

Persist events.

Add a database table or use existing persistence primitives.

Verify old sessions still load.

## Step 4

Wire goal and plan commands.

Implement `/goal`, `/plan`, and side-panel rendering.

This gives visible value early.

## Step 5

Wire tool-call events.

Emit `ToolCallStarted` and `ToolCallFinished`.

Render tool cards.

Add risk classifier.

## Step 6

Add changed-file and test-state tracking.

Use simple heuristics.

Expose `/review`, `/diff`, and `/tests`.

## Step 7

Add context and handoff export.

Extend `/context`.

Implement `/export handoff`.

## Step 8

Polish layout and docs.

Update README command list.

Add a short docs page if the repo has a docs directory.

---

# README/doc updates

Update the README TUI slash command list to include any commands actually implemented:

```text
/goal <text>
/plan
/plan add <text>
/plan done <index>
/plan skip <index>
/plan clear
/state
/review
/diff [path]
/tests
/export handoff
```

Add a concise explanation:

```markdown
## Structured TUI State

Codegg tracks task state separately from the chat transcript. The TUI can show the active goal, plan, changed files, tool calls, test state, context pressure, and model routing decisions. This makes long-running agent sessions easier to inspect and resume.
```

Do not document unimplemented commands.

---

# Implementation constraints

Prefer small PR-sized commits.

Do not rename major modules unless necessary.

Do not change public config format unnecessarily.

Do not break existing slash commands.

Do not require users to opt into the new state panel for basic functionality.

Do not store very large raw tool outputs directly in the event payload unless existing storage already does that safely.

Avoid adding global mutable state.

Avoid blocking terminal rendering on slow database calls.

Avoid running git commands on every frame. Cache changed-file state and refresh on explicit events or periodic low-frequency ticks.

---

# Performance considerations

The TUI must remain responsive while tools and model streams are running.

Rules:

- Event application should be cheap.
- Rendering should not perform filesystem scans.
- Git status/diff should be computed on demand or cached.
- Long raw outputs should be collapsed and lazily viewed.
- Database persistence should be async and not block UI input.
- Large event histories should be paginated or summarized in future work.

For this pass, it is acceptable to load all events for a session if session sizes are modest. If the repo already has long-session concerns, add a limit and derive current state from a snapshot plus recent events.

---

# Failure behavior

If event persistence fails, the user should see a non-fatal warning unless the action itself depends on persistence.

If a diff cannot be generated, show an explanatory error in the review view.

If git is unavailable, fall back to internally tracked file-change events.

If context token accounting is unavailable, show `ctx: unknown`.

If model route reason is unavailable, show only provider/model.

---

# Final acceptance checklist

The implementation is complete when:

- Code builds.
- Existing TUI still launches.
- Existing chat interaction still works.
- Existing slash commands are not broken.
- User can set a goal and view it in the TUI.
- User can create/update a plan and view it in the TUI.
- Tool calls render as structured cards.
- Permission prompts show risk where available.
- Changed files are visible.
- `/review` or equivalent opens a changed-file/diff view.
- Test state is visible and becomes stale after file changes.
- `/context` or equivalent shows structured context status.
- Compaction produces a visible report when available.
- Model switching/routing creates a visible trace.
- `/export handoff` creates a markdown handoff artifact.
- Session state survives reload.
- Tests cover event/state logic and risk classification.
- README/docs reflect only implemented commands.

