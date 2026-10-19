# Agent Loop Operational Reliability M005 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-operational-reliability/005-subagent-admission-and-failure-visibility.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md`

Repository baseline reviewed: `f37840ef5fca36d44b35daec5da6b26cd049a3ee`

Implementation commit: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

Verified implementation head: `c409e1e012d9e599abe67e975c28f4fac9dd66de`

## 1. Executive finding

Concurrent durable run admission now takes a SQLite write lock before reading the idempotency key, avoiding stale-snapshot upgrade errors. Terminal failures are redacted and bounded; task status surfaces typed failure details.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Concurrent idempotent admission | `codegg-core` AgentRunStore concurrency test | pass | Eight-connection admission yields one durable run. |
| Failure detail is safe to persist and expose | terminal failure redaction test | pass | Secret-shaped values are redacted and output bounded. |
| Child/task outcome is actionable | task status/get/wait regression tests; workspace suite | pass | Includes failure stage/class/details. |
| Existing broad checks | quick verification and workspace nextest | pass | 14,614 default-feature tests passed. |

## 3. Production implementation evidence

SQLite create-or-get begins an immediate transaction before the idempotency lookup. Terminal failure text is redacted and capped before persistence. Task status, wait, and get preserve access checks and include typed failure stage/class/details.

## 4. Verification executed

```bash
rtk cargo test -p codegg-core --locked sqlite_concurrent_duplicate_admission_converges_to_one_run
rtk cargo test -p codegg-core --locked terminal_failure_diagnostic_is_redacted_and_bounded
rtk cargo nextest run --workspace --locked --profile ci --no-fail-fast
rtk scripts/verify.sh quick
```

Concurrent admission passed (1 passed, 1,038 filtered); redaction passed (1 passed, 1,038 filtered); workspace 14,614 passed and quick verification passed. Workspace clippy also passed. No external DB service was required.

## 5. Invariant review

Idempotency key and session/run identity remain durable and unchanged. Admission is still one transaction, stored failures are bounded/redacted, and task ownership/access controls remain in force.

## 6. Failure and recovery review

The write lock serializes the read-then-create path under contention. Concurrent duplicate requests return the existing run. Terminal failure remains queryable without replaying work. Cancellation and scheduler recovery continue to use existing lifecycle owners.

## 7. Migration and compatibility review

No schema migration or protocol change. Existing AgentRun records remain readable.

## 8. Security review

Secret-like failure values are redacted before persistence and bounded. Task access checks and run identity fences are unchanged.

## 9. Documentation and operations

Updated `architecture/agent.md` with admission and failure-visibility behavior.

## 10. Unresolved findings

None within M005.

## 11. Roadmap disposition

Milestone closed; the final M007 combined qualification has completed.

## 12. Registry updates

M005 is closed. No successor in this line remains blocked.
