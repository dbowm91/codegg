# Linux Execution Isolation C008 — M007 Rootless Resource Delegation Feasibility

Status: **ready for bounded probe/fixture**, production required budgets blocked on native pre-exec attachment and verified cgroup delegation
Class: infrastructure / invariant
Closure target: `plans/closure/linux-execution-isolation/016-status.md`.
Baseline: main @ `ec43e495c3619966ac33d0acccf7904812dd37bc` (2026-10-13 EDT). Long-term: `plans/000-long-term-specification.md` §§4/17/27/29, `plans/001-terminology-and-domain-model.md` §8, and `plans/003-planning-process.md`. Execution authority: ADR-0001/0004/0016; command classification: currently accepted `plans/adrs/ADR-0028-command-syntax-and-execution-authority.md` (canonical ID to be reconciled under C004). Existing Landlock ABI >=3 filesystem-only support must remain intact. This is a handoff plan, not a claim of implementation or test execution.

## 1. Why this is an independent next line

M002's typed `ResourceBudget` request and `LinuxObtainedEnforcement` distinguish requested from obtained capability, but actual cgroup v2 limits are not installed. `ManagedProcessService` owns timeouts, output bounds and process group cleanup, **not** kernel-enforced RAM/CPU/PIDs quotas. The M007 plan's dependency on M002's interface is now satisfied conditionally for test-only work; lack of rootless delegation is not permission to enable setuid, modify host cgroups or pretend `RLIMIT_AS` equals cgroup memory.

This bounded work may proceed **in parallel** with M003 namespace feasibility. It cannot change default execution budgets or advertise enforced resource limits.

## 2. Native feature and lifecycle packages

1. Read and test cgroup v2 mount, safe operator-delegated writable subtree, `cgroup.controllers`, `cgroup.subtree_control`, `memory.max`, `pids.max`, `cpu.max`, `cgroup.kill` and actual non-root process attachment. Distinguish read-visible `controllers` from **writable delegation** and from attach-before-exec ability. Record systemd vs non-systemd user sessions and no-cgroup/no-delegation hosts; never write global parents.
2. In a disposable test-only fixture, create a unique attempt-owned leaf, install narrow safe limits, move a harmless compiled child **before user entry** and prove descendants inherit cgroup membership. Record actual OOM/pids/cpu throttling with small safe budgets and helper/target membership. If attach race cannot be eliminated, refuse required budget and stop.
3. Test cancel/timeout/crash, `cgroup.kill` availability, live-descendant detection, idempotent leaf cleanup, daemon restart stale-leaf identity and concurrent jobs. Never delete or signal unrelated host resources. Preserve bounded output and audit no secrets.
4. Model required/optional/unrequested modes with real `ObtainedState` verification. Explicit limited rlimits (if adopted) must be labeled separate semantics; no claim of cgroup v2 memory/CPU/PID guarantee from timeouts, max output or `RLIMIT_AS`.
5. Benchmark representative `cargo check`, rustc, Python, test runner and process-heavy but benign jobs within budgets; declare known false-positive pressure and establish opt-in operator ceilings if later admitted.

## 3. Acceptance, prerequisites and stop

Test-only closure may report feasibility positive, negative or unavailable with exact Linux host/kernel/cgroup delegation details, command invocation, kernel-file measurements and no production cgroup selection. For **production** M007, require accepted launch architecture, scheduler-owned lease, resource attach-before-exec, native required budget denial, cleanup/recovery, observed limits and explicit rollout authorization. No fallback from required to optional/unrestricted.

Checks: `cargo fmt --all -- --check`, `cargo test -p codegg --lib managed_process`, `cargo test -p codegg --test managed_process_descendants -- --test-threads=1`, `python3 scripts/check_execution_ownership.py`, `scripts/verify.sh quick` plus safe opt-in cgroup fixture with exact host feature result.

Stop if rootless delegation absent for claimed profile; if the child runs before joining a cgroup; if a quota or teardown could kill the daemon/another session; or if required quotas can be bypassed by alternate executors. No new privileged service or host-wide provisioning is authorized by this plan.
