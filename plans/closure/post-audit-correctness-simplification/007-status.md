# Post-Audit Correctness, Simplification, and Footprint Milestone 007 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/post-audit-correctness-simplification/007-execution-model-pass-through-cleanup.md`
Source subsystem roadmap: `plans/subsystems/post-audit-correctness-simplification-roadmap.md#7-ordered-milestones`
Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`
Implementation commits: `17e1f5a — simplify execution model pass-throughs`

## 1. Executive finding

M007 is closed. Repository inspection found two genuinely redundant internal
representations and removed them without changing the typed intent → validated
plan → executor → persisted outcome contract. The public planner compatibility
surface and dispatch routing enum were retained because they have active
consumers or carry dispatch-only state.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Remove concretely redundant pass-throughs | Deleted unused `ActualInvocation` and `ActualExecutor::into_invocation`; deleted unused `decision_to_planned_backend`. |
| Preserve canonical planning ownership | Internal production imports now use `command_intent::plan`; `src/command_planner.rs` remains as a documented compatibility re-export. |
| Preserve dispatch semantics | `RoutingDecision` retained because it adds parsed argv, scope, timeout, cwd, and typed Git request data. Focused routing/Bash tests pass. |
| Preserve persistence/provenance | `ActualExecutor`, `PlannedBackend`, `ActualBackend`, and core `RunInvocation` remain unchanged. Outcome tests and Bash tests pass. |
| Preserve safety/ownership boundaries | Execution-ownership and sandbox guards pass; no subprocess spawn sites were added. |
| Reduce source complexity | 130 removed lines, 27 added/adjusted lines; no replacement generic abstraction introduced. |

## 3. Production implementation evidence

### Before/after representation map

| Concern | Before | After | Disposition |
|---|---|---|---|
| Intent planning | `command_intent::plan`, plus internal imports through `command_planner` | `command_intent::plan` directly in production | Compatibility re-export kept for active callers; internal pass-through use removed. |
| Dispatch request | `ExecutionBackend` → `RoutingDecision` | Same | Kept: routing adds executor-facing data and is a real dispatch boundary. |
| Runtime execution truth | `ActualExecutor` plus unused `ActualInvocation` conversion mirror | `ActualExecutor` | Deleted the unconsumed mirror; runtime details remain available to dispatch and persistence mapping. |
| Persisted invocation | Core `RunInvocation` | Core `RunInvocation` | Kept: stable storage-facing representation. |
| Planned backend persistence mapping | `ExecutionBackend` mapping plus unused `RoutingDecision` mapping | `ExecutionBackend` mapping only | Deleted the dead duplicate; planned provenance continues to derive from the plan. |

### Candidate retention-test decisions

- `command_planner` re-export: kept. It is a public crate module used by
  integration tests and compatibility callers; deleting it would change an
  active API surface without removing an ownership boundary.
- `RoutingDecision`: kept. It has active integration consumers and carries
  scope labels, timeouts, cwd, parsed test argv, and typed Git requests that
  are not isomorphic to `ExecutionBackend`.
- `ActualExecutor`: kept. It represents actual runtime execution, including
  full argv/cwd and fallback/rejection details, whereas persistence stores a
  deliberately smaller backend and invocation contract.
- Core `PlannedBackend`, `ActualBackend`, and `RunInvocation`: kept. They cross
  the RunStore persistence boundary and preserve compatibility.
- `ActualInvocation` and `into_invocation`: deleted. Repository-wide search
  found no consumers; they duplicated the persistence-side invocation concept
  and added a fabricated rejected/Python representation.
- `decision_to_planned_backend`: deleted. It was dead code and duplicated the
  live `ExecutionBackend` → `PlannedBackend` mapping.

## 4. Verification executed

All commands were run locally on implementation commit `17e1f5a`:

- `cargo fmt --all` — passed.
- `cargo test --lib command_intent` — 258 passed.
- `cargo test --lib command_routing` — 17 passed.
- `cargo test --lib command_outcome` — 6 passed.
- `cargo test --lib tool::bash` — 71 passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/check_sandbox_contract.py` — passed.
- `scripts/verify.sh quick` — passed, including formatting, generated-agent
  checks, core-boundary checks, static guards, and workspace all-target check.
- `git diff --check` — passed.

Hosted verification is owned by M008 and was not claimed here.

## 5. Invariant review

Native argv remains represented by typed `NativeCommand` and actual argv
vectors. Raw shell remains a distinct `ExecutionBackend::RawShell` and
`ActualExecutor::RawShell` path. Backend selection remains deterministic in
the planner. Permission/risk data remains on `CommandPlan`; scheduler-backed
dispatch and managed-process supervision were not changed. Actual backend,
fallback, ownership, and persisted outcome mappings remain in place.

## 6. Failure and recovery review

No execution lifecycle, cancellation, restart, scheduler admission, or
managed-process code was changed. The removed code had no call sites and could
not participate in recovery. Existing active routing failure behavior remains
terminal rather than falling back to a second execution.

## 7. Migration and compatibility review

No storage schema, protocol, CLI, tool-schema, config-path, or state-path
change was introduced. The internal `command_planner` path remains available
as a compatibility re-export. The deleted `ActualInvocation` API had no
repository consumers and was not part of the persistence or wire contracts.

## 8. Security review

The execution-ownership and sandbox guards pass. No direct subprocess bypass,
policy recomputation, provenance loss, scheduler bypass, or raw-shell/native
route merge was introduced. The retained routing boundary continues to carry
the typed Git request and distinct shell/native semantics.

## 9. Documentation and operations

`architecture/command_planner.md` now identifies the re-export as compatibility
only, and `architecture/command_routing.md` documents why routing remains a
distinct dispatch boundary. No operational or CI contract changed.

## 10. Unresolved findings

None at critical, high, medium, or low severity. Broader measurement and
integrated hosted verification remain intentionally assigned to M008.

## 11. Roadmap disposition

M007 is closed. M008 is dependency-ready and owns the final integrated
measurement, hosted verification, and workstream closure; no corrective pass
was created.

## 12. Registry updates

- M007 moved from `active` to `closed`.
- M007 was added to recently closed implementation history with commit
  `17e1f5a`.
- The blocked-work audit found M008's only remaining hard dependency was M007;
  M001–M006 were already closed. M008 was therefore moved from `blocked` to
  `ready` in `plans/registry.md` in this closure commit.
