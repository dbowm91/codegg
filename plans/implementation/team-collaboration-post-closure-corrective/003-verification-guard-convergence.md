# Team Collaboration Post-Closure Corrective Milestone 003 — Verification Guard Convergence

Status: ready for handoff

Repository baseline: `626585a1fad449a637e4828777bfa21d222abea0`

Source roadmap: `plans/subsystems/team-collaboration-post-closure-corrective-addendum.md#m003--verification-guard-convergence-and-canonical-gating`

Hard dependencies:

- M001 registration authority boundary — must be closed.
- M002 Workspace task cancellation ownership — must be closed.

Long-term requirements:

- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md`

Applicable ADRs: none required.

Primary class: invariant

## 1. Objective

Repair the stale authority/scheduler/audit static guards, make them accurately reflect the corrected M001/M002 production semantics, and include the cheap high-value guards in canonical quick verification and routine CI so future closure cannot report green while these invariants are mechanically broken.

## 2. Why this milestone is blocked

The guard semantics should describe the final production boundary. M001 changes project/workspace registration disposition and M002 changes Workspace task ownership. Running a canonicalization pass before those fixes would either encode known-bad semantics or require a second guard rewrite.

Once M001 and M002 close, this milestone is mechanically dependency-ready.

## 3. Current implementation evidence

### Audit coverage guard

`scripts/check_audit_coverage.py::_authz_operations` reads `crates/codegg-core/src/authorization.rs` and searches for literal `OperationDescriptor::new(...)` calls. The canonical descriptor table moved to `crates/codegg-core/src/authorization/policy.rs`, so the guard no longer inventories the real operation set.

M004 closure recorded this as a pre-existing failure rather than repairing it.

### Scheduler bypass guard

`src/agent/snapshot_capture.rs` already labels its standalone fallback with `// scheduler-audit: standalone-compat`, but the direct `pool.spawner().send(...)` is just outside `check_scheduler_bypass.py`'s backward annotation window. The guard therefore reports a false positive.

Prefer moving the annotation immediately adjacent to the audited call or making the checker bind an annotation structurally to the call. Do not broadly enlarge the ignore window without a regression fixture.

### HTTP route-disposition invariant

`server::authz::tests::every_shared_authz_row_names_a_capability` correctly rejects `SharedAuthz + none`, but `POST /api/project` currently uses exactly that disposition. M001 should move that route to LocalOwner-only, making the invariant truthful rather than weakening the test.

### Canonical verification gap

`scripts/verify.sh quick` and routine CI run core-boundary, sandbox, execution-ownership, TUI authority, format/check/clippy/tests. They do not run:

- `scripts/check_http_route_disposition.py`;
- `scripts/check_audit_coverage.py`; or
- `scripts/check_scheduler_bypass.py`.

All three are cheap static guards over authority/ownership boundaries and were directly relevant to findings accepted as "pre-existing" during the collaboration campaign.

## 4. Invariants that must not regress

- Static guards parse the canonical source of truth, not a historical file location.
- A guard is never weakened simply to make current code pass.
- Every SharedAuthz route names a semantic project/session capability; scope-less compatibility is LocalOwner-only or another explicit disposition.
- Scheduler exceptions are explicit and tightly adjacent/structural.
- Audit operation coverage accounts for every canonical daemon operation.
- Canonical quick/CI verification fails when any of these authority invariants fail.
- Verification remains bounded; do not add optional external-tool or real-server tests to routine CI.

## 5. Scope

In scope:

- `check_audit_coverage.py` canonical operation-source repair.
- `check_scheduler_bypass.py` annotation association / the `snapshot_capture.rs` annotation placement.
- `check_http_route_disposition.py` and authz unit invariant reconciliation after M001.
- `scripts/verify.sh quick`.
- `.github/workflows/ci.yml`.
- `AGENTS.md` and `architecture/testing.md` verification taxonomy/docs.
- Focused script self-tests/fixtures where practical.

Out of scope:

- Rewriting the audit architecture.
- Reclassifying genuine unaudited mutations as uninstrumented without evidence.
- Broad scheduler refactors.
- New CI jobs/matrices.
- Running `lsp-real-server-tests` or `--all-features`.
- Making all change-triggered guards routine.

## 6. Required production changes

### A. Audit guard source-of-truth repair

Point operation extraction at the actual canonical descriptor module (`authorization/policy.rs`), or expose a stable generated/list function that the script can validate without source-location coupling.

After repair, run the guard. If it reports genuine unclassified operations, classify them truthfully in `audit_instrumentation.rs` or stop/register a separate audit corrective. Do not mark mutations uninstrumented merely to pass.

Add a regression that would fail if the descriptor table moves or the script again reads an empty/noncanonical set.

### B. Scheduler guard precision

Make the existing standalone `snapshot_capture` exception pass for the reason it already declares.

Preferred correction: move `// scheduler-audit: standalone-compat` immediately above the direct send or otherwise bind it closely to the call. If the script itself changes, add a fixture proving:

- adjacent valid annotation passes;
- missing annotation fails;
- unrelated annotation farther away does not bless a call.

Do not turn the current 24-line scan into an effectively file-wide waiver.

### C. Route-disposition invariant

After M001, confirm:

- `POST /api/project` is not `SharedAuthz + none`;
- every `SharedAuthz` row names a capability;
- every mounted authenticated route has exactly one disposition;
- payload authority checks remain green.

Do not weaken `every_shared_authz_row_names_a_capability`.

### D. Canonical verification

Add the three cheap static guards to `scripts/verify.sh quick` and the routine CI `verify` job:

```text
python3 scripts/check_http_route_disposition.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_scheduler_bypass.py
```

Keep resource policy unchanged.

Update `AGENTS.md` so the quick-start and change-triggered section no longer describe these three as absent from routine verification. Update `architecture/testing.md` with the canonical guard set.

## 7. Ordered work packages

### Work package A — Repair each guard independently

Run each guard on M001+M002-closed baseline. Fix source/annotation truth without touching product semantics unless a real defect is discovered.

Record before/after output in closure evidence.

### Work package B — Add regression fixtures

Where scripts have no direct self-test harness, add minimal deterministic fixture/self-test coverage or source assertions that pin the specific failure mode:

- audit checker must discover known operations from policy.rs;
- scheduler checker must distinguish adjacent annotation from absent annotation;
- route disposition must reject SharedAuthz/none.

### Work package C — Wire canonical verification

Add guards to quick mode and CI in an order that fails early before Cargo-heavy work. Keep the commands identical locally and in CI where practical.

### Work package D — Final collaboration requalification

Rerun M001/M002 post-closure regressions, M006 trajectory, authorization matrix, and canonical quick verification. Any high/medium issue found becomes a new corrective plan; do not close M003 around it.

## 8. Failure, cancellation, restart, contention semantics

This milestone changes verification tooling only. Scripts must be deterministic, offline, and side-effect free.

A guard parser error must fail closed with a clear message, not silently return an empty operation set as success.

CI/quick stops on first failing guard via existing `set -euo pipefail` semantics.

## 9. Compatibility and migration

No runtime migration.

Developer/CI compatibility change: revisions carrying pre-existing violations will now fail quick/CI. That is intentional. Keep diagnostics actionable so failures point to the exact guard and source.

## 10. Required tests

At minimum:

- direct execution of all three guards;
- any new script self-tests;
- authz route-disposition unit tests;
- M001 registration authority regression;
- M002 Workspace cancellation regression;
- M006 trajectory suite;
- full routine quick verification.

If `check_audit_coverage.py` reveals actual operation coverage debt, add focused audit tests before closure.

## 11. Required verification commands

```bash
python3 scripts/check_http_route_disposition.py --verbose
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_authorization_matrix.py --verbose
cargo test -p codegg --features server --lib server::authz::tests -- --test-threads=1
cargo test --features server --test team_collaboration_postclosure_m001_registration_auth -- --test-threads=1
cargo test --test workspace_postclosure_m002_task_cancellation -- --test-threads=1
cargo test --features server --test team_collaboration_m006_trajectory -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
cargo test -p codegg --locked --features server,plugins,lsp-test-support -- --test-threads=1
git diff --check
```

CI evidence should show the new guard steps in the routine `verify` job.

## 12. Documentation updates

Update:

- `AGENTS.md` quick-start and guard taxonomy;
- `architecture/testing.md`;
- `architecture/authorization.md` if route disposition wording changes;
- `architecture/audit.md` if the guard source-of-truth description is stale;
- execution-ownership docs only if annotation semantics change.

## 13. Acceptance criteria

- `check_audit_coverage.py` reads the canonical authorization descriptor set and passes with a non-empty known operation inventory.
- `check_scheduler_bypass.py` passes the deliberate standalone fallback without widening exemptions and still rejects an unannotated direct send.
- `every_shared_authz_row_names_a_capability` passes unchanged.
- `check_http_route_disposition.py` passes.
- The three guards run from `scripts/verify.sh quick` and routine CI.
- M001 registration and M002 cancellation regressions pass.
- M006 trajectory remains green.
- No unresolved high/medium audit/authorization/scheduler finding remains.

## 14. Stop conditions

Stop and register a separate corrective if:

- the repaired audit guard reveals genuine missing audit instrumentation for security-relevant mutations;
- the scheduler guard exposes an actual daemon-mode bypass rather than an annotation-location false positive;
- making route dispositions truthful requires a new authorization capability/ADR;
- adding the guards makes routine CI depend on external services or optional host capabilities.

## 15. Closure evidence required

Closure record: `plans/closure/team-collaboration-post-closure-corrective/003-status.md`.

It must include:

- direct pre/post outputs for each guard;
- evidence that M001+M002 are closed dependencies;
- canonical quick/CI wiring evidence;
- final multi-user trajectory result;
- unresolved finding table; and
- registry audit proving no further corrective is hidden.

## 16. Handoff notes

This plan closes a verification-trust problem, not just three script bugs. The final state should make the cheap authority invariants unavoidable during routine development. Preserve the distinction between routine static guards and expensive/host-specific change-triggered verification.
