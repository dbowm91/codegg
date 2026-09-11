# Multi-Project TUI Frontend Convergence M009 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tui-project-sessions/009-keyboard-sidebar-agent-tree-inspector.md`
Source subsystem roadmap: `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#7-milestones`
Repository baseline reviewed: `98bc89fa613f5a1390202a90b497f59d5732d431`
Implementation commits: `2d10e72 — feat(tui): add keyboard sidebar agent tree inspector`

## 1. Executive finding

M009 is complete. The existing TUI sidebar now has stable logical focus targets shared by keyboard and mouse, bounded canonical agent-tree rendering, lazy run-detail inspection, and project-scoped selection. The implementation remains presentation-only: it consumes projection and durable-run summaries without adding storage, protocol, polling, or control authority.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Stable row identity and shared mouse/keyboard targeting | `SidebarFocusTarget`, `focus_targets`, `line_targets`, hover-to-focus mapping, and focused rendering in `src/tui/components/sidebar.rs` | Satisfied |
| Keyboard entry, traversal, collapse/expand, inspect, and exit | `FocusSidebar`, arrows/`j`/`k`, page movement, `h`/`l`, Space, Enter, Escape, and sidebar action dispatch in `src/tui/input.rs` and `src/tui/app/input.rs` | Satisfied |
| Canonical nested hierarchy with bounded useful context | `active_turn.agent_tree`/recent projection fallback, `parent_task_id` ordering, status/attention display, worktree/branch/result hints, and 64-node cap | Satisfied |
| Lazy detail integration | Exact string join from projected task ID to durable `task_id`; inspect routes through existing `OpenRunDetail`; unmatched nodes remain partial and detached runs remain inspectable | Satisfied |
| Stable update, reconnect, and tab/project behavior | Stable IDs survive row reordering; disappearance clamps to a valid nearby row; project scope clears focus/scroll; projection refresh rebuilds the tree | Satisfied |
| Observer/read-only safety | Sidebar exposes only section/node toggles and existing run inspection; no cancel, steer, permission, worktree integration, or mutation action was added | Satisfied |
| Documentation and help | `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`, normal-mode help, and keybinding dialog updated | Satisfied |

## 3. Production implementation evidence

- `SidebarWidget` owns only bounded presentation state: logical selection, collapse state, scroll, and compact adapters for existing summaries.
- Tree rows are reconstructed parent-first from `AgentTreeNodeProjection.parent_task_id`; missing parents become roots and malformed cycles cannot hide the bounded remainder.
- Durable metadata is joined only by the canonical task ID string. Missing association never invents a run ID.
- Agent tree input is capped at `MAX_SIDEBAR_AGENT_TREE_NODES = 64`; detached durable runs are separately bounded by the existing run-summary input.
- Sidebar focus is consumed before prompt handling, while modal/detail ownership remains with the existing M007 focus boundary.
- No new storage migration, protocol field, background task, or polling path was introduced.

## 4. Verification executed (commands + results; local vs CI truthfully)

All results below are local. Focused test binaries were run with the repository's normal commands and initially encountered the known host linker mismatch (`x86_64` Rust target versus arm64 `/opt/local` `liblzma`/`libiconv`). The same targets then passed using the available x86_64 Homebrew libraries via `PKG_CONFIG_LIBDIR=/usr/local/Cellar/xz/5.8.3/lib/pkgconfig` and compatible `RUSTFLAGS`.

- `cargo test -p codegg --lib -- tui::components::sidebar` — 4 passed.
- `cargo test --test tui_render` — 99 passed.
- `cargo test --test session_projection_consumer` — 8 passed.
- `cargo test -p codegg-protocol` — 177 passed across 2 suites.
- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — passed with no issues.
- `scripts/verify.sh quick` — passed, including generated-agent, core-boundary, sandbox, execution-ownership, project-authority, and workspace check guards.
- `cargo check --workspace --all-targets` and `cargo check -p codegg --lib` — passed.
- `git diff --check` — passed.

No CI result is claimed here; hosted CI remains the repository's compatible-host confirmation path.

## 5. Invariant review

The sidebar remains a projection consumer and does not own run/job/worktree state. Selection is logical and stable rather than a rendered coordinate or positional run index. Bounds are applied before tree presentation, and hidden descendants of collapsed nodes are not reintroduced as fallback roots. Prompt input cannot receive keys consumed by sidebar focus. Existing modal focus remains authoritative.

## 6. Failure and recovery review

Projection eviction or completion removes a selected node by stable ID and clamps to the nearest remaining logical row without rebinding to an unrelated row by index. Reconnect/replay repopulates the tree from canonical projection state. Detail failures continue through the existing bounded detail/toast behavior. No per-row tasks or polling were added.

## 7. Migration and compatibility review

There is no storage migration and no protocol change. Existing flat durable-run adapters remain compatible, while the rendered agent hierarchy is derived from canonical projection lineage. Existing mouse behavior and unused-sidebar behavior remain available. Insert-mode spaces remain prompt text; the sidebar focus binding is normal-mode only.

## 8. Security review

The new surface is inspect-first and read-only. It does not introduce a new authorization path or expose raw transcript/log content. Only already-present bounded projection fields and safe durable summary hints are rendered. Observer mode therefore gains inspection of authorized projected data only, not mutation or control capabilities.

## 9. Documentation and operations

Architecture, TUI skill guidance, normal-mode help, and the keybinding action catalog document sidebar focus, stable identity, tree behavior, lazy inspection, and project scoping. No CI lane, scanner, dependency, or operational service was added.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Critical/high/medium: none.
- Low operational note: the default local macOS test environment has mixed-architecture `/opt/local` libraries; focused tests require the compatible x86_64 library path described in section 4 on this host. The targets pass with that environment and the condition is unrelated to M009 source behavior.

## 11. Roadmap disposition

M009 is closed in the frontend-convergence corrective roadmap. M010 remains `ready`; it has only a soft sequencing dependency on M009 and was already dependency-ready. No corrective pass is required.

The registry's blocked-work audit found no registered plan whose hard or interface blocker is this M009 milestone. The unrelated Architecture M009 strict operational-evidence item and Runtime Safety C002 supported-Linux evidence item remain blocked/conditional as recorded; neither is unblocked by this closure.

## 12. Registry updates

- Implementation plan status moved from `closing` to `implemented`.
- Frontend-convergence roadmap milestone M009 moved from `closing` to `closed`.
- `plans/registry.md` records the subsystem as active with M010 ready, removes M009 from dependency-ready plans, and adds M009 to recently closed work with commit `2d10e72`.
- No blocked plan moved to `ready` because the dependency audit found no newly satisfied hard/interface dependency.
