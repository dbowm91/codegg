# Linux Execution Isolation M007 — Bounded Resource Envelopes

Status: blocked — hard: M002; independent of M003–M006 after interface closure
Primary class: capability
Repository baseline: `main` @ `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`, 2026-10-13.
Source: `plans/subsystems/linux-execution-isolation-roadmap.md`.
Long-term requirements: `plans/000-long-term-specification.md` §§4,17,27,29; `plans/001-terminology-and-domain-model.md` §8.
<<<<<<< HEAD
Applicable accepted decisions: ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0031.
=======
Applicable accepted decisions: ADR-0001, ADR-0002, ADR-0004, ADR-0016, ADR-0035.
>>>>>>> d8da9933 (fix(plans): reconcile Linux ADR identity on current main)
No production implementation or verification is asserted by this plan.
Target closure record: `plans/closure/linux-execution-isolation/007-status.md`.

## 1. Objective
Add scheduler-owned optional/required Linux per-attempt limits for memory, CPU and process count (and I/O where truly delegated), using cgroup v2 where available and truthful narrower rlimit-only fallback when permitted. Existing output caps, wall-clock timeouts and process-group cancellation remain complementary and do not themselves count as hard resource containment.

## 2. Why this milestone and existing evidence
`src/managed_process.rs` currently enforces bounded capture, timeout and process-group cleanup but has no canonical cgroup-v2 cgroup ownership/attachment. M002 supplies requested/obtained dimensioned launch and trusted helper path. cgroup v2 controllers and writable delegated subtrees are not universal on rootless Linux hosts.

## 3. Non-regression and security invariants
Immutable scheduler-approved job budget; model/code/env/child cannot widen limits or select arbitrary parent cgroup. Required limit unavailable => no target execution; optional limit absent => report unsupported, never claim enforced. Target and **all descendants** must enter configured owned cgroup before running user code. Never write host-wide controller settings, kill an unrelated reused PID/cgroup, or require root privileges implicitly. `memory.max` is not RSS and `cpu.max` is a bandwidth limit, not deterministic CPU time; `RLIMIT_AS` is not equivalent to cgroup memory containment.

## 4. Scope, required production changes and ownership
Define memory bytes, CPU bandwidth/period, pids maximum and I/O requirements with overflow validation and parent-to-child ceiling. Resolve operator-delegated cgroup-v2 parent, validate writable controllers, create unique attempt-bound leaf, install limits, place helper/target prior to exec, verify actual membership and effective limits. Integrate `cgroup.kill` or owned safe pidfd/process cleanup, monitor OOM/pids/cpu/IO events, and report independently from output and timeout status. Use a separately named rlimit-only outcome only for guaranteed rights; no fake full cgroup outcome. Add bounded per-job admission and conservative defaults without modifying existing default sandbox policy.

Explicit non-goals: altering macOS/Windows execution defaults; creating a second independent process executor; changing worktree/session authorization; agent-selected escalation to FullHost; undocumented broadening of permissions; marking a requested but unproven enforcement result as applied.

Core/domain: retain typed policy snapshot, lease and immutable parent/child ceilings. Storage/migrations: no initial DB schema change; if durable preferences or public protocol/DTO fields change, design a versioned migration and compatibility test before shipping. Runtime/concurrency: execution belongs to `ManagedProcessService`, parent and helper must bind the status to the specific child before user exec. Frontend/operator surface: show requested and obtained dimensions and a safe explicit denial/next action. Security: untrusted workspace and child inputs never select helper, override enforcement, or control trusted broker metadata. Documentation/guards: update `architecture/security.md`, relevant jobs/tool docs, tests and static guards.

## 5. Ordered work packages and evidence
A. Probe cgroup mount, delegation, controllers, `cgroup.kill`, process identity and permission, with negative no-delegation host.
B. Implement typed resource envelope + owned cgroup lease and child-before-exec attach in M002 launch.
C. Wire supervised cancellation/OOM/timeout, per-attempt kill and cleanup with stable identity; avoid PID reuse and orphaned helper.
D. Test fork-heavy, memory and CPU fixture under safe caps with concurrency. Exercise I/O only when delegated.
E. Publish per-dimension status, docs, compatibility tests and closure.

## 6. Failure, cancellation, restart and contention
On setup failure remove only owned inactive leaf before launch. After cancellation/OOM/timeout, reap descendants and remove owned cgroup after verifying no remaining processes. Restart reconciler must not kill a still-valid different generation or stale PID. Two jobs receive distinct leaves and do not starve the daemon. Disable remote/staged execution claims unless their execution node enforces local limits.

## 7. Compatibility and migration
Default `ReadOnly`/`WorkspaceWrite`/`FullHost` vocabulary, existing Landlock ABI>=3 baseline and legacy unrestricted shell network stay truthful and usable until explicit, reviewed adoption. No migration is implied by writing this plan. Platform unsupported or optional skipped test is not a proof of security. A new public protocol or persisted preference must be versioned; older consumers cannot silently treat unsupported required constraints as met.

## 8. Focused, integration, negative and qualification tests
Unit: budget overflow, narrowing, controller capability, required/unavailable semantics, status and ownership. Native Linux: `memory.max` OOM, pids cap, `cpu.max` throttling, helper/target membership before marker, descendant cleanup, no-delegation refusal, concurrent jobs and restart state. Use small bounded fixtures safe for shared CI. Commands: `cargo fmt --all -- --check`; `cargo test -p codegg --lib managed_process`; `cargo test -p codegg --test managed_process_descendants -- --test-threads=1`; `scripts/verify.sh quick`. Add guarded Linux cgroup test target and record real positive supported delegation; an all-skipped result cannot close required resource support.

## 9. Acceptance criteria and closure evidence
On an admitted delegated Linux host each required quota is genuinely configured **before user exec**, observed, respected by descendants and cleaned on success/cancel/OOM/restart. No-delegation required mode refuses before execution and optional mode reports exact absence. Legacy processes remain functional with no false resource enforcement claim.

Record implementation commits, host capability/kernel version, test output, skips/blocks, requested/obtained status, requirement-to-evidence table, compatibility, rollback, unresolved findings with severity and a justified closed/conditionally closed/blocked recommendation in `plans/closure/linux-execution-isolation/007-status.md`. Do not claim verification merely because fixtures exist.

## 10. Stop conditions
Stop if a user command executes before cgroup attachment, if cleanup can kill unrelated processes/daemon, if quotas require unapproved host privileges, or if missing controller is interpreted as enforcement.
