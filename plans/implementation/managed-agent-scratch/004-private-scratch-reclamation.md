# Managed Agent Scratch M004 — Session and Agent-Private Scratch Reclamation

Status: active
Repository baseline: `plan/managed-scratch-spaces` after M003 implementation `d83dfda1a6cf3676e6d221b10dd1fe080eb84794`.

Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`.
Long-term requirements: `plans/000-long-term-specification.md` §§4, 26–29; `plans/001-terminology-and-domain-model.md` §§5–8.
Applicable ADRs: ADR-0001, ADR-0002, ADR-0028. Preserve existing permission precedence and session/run ownership.
Primary class: invariant and lifecycle correctness.

## 1. Objective

Reclaim private scratch for terminal delegated runs and deleted sessions so managed temporary data does not accumulate after its owner ends. Preserve artifacts while a durable run or job can still use them.

## 2. Why this corrective is ready

M001 provides stable session roots; M002 provides run-owned private roots and durable job/run state; M003 provides validated tombstone reclamation for revoked shared scratch. M003's implementation did not connect that safe deletion mechanism to session deletion or terminal run lifecycle, leaving private scratch behind.

## 3. Current implementation evidence

`src/scratch.rs` allocates session, agent-run and shared-resource directories beneath the per-user cache. Only revoked shared resources are reclaimed. Session roots and run-private roots have owner records but no lifecycle cleanup. Session deletion is soft-delete in `SessionStore`; durable runs and jobs are available through `AgentRunStore` and `JobStore`.

## 4. Invariants

- Cleanup accepts typed host-owned session/run identity, never a caller-supplied path.
- Reclamation uses atomic tombstones, validates the owner record and full tree, rejects symlinks/reparse points and refuses unsafe traversal.
- A run's private scratch is retained until its run is terminal and its supervised child process has returned.
- A deleted session's full scratch tree is retained while any session run or durable session job is nonterminal. Restore before deferred cleanup preserves the tree.
- Daemon startup re-enqueues cleanup for deleted sessions so a process restart cannot permanently orphan a cleanup retry.
- Cleanup failure is best effort and retains/quarantines data; durable transcripts, run results and Git worktrees are outside the scratch tree.
- Workspace and scratch writes continue to bypass only implicit default `Ask`; explicit permission decisions remain authoritative.

## 5. Scope

In scope: typed run-private and session-root reclaimers, executor/session lifecycle wiring, deferred cleanup after active work completes, tests, and architecture/plan updates.

Out of scope: retention of scratch for non-deleted sessions, general cache quotas, native sandbox qualification, new storage migrations, deleting durable run results or managed Git worktrees, and shared-worktree coordination.

## 6. Required changes

### Core/runtime

Reuse the owner-validated tombstone deletion used by shared scratch. Reclaim a run-private root after durable terminal transition and after process completion. On session deletion, consult canonical run/job state; if work remains active, retry cleanup until it finishes or the session is restored.

### Storage/protocol

No schema or wire changes. Use existing session, AgentRun and Job stores. Treat query errors and result caps as a reason to defer cleanup.

### Security/docs

Keep cleanup rooted under managed cache directories, retain fail-closed path validation, and document that scratch is disposable while session transcripts, run results and worktrees are durable separately.

## 7. Ordered work packages

A. Generalize safe tombstone reclamation to agent-run private roots and deleted session roots; add filesystem tests.
B. Reclaim terminal run-private scratch after the process returns; prove one run cannot remove a sibling's root.
C. Reclaim deleted session scratch only after runs/jobs are terminal; retry while busy and stop if restored.
D. Reconcile roadmap, registry and closure evidence without changing the conditional M003 platform findings.

## 8. Failure, restart and contention

An active or ambiguous job/run retains session scratch. Cleanup query errors do not delete. A failed or interrupted filesystem removal leaves a retryable tombstone. Duplicate cleanup is idempotent. Restoring a session while the retry is waiting cancels reclamation. Startup scans soft-deleted sessions and restores retry watchers, so cleanup is recovered after daemon restart.

## 9. Compatibility

No migration, new config or protocol field. Existing scratch locators remain stable. Reclaimed scratch is disposable and is lazily recreated if a session is restored and used again.

## 10. Required tests

- Terminal run cleanup removes only that run's private root and preserves sibling roots.
- Session cleanup removes nested private/shared scratch and owner markers, and is idempotent.
- Active run/job blocks session cleanup; restored session blocks deferred cleanup.
- Symlink/reparse, malformed owner record, hard link and rename/tombstone recovery remain fail-safe.
- Existing workspace/scratch permission tests continue to prove implicit Ask correction and explicit Deny precedence.

## 11. Verification

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib scratch::tests
cargo test -p codegg --lib permission::tests::trusted_root_write_relaxes_only_the_default_ask
cargo test -p codegg --test sandbox_containment
scripts/verify.sh full
```

Obtain exact-head PR CI, desktop E2E and platform process checks after rebasing onto `main`.

## 12. Acceptance

Terminal agent-private scratch is reclaimed safely; deleted session scratch is removed only after all durable work is terminal, with retries for busy work and no deletion after restore. The cleanup tests and exact-head full CI pass. M003's unrelated platform-qualification conditions remain explicit.

## 13. Closure evidence

Record implementation SHA, cleanup lifecycle tests, exact-head CI results, permission regression evidence, platform limits, and the registry audit in `plans/closure/managed-agent-scratch/004-status.md`.
