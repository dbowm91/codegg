# Phase 08: First-Class Run Surfaces in TUI and Protocol

## Objective

Expose command, test, native-route, and Python runs as first-class inspectable objects in codegg’s TUI and protocol surfaces. Phase 07 creates durable run manifests and artifacts; this phase should make those runs visible, navigable, rerunnable, promotable, and reversible without dumping raw logs into the conversation by default.

The user-facing goal is that executable activity appears as a coherent run cell with status, backend, policy, projection, artifacts, and changed files rather than as undifferentiated tool text.

## Scope

This phase covers:

- TUI run cells;
- run detail overlays/panels;
- artifact browsing;
- protocol events and serialization;
- rerun actions;
- context promotion actions;
- rollback/revert actions for supported transforms;
- accessibility and compact rendering;
- tests for frontend-independent view models and TUI behavior.

## Existing substrate to reuse

Reuse:

- Phase 07 `RunManifest`, `RunSummary`, and artifact APIs;
- current TUI tool-call cells and result rendering;
- shell/test-runner status rendering;
- diff/hunk navigation primitives;
- protocol conversion patterns;
- session event stream;
- context promotion semantics from `!`/`!!` and projection architecture;
- existing modal/overlay navigation conventions.

## Design principles

1. The run manifest is the source of truth; the TUI must not reconstruct semantics from log text.
2. Compact summaries should be useful without opening details.
3. Raw artifacts remain opt-in.
4. Changed files and diffs should use existing source/diff navigation.
5. Rerun and rollback are explicit actions with permission checks.
6. Protocol consumers should receive structured events, not TUI-specific formatting.

## Workstream A: Define frontend-independent run view models

Add a view-model layer such as:

```rust
pub struct RunCellView {
    pub run_id: RunId,
    pub title: String,
    pub kind: RunKind,
    pub status: RunStatus,
    pub backend_label: String,
    pub duration: Option<Duration>,
    pub risk_label: String,
    pub sandbox_label: Option<String>,
    pub summary: String,
    pub changed_file_count: usize,
    pub artifact_count: usize,
    pub can_rerun: bool,
    pub can_rollback: bool,
    pub context_state: ContextPromotionState,
}
```

Add detail models for:

- invocation;
- permissions;
- policy/sandbox evidence;
- output artifacts;
- projection;
- changed files/diff;
- parent/child rerun lineage.

Keep view model construction outside rendering code.

## Workstream B: Add compact run cells

Each run cell should render:

- concise title (`cargo test`, `Python transform`, `git diff`, etc.);
- state icon/label: queued, running, success, failed, timed out, denied, cancelled;
- backend: raw shell, test runner, Python, native git, managed argv;
- elapsed duration;
- compact projection/summary;
- changed-file count;
- policy warning when portable fallback or degraded enforcement was used;
- indication when output is truncated or RTK-compressed;
- expandable artifact indicator.

Avoid showing long classifier/planner metadata in the default cell.

## Workstream C: Add run detail overlay/panel

Provide tabs or sections:

1. Summary
2. Invocation
3. Output
4. Artifacts
5. Changes
6. Policy
7. Context

Required actions:

- open full stdout/stderr artifact;
- switch between raw and projected output;
- copy invocation/run ID;
- open changed file or diff hunk;
- rerun;
- promote projection or selected artifact excerpt;
- pin/unpin run;
- rollback when supported;
- open parent/child rerun lineage.

Use bounded paging/streaming for large artifacts.

## Workstream D: Artifact viewer

Implement a reusable artifact viewer supporting:

- text/log artifacts;
- JSON pretty view;
- unified diffs;
- line numbers;
- search within artifact;
- byte/line range loading;
- truncation marker and load-more;
- copy selected range;
- promote selected range to context;
- redaction state indicator.

Do not load entire multi-megabyte artifacts into TUI memory.

## Workstream E: Changed-file and rollback actions

For Transform/Python or future mutating runs:

- show changed files and status (created/modified/deleted);
- open unified diff using existing diff navigation;
- record pre-run snapshot/worktree state required for rollback;
- support rollback only when the current workspace state is compatible;
- detect conflicts or subsequent edits and refuse destructive rollback;
- provide preview before rollback;
- rerun current permissions for rollback mutation.

Rollback should create its own run record with parent linkage.

## Workstream F: Rerun actions

Rerun flow:

```text
selected RunManifest
  -> reconstruct RerunDescriptor
  -> classify/plan again
  -> resolve current permissions/config/workspace
  -> execute as new RunId
  -> link parent_run_id
```

Allow controlled edits before rerun where appropriate:

- command args;
- Python mode/source;
- cwd within workspace;
- timeout.

Never reuse stale permission approvals automatically.

## Workstream G: Context promotion UI

Expose explicit context states:

- local-only;
- projection included;
- selected artifact range included;
- pinned for future context;
- excluded/redacted.

Actions should use the shared context-promotion policy rather than directly appending text to chat state.

The TUI should show token/size estimates before promoting large artifacts.

## Workstream H: Protocol/events

Add stable protocol messages/events for:

- run started;
- run updated/progress;
- artifact created;
- projection ready;
- run completed;
- run denied;
- run pinned/unpinned;
- context promotion changed;
- rollback/rerun relationship.

Ensure events are frontend-neutral and versioned. GUI/web/mobile clients should be able to render equivalent run surfaces later.

## Workstream I: Running-state updates

For long-running tests/commands:

- update elapsed time without LLM polling;
- show deterministic progress from test runner/process monitor;
- attach partial output artifacts without promoting them automatically;
- support cancel/interrupt;
- preserve final run record after cancellation.

This should integrate with the deterministic test-handling work rather than create a second polling loop.

## Workstream J: Tests

Add tests for:

- manifest -> run cell view mapping;
- all run statuses;
- degraded sandbox warning;
- artifact paging and range reads;
- raw/projected toggle;
- context promotion state transitions;
- rerun re-evaluates permissions;
- rollback refusal after conflicting workspace changes;
- parent/child lineage;
- protocol event serde/versioning;
- long-running progress events;
- cancellation persistence;
- TUI keyboard navigation and focus behavior.

Use snapshot/golden tests only where stable and maintainable.

## Validation commands

```bash
cargo test -p codegg-core run_store
cargo test -p codegg --lib protocol_conversions
cargo test -p codegg --lib tui
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib python_script
```

Run focused TUI integration tests if present, then:

```bash
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

## Acceptance criteria

- command/Python/test/native runs render as structured TUI cells;
- users can inspect raw and projected artifacts without context injection;
- changed files and diffs are navigable;
- rerun creates a new run and rechecks policy/permissions;
- rollback is guarded and conflict-aware;
- context promotion is explicit and size-aware;
- protocol events are frontend-neutral;
- long-running progress does not require LLM polling;
- the TUI remains responsive with large artifacts.
