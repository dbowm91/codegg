# Agent Run, Async Delegation, and Worktree Concurrency M008 — Closure Status

Status: closed

Closure date: 2026-09-01

Source implementation plan: `plans/implementation/agent-run-worktree-concurrency/008-call-identity-projection-and-strict-closure.md`

Source corrective roadmap: `plans/subsystems/agent-run-worktree-concurrency-corrective-closure-addendum.md`

Repository baseline reviewed: `5ced31bf` (`main`, implementation candidate)

Implementation commits: `5ced31bf` — Implement M008 call identity and projection authority

Superseded strict subsystem disposition retained: `plans/closure/agent-run-worktree-concurrency/006-status.md`

## 1. Executive finding

M008 is fully implemented and strictly closed. TaskTool now preserves the
accepted model tool-call identity through the structured broker path, and
explicit keys retain precedence for callers that provide them. Distinct
identical messages, spawns, groups, and `spawn_many` calls no longer collapse;
retries of one accepted identity remain idempotent. Durable request
fingerprints and mailbox operation comparisons reject incompatible reuse of an
idempotency key.

Projection depth is now sourced only from `AgentRunRecord.depth`; the caller
supplied depth parameter and scheduler 0/1 inference were removed. Group
projection exposes bounded turn/run ownership metadata additively. M007’s
owner, lineage, depth, worktree, scheduler, and authorization corrections were
independently re-reviewed and remain intact. The historical M006 strict
closure claim is superseded by the additive M007/M008 corrective records; its
evidence was not rewritten.

## 2. Requirement-to-evidence matrix

| Finding / requirement | Production evidence | Verification evidence | Disposition |
|---|---|---|---|
| F1 root fan-out owner | M007 `AgentOrchestrationOwner::Turn` and turn-owned groups remain wired; M008 uses the same owner-scoped path | Core group tests; M007 closure review | closed |
| F2 current/parent identity | M007 durable current-run context remains the nested owner; M008 does not regress it | subagent, scheduler, and core suites | closed |
| F3 nested context | M007 store-derived project/repository/workspace/turn context remains attached to nested TaskTool | subagent and worktree suites | closed |
| F4 authoritative depth | `AgentRunRecord.depth` remains transactionally validated and is the sole projection source | 514 core tests; projection replay tests; quick verification | closed |
| F5 control authorization | M007 exact-turn/direct-parent authorization remains unchanged; M008 only changes mailbox identity | run-control 4-test suite; scheduler cancellation/restart suites | closed |
| F6 call identity collapse | structured TaskTool execution consumes `ToolExecutionContext`; tool batch forwards `invocation_key`; delegation/group/control keys derive from resolved identity | TaskTool 3 tests; control 4 tests; core 514 tests | closed |
| F7 caller-supplied projection depth | `agent_run_summary` reads `run.depth`; daemon and scheduler no longer pass inferred depth | projection replay and session projection suites; quick verification | closed |
| incompatible spawn replay | additive `agent_task.request_fingerprint` migration and memory/SQLite conflict checks | core agent-run suite, including memory and SQLite conflict tests | closed |
| incompatible mailbox replay | mailbox duplicate lookup compares kind, payload, and sender before returning existing record | run-control suite | closed |
| additive owner projection | group DTO has serde-defaulted owner kind/session/turn fields and derives them from durable group owner | core projection test; protocol projection 70 tests | closed |

## 3. Production implementation evidence

- `TaskTool::execute_structured` and its shared context-aware implementation
  resolve explicit input key, accepted invocation key, then a fresh bounded
  legacy key. There is one action parser and one execution path.
- The agent tool-batch broker adapter forwards the accepted invocation key as
  `BrokerInvocationContext.submission_key`, which becomes
  `ToolExecutionContext.invocation_key` at the tool boundary.
- Delegation identities are call-derived and payload fingerprints are stored
  separately. Same-call retries reuse the durable task/run/job; different
  accepted calls with equal payloads receive different identities.
- `spawn_many` invokes the internal context-preserving helper with bounded,
  deterministic `/member/<ordinal>` identities. It does not recurse through a
  context-free public execute path.
- Control mailbox duplicates compare immutable operation fields and return an
  explicit `IdempotencyConflict` for incompatible reuse.
- Storage migration v44 adds the bounded request fingerprint additively and
  raises `STORAGE_LAYOUT_VERSION` to 44. Historical empty fingerprints remain
  readable and are not re-keyed.
- Projection adapters derive depth from the durable run record and project
  only bounded group owner metadata. Prompts, paths, mailbox bodies, authority
  bodies, credentials, and hidden reasoning remain excluded.

## 4. Verification executed

All results below are local evidence run against implementation candidate
`5ced31bf`; no hosted result is claimed.

- `rtk cargo test -p codegg-core --locked -- --test-threads=1` — 514 passed.
- `rtk cargo test -p codegg-core agent_run --locked -- --test-threads=1` — 17 passed.
- `rtk cargo test -p codegg-protocol projection --locked -- --test-threads=1` — 70 passed.
- `rtk cargo test --lib tool::task --locked -- --test-threads=1` — 3 passed.
- `rtk cargo test --lib agent::run_control --locked -- --test-threads=1` — 4 passed.
- `rtk cargo test --lib scheduler --locked -- --test-threads=1` — 75 passed.
- `rtk cargo test --test subagent --locked -- --test-threads=1` — 22 passed.
- `rtk cargo test --test worktree --locked -- --test-threads=1` — 14 passed.
- `rtk cargo test --test session_projection_consumer --locked -- --test-threads=1` — 8 passed.
- `rtk cargo test --test scheduler_restart_recovery --locked -- --test-threads=1` — 15 passed.
- `rtk cargo test --test scheduler_cancellation --locked -- --test-threads=1` — 10 passed.
- `rtk cargo test --test scheduler_contention --locked -- --test-threads=1` — 14 passed.
- `rtk bash scripts/verify.sh quick` — passed, including workspace all-target
  check and generated-agent/core-boundary/sandbox/ownership checks.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk git diff --check` — passed before closure documentation changes.
- Relevant static guards — passed: core boundary, scheduler bypass,
  execution ownership, daemon CWD, identity paths, Git forbidden patterns,
  tool-broker boundary, projection disclosure/publication/transport/lifecycle,
  WebSocket bounds, and project-catalog invariants.

The initial guard sweep included one malformed local invocation that attempted
to run a shell script through `python3`; it was immediately corrected. The
correct documented guard commands passed and only those corrected results are
closure evidence.

## 5. Invariant review

Call identity is accepted provenance, not model prose or display text. Explicit
keys override transport identity, and legacy direct calls receive fresh keys.
All stored identity/fingerprint values are bounded digests or bounded opaque
references. Different call IDs remain distinct even when request content is
identical.

Projection remains derived and non-authoritative. Depth is copied from durable
run state, including descendants at depth 2 and beyond; no TUI nesting,
parent-presence shortcut, path name, or event order determines it. Additive
serde defaults preserve old snapshots and clients.

The scheduler remains the only daemon machine-resource authority. M007’s
direct-owner control rules, nested worktree isolation, repository/base
identity, dirty/conflicted retention, and compatibility TaskStore boundary
were not weakened.

## 6. Failure and recovery review

Same-identity retries resolve existing durable acceptance, job, group, or
mailbox state. Incompatible request or mailbox data returns an explicit
conflict instead of returning unrelated old state. SQLite migration and
round-trip tests cover restart-readable fingerprints and existing durable
records. Existing scheduler restart, cancellation, contention, subagent, and
worktree suites passed, including terminal-state and cleanup behavior.

`wait` and status remain observational and bounded; they do not create mailbox
records or change authority. `spawn_many` member identities are stable across
retry and preserve explicit partial rejection reporting.

## 7. Migration and compatibility review

Migration v44 is additive and idempotent: it adds
`agent_task.request_fingerprint TEXT NOT NULL DEFAULT ''`. Existing M001–M007
records remain readable, historical identities are not silently re-keyed, and
the existing mailbox/run/group stores remain authoritative. The legacy
numeric TaskStore, direct `execute()` callers, old snapshots, and additive
projection clients remain supported.

## 8. Security review

No hidden reasoning, credentials, prompts, full paths, mailbox bodies, or
authority bodies are added to projection or identity records. Fingerprints are
bounded SHA-256 digests and do not duplicate request bodies. Control authority
continues to require exact turn ownership or direct parent ownership; same
session alone is insufficient. Static disclosure and broker-boundary guards
passed.

## 9. Documentation and operations

Updated `architecture/agent.md`, `architecture/scheduler.md`, and
`architecture/projection.md` with call identity, fingerprint conflict,
authoritative depth, and bounded group-owner projection contracts. The
corrective addendum and registry are reconciled below. No new CI lane, live
provider dependency, scanner, release automation, or coverage/benchmark gate
was added.

## 10. Unresolved findings (severity: critical/high/medium/low)

None in M008 or the M007 corrective scope. No critical, high, medium, or low
corrective finding remains open for this subsystem. Hosted verification was not
required by the plan and is not represented as local evidence.

## 11. Roadmap disposition

M008 closes F6 and F7 and independently accepts M007’s F1–F5 corrections.
The corrective addendum is now closed, and the agent-run/worktree subsystem
returns to strict `closed` status. M006 remains an unchanged historical record
whose strict subsystem disposition is superseded by this corrective closure.

The registry and affected roadmap were audited for downstream dependencies.
No registered implementation plan lists M008 as a remaining hard or interface
dependency, and the Blocked work section contains no agent-run/worktree plan.
Therefore no future plan was unblocked or newly registered in this closure.

## 12. Registry updates

- Marked the M008 implementation plan `implemented`.
- Marked the corrective addendum `closed`.
- Marked the active agent-run/worktree registry row `closed`.
- Removed M008 from dependency-ready plans.
- Added M008 to recently closed control points with implementation and closure
  references.
- Recorded that the full blocked-work/dependency audit unblocked nothing.
- M001–M006 closure records, especially the superseded M006 strict record,
  were not rewritten.
