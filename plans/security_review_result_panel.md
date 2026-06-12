# Security Review Result Panel / Cancellation Handoff Plan

## Purpose

Build the next UX layer for `/security-review` now that the command runs asynchronously and local-mode LSP enrichment is wired. The goal is to make security review output usable beyond a long message-log report: add a dedicated result panel with finding/prompt navigation, filters, latest-review persistence, and cancellation for in-flight reviews.

Current state:

- `/security-review` dispatches asynchronously through `TuiCommand::SecurityReviewRun`.
- `run_security_review_background(root, args, lsp_tool)` owns its inputs and preserves local LSP enrichment.
- `App.security_review_running: Option<String>` prevents concurrent review pileups.
- Completed reviews are pushed into the message timeline as an assistant message with a `[Security Review]` label.
- Remote/socket mode still falls back to deterministic stage-1 with an unavailable enrichment note.

This pass should improve review ergonomics without changing the core evidence/finding model.

## Non-Goals

Do not change the security review synthesis rules.

Do not make LSP enrichment default.

Do not mutate source files.

Do not add exploit generation, network scanning, or dependency/CVE lookup.

Do not require a live LSP server in unit tests.

Do not build a full IDE problem panel. This is a focused security review panel.

## Phase 1 — Add Structured Review Receipt DTO

The TUI currently receives a rendered report string. For a navigable panel, preserve structured data as well.

Add a TUI-facing receipt type near the workflow/report boundary or TUI state layer:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReviewReceipt {
    pub id: String,
    pub root: PathBuf,
    pub args: SecurityReviewCommandArgs,
    pub output: SecurityReviewOutput,
    pub rendered_report: String,
    pub completed_at_ms: i64,
    pub enriched: bool,
    pub lsp_available: bool,
}
```

If `SecurityReviewOutput` is too large or not convenient to store directly in TUI state, create a smaller projection:

```rust
pub struct SecurityReviewPanelItem {
    pub kind: SecurityReviewPanelItemKind, // Finding | Prompt | Note | Preflight
    pub file_path: Option<PathBuf>,
    pub line: Option<usize>,
    pub title: String,
    pub severity: Option<SecuritySeverity>,
    pub confidence: Option<SecurityConfidence>,
    pub summary: String,
    pub detail: Vec<String>,
}
```

Acceptance criteria:

- the async runner or completion handler can produce a structured receipt;
- rendered text remains available for message-log output;
- receipt is cloneable enough for TUI state;
- no source mutation or patch data is embedded.

## Phase 2 — Extend Completion Path to Carry Receipt

Update the security review completion flow so the event loop can store the receipt and render the panel.

Current flow likely resembles:

```rust
TuiCommand::SecurityReviewRun { id, root, args, lsp_tool }
```

Add either:

```rust
TuiCommand::SecurityReviewFinished {
    id: String,
    receipt: SecurityReviewReceipt,
}
```

or keep the handler local but have it call a state mutation helper:

```rust
app.set_latest_security_review(receipt);
```

Rules:

- still push a concise `[Security Review]` report into the message timeline;
- also store `latest_security_review` in `App` or `DialogState`;
- completion must clear `security_review_running` only when the id matches;
- stale completion ids must not clear a newer run.

Acceptance criteria:

- latest review can be reopened after completion;
- stale run completion does not corrupt active state;
- message timeline behavior remains intact;
- remote fallback notes are preserved in the receipt.

## Phase 3 — Add Result Panel State

Add a dedicated panel/dialog state, likely under `DialogState` or a new component module.

Suggested state:

```rust
pub struct SecurityReviewDialog {
    pub receipt: Option<SecurityReviewReceipt>,
    pub selected_index: usize,
    pub scroll: u16,
    pub filter: SecurityReviewFilter,
    pub show_notes: bool,
    pub show_prompts: bool,
    pub show_findings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityReviewFilter {
    All,
    Findings,
    Prompts,
    Notes,
    HighConfidence,
    MediumOrHigherSeverity,
}
```

Add to `Dialog` enum:

```rust
SecurityReview
```

or reuse an existing review dialog only if it is already meant for this purpose.

Acceptance criteria:

- panel can open with latest receipt;
- panel has deterministic selection/scroll state;
- panel remains useful for output with only prompts and no findings;
- panel handles empty review output gracefully.

## Phase 4 — Render Result Panel

Render the panel with a concise master/detail layout.

Preferred layout:

```text
Security Review — <root or branch/ref>
Findings: N  Prompts: N  Notes: N  Enrichment: local-lsp | unavailable | off

[List]
[HIGH/MEDIUM] file.rs:42  SQL interpolation review
[PROMPT]      auth.rs:88  Authorization bypass review
[NOTE]        LSP enrichment executed 3 request(s).

[Detail]
Evidence: ...
Recommendation: ...
Suggested tests: ...
```

Minimum panel requirements:

- counts in header;
- list findings first, then prompts, then notes;
- selected item detail view;
- file path and line shown when available;
- clear marker that prompts are not confirmed findings;
- enrichment status visible.

Acceptance criteria:

- long reports are navigable without relying on a giant message bubble;
- findings/prompts remain distinguishable;
- notes and fallback states are visible;
- no panic on missing file/line/severity/confidence.

## Phase 5 — Navigation and Actions

Add key handling while panel is open.

Suggested bindings:

```text
j/k or Down/Up       move selection
PageDown/PageUp      scroll detail/list
f                    cycle filters
n                    toggle notes
p                    toggle prompts
Enter                open/jump to selected file if available
Esc/q                close panel
```

For jump-to-file:

- use an existing file-open/navigation command if one exists;
- if no editor/file viewer exists yet, copy or surface the path/line and show a toast;
- do not auto-modify the file.

Acceptance criteria:

- list navigation works;
- filter cycling works;
- jump action is safe and read-only;
- panel close returns to prior TUI state.

## Phase 6 — Add `/security-review-show` Command

Add a lightweight command to reopen the latest result panel.

```text
/security-review-show
/security-review-show latest
/security-review-show report   # optional: show rendered report in message log again
```

Behavior:

- if no review exists, show `No security review result available yet.`;
- if a receipt exists, open `Dialog::SecurityReview`;
- do not rerun the review.

Acceptance criteria:

- latest review is accessible after the completion toast disappears;
- command does not trigger new security analysis;
- works after prompt clearing and normal navigation.

## Phase 7 — Add Cancellation Path

The current reentrancy guard says wait for completion. Add cancellation if feasible now.

Preferred implementation:

```rust
pub struct SecurityReviewTaskState {
    pub id: String,
    pub abort_handle: tokio::task::AbortHandle,
}
```

Store in `App`:

```rust
pub security_review_running: Option<SecurityReviewTaskState>
```

If this makes `App` `Clone`/`Debug` awkward, keep:

```rust
pub security_review_running: Option<String>,
pub security_review_abort: Option<tokio::task::AbortHandle>,
```

Add command:

```text
/security-review-cancel
```

Behavior:

- if no review running: warning toast `No security review is running.`;
- if running: abort the task, clear active guard, show `Security review cancelled.`;
- if completion arrives after cancellation, ignore it unless id still matches active guard.

Important:

- aborting a spawned task should not leave UI busy state stuck;
- cancellation is best-effort. If lower layers are in a non-cancellable blocking operation, document that completion may still arrive;
- no partial report should be marked as successful unless explicitly supported.

Acceptance criteria:

- user can cancel an in-flight security review;
- active guard clears on cancellation;
- stale completion is ignored;
- cancellation does not mutate files;
- remote/local fallback semantics are unchanged.

## Phase 8 — Store Latest Receipt Without Persistence First

For this pass, in-memory latest-review state is enough.

Add:

```rust
pub latest_security_review: Option<SecurityReviewReceipt>
```

or place it under `DialogState` if better aligned with current TUI architecture.

Do not add database/session persistence in this pass unless an existing message metadata mechanism makes it trivial.

Acceptance criteria:

- latest receipt is available until app exit or session reset;
- reset behavior is explicit;
- no database migration required.

## Phase 9 — Tests

Add tests around state and rendering helpers rather than live TUI rendering if needed.

Suggested tests:

```text
security_review_receipt_projection_preserves_findings_prompts_notes
security_review_panel_filters_findings_only
security_review_panel_filters_prompts_only
security_review_panel_handles_no_findings
security_review_panel_selection_clamps_on_filter_change
security_review_show_without_receipt_warns
security_review_show_with_receipt_opens_dialog
security_review_cancel_without_active_run_warns
security_review_cancel_aborts_and_clears_guard
security_review_cancel_ignores_stale_completion
security_review_completion_stores_latest_receipt
security_review_completion_opens_or_updates_panel_state
```

If render tests exist:

```text
security_review_panel_renders_enrichment_unavailable_note
security_review_panel_renders_prompt_not_finding_marker
security_review_panel_renders_file_line_location
```

Acceptance criteria:

- no test needs a live LSP server;
- no test mutates repo files;
- cancellation/reentrancy state is covered;
- panel projection/filter logic is covered.

## Phase 10 — Docs Updates

Update:

```text
README.md
AGENTS.md
architecture/lsp.md
architecture/tool.md
.opencode/skills/security/SKILL.md
```

Document:

- `/security-review` runs asynchronously;
- completed reviews open/store a dedicated result panel;
- `/security-review-show` reopens latest result;
- `/security-review-cancel` aborts active review;
- prompts are not confirmed findings;
- local LSP enrichment remains opt-in;
- remote/socket mode still falls back deterministically.

Acceptance criteria:

- docs match implemented command names and keybindings;
- docs mention that cancellation is best-effort if that is true;
- docs do not imply exploit generation or mutation.

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
cargo test -p codegg security_review_panel
cargo test -p codegg security_review_receipt
cargo test -p codegg security_review_cancel
cargo test -p codegg security_review_show
cargo test -p codegg security_review_background
rg "SecurityReviewDialog|SecurityReviewReceipt|latest_security_review|security_review_cancel|security-review-show|security-review-cancel" src tests README.md AGENTS.md architecture .opencode
rg "AbortHandle|security_review_running|SecurityReviewRun" src/tui src/security tests
```

Manual smoke:

```text
1. Run /security-review --changed. Confirm result arrives and latest panel can open.
2. Run /security-review-show. Confirm latest receipt opens without rerunning.
3. Run /security-review --changed --enrich in local mode. Confirm enrichment status appears in panel.
4. Run /security-review --changed --enrich in remote/socket mode. Confirm unavailable note appears in panel.
5. Start a review, then run /security-review-cancel. Confirm guard clears and no success report is shown unless task completed before cancellation.
6. Try /security-review-cancel when idle. Confirm warning toast.
7. Navigate findings/prompts/notes with keyboard.
8. Use jump action on a file/line item. Confirm it is read-only.
```

## Done Criteria

This phase is complete when:

- latest security review is stored as a structured receipt;
- a dedicated result panel/dialog exists;
- findings, prompts, and notes are navigable;
- prompts are clearly labeled as not confirmed findings;
- latest result can be reopened without rerunning;
- cancellation exists for active reviews or is explicitly deferred with a reason;
- stale completion cannot corrupt active state after cancellation;
- long reports are no longer only consumable as message-log text;
- all behavior remains read-only, opt-in, bounded, and fail-soft.

## Follow-Up Passes

After this phase, likely next work:

1. Persist security review receipts to session/message metadata.
2. Add side-by-side diff/hunk view for selected findings.
3. Add project policy config for severity thresholds and ignored paths.
4. Add remote-core LSP enrichment support if the daemon owns LSP state.
5. Add dependency/CVE enrichment for dependency review targets.
