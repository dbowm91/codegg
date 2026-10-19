# Managed Agent Scratch Native Qualification — Post-Closure Corrective Addendum

Status: closed — C001 closed with native Linux evidence; C002 closed as blocked on the independent Windows Required backend.

Planning baseline: `83fc052f` on `plan/managed-scratch-spaces` (2026-10-19); PR #128 open and still unmerged. The C001 native Linux evidence was produced by the repository's existing `CI` workflow, run [39294747903](https://github.com/dbowm91/codegg/actions/runs/39294747903) on `ubuntu-latest` at that exact SHA. C002 stopped at work package A and was recorded as a blocked closure. Do not reintroduce already-corrected PR merge or private-scratch reclamation work.

Source subsystem roadmap: `plans/subsystems/managed-agent-scratch-roadmap.md`.
Historical implementation and closure (do not rewrite their facts):
- `plans/implementation/managed-agent-scratch/003-permission-guidance-and-lifecycle-qualification.md`; `plans/closure/managed-agent-scratch/003-status.md` (conditional).
- `plans/implementation/managed-agent-scratch/004-private-scratch-reclamation.md`; `plans/closure/managed-agent-scratch/004-status.md` (closed).
- `plans/closure/managed-agent-scratch/001-status.md` and `002-status.md` (closed).

Long-term references:
- `plans/000-long-term-specification.md` §§4, 16–18, 26–29 (owner authority, reliable execution, isolation).
- `plans/001-terminology-and-domain-model.md` §§5–8 (session, agent run, jobs and execution context).
- `plans/002-long-term-roadmap.md` Phases 9–11, 19; `plans/003-planning-process.md` §§4, 7–10.

Relevant accepted architecture: `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`, `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`, accepted ADR-0028 command authorization. The proposed Linux mechanism ADR-0027 and independent Windows sandbox roadmap are **not** silently accepted or implemented here.

## 1. Purpose and ownership boundary

Close the **remaining native qualification evidence** for managed scratch without reopening the correct and already-delivered private/shared scratch feature or broadening Linux/Windows sandbox architecture. Scratch owners remain daemon Session/AgentRun/Job stores; `ManagedProcessService` is the only managed process launcher; `ToolBroker` and established permission system remain authoritative; platform sandbox backends enforce only what their obtained status supports.

This is a qualification corrective, not a new filesystem service, new model tool, generic Linux sandbox project, or Windows Required backend implementation.

## 2. Work classification

**Invariant:** claimed sandbox guarantees must match obtained restrictions, explicit denies and delegated grant snapshots; private scratch, granted shared roots and workspace are the only expected writable scratch roots under Required containment.

**Capability:** on supported native Linux Required launches, agents can use scoped scratch without sibling/host-temp writes. Windows Required behavior remains blocked until the independently qualified backend exists.

**Infrastructure:** focused native launch fixtures and authenticated attempt tracing, reusing `tests/sandbox_containment.rs`, `tests/managed_scratch_job_admission.rs`, existing Windows launcher workflow and status-reporting seams.

**Polish:** up-to-date platform evidence matrix and closure/registry reconciliation, never retroactive claims.

## 3. Non-goals

No new `/tmp` redirection, regex shell rewrite, dedicated scratch tool, broad shared /tmp or Cargo-cache grant, OS virtualization, new daemon scheduler or watcher, changes to Windows token/AppContainer research scope, remote Eggwork scratch transport, Linux namespace/IPC/network enforcement, or new sandbox guarantees absent native proof. No branch merge performed by planning or mandated by these milestones.

M004 already closes **terminal delegated-run and deleted-session** cleanup. It deliberately does not garbage-collect scratch for extant idle sessions, because those sessions remain resumable. A new dormant-session TTL/usage quota would change resource preservation policy; no correctness failure or explicit retention contract currently justifies opening such a workstream. Reconsider only with real accumulation evidence and an approved retention policy.

The M004 optional macOS `server,plugins,lsp-test-support` failures are documented in its closure, with two unrelated sandbox policy assertions and one SIGKILL. They do not prove a scratch defect. Investigate under general sandbox/test qualification if reproducible; do not widen this successor on that basis.

## 4. Current repository evidence and unclosed requirements

- `src/scratch.rs` provides deterministic session/run-private roots and typed shared `ScratchId` paths, owner records, tombstone reclamation and symlink/junction/hardlink checks; `crates/codegg-core/src/agent_run.rs` persists authorized shared grants and attempt leases.
- `src/managed_process.rs` wires scratch environment and authorized read/write paths into `SandboxRequest::Required`; Python and shell paths have demonstrated macOS behavior.
- `tests/sandbox_containment.rs` has Linux Landlock negative fixtures for outside writes, truncation and link/rename, plus cross-platform `scratch_space_is_writable_so_tooling_still_works` and `delegated_shared_scratch_is_explicitly_granted_to_the_contained_child` using helper launch specs. Those latter tests do not alone prove actual authenticated **session → ToolBroker → scheduler → ManagedProcessService** grant propagation on Linux.
- `tests/linux_native_scratch_containment.rs` is the C001 native Linux qualification fixture. It is a whole-file `cfg(target_os = "linux")` target, admits a delegated run through the real `TaskTool` → `JobSubmissionService` → `JobScheduler` path, rebuilds the effective scope from the durable `AgentRunRecord`, and launches it through the real `ManagedProcessService` and the real `codegg-sandbox-helper` with a spec that names only the workspace. A Linux host that cannot obtain Landlock **fails** the fixture instead of skipping it, so an unavailable qualification can never be counted as a satisfied one.
- `src/security/sandbox/landlock.rs` requires ABI >=3, `no_new_privs`, and reports obtained backend status; Linux enhanced containment roadmap remains separately active. C001 may test existing Landlock, not implement that roadmap.
- Windows shared cleanup **is implemented** after original M003 closure; M003's appended factual correction and M004 closure already cite successful native Windows lifecycle tests (11/11) and Windows launcher (8/8). Do not fabricate a missing Windows cleanup feature.
- The existing Windows Required-sandbox enforcement path remains **unqualified/unsupported** under the independent `plans/subsystems/windows-native-sandbox-roadmap.md`. A passing scratch cleanup unit test is not native Required containment evidence.
- Current `plan/managed-scratch-spaces` head CI and Desktop E2E passed; PR #128 is mergeable. No new PR reconciliation or generic regression-plan milestone is justified by that former blocker.
- The C001 native Linux qualification ran in the repository's ordinary CI lane on the Linux runner: implementation `2597ec9e` with the two fixture corrections `ea400ff7` and `83fc052f`, run `39294747903`. Two earlier dispatched runs did not qualify: `39289232898` was cancelled, and `39291133332` failed on a defect in the fixture itself (the observation script lacked the execute bit the tool-boundary invocation needs). Both defects were in the test, both were fixed, and neither superseded run is cited as evidence. The `pull_request` trigger did not start for that commit during the closure session, so the same existing workflow was dispatched explicitly on the branch ref; a dispatch checks out the reviewed tree rather than a synthetic merge commit, which is the stricter placement for this evidence. The `main` branch's own push run `39282749528` is red for an unrelated reason (`clippy::unnecessary_sort_by` in `crates/codegg-core/src/workspace_services.rs`, code that is not on this branch). PR #128 remains unmerged; no merge was performed by this milestone.

Historical M003 requirement gap: **Linux native end-to-end scratch confinement and Windows Required-sandbox scratch confinement remain unqualified**. The previous M003 tests ran on an x86_64 macOS host; a Linux hosted unit suite and Windows ordinary launcher don't establish corresponding effective Requested→Obtained per-attempt native boundaries.

## 5. Target qualification contract

The traceable test subject is an authenticated, admitted model-owned launch with immutable effective `{session_id, optional AgentRunId, private_scratch, granted_shared_scratch[]}`. Native Required setup must return actual backend status and fail before target exec if enforcement cannot be obtained. Verify private scratch and intentionally granted shared scratch are writable; sibling scratch, scratch parent, unrelated host temp, credential roots and explicitly denied paths remain inaccessible for writes. Preserve OS-specific truthful **read** limitations, particularly on macOS. A job/attempt lease must outlive all child I/O and prohibit cleanup during execution.

Qualification must identify OS/kernel/runner, Landlock ABI or Windows backend, helper/probe status, permission/rule snapshot, what was tested, which tests were skipped and why, and a test that would fail if a sibling write incorrectly succeeded. Denial/error strings alone cannot establish obtained confinement.

## 6. Dependency graph

```text
M001/M002 closed, M003 conditional, M004 closed, existing Landlock ABI>=3
  |
  +-- C001 Linux native scratch admission/containment qualification [closed]
  |      (hard: existing managed scratch; operational: capable Linux host)
  |
  +-- C002 Windows Required sandbox scratch qualification [blocked]
         (hard: independent Windows native Required filesystem backend qualification)
```

C002 does not depend on C001; C001 closure cannot make an unavailable Windows backend available. Independent Windows scratch reclamation is already qualified and must not be reimplemented. C002 reached its own stop condition at work package A and was recorded as a blocked closure rather than started speculatively.

## 7. Milestones

### C001 — Native Linux Required scratch and delegated grant qualification

Class: invariant. **Closed** with native evidence.

Plan: `plans/implementation/managed-agent-scratch-native-qualification-corrective/001-linux-native-scratch-containment-qualification.md`.
Closure: `plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`.
Exit: actual model/process launch with authenticated session/child grant scope on Linux proves positive private/shared scratch and negative sibling/outside writes, with obtained Landlock status, concurrency/lease semantics and truthful documentation. A missing capable host means *blocked for evidence*, not passed.
Result: met. Implementation `2597ec9e` with the fixture corrections `ea400ff7` and `83fc052f`; native Linux evidence from the repository's existing `CI` workflow, run `39294747903` on `ubuntu-latest`. The delegated grant path is qualified on a real Landlock enforcement, the obtained status (Landlock, ABI >= 3, `no_new_privs`) is asserted, and a defect-injection control proves the denials are discriminating. A Linux host that cannot obtain Landlock **fails** the fixture rather than skipping it. Filesystem dimension only.

### C002 — Windows Required sandbox scratch compatibility and native proof

Class: invariant. **Blocked** until `plans/subsystems/windows-native-sandbox-roadmap.md` delivers an accepted, implemented and natively qualified Required filesystem-writing backend contract. Work package A was executed and found that contract absent, so the milestone stopped at its own stop condition and was recorded as a blocked closure: `plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md`.

Plan: `plans/implementation/managed-agent-scratch-native-qualification-corrective/002-windows-required-scratch-native-proof.md`.
Exit: genuine Windows Required process launch enforces only the authorized scratch roots and excludes siblings/outside paths, with claim-grade obtaining evidence; otherwise retain the conditional status. Existing Windows scratch cleanup tests remain accepted separately.

## 8. Cross-cutting requirements

**Storage/migration:** no new schema, registry or wire fields expected. Use existing `ScratchId`, SessionId, AgentRunId, JobId and lease provenance; optional evidence-only test fixtures. Any change requires reasoned compatibility and `STORAGE_LAYOUT_VERSION` checks, not casual bump.

**Authorization:** effective grants come from validated daemon identity/parent authority. Explicit tool/path Ask and Deny prevail over the implicit default Ask exemption for authenticated workspace/scratch. Paths are locators, never proof of grants.

**Restart/cancel/contention:** use existing job/attempt lease, safe cleanup, deleted-session restoration and crash recovery. Launch admission/reaping must not produce orphaned temporary artifacts or delete a live child's scratch. Test at least one runtime negative case, not merely serialized DTOs.

**Observability:** redacted obtained status with supported security dimensions only; do not expose another session's paths in logs or projections.

**Performance:** use existing native smoke/CI lanes; no new costly continuous research workflow, SBC startup benchmark or full network-dependent benchmark.

## 9. Verification strategy

Focused Linux native `sandbox_containment`, `managed_scratch_job_admission`, managed process scratch tests and permission negatives; Linux ABI and helper status checks; existing Windows workflow once upstream backend is ready; `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, `python3 scripts/check_sandbox_policy_wiring.py`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_scheduler_bypass.py` and `bash scripts/check-core-boundary.sh`. Reject vacuous skip-based closure. End-to-end positive and negative cases are required even when broad CI is green.

## 10. Risks and decision points

Linux CI kernel LSM availability, CI container/runner policy or inability to acquire effective ABI 3 are **operational blockers**, not automatic product defects. Windows Required may remain unavailable for a long time. Test-only Windows sandbox fixtures must not be reinterpreted as a product guarantee. If investigation discovers a real grant/containment bypass, stop new qualification claims, record a severity-assigned corrective finding, and fix inside the owning existing security module rather than building another authorization engine. Existing M004 optional macOS feature-tier failures stay recorded as independent unqualified evidence.

## 11. Completion definition

C001 and C002 each have accepted `plans/closure/managed-agent-scratch-native-qualification-corrective/00N-status.md` with native target evidence and stop-condition disposition; original M003 remains historically conditional, with later supplemental evidence appended/linked rather than its history overwritten. Only then may the corrected roadmap/registry advertise full Linux+Windows scoped native qualification. Any unavailable Required backend remains explicitly blocked.

Met. C001 closed with native Linux evidence and C002 closed as blocked at its work-package-A stop condition; the original M003 received a factual correction recording the Linux qualification while its Windows condition, its historical findings and its conditional disposition were left intact. The corrected roadmap and registry therefore advertise the qualification **as it actually stands**: the Linux filesystem dimension is qualified, and the Windows Required backend remains explicitly blocked in its owning roadmap.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure | Blocker |
|---|---|---|---|---|
| C001 | closed | `plans/implementation/managed-agent-scratch-native-qualification-corrective/001-linux-native-scratch-containment-qualification.md` | `plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md` | — (discharged: native Linux Landlock ABI>=3 evidence, CI run `39294747903` at `83fc052f`) |
| C002 | blocked (stop condition reached; closure recorded) | `plans/implementation/managed-agent-scratch-native-qualification-corrective/002-windows-required-scratch-native-proof.md` | `plans/closure/managed-agent-scratch-native-qualification-corrective/002-status.md` | hard: independently qualified Windows Required filesystem sandbox backend |
