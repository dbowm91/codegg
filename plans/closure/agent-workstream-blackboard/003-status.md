# Agent Workstream Blackboard M003 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-workstream-blackboard/003-task-claims-and-dependencies.md`

Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#milestones--deliverables`

Repository baseline reviewed: `720c9ba6c10cb9748af9c8faadf12b4046e7dd30` (M002 closure); implementation at `a59721e0196e9db6185e3ace92cb0417906b51fa`.

Implementation commit:

- `a59721e0196e9db6185e3ace92cb0417906b51fa` — add durable atomic claims and dependencies

## 1. Executive finding

M003 is complete. CodeGG now stores bounded, group-scoped task/path/interface intents and dependency edges in SQLite. Claim creation arbitrates transactionally; lifecycle changes use row-version and lease-generation CAS; owner handoff is offered to a named group actor and requires recipient acceptance. Expiry releases and generation-fences stale owners. Authorized readers can build the M002 workstream projection from the stored claim/dependency view.

These records coordinate declared responsibility only. They do not authorize filesystem, Git, shell, process, sandbox, scheduler, or worktree access; they do not reserve files; and they do not alter WorkOrder, WorkPlan, Todo, or job admission semantics. Existing execution containment remains authoritative.

M004 is now ready. M005 remains blocked on M004 closure, explicit acceptance of proposed ADR-0032, and verified writer-path containment. No other plan in this line became eligible.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed durable claim/dependency contract and additive migration | `crates/codegg-core/src/workstream_claim.rs`; `migrate_v79` in `crates/codegg-core/src/session/schema.rs`; `WorkstreamClaimId`; storage layout 79 | pass | Three tables hold claims, dependency edges, and append-only transition provenance. |
| Atomic claim arbitration and idempotent retry | `workstream_claim_m003::simultaneous_overlapping_claims_have_one_winner_and_stale_cas_fails` | pass | Eight member runs contend on one advisory path and one exact task; one winner per target. Same-key/same-payload retries return the original row; changed target is rejected. |
| Versioned lifecycle, handoff, expiry and stale-owner fencing | Same integration test; `dependencies_reject_cycles_and_expiry_replacement_fences_old_claim`; `terminal_owner_cannot_transition_a_live_claim`; `renewed_claim_survives_reopen_and_rejects_pre_restart_cas` | pass | Covers renew, stale release, offer/accept handoff, terminal actor denial, expiry replacement, generation fencing, restart persistence, and pre-restart CAS rejection. |
| Scope and authorization | `nonmember_actor_is_denied_and_schema_survives_reopen`; store authorization in `validate_group_actor` | pass | Run actors must remain active group members or the active owner run; principal actors must match the turn-owned group session origin. Task targets must be a task in the same group/project/workspace. |
| Dependency integrity | `dependencies_reject_cycles_and_expiry_replacement_fences_old_claim` and `add_dependency_locked` | pass | Self/missing/cross-scope/duplicate/cyclic prerequisites and fanout above 32 are refused; edges are CAS-versioned and do not enqueue work. |
| Bounded M002 projection consumer | `WorkstreamClaimStore::project_group`; integration assertion in `simultaneous_overlapping_claims_have_one_winner_and_stale_cas_fails`; protocol workstream projection tests | pass | The store applies group actor authorization, reads claims and edges, then invokes the pure protocol projector on the caller's authorized snapshot and preserves its `event_seq`. |
| Migration upgrade/reopen and rollback behavior | `nonmember_actor_is_denied_and_schema_survives_reopen`; migration v79 uses the existing transactional migration runner | pass | Schema version 79 and all three tables survive close/reopen. Failure remains governed by the transaction rollback and migration retry path. |
| Filesystem/Git permission boundary and unsafe labels | `normalize_relative_intent`, overlap unit tests, core boundary check, `architecture/workstream_claims.md` | pass with explicit limits | Traversal, absolute paths, backslashes, bidi controls, oversized/broad patterns are rejected. Path/interface overlap is conservative and advisory. The store does not resolve linked worktrees, symlinks, junctions, or filesystem case-fold rules and never alters `allowed_paths` or Git authority. |
| Secret/prompt minimization and bounded provenance | Claim target bounds, event schema and `insert_claim_event`; projection allowlist | pass | No description, prompt, raw output, or arbitrary agent prose is stored in claim rows or transition events. Group caps: 128 claims, 4096 events; per-claim dependency cap: 32. |

## 3. Production implementation evidence

Migration 79 adds `workstream_claim`, `workstream_claim_dependency`, and `workstream_claim_event`, with group/scope indexes, active-target uniqueness, typed actor/target/state constraints, bounded identifiers, and foreign keys. `WorkstreamClaimStore` uses SQLite `BEGIN IMMEDIATE` around create, transition, dependency insertion, and their provenance events. Stale revisions and generations return typed errors. Expired claims are changed to released with a generation bump and an `expired` provenance event before replacement.

Task claims use exact `AgentTaskId` identity. Path and interface intents are NFC-normalized relative labels with bounded syntax and conservative overlap detection; they are not file locks. Handoff moves a claimed/blocked responsibility into an `offered` state owned by the recipient, and that recipient must accept it with a fresh CAS before it becomes claimed. Dependency traversal enforces acyclicity and bounded fanout.

`WorkstreamClaimStore::project_group` is the storage-backed bridge to M002. It validates the active group actor, scopes reads by group/project/workspace, converts records to bounded projection DTOs, and calls `project_workstream_with_claims` on the authorized source snapshot. The projection keeps the source snapshot revision; claim row revisions remain independently visible on each claim.

## 4. Verification executed

All commands ran in a native arm64 shell with Rust 1.89, `CARGO_BUILD_JOBS=2`, and `CARGO_TARGET_DIR=/tmp/codegg-blackboard-arm64-target`.

```bash
cargo fmt --all
cargo test --offline -p codegg-core workstream_claim::tests
cargo test --offline -p codegg-protocol projection::workstream
cargo test --offline --test workstream_claim_m003
python3 scripts/check_project_catalog_invariants.py
bash scripts/check-core-boundary.sh
bash scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- Core claim unit tests: 2 passed.
- Protocol workstream projection tests: 6 passed.
- M003 SQLite integration tests: 5 passed.
- Project catalog invariants: 7/7 passed; core boundary passed.
- `scripts/verify.sh quick`: passed, including formatting, agent schema, authority/boundary guards, and workspace all-target check.
- Workspace Clippy did not pass: it stopped at the pre-existing `clippy::explicit_auto_deref` warning in `crates/codegg-providers/src/provider_core.rs:2941` (`*secret` should be `secret`). The provider file is outside M003 changes; no Clippy success is claimed.

Hosted CI and non-macOS platforms were not exercised.

## 5. Invariant review

- Claims cannot grant filesystem or Git authority. No permissions, allowed paths, scheduler keys, job state, or worktree policy are read or written by the claim store.
- Mutations validate an active group member or owner and the claim's persisted project/workspace/group scope under the write transaction.
- Task claims require an actual task in the same group scope. Run dependencies require a real same-group member run.
- Handoff is initiated by the current claim owner and can only target an authorized group actor; the recipient must separately accept.
- A terminal run cannot renew, release, or otherwise mutate a live claim. Expired and replaced claim generations invalidate old handles.
- Event records are append-only, bounded, and omit free-form content. The M002 projection filters claim records by project/workspace/group and caps claim/dependency output.
- No background polling, repeated model calls, automatic job submission, or agent respawn was added.

## 6. Limitations and unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Path/interface matching is conservative fixed-prefix overlap and does not inspect filesystem topology or platform-specific case behavior. | False positives are possible; sound file disjointness is not claimed. | Keep these intents advisory. Any future writer enforcement needs its own accepted policy and universal mutation boundary. |
| low | Claim event rows are a bounded audit of claim transitions, not a new session event stream. | The M002 source `event_seq` does not advance merely because a claim row changes. | M004 must consume claim revisions/events through its own bounded observation contract and re-read canonical state by revision. |
| low | Hosted CI and non-macOS builds were not run locally. | No hosted or cross-platform evidence on this SHA. | Use normal CI for hosted confirmation. |
| low | Workspace Clippy remains red on the unrelated provider warning above. | Strict workspace Clippy signal is unavailable for this head. | Fix in the provider's owning scope; no M003 rework is indicated. |

No medium/high findings remain in M003 scope.

## 7. Downstream unblocking audit

- **M004 peer observation and model-facing coordination:** strict M003 dependency is met; status changed from blocked to ready. Its plan is `plans/implementation/agent-workstream-blackboard/004-peer-observation-and-tools.md`.
- **M005 group worktree placement:** remains blocked on M004 strict closure, explicit acceptance of proposed ADR-0032, and verified containment of every writer path. M003's advisory claims do not satisfy these gates. ADR-0032 remains proposed; this closure does not accept it.
- **M006 and M007:** remain blocked on M005. M007 interface research may proceed independently, but there is no registered implementation prerequisite now unblocked by M003 alone.
- **M008:** remains blocked on M004, M006, and M007, inheriting M005/ADR gates.
- No other registered plan in this line became ready.

## 8. Registry and documentation updates

- `plans/implementation/agent-workstream-blackboard/003-task-claims-and-dependencies.md` is marked closed and links this record.
- `plans/implementation/agent-workstream-blackboard/004-peer-observation-and-tools.md` is marked ready.
- `plans/registry.md` and `plans/subsystems/agent-workstream-blackboard-roadmap.md` record M003 closure, activate M004, and retain M005's ADR/writer-path gates.
- `architecture/workstream_claims.md`, `architecture/storage.md`, `architecture/projection.md`, `architecture/overview.md`, `.opencode/skills/session-storage/SKILL.md`, and `.opencode/skills/bus-projection/SKILL.md` document ownership and limits.
