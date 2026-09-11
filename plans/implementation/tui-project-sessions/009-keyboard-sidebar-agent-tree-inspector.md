# Multi-Project TUI Frontend Convergence M009 — Keyboard Sidebar and Agent-Tree Inspector

Status: implemented

Repository baseline: `98bc89fa613f5a1390202a90b497f59d5732d431`

Source roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
- `plans/002-long-term-roadmap.md#phase-8--read-only-observation-mode`
- `plans/002-long-term-roadmap.md#phase-9--durable-multilevel-agent-run-service`

Related closed foundations:

- `plans/subsystems/session-projections-roadmap.md`
- `plans/subsystems/agent-run-worktree-concurrency-final-corrective-closure-addendum.md`
- `plans/subsystems/presence-observation-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md` where worktree/run ownership is presented.

Primary class: capability / polish

Hard dependency: M007 modal/focus convergence. M005 is a soft dependency for project-correct detail actions. Projection/agent-run interfaces are already closed.

Closure record to create: `plans/closure/tui-project-sessions/009-status.md`

## 1. Objective

Make the existing sidebar/activity surface fully keyboard-operable and expose the canonical nested agent hierarchy through that surface. The implementation should evolve the current `AgentRuns` sidebar section into a bounded tree/inspector backed by existing session projection and durable run metadata, with detail delegated to existing on-demand run/source/artifact surfaces rather than copying large output into sidebar state.

## 2. Why this milestone is ready

The backend capability is already present: session projections carry bounded `agent_tree` state, durable agent runs own lineage/worktree/result metadata, and observer projection semantics are closed. The sidebar also already renders several activity families. What is missing is a stable keyboard/focus contract and tree presentation.

M007 is closed in `plans/closure/tui-project-sessions/007-status.md` and provides the
canonical modal/focus ownership contract. M005, M006, M008, the session projection
interfaces, and the durable agent-run interfaces are also closed or stable. M009 can
therefore add presentation-only sidebar selection while routing modal/detail focus
through the existing `FocusManager` boundary.

## 3. Current implementation evidence

Before editing, inspect:

- `src/tui/components/sidebar.rs`: `SidebarSection`, `HoveredElement`, row rendering, `line_targets`, collapse booleans, scroll behavior, `SidebarAgentRun`, Tool Programs/Convergences;
- stub methods `toggle_focused`, `focus_next`, `focus_prev`, `focused_name` and all callers;
- mouse hover/click handling and how sidebar area/scroll offset are tracked;
- input modes/actions and existing sidebar toggle/focus keybindings;
- `src/tui/app/state/projection_client.rs` and projection reducer adapters for `agent_tree`/`active_subagents`;
- `AgentTreeNodeProjection` fields/status normalization and bounds;
- current `SidebarAgentRun` population from durable run/group/convergence summaries;
- `RunDetailDialog`, source preview, diff/review and worktree detail surfaces available for inspection;
- observer mode `blocks_command`/allowed command matrix and projection redaction;
- tab-local state persistence/restoration rules: selection/focus is presentation-only and should not become durable project authority.

Build a table mapping sidebar row families to stable identity, display state, permitted inspect action, collapse behavior, mouse action, and keyboard action.

## 4. Invariants that must not regress

- Sidebar and agent tree are projections, never owners of run/job/worktree state.
- Agent lineage comes from canonical projection/run IDs; no parentage is inferred from display order or names.
- Large output/logs/diffs stay behind existing detail/artifact handles and are loaded on demand.
- The list/tree remains bounded under high agent/run activity and reconnect/replay.
- Keyboard and mouse target the same row identity/selection state.
- Focused sidebar input does not leak to prompt input.
- Leaving sidebar focus restores prompt/global focus predictably.
- Observer/read-only sessions may inspect only projected authorized data and cannot cancel, steer, answer permissions, integrate worktrees, or mutate through this surface.
- Project/tab switching cannot carry selected run IDs into a different project.
- Existing sidebar sections remain usable and their collapse state/scroll bounds do not corrupt selection.

## 5. Scope

### In scope

- A stable `SidebarFocusTarget`/row identity model or equivalent covering sections and actionable rows.
- Keyboard focus entry/exit and navigation: arrows and/or `j/k`, page movement where appropriate, collapse/expand, inspect/activate.
- Reuse of M007 focus primitives and configurable TUI input actions.
- Parent/child indentation and status/attention rendering for agent tree.
- Bounded worktree/branch/result-commit hints where already present in canonical run summaries.
- Mapping selected tree nodes to existing RunDetail or another canonical detail view.
- Mouse click/hover alignment with keyboard selection.
- `Space a` or equivalent configurable interaction concept for focusing/opening the agent tree, consistent with canonical TUI target behavior.
- Focused help/keybinding/render tests.

### Explicitly out of scope

- New agent-run storage/projection subsystem.
- New steering/cancel/retry controls unless an already-authorized explicit action is simply linked from existing UI; default M009 is inspect-first.
- Streaming full subagent transcripts into the sidebar.
- New worktree integration semantics.
- General command-palette taxonomy (M010).
- Visual redesign of the whole TUI.

## 6. Required production changes

### Projection/view-model

Build a bounded sidebar tree view from `AgentTreeNodeProjection` and existing durable run summaries. Define deterministic reconciliation when a tree node has both projection lineage and richer durable run metadata: canonical IDs join the records; absence of detail yields a partial row rather than invented values.

Tree rendering should represent at least:

- depth/parent-child relationship;
- stable short identity or agent label;
- running/completed/failed/cancelled/attention state;
- worktree/branch/result-commit hints when available and safe;
- bounded current/recent progress summary only if already present in projection and within limits.

Do not add large fields to normal snapshots solely for rendering.

### Keyboard/focus

Replace no-op sidebar focus methods with explicit selection. Selection must be based on logical visible rows, not raw rendered y positions, so collapse/scroll/narrow-width changes remain correct. Mouse hit testing should resolve through the same row model.

Suggested semantics, configurable through existing input machinery:

```text
focus sidebar/agent tree: existing sidebar focus action or Space a
j/k or Down/Up: next/previous visible actionable row
Left/h: collapse parent/section or move to parent
Right/l: expand parent/section
Enter: inspect selected item
Space: toggle selected collapsible section/node where appropriate
Esc: leave sidebar/detail focus without submitting prompt
PgUp/PgDn: bounded viewport navigation
```

Exact keys may adapt to current conflicts; the conceptual operations must exist and be documented.

### Detail integration

Use existing `OpenRunDetail`, diff/source/worktree/info surfaces where possible. If `AgentTreeNodeProjection` exposes only a task ID and durable run detail requires run ID, use existing run-group/projection association rather than inventing a derived ID. If no association exists, inspect the protocol before adding an additive bounded ID field.

### Storage/protocol/security

No storage migration. Protocol changes are not expected; additive projection linkage is allowed only after proving existing IDs cannot resolve detail. Respect existing visibility/redaction classifications.

## 7. Ordered work packages

### Work package A — Sidebar row/focus model

Create the row-family matrix and introduce one logical visible-row model shared by rendering, focus selection and mouse hit testing. Handle collapse/scroll/empty states and row identity.

Acceptance evidence: keyboard/mouse select the same stable target after scrolling/collapse.

### Work package B — Keyboard navigation

Implement focus entry/exit, next/previous, page, collapse/expand and Enter inspect behavior. Integrate with M007 so modal focus remains above sidebar and prompt focus does not receive consumed keys.

Acceptance evidence: keyboard-only test navigates all existing actionable sidebar families; no-op focus methods are removed or given real behavior.

### Work package C — Agent tree view model

Consume canonical `agent_tree`, join durable run summary metadata by stable IDs where possible, produce bounded visible nodes and deterministic ordering/indentation. Preserve active/recent counts in status bar but make the tree the detail surface.

Acceptance evidence: three-level synthetic tree renders parent/child structure and statuses correctly; high node counts respect existing projection/sidebar bounds.

### Work package D — Inspect/detail integration

Map agent node/run selection to existing detail surfaces. For nodes with unavailable detail, show a concise bounded info state rather than erroring or fetching arbitrary data.

Acceptance evidence: selected completed/running/failed runs open correct canonical detail; no sidebar copy of full output is introduced.

### Work package E — Observer/project/reconnect correctness

Exercise observer mode, tab switch, reconnect/replay, agent completion/reordering, selected-node disappearance, and project close. Selection should clamp/clear deterministically.

Acceptance evidence: observer cannot invoke mutation; stale project selection never opens another project's run.

### Work package F — Help/render polish

Document focus keys and `Space a` concept, add narrow-width/zero-item/loading/error render tests, ensure selected styling is theme/accessibility-compatible without hard-coded colors.

## 8. Failure, cancellation, restart, and contention semantics

A selected node may disappear due to bounded projection eviction or run completion. Selection follows stable identity when still present; otherwise it falls to nearest valid row/section or clears. It must never rebind by index to a different run.

Detail load failure produces a bounded toast/info state and leaves sidebar usable. Reconnect rebuilds the tree from canonical snapshot/events; local selection is retained only if the stable node/run is still present in the same project/session.

No new background polling. Use existing projection events/summaries and existing detail-fetch commands. Rapid updates should coalesce through normal TUI render cadence rather than spawning per-row work.

## 9. Compatibility and migration

Existing sidebar display remains available when keyboard focus is unused. Preserve mouse behavior while converging it on shared row identities. Flat `SidebarAgentRun` helper types may remain as an adapter during implementation if needed, but the final rendered hierarchy must derive from canonical parent-child data, not a second independently maintained tree.

## 10. Required tests

### Focused unit tests

- logical row generation for collapsed/expanded sections;
- stable focus target navigation/clamping;
- nested agent tree ordering/indentation/status;
- mouse hit-test equivalence;
- narrow-width truncation without identity loss.

### Integration tests

- keyboard-only sidebar traversal and inspect;
- three-level agent tree + RunDetail open;
- completion/failure updates preserve correct selected identity;
- tab A tree selection cannot resolve after switching to B unless B has its own selection.

### Restart/recovery tests

- reconnect snapshot rebuild and selection retention/clear by stable ID;
- bounded projection eviction behavior.

### Contention/cancellation tests

- rapid tree updates do not create tasks or unbounded rows;
- modal opening over focused sidebar captures keyboard until close.

### Security/negative tests

- observer tree inspect works only for visible data;
- mutation/cancel/steer/permission actions remain blocked in observer mode;
- redacted fields never appear in sidebar/detail fallback.

## 11. Required verification commands

```bash
cargo test -p codegg --lib -- tui::components::sidebar
cargo test --test tui_render
# focused projection/agent-run consumer tests
cargo test --test session_projection_consumer
# focused observer/presence tests affected by tree view
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Use actual target names present at implementation time. No new CI lane.

## 12. Documentation updates

- `architecture/tui.md`: sidebar focus and agent-tree interaction.
- `architecture/agent.md` or current agent-run architecture doc: TUI projection/inspection surface, only if source ownership descriptions change.
- `architecture/presence.md`: observer tree behavior if needed.
- keybinding/help source and `.opencode/skills/tui/SKILL.md`.

## 13. Acceptance criteria

M009 closes when a keyboard-only user can focus, navigate, collapse and inspect the sidebar; a real three-level canonical agent hierarchy is visible with bounded useful run/worktree context; mouse and keyboard share row identity; detail stays lazy; tab/reconnect changes are stable-ID correct; and observer mode cannot gain control through the new surface.

## 14. Stop conditions

Stop if:

- M007 is not closed;
- existing projections cannot associate a visible agent node with the detail surface and solving it would require speculative IDs or large payloads;
- implementation starts building a second run/agent store in TUI state;
- a new focus framework is proposed instead of using M007's canonical model;
- control/steering semantics become necessary to call the inspector complete.

## 15. Closure evidence required

Include implementation commits, row/focus matrix, agent-tree data-source/join description, keyboard-only test evidence, three-level tree render/detail evidence, bounds/reconnect/tab-switch results, observer negative-control evidence, focused/broad verification outcomes, any additive protocol field justification, and residual findings with severity.

## 16. Handoff notes

Treat the sidebar as a bounded index into canonical state, not a dashboard that copies everything. Prefer one useful inspect action over adding several mutation shortcuts. M010 will handle broader command/help taxonomy after this interaction surface is stable.
