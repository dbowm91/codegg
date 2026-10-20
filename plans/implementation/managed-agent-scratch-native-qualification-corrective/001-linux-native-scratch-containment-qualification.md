# Managed Scratch Native Qualification C001 — Linux Required Scratch Containment

Status: ready for handoff (operational: native Linux host with Landlock ABI >=3 required)
Repository baseline: `fb84bee63dda0b1c3407f377c695db99ec78324b` (`plan/managed-scratch-spaces`; inspect exact head before edits)
Source roadmap: `plans/subsystems/managed-agent-scratch-native-qualification-corrective-addendum.md#7-milestones`
Original gap: `plans/closure/managed-agent-scratch/003-status.md` M003 conditional Linux native result, after M001/M002 closed and M004 private reclamation closed.
Long-term requirements: `plans/000-long-term-specification.md` §§4, 16–18, 26–29; `plans/001-terminology-and-domain-model.md` §§5–8; `plans/002-long-term-roadmap.md` Phases 9–11 and 19.
Applicable ADRs: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`, `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`, accepted ADR-0028; Linux enhancement ADR-0027 is **proposed**, not a prerequisite for testing the existing Landlock backend.
Primary class: invariant.

## 1. Objective

Obtain **native, end-to-end Linux Landlock evidence** that a real authenticated CodeGG model-owned process can write its session/run-private and explicitly granted shared scratch, but cannot write another session/child's scratch or unrelated host temp, with the correct obtained-enforcement status. Correct only defects discovered inside the existing scratch grant/ManagedProcessService/landlock seam; do not implement enhanced Linux sandbox architecture.

## 2. Why ready

M001/M002/M004 are closed; existing Linux `landlock.rs` enforces ABI>=3 plus `no_new_privs` and `tests/sandbox_containment.rs` has related native fixtures. `tests/managed_scratch_job_admission.rs` exercises accepted task grant persistence; new qualification can compose these existing interfaces. **Operational** host/kernel/Landlock availability gates evidence, not plan creation.

## 3. Current implementation evidence and why M003 missed this

`src/scratch.rs` derives owner-scoped paths and `AgentScratchScope`; `src/tool/task.rs` and `crates/codegg-core/src/agent_run.rs` grant explicit shared IDs; `src/scheduler/executors.rs`, `src/tool/bash/process.rs`, `src/managed_process.rs` forward scope into launch policy; `src/security/sandbox/landlock.rs` supplies native enforcement and obtained ABI status. Existing `sandbox_containment` cases `scratch_space_is_writable_so_tooling_still_works` and `delegated_shared_scratch_is_explicitly_granted_to_the_contained_child` use manually constructed helper launch specs. M003's native tests ran on macOS; hosted Linux compilation/unit and the macOS Required helper do not prove actual Linux **admitted owner → subprocess** grants. Record exact use of real scheduler and tool boundary in new fixture(s).

## 4. Invariants

- ToolBroker is the model tool boundary; JobSubmissionService/JobScheduler own machine admission; ManagedProcessService owns process supervision. Do not add a raw spawn path.
- Immutable accepted `ScratchId` grants, private temp defaults, direct parent/run authority, and lease ownership do not widen for a sibling or unrelated session.
- Required launch **fails before target exec** when Landlock ABI or helper is unavailable; don't silently select FullHost.
- Existing deny precedence, credential protection and `workspace_root` vs `child_workspace_ceiling` semantics remain unchanged.
- No claims of process, IPC, network or same-UID read isolation beyond obtained Landlock filesystem dimensions.

## 5. Scope

In: native Linux launch evidence under actual CodeGG managed runner and authenticated session/run/grant context; positive/negative filesystem writes/truncate/rename/link; env and sandbox obtained status; regression fixture for an ungranted child, active job lease and cleanup-after-terminal; documentation/closure supplement.

Out: new Linux namespace/proxy/seccomp/cgroup backend, trusted Git hardening, cross-platform Windows backend, daemon protocol changes, unrelated optional macOS feature-tier flake, broad default temp or cache write allowlists.

## 6. Required production changes

**Core/domain:** expect no new production subsystem. If tracing exposes an actual lost grant or skipped Required backend, repair the smallest existing scope construction route (`AgentScratchScope` → accepted invocation → existing scheduler/managed process). Add a negative test that fails on that defect.

**Storage:** reuse v83 typed IDs/grant/attempt leases and existing session/agent-run/job stores; no migration expected. Persisted job attempt uses an immutable effective grant snapshot, even after parent/grantee changes.

**Protocol/DTOs:** no user-supplied scratch paths or untrusted owner IDs. Avoid public DTO edits; if essential, use defaulted, validated, versioned fields and explain migration.

**Runtime/concurrency:** use actual `SandboxRequest::Required` and Linux helper, `no_new_privs` and effective ABI proof. Make test arguments identity-bound to authorized scope; test process tree cancellation and lease-safe cleanup, not just `SandboxLaunchSpec` in isolation.

**Frontend/operator:** do not add UI; report supported/unavailable with backend/status + bounded redacted diagnostics.

**Security:** refuse scratch parent, sibling, ambient `/tmp`, explicit deny/credential locations, malformed/foreign shared ID, and symlink traversal. Structured tool permission bypasses *only implicit* Ask on authorized roots.

**Docs/static guards:** update `architecture/security.md`, `architecture/process-tool-execution-ownership.md`, `docs/tools.md` and the corrective roadmap with native evidence, not generalized isolation promises.

## 7. Ordered work packages

A. **Trace baseline:** identify actual session/child env and sandbox policy derived at accepted invocation/attempt. Record where authenticated principal/run identity and shared grant snapshot are validated. Evidence: source-level ownership matrix.

B. **Host and negative fixture:** run on a native Linux kernel with available Landlock ABI3+. Prove helper status `Enforced` and `no_new_privs`; direct child opens a file under its private root and under a granted shared root, cannot modify sibling or host temp, and cannot truncate/link/rename outside authorized write roots. Evidence: native process exit/contents and obtained status; neither a skipped test nor `FullHost` acceptable.

C. **Real admitted integration:** use a CodeGG-admitted model-owned shell/TaskTool job, with actual `JobSubmissionService` and `ManagedProcessService`, rather than manual environment writes to a direct helper. Prove private temp vars and shared alias scope; running job owns lease across parent return, then releases/reclaims only after reaping. Include a foreign-child replay/unknown grant denial and one sandbox setup failure that leaves target unexecuted.

D. **Regression/closure:** run focused and broad checks, mark source-specific qualification status, append a factual evidence section to historical M003 if appropriate, write C001 closure and update registry/roadmap. Report any unsupported kernel/environment exactly; do not downgrade failure to success.

## 8. Failure, cancellation, restart and contention

Fork/exec and helper setup failure must not execute target or lease ungranted scratch. Background job cannot lose scratch while running; after cancel/timeout, process tree is reaped before cleanup and durable status transitions are reconciled. Retrying one accepted invocation retains the frozen grant identity and does not acquire new shared roots. Crash/restart conservatively retains uncertain active scratch until reconciliation; test with existing recovery fixture where possible.

## 9. Compatibility/migration

Existing macOS Seatbelt and Windows FullHost/unsupported Required behavior unchanged. No new config defaults, schema or network policy. Do not convert a formerly unsupported Linux host into silent unsandboxed success.

## 10. Required tests

**Focused:** effective private/shared allowed roots and exactly negative sibling/temp/credential roots; sanitized `CODEGG_SCRATCH_DIR`, `TMPDIR`, `TMP`, `TEMP`; immutable `ScratchId` and parent/foreign session authorization.

**Integration:** `sandbox_containment` Linux native plus real admitted Bash/TaskTool attempt; assert recorded backend is Landlock ABI>=3 and `no_new_privs`. A test returning early for missing Landlock must be counted unavailable, not passed.

**Recovery:** active background attempt across parent return, cancel/timeout and cleanup-after-reap. No orphaned attempt leases on error.

**Contention/security:** sibling parallel writes, symlink escape, cross-boundary rename/link/truncate, setup failure with a target side-effect sentinel.

**Compatibility:** macOS and Windows compilation/static checks stay green; no accidental general sandbox semantics change.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --test sandbox_containment --locked -- --nocapture
cargo test -p codegg --test managed_scratch_job_admission --locked -- --nocapture
cargo test -p codegg --lib scratch::tests --locked -- --nocapture
cargo test -p codegg-core --locked
python3 scripts/check_sandbox_policy_wiring.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Run any new focused Linux test by exact test-name and capture native helper obtained status. Use existing CI only; no mandatory SBC startup or live provider inference.

## 12. Documentation updates

Update the qualified platform matrix in `architecture/security.md`, `docs/tools.md`, native sandbox test docs if changed, and registry/corrective roadmap. Do not rewrite original M003 failures as if they never occurred; refer to C001 supplemental closure.

## 13. Acceptance criteria

One **actual** authenticated, admitted, Required Linux managed process writes private and granted shared scratch; an ungranted sibling and unrelated `/tmp`/credential location are rejected by observed native kernel policy, with proper obtained backend status. Process cancel/lease/cleanup and explicit authorization negatives pass. Repeated test is non-vacuous: force a broad scratch grant in a local negative mutation to show fixture fails, or equivalent defect-injection proof.

## 14. Stop conditions

Stop as **operationally blocked** when no Linux host with ABI>=3 and usable helper; don't mark C001 closed on macOS or on skipped tests. Stop as **corrective required** if backend misreports its obtained enforcement, if grant propagation needs a new authority store, if sibling mutation succeeds, if secure cleanup would require unqualified kernel primitives, or if completion requires ADR-0027's unaccepted enhanced sandbox design. Preserve existing result, document blocker, escalate to owning Linux isolation roadmap.

## 15. Closure evidence required

`plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`: exact source and CI SHA, Linux kernel and ABI, helper provenance, request-to-obtained guarantee, authenticated principal/run job chain, positive/negative matrix including path contents, failure/cancel/restart, tests/guards executed and unrun, migration assessment, security limits, residual severity, disposition and whether any downstream work became newly ready. Cite new evidence without rewriting historical M003.

## 16. Handoff notes

PR #128 `fb84bee63dda0b1c3407f377c695db99ec78324b` already green and mergeable at planning baseline. Do not create a rebase conflict/CI corrective without new evidence; never merge as part of native test handoff. Native landlock availability can be absent in containers; use a qualified Linux host and classify that as operational evidence, not an automatic broad Linux release guarantee.
