# M005 Writer-Containment Corrective C006 — Restricted Shared Checkout End-to-End Qualification (Q1–Q5)

Status: blocked — C004 AND C005 strict closure, C001–C003/ADR-0042 upstream gates inherited
Primary class: capability closure / invariant
Planning baseline: plan/agent-workstream-m005-writer-audit 2026-10-19; revalidate exact implementation HEAD.
Source: plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md C006
Original: plans/implementation/agent-workstream-blackboard/005-group-worktree-placement.md, Q1–Q5 and strict M005 closure.
ADRs: accepted ADR-0002/0032 plus **accepted ADR-0042 (option B, 2026-10-19)**; the C004+C005 strict closures remain the binding gate.

## Objective

Qualify and, only if fully supported, expose an opt-in **CodeGG-mediated serialized checkout** to a bounded two-agent workstream. Native model file and typed Git operations serialize; permitted readers overlap; every other CodeGG writer either participates or denies before effect; arbitrary process-backed capabilities remain unavailable. Confirm explicit checkout/group ownership, authorization, cancellation and recovery with real temporary Git worktrees on every platform *claimed supported*. If positive qualification is impossible, record a negative or conditionally closed disposition and leave the isolated-worktree default; do not force C006 success.

## Work packages

A. Entry contract and preflight: add the narrow opt-in placement to task spawn/spawn_many and durable run construction only after C001–C005 completion. Validate authenticated group owner/member, narrow effective tool profile, current managed WorktreeId/lease generation, target checkout health, repository branch, existing editor/PTY/LSP/MCP process owners and active reservation, before any child execution. No two old mutating children silently end up sharing a checkout. Nested child cannot grant itself group membership or broader sandbox permissions. Older API clients remain compatible and default to auto-isolated.

B. Q1/Q2 composed: real SQLite service plus root daemon/AgentRun/JobScheduler/ToolBroker and Git in a temporary repository. Two simultaneous mutation-capable agents race for one shared checkout while readers progress. Prove one final writer, stable journal ordering, clean writer release, correct Git index/HEAD state, cancellation/exception/restart quarantine and authorization. Exercise nested parent base, sequential commits, changed interface claims, dirty/conflicted preservation and explicit integration/rejection, not automatic merge.

C. Q3 negative coverage: run each known CodeGG file API and prohibited process surface against the reserved worktree during two-run execution. File/edit/Git writes take permit. HTTP/file, editor/save, LSP preview, init/restore and checkout-local state participate or fail before first effect. Shell/Python/test/build/PTY/LSP/MCP/plugin/hook/Tool Program jobs must not run under v1. Include misleading 'read-only' tool metadata and cross-principal or unrelated session routes. There must be no out-of-band CodeGG-owned writer during reservation.

D. Q4 native matrix: qualify on Linux, macOS and Windows independently with actual supported backends and adversarial cancellation/race/restart. Missing native containment/guard evidence means disabled on that OS. Process-tree escape evidence is recorded as a negative result for process-capable extensions; it does not invalidate v1 if the path cannot launch. External OS same-user edits are explicitly outside contract; run a negative demonstration where practicable to ensure documentation never claims universal exclusion.

E. Q5 rollout and UX: small backward-compatible CLI/TUI/daemon placement selector and typed errors only for platforms that pass; show 'CodeGG-mediated serialized (native file/Git only)' rather than 'fully isolated'; list denied tools. Use bounded existing group/agent projections; no new model turns from state observation. Controlled opt-in (off by default) with clear feature availability reason and operator recovery for Quarantined checkout.

F. Closure report: finish requirement/evidence matrix with tests, source lines, commands, platform outcomes, run/group/worktree lease identities, regression evidence, dirty cleanup, resource fairness, no new scheduler, and accepted ADR chain. Verify parent M005 checklist; mark strict closed ONLY if every parent acceptance clause applicable to the qualified restrictive profile has valid production evidence, otherwise mark conditional/blocked and file narrow follow-up.

## Test matrix / commands

At least N=2 live-scheduler local fixture with barriers and real temporary Git checkout, N=16 contention store test, read/write concurrency, two authorized sessions from same and different principals, cross-project denial, stale generation/replay, cancellation during file rename and Git commit, process pre-spawn deny, daemon kill/restart while writer held, Quarantined recovery and separate worktree parallel progress. Ensure single-agent and auto-isolated behavior unchanged. Use scripts/verify.sh quick, cargo fmt --all -- --check, cargo clippy --workspace --all-targets --locked -- -D warnings where supported, focused core/root/worktree/Git/job/http/document/PTY tests, bash scripts/check-core-boundary.sh, python3 scripts/check_scheduler_bypass.py, scripts/check_execution_ownership.py and relevant authorization/sandbox guards. Hosted platform tests only in existing workflows. No new CI lane or benchmark threshold.

## Stop and closure

Any unmediated CodeGG writer, process denial bypass, wrong lease generation, unauthorized group member, premature release, unresolved medium-or-higher correctness/security finding, or platform without genuine evidence disables that platform's shared checkout mode. The restricted v1 cannot claim useful Rust builds within the shared checkout; M007 isolated per-workspace scheduling remains separate. If C006 is negative, preserve isolation and close this corrective only as a negative research disposition, not a completed product capability. File plans/closure/agent-workstream-m005-writer-containment-corrective/006-status.md and reconcile parent M005 roadmap/registry/unblocking in the same closure commit.
