# Eggwork Worker M003 — Remote Resource Policy and Scheduler Capacity Convergence

Status: **blocked until CodeGG worker M001 closes**; independently implementable without Gregg/Eggwork telemetry M002
Class: scheduler / resource enforcement
Source: `plans/subsystems/eggwork-worker-telemetry-and-targeting-roadmap.md`
Reviewed baseline (2026-10-19): `src/scheduler/eggwork.rs::build_spec`, `crates/codegg-core/src/jobs/mod.rs::ResourceRequest`, `src/scheduler/admission.rs`, `src/scheduler/permit.rs`, `src/scheduler/scheduler.rs`; Eggwork `ResourceRequirements`, `Requirement<T>`, resource capabilities and server admission.
Existing remote execution closure: `plans/closure/eggwork-fixed-target-remote-execution/002a-status.md`.

## 1. Objective

Stop unintentionally treating remote compute as local compute, and allow CodeGG operators to request honest, enforceable Eggwork per-execution memory/CPU/PID policies on **explicitly selected nodes**. Keep one CodeGG global scheduler and one Eggwork local admission boundary. Do not add automated worker selection or consume Gregg telemetry for hard security/resource enforcement.

## 2. Concrete baseline findings

- `build_spec` sends `memory_bytes`, `cpu_millis`, `pids` as `Requirement::NotRequested` unconditionally, despite Eggwork cgroup-v2 support.
- CodeGG `ResourceRequest` has abstract CPU/memory/process/IO/network scheduler hints, but not necessarily equivalent physical units or CPU-time/quota semantics. Blind `ResourceRequest` -> Eggwork `ResourceRequirements` translation is invalid.
- Existing scheduler keeps `ResourcePermitGuard` over whole remote attempt, potentially counting remote CPU/process slots against the controller's physical capacity. This is a global admission policy problem, not an Eggwork bug.
- Eggwork node `NodeStatus` reports running/max slots and rejects busy/draining; it has no global queue. Terminal resource outcome is categorical and per-attempt numerical metrics remain absent.

## 3. Work packages

A. Audit exact Eggwork `cpu_millis` semantics (CPU quota / period vs execution CPU budget), maximum valid ranges, Linux enforcement capabilities, non-Linux differences and current CodeGG resource token meanings. Document unit-safe typed conversion. Introduce an explicit node/profile remote limit policy with required vs best-effort choices (and conservative defaults preserving prior no-limits behavior unless explicitly configured), not arbitrary model-supplied cgroup numbers. Server config/operator ceilings are authoritative; user job hint cannot widen or disable a hard configured ceiling. Validate against fresh authenticated capabilities in preflight; fail closed before upload when `Required` cannot be enforced. Never downgrade to `NotRequested` silently.

B. Thread validated policy into `build_spec` and record exact requested/applied/not-applied/limit-exceeded outcomes in bounded audit/RunStore metadata. Keep live handles, lease renewal, event streaming, and restart reconciliation identical. Do not populate `ExecutorMetrics.cpu_time_ms` or `peak_memory_mb` with Gregg host numbers. If separate Eggwork exact per-execution counters land later, use an independent tested mapping.

C. Inspect scheduler permit semantics and split **local** machine CPU/process/IO budgets from bounded **remote** node-specific reservations/connection+transfer budgets while preserving one scheduler/fair queue. A remote reservation consumes a named-node capacity key and a small controller transfer/network budget; it must not unconditionally reserve local CPU/memory as if running there. Preserve workspace exclusivity and cancellation/terminal release. If admission semantics cannot be changed without affecting other job classes, stop for an ADR rather than bypassing the existing controller.

D. Handle node busy, draining, lost/offline and capability mismatches with typed outcomes. Differentiate retryable node contention from nonretryable policy mismatch. For explicit target, retry/wait remains on that same node under bounded backoff/deadline and durable job idempotency; never silently move to another node or locally respawn an accepted attempt. Avoid head-of-line blockage of other workspaces, retry storms and stale-permit leakage.

E. Optional observation phase (after CodeGG M002) may use authenticated **fresh advisory** telemetry to decide if a queued explicit-target job should be deferred under operator-specified admission preferences. Must treat stale/unavailable as unknown with configurable safe behavior; authoritative execution admission continues to occur at Eggwork. **Do not gate M003 closure on optional telemetry**; resource caps and reservation correctness stand alone.

## 4. Required tests

- Legacy no-remote-limit profile unchanged; explicit Linux cgroup Required memory/CPU/PID limits are applied or refuse before submit; best-effort reports NotApplied truthfully when backend unsupported.
- Invalid units, overflow, resource token confusion, malicious job override and policy downgrades fail closed.
- Local work and several remote jobs across two nodes use separate budgets; remote concurrency no longer consumes fictitious local CPU capacity, while network-transfer and workspace locks remain bounded.
- Concurrent remote submissions to one saturated node, fair queue contention, backlog drain, cancellation at all phases, restart before/after remote acceptance, lease reconciliation without duplication, node outage and retry exhaustion.
- Preserve positive tests for live required Landlock and no remote/local fallback. Resource observation cannot grant additional execution permissions.

Focused `cargo test -p codegg-core --locked`, `cargo test --test scheduler_permit_lifecycle --locked`, `cargo test --test scheduler_contention --locked`, `cargo test --test eggwork_remote_execution --locked`, `cargo test --test eggwork_remote_execution_live --locked`, routing and bypass guards plus `scripts/verify.sh quick`; update `architecture/scheduler.md`, `architecture/jobs.md`, node operator docs, and config merge tests.

## 5. Acceptance and closure

Document the explicit limits/units and required/best-effort matrix by platform, typed retry policy, node-scoped permit conservation and acceptance receipts on a real remote Linux node. Create `plans/closure/eggwork-worker-telemetry-and-targeting/003-status.md` with benchmarked contention evidence, prior behavior/compatibility, implementation commits, test outputs and residual risks.

## 6. Stop conditions

Stop before implementation if one cannot preserve one scheduler authority and remote lease identity, if local and remote resource tokens cannot be distinguished safely, if required limits would silently degrade, or if adding remote admission causes a second queue or an unbounded job retry loop. Escalate genuine architecture changes into an ADR and separate milestone.
