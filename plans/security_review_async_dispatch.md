# Security Review Async Dispatch / UI Responsiveness Plan

## Purpose

Move `/security-review` execution off the inline blocking TUI command path. Preserve the newly wired local-mode `LspSecurityContextExecutor`, preserve deterministic remote/socket fallback, and make the UI remain responsive while diff discovery, preflight checks, and optional LSP enrichment run.

Current state:

- Local TUI mode owns `App.lsp_tool: Option<Arc<LspTool>>`.
- Local startup creates `LspTool` from configured LSP settings and allowed project root.
- `/security-review --enrich` builds `LspSecurityContextExecutor` from `App.lsp_tool` when available.
- Remote/socket mode has `lsp_tool = None` and falls back to deterministic stage-1 with an unavailable note.
- The current TUI handler still executes the review with `tokio::task::block_in_place` plus `Handle::current().block_on(...)`, which can stall the UI.

This pass should change execution dispatch, not the security review logic itself.

## Non-Goals

Do not rewrite `run_security_review_workflow`.

Do not change the security evidence/finding eligibility model.

Do not make enrichment default.

Do not remove deterministic fallback.

Do not add dependency/CVE lookup.

Do not mutate source files.

Do not add network scanning.

Do not generate exploit payloads or offensive guidance.

Do not introduce an unbounded task queue.

## Phase 1 — Identify Existing Async TUI Command/Event Pattern

Before adding new machinery, inspect how long-running TUI work is currently dispatched.

Search:

```bash
rg "tokio::spawn|spawn_blocking|block_in_place|Handle::current\(\)\.block_on" src/tui src crates -g '*.rs'
rg "TuiCommand|RemoteTuiMessage|AppEvent|Toast|ResearchLoad|Goal" src/tui src crates -g '*.rs'
rg "messages_state.toasts|add_assistant|add_live_output_delta|handle_remote_event" src/tui -g '*.rs'
```

Classify available patterns:

1. background task sends back a TUI command/event;
2. remote core event arrives and updates state;
3. direct toast/dialog update after synchronous work;
4. command palette dispatch via `tui_cmd_tx`.

Pick the lowest-friction existing pattern. Prefer reusing an existing TUI command/event channel over adding a new one.

Acceptance criteria:

- chosen dispatch path is documented in the implementation comments;
- no new global runtime is created;
- no long-running work remains inside the immediate command handler.

## Phase 2 — Add Security Review Task Events

Add explicit events/messages for security review lifecycle.

Preferred shape if using existing app-local events:

```rust
pub enum AppEvent {
    // existing variants...
    SecurityReviewStarted { id: SecurityReviewRunId },
    SecurityReviewFinished { id: SecurityReviewRunId, report: String },
    SecurityReviewFailed { id: SecurityReviewRunId, error: String },
}
```

If the codebase uses `TuiCommand` for internal work, use equivalent variants there:

```rust
pub enum TuiCommand {
    // existing variants...
    SecurityReviewRun {
        id: String,
        root: PathBuf,
        args: SecurityReviewCommandArgs,
        lsp_tool: Option<Arc<LspTool>>,
    },
    SecurityReviewFinished {
        id: String,
        report: String,
    },
    SecurityReviewFailed {
        id: String,
        error: String,
    },
}
```

Avoid sending huge structured outputs through the TUI channel unless there is an existing pattern for it. A rendered report string is acceptable for this pass.

Acceptance criteria:

- start/success/failure state can be represented;
- event payloads are bounded;
- no source mutation or security action is embedded in the event.

## Phase 3 — Add a Background Runner Function

Extract the actual review execution into a function that can be spawned.

Recommended helper:

```rust
async fn run_security_review_background(
    root: PathBuf,
    args: SecurityReviewCommandArgs,
    lsp_tool: Option<Arc<LspTool>>,
) -> Result<String, String> {
    let executor = lsp_tool
        .map(|tool| LspSecurityContextExecutor::new(tool));
    let executor_ref = executor
        .as_ref()
        .map(|e| e as &dyn SecurityContextExecutor);

    run_security_review_command_with_executor(
        &root,
        &args,
        executor_ref,
    ).await
}
```

Important lifetime detail:

- construct `LspSecurityContextExecutor` inside the spawned task from `Arc<LspTool>`;
- do not borrow `self.lsp_tool` across spawn boundaries;
- clone `Arc<LspTool>` before spawning.

Acceptance criteria:

- helper owns `root`, `args`, and cloned `Arc<LspTool>`;
- no borrowed `&self` survives into the task;
- local-mode enrichment still uses the real executor;
- remote/socket mode passes `None` and gets deterministic fallback.

## Phase 4 — Replace Inline Blocking Handler

Replace the current inline behavior:

```rust
tokio::task::block_in_place(|| {
    tokio::runtime::Handle::current().block_on(async { ... })
})
```

with non-blocking dispatch.

Target behavior in `/security-review` command handler:

1. parse args;
2. capture `root`;
3. clone `self.lsp_tool`;
4. create run id;
5. immediately show toast/status: `Security review started`;
6. spawn background task;
7. return control to TUI.

Pseudo-code:

```rust
let args = parse_security_review_args(raw_args);
let root = current_dir_or_project_dir();
let lsp_tool = self.lsp_tool.clone();
let tx = self.tui_cmd_tx.clone();
let id = SecurityReviewRunId::new();

self.messages_state.toasts.info("Security review started...");
self.session_state.session_status = SessionStatus::Working; // only if appropriate

tokio::spawn(async move {
    let result = run_security_review_background(root, args, lsp_tool).await;
    match result {
        Ok(report) => send_finished(tx, id, report),
        Err(error) => send_failed(tx, id, error),
    }
});
```

If no suitable `tx` exists in the command handler, add a small app-local mpsc channel for security-review completions and poll it in the existing event loop.

Acceptance criteria:

- no `block_in_place` remains in `/security-review` path;
- handler returns immediately;
- user gets immediate start feedback;
- final report/error arrives via event/channel;
- local LSP executor still works inside the spawned task.

## Phase 5 — Handle Completion in UI

Add handling for completion/failure events.

On success:

- clear any security-review busy status;
- show a concise success toast;
- add report to an appropriate UI surface.

Preferred report surface for this pass:

1. existing message timeline as assistant/system text, if available;
2. existing review/dialog panel, if available;
3. toast fallback only if there is no multiline report surface.

Avoid putting a long full report only in a toast if the report can exceed a few lines.

On failure:

- clear busy status;
- show error toast;
- preserve error text in the message log if that is standard for command failures.

Acceptance criteria:

- final report is visible and not lost;
- errors are visible and do not panic;
- UI busy state does not get stuck;
- remote fallback note is visible when applicable.

## Phase 6 — Add Cancellation / Reentrancy Guard

Prevent multiple heavy security reviews from piling up accidentally.

Minimal acceptable policy:

```rust
pub security_review_running: Option<String>
```

If a run is already active and user starts another:

```text
Security review already running. Wait for it to finish or cancel it.
```

If existing task cancellation infrastructure exists, wire it later; for this pass, a reentrancy guard is enough.

On completion/failure, clear the active run id.

Acceptance criteria:

- repeated `/security-review` commands do not spawn unbounded concurrent runs;
- active flag clears on success/failure;
- tests or manual smoke cover duplicate start behavior.

## Phase 7 — Preserve Security Boundaries

The async move must not weaken current security constraints.

Verify:

- `LspSecurityContextExecutor` still validates requests;
- allowed root is still project root;
- enrichment remains opt-in via `--enrich`;
- no executor in remote/socket mode still yields deterministic stage-1 plus unavailable note;
- no mutation, shell execution, or network scanning is introduced;
- no long-lived borrow of mutable TUI state crosses await boundaries.

Acceptance criteria:

- spawned task owns only safe cloned handles and arguments;
- no mutable UI state is moved into the task;
- command remains read-only.

## Phase 8 — Tests

Add unit tests where possible and keep integration tests light.

### Command dispatch tests

If TUI command handling is testable:

```text
security_review_command_dispatches_background_task
security_review_command_does_not_block_inline
security_review_command_rejects_second_run_while_active
security_review_command_completion_clears_active_run
security_review_command_failure_clears_active_run
```

### Background runner tests

These should not require a live LSP server:

```text
security_review_background_without_executor_returns_unavailable_note
security_review_background_with_fixture_executor_uses_enrichment
security_review_background_json_mode_returns_json
security_review_background_preserves_prompts_only
security_review_background_preserves_findings_only
```

### Regression tests

```text
security_review_default_path_does_not_create_executor
security_review_remote_mode_none_executor_is_deterministic
security_review_local_mode_executor_arc_is_cloned_not_borrowed
```

If UI-level async tests are too heavy, test extracted pure helpers and event handlers.

Acceptance criteria:

- no test requires live LSP;
- fixture/noop executors cover both branches;
- no blocking calls remain in command handler path;
- active-run guard is covered.

## Phase 9 — Docs Updates

Update:

```text
AGENTS.md
README.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
```

Document:

- `/security-review` runs asynchronously in the TUI;
- UI remains responsive while review runs;
- local mode can use real `LspSecurityContextExecutor` for `--enrich`;
- remote/socket mode falls back deterministically with unavailable note;
- only one security review can run at a time, if guard is added;
- enrichment remains opt-in, read-only, bounded, and fail-soft.

Acceptance criteria:

- docs match actual runtime behavior;
- docs do not imply remote mode has local LSP enrichment;
- docs mention no mutation/no exploit/no network scanning semantics.

## Validation Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Targeted:

```bash
cargo test -p codegg security_review_background
cargo test -p codegg security_review_command_dispatch
cargo test -p codegg security_review_command_with_executor
cargo test -p codegg security_context_request_validation
rg "block_in_place|Handle::current\(\)\.block_on" src/tui src/security crates
rg "SecurityReviewStarted|SecurityReviewFinished|SecurityReviewFailed|security_review_running|run_security_review_background" src crates tests
rg "LspSecurityContextExecutor|lsp_tool|--enrich" src/tui src/security README.md AGENTS.md architecture .opencode
```

Manual smoke:

```text
1. Run /security-review --changed. Confirm UI remains responsive immediately after command.
2. Run /security-review --changed --enrich in local mode. Confirm LSP executor path still works and report arrives later.
3. Run /security-review --changed --enrich in socket/remote mode. Confirm deterministic fallback plus unavailable note.
4. Start a second security review while one is running. Confirm guard prevents unbounded concurrent runs.
5. Run /security-review --changed --json --enrich. Confirm JSON report arrives through completion path.
```

## Done Criteria

This pass is complete when:

- `/security-review` no longer uses `block_in_place` or inline `block_on`;
- review execution runs in a background task or existing async command pipeline;
- local-mode `LspSecurityContextExecutor` is preserved;
- remote/socket fallback is preserved;
- final report/error is delivered through UI event handling;
- UI active/busy state is cleared on success and failure;
- unbounded concurrent review runs are prevented;
- docs and tests reflect async dispatch;
- security boundaries remain read-only, opt-in, bounded, and fail-soft.

## Follow-Up Passes

After this lands, likely next targets are:

1. Dedicated security review result panel with finding navigation.
2. Persist latest security review receipt in session state.
3. Project-level security policy config for thresholds, budgets, and ignored paths.
4. Remote-core LSP enrichment support if the daemon owns LSP state.
