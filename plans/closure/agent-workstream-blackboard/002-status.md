# Agent Workstream Blackboard M002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-workstream-blackboard/002-durable-workstream-state.md`

Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md#milestones--deliverables`

Repository baseline reviewed: `3fd262fff06e72b0caaf69917d68b6f23bf44710` (M001 closure); implementation reviewed at `9c60c8e5d9e867c45b1cf9ca20ce31f0b9ac5499`.

Implementation commits:

- `9c60c8e5d9e867c45b1cf9ca20ce31f0b9ac5499` — add bounded workstream snapshot projection

## 1. Executive finding

M002 is complete as a read-only projection over the existing authorized session projection stream. `project_workstream` joins one run group to bounded durable run, job, and worktree facts; it uses the source snapshot's `event_seq` as its revision, exposes missing facts as unavailable, and grants no execution or control authority. Existing `ProjectionSubscribe` authorization and snapshot/replay/restart machinery remain the only transport and access boundary. No new store, migration, worker, or polling loop was introduced.

M003 is now dependency-ready. M004 remains blocked on M003. M005 remains blocked on M004 and explicit acceptance of proposed ADR-0032; no shared-write authority is granted by this closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Bounded typed view over existing group/run/job/worktree facts | `crates/codegg-protocol/src/projection/workstream.rs`; five unit tests | pass | Group and member list are capped at 16; text identifiers are byte-bounded. |
| Existing project and session authorization; no identifier-only access | Public projector accepts only an already-authorized `SessionProjectionSnapshot`; daemon subscription/resume remains guarded by the existing principal/project/session policy. Projection replay integration includes project and session stream isolation cases. | pass | No new CoreRequest or unguarded endpoint was added. |
| Correctly scoped facts and explicit missing-data behavior | Project/workspace mismatch, cross-workspace job, missing worktree, and missing run tests | pass | Linked but unavailable jobs/worktrees are typed as unavailable; run omissions are listed. |
| Replay, reconnect, restart, and completion convergence | View revision is the source `event_seq`; existing projection replay integration suite covers restart high-water, replay, resume, project/session isolation, and ordered delivery. | pass | The derived view adds no independent sequence or lifecycle worker. |
| No prompts, task descriptions, natural-language result prose, job summaries, paths, or logs | DTO allowlist in `AgentWorkstreamMember`; serialization redaction assertion; existing core adapter sentinel test | pass | Only bounded status and commit/validation facts are returned. |
| No authority to create runs/worktrees or trigger model continuation | Pure protocol projector performs no I/O; architecture documents read-only semantics. | pass | Existing execution APIs and ownership remain unchanged. |

## 3. Production implementation evidence

`AgentRunSummary` now carries serde-defaulted project, repository, workspace, and job IDs from the canonical task/run records. `projection::workstream` exports `AgentWorkstreamSnapshot`, member facts, and explicit job/worktree availability enums. It checks project/workspace scope, checks worktree ownership against the run, caps the group membership and emitted member list at 16, and omits arbitrary job summaries and terminal prose. The view's revision and source session are taken from the authorized `SessionProjectionSnapshot`.

The adapter is deliberately a derived library surface over the native projection rather than a second store or transport family. Snapshot, event replay, subscription authorization, and restart recovery continue through the existing daemon projection service.

## 4. Verification executed

All commands ran in a native arm64 shell with Rust 1.89 and `CARGO_TARGET_DIR=/tmp/codegg-blackboard-arm64-target`.

```bash
cargo fmt --all
cargo test --locked -p codegg-protocol projection::workstream
cargo test --locked -p codegg-core projection_replay::agent_runs
cargo test --locked --test headless_projection_consumer
cargo test --locked --test projection_replay
bash scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- Workstream projector tests: 5 passed.
- Core run projection adapter tests: 2 passed.
- Headless projection consumer tests: 2 passed.
- Projection replay integration tests: 68 passed.
- `scripts/verify.sh quick`: passed, including format, agent schema, client/desktop/core boundaries, scheduler and authorization guards, provider guards, and `cargo check --workspace --all-targets --locked`.
- Workspace Clippy stopped on the pre-existing `clippy::explicit_auto_deref` at `crates/codegg-providers/src/provider_core.rs:2941` (`*secret` should be `secret`). This file is outside the change; no Clippy success is claimed.
- One initial attempt used the plan's stale integration-target spelling `session_projection_consumer`; Cargo reported the registered target is `headless_projection_consumer`. The corrected target passed.

These are local results; hosted CI was not run.

## 5. Invariant review

- Session/project authorization is inherited from the native projection subscription; the helper cannot fetch data by ID.
- Group membership and emitted facts remain bounded; a truncated group is identified as such.
- Foreign-project/workspace runs and foreign-workspace jobs are omitted or marked unavailable; a worktree must identify the member run as owner.
- Projection fields are an allowlist of bounded facts; prompts, task bodies, paths, logs, terminal prose, and job summaries are excluded.
- Reads do not allocate work, mutate durable state, or continue model execution.

## 6. Failure and recovery review

The workstream projection is recomputed from each authorized snapshot, so reconnect and daemon restart naturally refresh from canonical run/group/job/worktree stores. Its revision follows the existing replay cursor. Missing run records are explicitly recorded; missing jobs/worktrees use typed unavailable variants; incomplete membership is never filled with inferred terminal state. Existing projection sequence and gap handling remain authoritative.

## 7. Migration and compatibility review

No schema migration was required. Four additive `AgentRunSummary` fields use `serde(default)` for old snapshots. The workstream view is a new exported protocol helper and does not change existing request/event variants or agent execution defaults. Older clients continue to ignore additive fields through existing serde compatibility.

## 8. Security review

The join is only available after obtaining an authorized session projection. It neither resolves caller identity nor creates a new access path. Project/workspace mismatches are filtered, linked job facts are checked against the source workspace, and linked worktree facts must be owned by the matching run. No user-authored descriptions, prompts, or raw output are copied into this view.

## 9. Documentation and operations

- `architecture/projection.md` documents the bounded workstream view, source cursor, redaction, and read-only boundary.
- `.opencode/skills/bus-projection/SKILL.md` indexes the new `workstream` module and its ownership contract.
- Operators use the existing projection subscription/replay and daemon lifecycle; no extra worker or configuration is required.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Workspace Clippy remains red on an unrelated pre-existing provider warning. | Strict Clippy signal is unavailable for this head. | Correct the provider warning in its owning scope; no M002 rework is indicated. |
| low | Hosted CI and native non-macOS platforms were not exercised. | No hosted or cross-platform evidence on this SHA. | Use normal CI for hosted confirmation. |

No medium/high findings remain in M002 scope.

## 11. Roadmap disposition

M002 closes the read-only durable projection prerequisite. M003 can begin its transactional claim/dependency lifecycle using this projection contract. M004 remains blocked on M003. M005 remains blocked on M004 and explicit ADR-0032 acceptance, plus verified writer-path containment. No other registered plan in this line became ready.

## 12. Registry updates

`plans/registry.md` marks M002 closed, registers M003 as ready, and leaves M004/M005 blocked with their existing precise gates. The subsystem roadmap records M002 closure and the M003 handoff. The M002 implementation plan is marked closed and links this record.
