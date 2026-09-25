# Eggplan Assessment Integration C002 — AgentRun-Link Resolution and Bridge-Fixture Corrective

Status: active

Repository baseline: `841ad117`

Source roadmap: `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Source milestone plan:
`plans/implementation/eggplan-assessment-integration/001-durable-execution-subject-provenance.md`

Source closure (immutable predecessor evidence):
`plans/closure/eggplan-assessment-integration/001-status.md`
(implementation `a7cf63c4` + `4fab486f` + `418fdc85`; hosted CI run `36106606574` green)

Primary class: infrastructure / provenance-correctness corrective

## 1. Objective

Repair three gaps found in independent review of the closed M001
implementation, without reopening closed scope or changing closed APIs:

- **D001 (defect):** `assemble_resolved` — and the legacy `assemble` —
  look up AgentRun links with `WHERE id = ?1`, but the `agent_run`
  table's primary key is `run_id`. Every AgentRun enriched resolution
  therefore misses, including positively-linked completed runs: status
  stays `Unavailable` and the exact attempt subject never resolves.
  This violates plan §10 (`AgentRun with exact job+attempt link ->
  resolve attempt provenance`) and acceptance criterion 6.
- **D002 (missing executable evidence):** criterion 8 / WP6
  (lossless conversion to Eggplan's five-field `SubjectRevision`)
  rests on prose recheck only. Add a pure projection helper plus
  golden tests pinned to the rechecked Eggplan HEAD shape.
- **D003 (missing required guard):** plan §17 requires static guards
  proving subject-ownership boundaries. Port a dedicated ownership
  guard to the closed API names.

## 2. Why original verification did not catch it

- D001: no positive-path AgentRun test exists for `assemble_resolved`;
  the latent wrong-column predicate predates M001 in the legacy
  `assemble` path, where the job-store fallback masked it (status still
  resolved via the fallback). The enriched path has no such fallback,
  so the same predicate fails openly there.
- D002: the Eggplan HEAD/shape recheck was recorded as prose; no test
  pins the projection.
- D003: existing execution-ownership and git-pattern guards passed and
  cover process spawning generally, but no guard names the
  subject-capture owner, the resolver non-capture rule, or attempt
  authority.

## 3. Current implementation evidence

- `src/work_plan_evidence.rs`: `agent_run_evidence_status` and
  `assemble_resolved` both query `FROM agent_run WHERE id = ?1`;
  `agent_run` DDL uses `run_id TEXT PRIMARY KEY` (schema vMMM; no `id`
  column), so both queries always error to `Unavailable`.
- `crates/codegg-core/src/jobs/mod.rs`: `ExecutionSubjectRevision`
  has `validate()` but no Eggplan projection helper.
- No `scripts/check_execution_subject_ownership.py` exists.

## 4. Invariants that must not regress

- Legacy `assemble` behavior for all other kinds is unchanged; the
  AgentRun column fix must preserve the job-store fallback for
  delegated-run handles that share the id space (same status mapping,
  previously reached only via fallback).
- No `JobStore` trait signature changes; no migration; no new storage
  version; no assessor swap; no Eggplan production dependency.
- Closed M001 API names and semantics (`Started` disposition,
  `materialization` envelope, context-carried seal) are retained.

## 5. Scope (in / out)

In:

- `WHERE id` → `WHERE run_id` (2 sites in `src/work_plan_evidence.rs`).
- Pure `to_eggplan_fields()` (+ fixture struct) on
  `ExecutionSubjectRevision`; golden clean/dirty JSON tests.
- Positive-path resolver tests: linked AgentRun → `Passed` + exact
  Stable subject + native ids; dangling link → status + unavailable;
  legacy/running/drifted/missing matrix for `assemble_resolved`.
- `scripts/check_execution_subject_ownership.py` ported to the closed
  names (`capture_git_source_subject`, context seal method,
  `assemble_resolved`).

Out:

- No store-trait changes (attempt/job mismatch rejection stays a
  documented accepted deviation: mismatch is structurally impossible
  through the single scheduler-owned call path, which derives
  `attempt_id` from `begin_attempt` on the same job and verifies S1
  continuity at seal).
- No change to the remote-drift-refuses-submit strictness (deliberate
  closed behavior, documented in review).
- No M002 work.

## 6. Required production changes

- `src/work_plan_evidence.rs`: 2 one-line predicate fixes.
- `crates/codegg-core/src/jobs/mod.rs`: additive pure helper only.
- `scripts/check_execution_subject_ownership.py`: new guard (test-only
  surface otherwise).

## 7. Ordered work packages

### WP1 — AgentRun link predicate fix

Change both `agent_run` lookups to `WHERE run_id = ?1`. Add
`tests/work_plan_resolved_evidence.rs` covering: linked completed
AgentRun resolves `Passed` + Stable subject + native ids; dangling
link resolves status + unavailable subject; missing ref stays
`Unavailable`; legacy NULL → `Passed` +
`LegacyMissingProvenance`-equivalent unavailable disposition; running
→ `InProgress` without exact subject; drifted → terminal status +
`Drifted` without subject.

### WP2 — Bridge projection fixtures

Add `to_eggplan_fields()` returning the five Eggplan fields
(`subject_kind`/`repository_id`/`revision`/`state`/`dirty_digest`)
with `state` serialized as `"clean"`/`"dirty"` per Eggplan
`SubjectState` at rechecked HEAD `47e6f11`. Golden tests assert exact
JSON for clean and dirty revisions.

### WP3 — Ownership guard

Port the subject-ownership guard: capture-owner uniqueness
(`crates/egggit/src/subject.rs`), caller confinement
(`src/scheduler/*` + tests), resolver non-capture
(`src/work_plan_evidence.rs`: no `egggit`, no capture, no spawn, no
`current_dir`, no provenance writes), no new raw `git` subprocess
owner, attempt authority (no `source_subject` on `JobRecord`/labels),
v67 presence.

## 8. Failure, cancellation, restart, contention semantics

Unchanged from M001. The predicate fix only changes a query that
previously always errored; error paths (`Ok(None)`/fallback) are
preserved line-for-line.

## 9. Compatibility and migration

No migration. No trait changes. The legacy `assemble` AgentRun path
keeps its job-store fallback; rows that previously resolved via
fallback resolve identically (same status mapping function).

## 10. Required tests

- New `tests/work_plan_resolved_evidence.rs` (WP1 matrix + WP2 goldens).
- Existing suites that must stay green: `work_plan_projection_arbiter`
  (root), `long_horizon_trajectory_qualification`,
  `scheduler_authority_matrix`, `eggwork_remote_execution`,
  `codegg-core --lib`, root `--lib` (spot), `egggit --lib subject`.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test --test work_plan_resolved_evidence
cargo test --test work_plan_projection_arbiter
cargo test --test long_horizon_trajectory_qualification
python3 scripts/check_execution_subject_ownership.py
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
./scripts/verify.sh quick
git diff --check
```

## 12. Documentation updates

- `plans/registry.md` (002 row active → closed; M001 row notes the
  corrective).
- This plan file status → implemented on close.
- Closure: `plans/closure/eggplan-assessment-integration/002-status.md`.

## 13. Acceptance criteria

1. A completed AgentRun with an exact job+attempt link resolves
   `Passed` plus the Stable attempt subject through
   `assemble_resolved`.
2. Dangling/missing links resolve status with unavailable subject and
   never consult the worktree.
3. The Eggplan five-field projection is pinned by golden tests matching
   rechecked HEAD `47e6f11`.
4. The ownership guard passes.
5. All pre-existing suites in §10 remain green; no closed API changed.

## 14. Stop conditions

Stop and report if the `agent_run` table seen at runtime differs from
the migrated schema (i.e. `run_id` is not the lookup key), or if fixing
the predicate changes any legacy `assemble` outcome (it must not: same
mapping, fallback preserved).

## 15. Closure evidence required

`plans/closure/eggplan-assessment-integration/002-status.md` with the
defect diffs, test results, guard output, verification commands, and
the accepted-deviation note for store-level mismatch rejection.

## 16. Handoff notes

Corrective only. M001 remains closed; M002 remains ready. Do not start
M002 assessment adoption in this pass.
