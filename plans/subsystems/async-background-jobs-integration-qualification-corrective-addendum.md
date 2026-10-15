# Async Background Jobs — Post-C005 Integration and Qualification Corrective Addendum

Status: **active** — C006 tool-to-idle-resume lifecycle **closed**; C007 PR #119 hosted integration **blocked** on tool-advisor receipt corrective C001 formal closure/integration and C007-owned qualification.
Opened: 2026-10-15; planning baseline: `247edec5f37851ca3f67db5943675c896d19e9d9`, `work/async-background-jobs-production-wiring-c004-plans`, PR #119. Latest reviewed `main`: `90ab44775a36ebe30466a0d6e340f584324de114`. Revalidate on execution.
Historical roadmaps: `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md` (M001–M004 historically closed) and `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md` (C001–C005 history; C004 delivery partially closed, C005 accepted ADR-0031 implemented and closed). **Do not rewrite closed milestones or reopen original scope without evidence.**

## 1. Reason for a distinct post-closure track

C004 provides durable session-scoped `BackgroundJobCompleted` and live/replayed hints; C005 added authenticated server-resolved `ContinuationGrantCreate/Revoke/Status` and actual daemon-internal delegated `TurnSubmit` under accepted ADR-0031, with 16/16 local tests and prior suites. The current positive C005 fixture artificially records a terminal notice and proves the back half. It does **not** prove that a model-facing `ci_watch` or scheduler-backed background `bash`/`test` tool actually feeds C005 after its origin turn ends; C006 owns this test gap. This is a **qualification omission**, not a speculative fault in C005.

The integration branch is a **draft, unmerged PR #119**, 44 ahead/21 behind `main` at review (GitHub currently reports mergeable true). Its PR description is stale ("ADR proposed", "C005 not implemented"); the branch has moved. Hosted checks at `247edec5`: Desktop E2E `38650792103` success, Windows process launcher `38650792129` success, CI `38650792114` **failure**, 4,267 workspace tests passed, 1 tool-advisor M003 receipt mismatch failed, and 9,522 tests not run. The failing check belongs to the **independent** tool-advisor CI corrective; an unrelated failure still prevents a green root-CI merge gate. Current-main integration must be rerun with new exact-head evidence.

## 2. Dependency map and ready/blocked authority

```text
Original async M001–M004 -> C001–C005 [historical code and closure]
                                    |
                 +------------------+-----------------+
                 |                                    |
           C006 tool-to-idle E2E               Tool-advisor receipt
            READY (async owner)                 C001 READY (other owner)
                 |                                    |
                 +------------------+-----------------+
                                    |
                   C007 latest-main integration,
                   PR #119, full hosted qualification
                             BLOCKED
```

| Work | Status | Handoff document | Strict acceptance |
|---|---|---|---|
| C006 real tool -> idle notice / authorized turn | **closed** | `plans/implementation/async-background-jobs-corrective/006-model-facing-tool-to-idle-resume-lifecycle-qualification.md`; closure `plans/closure/async-background-jobs-corrective/006-status.md` | Model-facing `ci_watch` reaches one host-authorized daemon continuation and background Bash completes through the scheduler with default zero-inference. 7/7 dedicated tests; C004/C005/C001/M004 suites 55/55; quick + Clippy green. |
| Tool-advisor M003 receipt CI C001 | **ready, independent** | `plans/implementation/tool-advisor-m003-receipt-ci-corrective/001-receipt-identity-reproduction-and-hosted-gate-restoration.md` | Diagnose stored `94ef4d...` vs fresh `a9c5af...` effect fingerprint; preserve historical negative disposition D and M002 selected; substantive repair and exact-head root CI green. |
| C007 mainline integration and qualification | **blocked** | `plans/implementation/async-background-jobs-corrective/007-mainline-pr119-hosted-qualification-and-merge-readiness.md` | C006 strict closure, tool-advisor C001 verified CI fix, review latest-main merge, exact-SHA entire `CI / verify` + Desktop E2E green, security/migration/ownership diff, updated PR body, explicit maintainer merge decision. |

The C006 proof is complete, and tool-advisor C001's substantive repair now has green exact-head hosted CI. C007 remains blocked until C001 receives its formal closure/integration record; after that it is ready for latest-main reconciliation and its own exact-head gates.

## 3. Preserved architectural constraints

One `CoreDaemon`, one `JobScheduler` machine execution authority, C001 daemon CI-watch loop, C004 durable completion event, C005 authenticated grant and delegated canonical turn submission; no second scheduler or token-using callback. Default `notify_only` zero inference. A model cannot mint trusted host consent; older unattributed grant rows stay non-autonomous; principal capability/project/session/workspace/model/connection budgets must be rechecked; only one accepted turn per notice/grant by CAS. After uncertain provider-side admission, retained actionable `needs_attention` and no blind replay.

Actual frontend projection/replay and one live authorized subscriber are required for C006; no fake connected client IDs or raw GitHub content promoted into trusted prompt. Sandbox `Required` must fail closed on unsupported platforms; no claims of enforced Windows or native macOS sandbox supported by this workstream.

Historical C005/C004 status lines, frozen tool-advisor negative M003 receipt, existing `main` docs, original ADRs and closure histories remain unchanged by these plans. Any new functional defect discovered in C006 is to be fixed with a bounded regression and documented in a new C006 closure, not silently recharacterized as a historical successful test.

## 4. Verification and closure policy

- Use deterministic local loopback HTTP and stub provider **at the provider boundary**; actual tool registration/ToolBroker, daemon runtime, scheduler, live event and canonical delegated turn must be production implementations.
- C006: dedicated focused suite plus existing M001–M004/C001–C005/broker/auth/projection/TUI/command-sandbox suites, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, and call-site/scheduler/execution ownership guards. Record if complete scripted-provider model tool call could not run.
- C007: run full hosted `CI / verify` on latest-main merge candidate **to completion**, not partial nextest; Desktop E2E, applicable OS checks, full migration path v72→v78, review PR #119 and exact SHA. Distinguish tested native lanes from merely green cross-compile/launcher. Never call the feature merge-ready while root CI red.
- C006 is closed at implementation `4712020` in `plans/closure/async-background-jobs-corrective/006-status.md`; C007 remains the integration and hosted gate owner. Tool-advisor owner closes `plans/closure/tool-advisor-m003-receipt-ci-corrective/001-status.md` before C007 proceeds. Record exact commit and workflow IDs, pass/fail counts, security/compatibility, unresolved findings, and explicit unblock audit. Update `plans/registry.md` and this addendum as milestones close. PR merge is a separate maintainer action, not part of plan-writing.
