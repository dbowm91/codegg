# Agent Tool-Use Operability Corrective C001 — Closure Status

Status: closed
Source plan: `plans/implementation/agent-tool-use-operability-post-audit-corrective/001-broker-accepted-authority-projection.md`
Source subsystem roadmap: `plans/subsystems/agent-tool-use-operability-post-audit-corrective-addendum.md#7-milestones`
Repository baseline reviewed: `4c86bac2a608dd717be085247838fd2f37d81ab6`
Implementation commit: `2913da8a` (`plans/agent-tool-use-operability-20261019`)
Implementation summary: accepted permission and host-derived workspace-policy authority are projected through broker attempts; Tool Program contract is frozen per call; native foreground/background and negative pre-job paths are qualified.
Recommendation: closed; C007 remains blocked on C002.

## 1. Finding and disposition

The broker now preserves accepted permission authority and a host-derived immutable Tool Program contract snapshot across attempts. Workspace path-policy identity is resolved from the daemon workspace registry/service lease and checked before source persistence or job submission. The new production-path harness proves foreground and background execution and verifies that invalid authority creates no scheduler job. C001's acceptance criteria are met.

## 2. Changes landed

- Projected accepted permission outcomes into Tool Program context (`Allow` → `allowed`, `Ask` → `ask`, `Deny` → `denied`).
- Captured the live catalog input-tool contract once before retry and reused that immutable snapshot for every broker attempt.
- Resolved the workspace path-policy fingerprint from the registered workspace instead of trusting model arguments; Tool Program rejects stale/mismatched identity and revision before submission.
- Added a real AgentLoop → broker → scheduler → native Tool Program trajectory for foreground and background `emit(str(1+1))`, plus deny and missing/mismatched/revoked/expired authority cases asserting no job was created.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Real authority values, no synthesized IDs | Workspace registry resolution in `JobSubmissionService::workspace_path_policy_fingerprint_for_root`; broker projection tests | Pass |
| Broker receipt/path-policy projection | `tool::broker`, `tool::tool_program`, and `agent::tool_batch` suites | Pass |
| Real foreground/background native Tool Program call | `tool_program_foreground_and_background_run_through_agent_broker_and_scheduler` in `tests/agent_loop_harness.rs`; terminal jobs and results asserted | Pass |
| Missing/mismatched/revoked/expired authority rejected before job creation | Same harness trajectory asserts each negative outcome and unchanged job count | Pass |
| Retry/cancellation/restart authority behavior | `tool_program_m012_process_recovery`, `tool_program_m012_child_ownership`, and `tool_program_m013_authority` integration suites cover fingerprint mismatch, cancellation/reattach, revocation/expiry and policy/session mismatch | Pass |

## 4. Before/after trace

Before, the normal broker attempt had neither the accepted permission projection nor the trusted workspace policy identity required by Tool Program, so a valid arithmetic-only program failed context validation. After, the broker derives the decision projection from the accepted result, snapshots the host's live input-tool contract, and validates the registered workspace policy before the scheduler accepts work. The AgentLoop integration reaches the native executor for foreground and background work and rejects negative authority before any job exists.

## 5. Verification

Passed:

- `cargo test --locked -p codegg --lib tool::broker` — 17 tests.
- `cargo test --locked -p codegg --lib tool::tool_program` — 18 tests.
- `cargo test --locked -p codegg --lib agent::tool_batch` — 28 tests.
- `cargo test --locked -p codegg --test tool_program_m012_process_recovery` — 14 tests.
- `cargo test --locked -p codegg --test tool_program_m012_child_ownership` — 12 tests.
- `cargo test --locked -p codegg --test tool_program_m013_authority` — 13 tests.
- `cargo test --locked -p codegg --test agent_loop_harness` — 66 tests passed on implementation commit `2913da8a`.
- `scripts/verify.sh quick` — passed on the implementation tree before the planning-only closure updates.

No hosted CI, live provider, or SBC benchmark is claimed or required by this plan.

## 6. Compatibility and migration

No storage migration, tool-name change, or public protocol migration. The workspace policy fingerprint is derived from the effective registered policy and remains ephemeral. Existing tool contracts remain bounded by the host's live catalog snapshot.

## 7. Security, failure, cancellation, and restart

Missing or stale authority fails closed before executable job creation. Model input cannot select a workspace policy identity. Retry reuses one immutable authority/contract snapshot. Existing restart, cancellation and child ownership suites pass; no scheduler bypass was introduced.

## 8. Limitations and severity

No remaining C001 acceptance gap was found. External live-provider performance and SBC measurements are outside this plan's scope. The separate active-workspace Eggsearch contract remains C002 and is not discharged by these tests.

## 9. Unblock audit

C001 is strictly closed and discharges one hard C007 dependency. C007 remains blocked solely on C002 after the C004 and C006 closures recorded alongside this update. No other registered plan was newly eligible.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| None | No C001 acceptance finding remains | None identified within this plan's scope | None |

## 11. Roadmap disposition

C001 is closed. C007 remains blocked solely by C002's unresolved workspace-to-Eggsearch scope contract; C002 research is recorded in its closure status. No other plan became eligible.

## 12. Registry updates

`plans/registry.md` and the source addendum mark C001 closed and C007 blocked on C002. This closure is based on implementation `2913da8a`; C002's separate architecture work is not part of C001.
