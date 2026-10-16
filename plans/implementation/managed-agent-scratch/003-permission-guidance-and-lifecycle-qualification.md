# Managed Agent Scratch M003 — Permission Guidance, Safe Reclamation and Cross-Platform Qualification

Status: active — M002 strict closure recorded; M003 is now eligible
Repository baseline: `2e0aa549233dff1ef0caeb759690738161e7f80d` (M002 implementation accepted; revalidated on 2026-10-16).
Source roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md#7-milestones`.
Long-term requirements: `plans/000-long-term-specification.md` §§4, 26–29; `plans/001-terminology-and-domain-model.md` §§5–8; `plans/002-long-term-roadmap.md` Phase 19.
Applicable ADRs: ADR-0001, ADR-0002, accepted ADR-0028. Respect current explicit Deny and approval precedence.
Primary class: polish (with safety invariant closure).

## 1. Objective

Make scratch the low-friction alternative to unnecessary out-of-workspace temporary writes through an actionable, **non-authoritative** permission diagnostic; qualify owned-resource cleanup, lease/restart correctness and platform behavior before closing scratch as a feature.

## 2. Dependencies and current evidence

M001 must deliver stable per-session scratch and existing tools/process env; M002 must deliver explicit shared grants and job/attempt leases. `src/tool/util.rs`, `src/tool/broker.rs`, `architecture/permission.md`, `src/permission/approval.rs` and `src/security/sandbox/` own structured paths, approval and enforced sandbox policy. `src/tool/bash/policy.rs` is advisory on syntax; model command parsing cannot enforce filesystem writes and must not be expanded for /tmp.

## 3. Invariants and non-goals

No silent rewriting of absolute /tmp or /var/tmp, no shell regex/path scanning, no automatic denial or override of explicit operator Deny, no alternative grant that bypasses workspace/child worktree policy, no pretending a rejected root path is a real system /tmp substitute. Do not auto-approve after a risky finding or bypass the existing human Ask path. No new model tool, VM, mount, remote scratch copy, or permanent CI stress service.

## 4. Scope and changes

**Tool/permission UX:** on a structured filesystem operation that fails because requested out-of-workspace temporary path needs approval, include a concise authorized private scratch path and choice: retry there, or pursue the existing permission request for the original path if necessary. Offer this only when scratch exists and is allowed; classify only plain known temp-root path components via structured path validation, **not** shell command parsing. Deny stays denied, not rewritten. Do not infer intent, alter read semantics, suppress true approval requests or expose unauthorized shared paths. Prompt includes one short stable instruction; no expanding repeated prompt tokens. If tool broker cannot safely return suggestion before Ask without changing approval authority, use a post-denial diagnostic and document behavior instead.

**Cleanup:** finalize session/run/job lease reconciliation and retention. Resource lives while owning authorized session, child runs, background attempts or explicit consumers remain live. After all leases end and configured grace elapsed, the daemon may reclaim. Use owner-only metadata, strict root validation, no following symlinks/junctions, and race-resistant traversal; refuse ambiguous replacements or hard links to unmanaged content. Cleanup operates against typed managed IDs, never arbitrary model/user path input; tombstone/atomic claim and generation fencing prevent double-free or deletion of a newly recreated resource. Fail safely on cleanup error, disk full, permission deny or daemon crash; retry is bounded and reported, not a tight polling loop.

**Operations:** bounded status and retention configuration through existing daemon/config surface if needed; inactive/stale usage inspection and safe cleanup (manual invocation optional through normal CLI, never a model-only scratch tool). Avoid expensive recursive size scans on hot path; define a reasonable default retention with operator override and conservative upgrade compatibility. Avoid deleting persisted RunStore artifacts and Cargo/target caches.

**Cross-platform:** test portable path/env defaults on Linux/macOS/Windows. Required sandbox effective grants must be observed on each backend separately; Windows Required remains unavailable until its independent native qualification, with no upgrade of its security claim. On macOS distinguish cooperative environment redirection from Seatbelt filesystem enforcement. Required sandbox unavailability remains fail-closed.

## 5. Ordered work packages

A. Trace filesystem path-check/Ask/Deny transitions and implement advisory recovery result in the smallest existing tool boundary, preserving approval queue/dialog identity.
B. Implement reconcile/lease GC/retention configuration with durable tombstones or equivalent idempotent reclamation; add bounded operator diagnostics.
C. Exercise matrix of normal tool writes, explicit Deny, genuine /tmp required read/write, shell hardcoded /tmp, symlink/reparse escape and overlapping sessions/runs.
D. Perform OS-specific runtime qualification where available, update security/permission/docs, record exact-head closure with residuals.

## 6. Failure, cancellation, restart and contention

Expired session with active background job is not deleted; killed/reaped job releases only its own lease. Recovery doesn't assume a timestamp or PID alone proves liveness. Concurrent GC and new spawn cannot delete an in-use subtree; symlink tricks never traverse outside manager root. A malformed manifest causes quarantine/no deletion and operator diagnostic. Diagnostic delivery failure never creates grant or autostarts another tool call.

## 7. Compatibility/migration

Existing scratch resources from M001/M002 are recognized; old files may expire under documented safe retention only if owner is demonstrably inactive. Any new config defaults preserve current subprocess/sandbox semantics. Existing approval cache and remembered command rules are unchanged. Host /tmp remains directly addressable only via existing policy.

## 8. Required tests

- An unauthorized structured `/tmp/foo` write yields actionable authorized scratch suggestion but no silent rewrite; read of real `/tmp/foo` is not redirected. Explicit Deny never becomes allowed; authentic approval still works.
- Model shell `echo > /tmp/foo` cannot be claimed intercepted on unsandboxed FullHost; containment tests verify native backend separately.
- Active foreground, detached session, queued and running job, retry, cancel, orphan-after-crash, stale-generation and concurrent GC cases.
- Symlink/junction and rename-race negative tests at parent and nested paths; user-created file in scratch pointing outside never authorizes writing there; no deletion beyond managed root.
- Cleanup after retention only removes CodeGG-owned inactive scratch; log/artifact and Cargo caches remain intact. Two frontends consistent; no credential/sibling path disclosure.
- Python and native compiler temp compatibility, Windows TEMP/TMP, macOS TMPDIR, Linux TMPDIR; documented unqualified cases.

## 9. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-core
cargo test -p codegg --test tool_execution
cargo test -p codegg --test sandbox_containment
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_daemon_cwd_usage.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
```

Add focused cleanup, permission and restart suites; run supported Clippy `--workspace --all-targets` and actual supported host integration. Do not introduce one-off continuous bench lane or SBC startup benchmark.

## 10. Static guards and docs

Update `architecture/permission.md`, `architecture/security.md`, `architecture/agent.md`, `architecture/jobs.md`, `docs/tools.md`, `docs/execution-ownership.md` and config docs if touched. Regenerate schema only if an actual DTO changes. No weakening `check_execution_ownership.py` or sandbox guards to obtain green CI.

## 11. Acceptance and stop

Close only after a model can routinely use scratch without needless approval, non-temp paths retain normal Ask/Deny, actual original /tmp semantics are preserved, shared resources are selectively granted, and cleanup survives crash/contention without deleting live or external data. If path authorization cannot be made race-resistant, release cleanup conservatively disabled and classify milestone blocked; don't label it safe. If only cross-compiled on a platform, report *unqualified*, not verified.

## 12. Closure evidence and handoff

`plans/closure/managed-agent-scratch/003-status.md` must contain exact-head implementation SHA/PR; requirement-to-test/negative matrix; race/crash/permissions/sandbox outcome; migration/config compatibility; host-specific evidence; usage/retention measurements; unresolved severity; strict closed/conditional/blocked disposition; and a fresh downstream unblock audit. Re-check other concurrent planning streams; do not revise canonical spec or accepted ADR history.
