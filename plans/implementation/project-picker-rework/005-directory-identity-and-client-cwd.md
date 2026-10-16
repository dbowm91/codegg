# Project Picker Rework Milestone 005 — Directory-tied 1:1 identity, auto-create/load, client cwd switch

Status: active

Repository baseline: `45100903` (M004 strict closure; sequential handoff)

Source roadmap:

- `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
  (Milestone 5)

Long-term requirements:

- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
  (accepted 2026-10-16 — satisfied; governs 1:1 identity,
  auto-create/load, client-owned working directory, grandfathering)

Primary class: capability

## 1. Objective

Enforce one directory ↔ one project: opening an unseen directory
auto-creates a project named for it, reopening a seen directory loads
the bound project, and selecting a project switches the tab's client
working directory — with the daemon process cwd never moving.

## 2. Why this milestone is ready

Blocked, not ready: ADR-0033 is accepted (satisfied), but M2 (picker +
surfaces), M3 (lifecycle), and M4 (rename/repath) must all be closed —
this milestone unifies their surfaces under the identity rule and
switches execution context on selection.

## 3. Current implementation evidence

- Launch already auto-creates: `bootstrap_launch_project_context`
  (`src/tui/app/mod.rs`) = idempotent `WorkspaceRegister` + idempotent
  `ProjectRegister { display_name = dir file_name, source =
  "tui-launch" }`, with existing `workspace_project_binding` returned
  unchanged. This is the pattern to generalize to picker-driven opens.
- Model permits N workspaces per project (`ProjectDetailsDto.workspaces`,
  `workspace_project_binding`); ADR-0033 grandfathers existing
  multi-bindings read-only and requires 1:1 for new bindings.
- Tab state `ProjectTabState { project_id, workspace_id, workspace_root,
  … }` + `ExecutionContext { project_id, session_id, workspace_root }`
  (`src/tui/app/state/`) with `active_workspace_root()` /
  `project_execution_context()` accessors — the client-owned working
  directory.
- `check_daemon_cwd_usage.py` forbids new `current_dir()` in
  workspace-bound daemon code; tool workdir defaults to the validated
  root.

## 4. Invariants that must not regress

- Daemon process cwd never moves (singleton); per-tab directories are
  client `ExecutionContext` only — guard stays clean.
- New bindings are 1:1; existing multi-bindings stay readable and
  cannot gain new bindings until rebound (grandfather rule).
- Tool/shell workdir resolution follows the active tab root; no tab
  leaks another tab's root.
- Generation/request scoping; closing a tab deletes nothing.

## 5. Scope

### In scope

- 1:1 enforcement on new bindings (refuse second project for a bound
  root / second root for a bound project, with hints).
- Unified open path: picker selection and launch share one
  ensure-bound-project (create-if-unseen / load-if-seen) flow.
- Selection switches the tab `ExecutionContext`; tools and shell
  resolve under the new root; status line/sessions panel reflect it
  (M2 surfaces).
- Restart/resume preserves bindings (manifest + daemon validation).

### Explicitly out of scope

- Multi-cwd split panels and split-switch chords (deferred future
  design; `Shift+H/J/K/L` untouched).
- Migrating grandfathered multi-bindings automatically (operator
  rebind via M4 only).
- Any daemon-cwd behavior (forbidden by the ADR).

## 6. Required production changes

### Core/domain

- Binding policy: new project↔workspace bindings constrained to 1:1;
  grandfathered rows flagged read-only for new binds.

### Storage and migrations

- Binding-constraint enforcement; if schema support is needed, migrate
  + bump `STORAGE_LAYOUT_VERSION` (keep
  `check_project_catalog_invariants.py` green).

### Protocol and DTOs

- Prefer no protocol change (enforcement is daemon policy + error
  copy). If a refusal needs a typed error, add it additively.

### Runtime and concurrency

- Shared ensure-bound flow for launch + picker; concurrent opens of one
  unseen directory coalesce to one project (idempotent register
  already coalesces — prove it, don't assume it).

### Frontend or operator surface

- Selection commits `ExecutionContext`; tab switch re-resolves tool
  workdir default; status line shows the switched cwd.

### Security and authorization

- No new op expected; creation still flows through existing
  register paths and their authz.

### Documentation and static guards

- `docs/tui.md` + architecture docs: 1:1 rule, auto-create/load,
  grandfathering, client-cwd semantics. Keep the cwd guard clean
  (prove with the guard run, not by assertion).

## 7. Ordered work packages

### Work package A — 1:1 enforcement + grandfathering

Intent: the identity rule holds for all new bindings.

Required changes: binding policy, refusal copy with hints,
grandfather read path.

Acceptance evidence: binding tests (refusals, grandfather reads,
rebind graduates a project to 1:1).

### Work package B — Unified ensure-bound open flow

Intent: launch and picker behave identically.

Required changes: shared open path; unseen→create (dir-named),
seen→load.

Acceptance evidence: open-A→B→A restores identity; concurrent-open
coalescence test.

### Work package C — Client cwd switch on selection

Intent: each tab works in its own directory.

Required changes: `ExecutionContext` commit on selection/switch;
workdir default follows; surfaces reflect.

Acceptance evidence: workdir-resolution tests per tab; no-leak test
(tab A action never resolves under tab B root); guard run clean.

## 8. Failure, cancellation, restart, and contention semantics

Concurrent opens coalesce (idempotent register); restart revalidates
bindings against the daemon (grandfathered rows survive); tab switch
mid-flight never retargets in-flight tool calls (context captured at
dispatch).

## 9. Compatibility and migration

Grandfathered multi-bindings keep working read-only; no forced
migration. No config change. Refusal copy guides operators to M4
rebind.

## 10. Required tests

### Focused unit tests

- Binding policy: 1:1 refusals with hints, grandfather reads, rebind
  graduation.
- Context: switch commits `ExecutionContext`, workdir follows,
  no cross-tab leak.

### Integration tests

- open-A→B→A identity restore (launch + picker paths).
- Concurrent opens of one unseen directory yield one project.
- Restart/resume preserves bindings incl. grandfathered rows.

### Restart and recovery tests

- Daemon restart + manifest restore: tabs revalidate, no silent
  rebind, cwd surfaces correct.

### Contention and cancellation tests

- Concurrent open coalescence; switch-during-flight isolation.

### Security and negative tests

- Bound-root second-project refusal; second-root-for-project refusal;
  guard run (`check_daemon_cwd_usage.py`) clean.

### Migration and compatibility tests

- Only if schema support proves necessary (§6); grandfather fixture
  in that case.

## 11. Required verification commands

```bash
cargo test -p codegg-core -p codegg --lib project
cargo test -p codegg --lib tui::
scripts/verify.sh quick
python3 scripts/check_daemon_cwd_usage.py
```

Do not claim commands that were not actually run in the closure record.

## 12. Documentation updates

- `docs/tui.md`: 1:1 rule, auto-create/load behavior, grandfathering,
  per-tab cwd semantics.
- `architecture/` project/session docs: identity + client-cwd authority
  (amend narrowly; do not rewrite closed-roadmap history).

## 13. Acceptance criteria

- Unseen directory opens create a dir-named project; seen ones load it.
- Second bindings refused with hints; grandfathered projects readable.
- Selecting/switching tabs switches the working directory client-side;
  tools resolve under it with no cross-tab leak.
- Daemon cwd untouched (guard clean); restart preserves everything;
  suites + quick green.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M2–M4 have not all closed (surfaces/semantics differ from §3);
- 1:1 enforcement requires rewriting grandfathered rows (forbidden —
  grandfather rule stands);
- any design moves the daemon cwd or adds `current_dir()` to
  workspace-bound daemon code;
- ADR-0033 is superseded;
- scope would expand into splits (deferred).

## 15. Closure evidence required

- Focused/integration/restart/contention/security suite results, guard
  run, and quick verification (local vs CI labeled).
- A→B→A + coalescence + no-leak + restart evidence enumerated.
- Manual note after reinstall + daemon restart: multi-directory
  open/switch flow with status-line cwd proof.
- Roadmap completion review: M1–M5 closure chain ready for the
  roadmap-close decision.

## 16. Handoff notes

- Rebase onto the M4 closure head; this is the last milestone in the
  chain — its baseline moves the most.
- The no-daemon-chdir rule is absolute: any implementation that
  `current_dir()`s the daemon is wrong no matter how convenient.
  Capture execution context at dispatch; never read ambient cwd.
- Leave `Shift+H/J/K/L` alone; the split future gets its own design
  and chord.
