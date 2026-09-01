# M002 — Run Mailbox, Journal, and Async Control Closure

Status: closed

Closure date: 2026-09-01

Reviewed implementation commit: `36e19e6f93610029e608549e40846508d96f692f`

Implementation plan: `plans/implementation/agent-run-worktree-concurrency/002-run-mailbox-journal-and-async-control.md`

Source roadmap: `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

## Executive finding

M002 is closed. Agent runs now have a daemon-owned, bounded mailbox and stable-boundary journal with typed control identities, durable SQLite persistence, deterministic per-run ordering, idempotent submission, restart-safe delivery state, and lineage-scoped authorization. The live bridge feeds follow-up, steering, and cancellation into active `AgentLoop` instances, while terminal completion can be delivered to an active parent without polling and remains durable when the parent is disconnected.

## Delivered scope

- Added typed mailbox and journal domain contracts plus in-memory and SQLite stores.
- Added storage migration 38 with bounded payload/metadata checks, per-run ordering, idempotency, pending/recent indexes, and additive/idempotent schema creation.
- Added `RunControlService` with owner/ancestor authorization, persist-before-signal delivery, replay of queued/delivered controls, bounded status/wait, terminal supersession, and compact parent completion follow-ups.
- Added live-run handles for follow-up, steering, and cancellation; integrated the service with `AgentLoop` at the `before_provider_turn` safe boundary while retaining existing cancellation checks around active work.
- Extended `TaskTool` with additive `status`/`get`, `message`, `interrupt`, `wait`, and `cancel` actions, bounded arguments, durable parent lineage, and compatibility-preserving legacy behavior.
- Added lifecycle and control journal events for run creation/queue/start, control queue/delivery, safe boundaries, cancellation, completion, and recovery-relevant transitions.
- Updated architecture documentation and preserved the explicit scope boundary: worktree leases, mutation isolation, groups, and final projection/compatibility deletion remain later milestones.

## Acceptance evidence

| Acceptance area | Evidence | Result |
|---|---|---|
| Bounded ordered mailbox/journal | Core store bounds and migration 38; concurrent in-memory sender test yields the complete ordered sequence; duplicate idempotency keys create one delivery | met |
| Live safe-boundary control | `RunControlService` dispatches through registered `LiveRunHandle`; `AgentLoop` records a safe boundary before the next provider turn; interrupt and cancellation use dedicated runtime signals | met |
| Restart and terminal behavior | SQLite service recreation replays queued/delivered controls once; terminal runs supersede pending controls; terminal completion is durable | met |
| Task surface and bounded wait | `TaskTool` supports spawn/status/get/message/interrupt/wait/cancel; wait timeout is capped at 30 seconds and uses bounded polling | met |
| Async completion | Terminal recording appends authoritative state and sends one compact follow-up to an active authorized parent; disconnected parents retain durable state | met |
| Authority and safety | Sender identity comes from the session/run context; owner and direct/indirect ancestor checks reject unrelated or forged actors; control text remains ordinary child input | met |
| Compatibility and scope | Legacy task spawn/get behavior remains available; no worktree/group/projection authority was introduced | met |

## Verification evidence

All verification was run from the reviewed implementation tree:

- `scripts/verify.sh quick` — passed.
- `scripts/verify.sh full` — passed, including the default and `server,plugins,lsp-test-support` workspace sweeps.
- `cargo check -p codegg-core` and `cargo check -p codegg` — passed.
- `cargo clippy -p codegg-core --all-targets -- -D warnings` — passed.
- Focused core mailbox/journal, run-control, scheduler-cancellation, subagent, projection-consumer, and static guard suites — passed.
- `git diff --check` — passed.

The full verification output contained only the existing macOS linker section-size warning and existing provider missing-key test warnings; no test, lint, format, boundary, or static guard failure occurred.

## Downstream dependency audit

- M003 — durable daemon worktree service and leases: remains `ready`; M001 remains satisfied and M003 does not depend on M002.
- M004 — isolated mutation and structured results: remains `blocked` on M003; M002 is now satisfied.
- M005 — run groups and background joins: remains `blocked` on M004; M002 is now satisfied.
- M006 — projection, compatibility, and closure: remains `blocked` on M001–M005.

No future plan was moved from `blocked` to `ready` by this closure. The subsystem roadmap remains active with M003 as the next dependency-ready handoff.

## Unresolved findings and disposition

No high- or medium-severity unresolved finding remains in M002 scope. The safe-boundary implementation is intentionally limited to the existing `AgentLoop` control seams; durable worktree ownership, mutation isolation, run groups, and final projection/compatibility simplification are explicitly deferred to M003–M006. M002 is formally closed.
