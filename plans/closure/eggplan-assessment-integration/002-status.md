# Eggplan Assessment Integration C002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggplan-assessment-integration/002-agentrun-link-and-bridge-fixture-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/eggplan-assessment-integration-roadmap.md`

Predecessor closure (immutable historical evidence):

- `plans/closure/eggplan-assessment-integration/001-status.md`
  (implementation `a7cf63c4` + `4fab486f` + `418fdc85`; hosted CI run
  `36106606574` green)

Repository baseline reviewed: `841ad117`

Implementation commits:

- `91b2bc7b` — C002 predicate fix, projection helper, ownership guard, regression tests.

## 1. Executive finding

C002 is complete and closed. Independent review of the closed M001
implementation found one functional defect, one missing executable
evidence item, and one missing required guard; all three are repaired
additively with no closed-API change:

- **D001:** `assemble_resolved` (and legacy `assemble`) looked up
  AgentRun links with `WHERE id = ?1`, but the `agent_run` table's
  primary key is `run_id`. Every AgentRun enriched resolution missed —
  including positively-linked completed runs. Both predicates now key
  on `run_id`; the legacy job-store fallback is preserved
  line-for-line. The new positive-path test fails on the old predicate
  and passes on the new one (verified by temporary revert).
- **D002:** a pure `ExecutionSubjectRevision::to_eggplan_fields()`
  helper now pins the lossless five-field Eggplan projection, with
  golden clean/dirty JSON tests matching rechecked Eggplan HEAD
  `47e6f11` (`SubjectState::{Clean, Dirty}` → `"clean"`/`"dirty"`).
- **D003:** `scripts/check_execution_subject_ownership.py` now proves
  the §17 boundaries against the closed names (capture-owner
  uniqueness, scheduler-only callers, resolver non-capture including
  the `run_id` key rule, no new raw git owner, attempt authority, v67
  presence).

M001 remains closed; M002 remains ready. No downstream plan changes
state in this commit.

## 2. Requirement-to-evidence matrix

| Corrective requirement | Evidence | Result |
|---|---|---|
| Linked AgentRun resolves `Passed` + Stable subject (§10, criterion 6) | `linked_agent_run_resolves_passed_plus_exact_subject` (fails pre-fix, passes post-fix) | pass |
| Dangling/missing links stay status + unavailable, never worktree | `agent_run_with_dangling_link_is_subject_unavailable`, `missing_ref_remains_unavailable` | pass |
| Legacy/running/drifted matrix for `assemble_resolved` | `legacy_completed_job_returns_passed_with_unavailable_subject`, `running_job_returns_in_progress_without_stable_subject`, `drifted_completed_job_returns_terminal_status_with_drift`, `completed_job_with_stable_subject_returns_passed_plus_subject` | pass |
| Lossless Eggplan projection pinned (criterion 8 / WP6) | `to_eggplan_fields` + `eggplan_bridge_golden_clean_subject`, `eggplan_bridge_golden_dirty_subject` | pass |
| §17 ownership guard | `scripts/check_execution_subject_ownership.py` → ok | pass |
| No closed-API change, no migration, no behavior change beyond the predicate fix | `git diff` review: 2 one-line query fixes + additive helper/guard/tests | pass |

## 3. Production implementation evidence

- `src/work_plan_evidence.rs`: `agent_run_evidence_status` and
  `assemble_resolved` now query `FROM agent_run WHERE run_id = ?1`.
  The legacy AgentRun branch keeps its job-store fallback for
  delegated-run handles that share the id space, with the identical
  status mapping — rows that previously resolved via fallback resolve
  identically (verified by `work_plan_projection_arbiter` and
  `long_horizon_trajectory_qualification` staying green).
- `crates/codegg-core/src/jobs/mod.rs`: additive
  `EggplanSubjectFields` + `ExecutionSubjectRevision::to_eggplan_fields()`
  only; `validate()` and all closed shapes untouched.
- `scripts/check_execution_subject_ownership.py` (new): six checks,
  all passing.
- `tests/work_plan_resolved_evidence.rs` (new): 9 corrective tests + 5
  shared-harness tests, 14/14 green.

## 4. Verification executed

```bash
cargo fmt --all -- --check
git diff --check
cargo test --test work_plan_resolved_evidence                        # 14 passed (9 new)
cargo test --test work_plan_projection_arbiter                       # 9 passed
cargo test --test long_horizon_trajectory_qualification              # 27 passed
cargo test --test scheduler_authority_matrix                         # 13 passed
cargo test --test eggwork_remote_execution                           # 25 passed
cargo test -p codegg-core --lib                                      # 808 passed
cargo test -p egggit --lib -- subject                                # 3 passed
cargo test --lib work_plan_evidence                                  # 1 passed
cargo test --lib scheduler::executor                                 # 10 passed
python3 scripts/check_execution_subject_ownership.py                 # ok
python3 scripts/check_execution_ownership.py                         # ok
bash scripts/check-core-boundary.sh                                  # pass
./scripts/verify.sh quick                                            # pass
```

D001 negative control: with the predicate temporarily reverted to
`WHERE id = ?1`, `linked_agent_run_resolves_passed_plus_exact_subject`
fails; with the fix restored, the full file passes 14/14.

Hosted CI: the push of this corrective triggers the canonical `verify`
workflow; the run id will be linked from the push output. M001's hosted
run `36106606574` remains the governing green evidence for the
predecessor scope.

## 5. Invariant review

- Attempt authority unchanged; no `JobRecord`/label provenance.
- Resolver still never captures: no `egggit`, no spawn, no `current_dir`,
  no provenance writes in `src/work_plan_evidence.rs` (guard-enforced).
- Legacy `assemble` outcomes unchanged for all kinds (same mapping +
  preserved fallback; full trajectory/projection suites green).
- No new process-spawn surface; no migration; storage remains v67.

## 6. Failure and recovery review

The predicate fix only changes a query that previously always errored;
all error paths (`Ok(None)` → `Unavailable` / fallback) are preserved.
No new failure mode is introduced. Restart/contention semantics are
M001's, untouched.

## 7. Migration and compatibility review

No migration, no schema change, no trait change, no serialization change
(the helper is pure; golden tests assert JSON shape only).

## 8. Security review

No persisted shape changed. The fixed query binds the ref id as a
parameter (as before); no string interpolation. The guard script is
read-only static analysis.

## 9. Documentation and operations

- This closure record; predecessor closure untouched.
- `plans/registry.md`: C002 row active → closed; M001 row notes the
  corrective.
- Corrective plan status → implemented.
- No operator action; no new verify.sh wiring (the guard is
  change-triggered evidence for the touched surfaces, recorded here).

## 10. Unresolved findings

None blocking. One accepted low-severity deviation, documented (not
repaired to avoid churning the closed store trait): plan §7 asks the
store to reject attempt/job mismatch, but the closed
`set_attempt_source_subject_started` / `seal_attempt_source_subject`
take only `attempt_id`. Mismatch is structurally impossible through the
single scheduler-owned call path — `attempt_id`s are UUIDs derived from
`begin_attempt` on the same job, and seal additionally verifies S1
continuity (`started.captured == provenance.captured`) — so the check
would be dead defense-in-depth. If a second store caller ever appears,
the trait must gain the `job_id` parameter with a mismatch rejection.

## 11. Roadmap disposition

M001 stays closed; M002 stays ready. The corrective fixes the §10 link
resolution the M002 adapter will consume, and pins the bridge projection
it will construct Eggplan values from. No new follow-up work is
registered.

## 12. Registry updates

- C002 row: active → closed with closure pointer.
- M001 recently-closed row: annotated with the C002 closure pointer.
- Blocked-work audit: no registered plan lists C002 or the link
  predicate as a dependency; nothing else changes state.
