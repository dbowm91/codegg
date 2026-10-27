# Harness Lab Continuation — Post-C006 Corrective Addendum

Status: D001–D003 closed, including D003's preregistered live C006 rate condition for the sampled host, binary, route and case mix; its bound is conditional on trial independence. D004 is owned by the closed Agent Recovery C002 milestone; exact-head CI E001/E002 is closed in its own corrective roadmap.

Reviewed baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511` (2026-10-26). Re-check latest main and any successor/PR before implementation.

Sources: `plans/subsystems/harness-lab-roadmap.md`; `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`; closure `plans/closure/harness-lab-post-pilot-corrective/{003,004,005,006}-status.md`; `plans/closure/harness-lab-post-pilot-corrective/003-evidence.md`; `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/003-planning-process.md`.

## 1. Purpose and ownership

Close the gap between an **accepted self-improvement candidate** and **verified behavior on current main**; correct ACP tool-outcome fidelity; instrument the actual sustained benchmark observation path; and hand stall diagnostics to the already established Agent Recovery owner. No speculative rewrite of the Python controller or daemon.

CodeGG's daemon/broker remains the tool authority. ACP is a projection/transport adapter; Python is the independent, dev-only assessor; OpenCode is an optional proposer, not the arbiter of acceptance. The trusted development workstation and private per-lane daemon/data/worktree remain first-class. No VM/container provision, unrestricted automated merge/push, default daemon mutation or newly granted provider spending is authorized.

## 2. Factual findings

- **P001 — Accepted code not proven present on main.** C003's authenticated proposal fixed missing headless workspace binding; 13/13 pairs with one correctness improvement and no regression were scored on an external candidate worktree. The C003 closure explicitly says it was *not merged*. Current `src/exec.rs` still calls `ToolRegistry::with_config_and_search_runtime(&config, search_runtime)` without an explicit `ToolRegistryOptions.workspace_root`, while `src/tool/mod.rs` wraps workspace tools as `UnrootedTool` when no root exists. Current `AgentLoop::new` takes a root, so a source-level symptom is **not** by itself proof of failure. `tests/exec_workspace_path_policy.rs`, cited in C003, is absent from current main. A re-run, not a guessed cherry-pick, decides.
- **P002 — Failed tool semantics are unfaithful in ACP.** C005 closure records real tool errors published as `CoreEvent::ToolCompleted { success: true }`, with no `ProjectionEvent::ToolFailed` producer, so a model/tool error may be observed as `status=completed` by ACP. The benchmark currently inspects output to infer failure. Preserve security/disclosure and acknowledge that this is a **native publication defect**, not an evaluator scoring tweak.
- **P003 — C006's live-rate evidence is narrow.** C006 repaired lost-event notification, made unknown measurements unscorable, bounded retry and enforced cumulative qualification deadlines. Only the `autonomy` runner observes ACP gaps; `sustained` does not. Fourteen instrumented live trials observed 0 losses, which does **not** establish a low true loss rate (about 19% one-sided 95% upper bound). The original C006 conditional status must remain until its specifically qualified evidence advances; no zero-rate claim.
- **P004 — Stall-control authority has a separate owner.** Five induced-failure live trials did not repeat identical failed calls; `RecoveryDecision::Stalled` is not constructed, and the test's expected stall terminal is not reachable under the accepted ADR-0040 Option B. The existing `plans/subsystems/agent-recovery-escalation-authority-corrective-addendum.md` C002 is dependency-eligible and owns the progress-observation/diagnostic contract. Do not introduce a second competing recovery state machine in the harness.
- **P005 — Exact-mainline qualification is failing outside this subsystem.** 2026-10-25 `CI / verify` `40224670033` failed the Tool Programs M015 crash/restart corruption test with SQLite `database is locked`; 2026-10-26 `40324996955` failed Workspace Clippy across several product files under Rust 1.99.0 (`-D warnings`). Separate CI integrity corrective plans own those, not the Harness Lab acceptance logic. Neither is proof that the other cause has gone away.

## 3. Classification and dependency diagram

- **Invariant:** actual mainline workspace-tool capability; truthful failed/completed native tool events; missing event measurement never silently passes; no wrong stop reason.
- **Capability:** runnable production headless test, reliable sustained 30–60 minute ACP assessment.
- **Infrastructure:** fixture/telemetry update, cumulative-budget and gap observer parity, exact-HEAD CI.
- **Polish:** diagnostics and status/reporting after proof.

```
D001 mainline headless parity / accepted-candidate disposition  ─ closed
D002 truthful native tool outcomes / ACP contract                ─ closed
  D003 sustained event-gap observer parity                         ─ closed; C006 scoped live rate qualified
Agent Recovery C002 diagnostics + ADR-0040 reconciliation         ─ closed (owned by other roadmap)
CI corrective E001/E002                                          ─ closed independently
```

All implementation dependencies are discharged and exact-head CI is green. The separately approved and reviewed C006 sample met its preregistered upper-bound target for the recorded host, binary, route and case mix; its independence caveat and raw receipts are recorded in the D003 live-sample closure.

## 4. Milestones

**D001 — Requalify/promote accepted headless workspace behavior** (invariant; ready): `plans/implementation/harness-lab-continuation/001-mainline-headless-accepted-fix-disposition.md`. Compare C003's original candidate with current main; reproduce original red regression under current main before any code change. If already fixed, record **no-op positive** source-to-runtime evidence and do not cherry-pick. If broken, implement minimal production fix through usual code review/PR, with explicit root + path-policy/tool-advertisement integration tests on `run`, `exec` and ACP; keep user default daemon unaffected. Do not mistake `AgentLoop::new(workspace_root)` alone for registry rooting.

**D002 — Truthful tool failure publication and projection** (invariant; ready): `plans/implementation/harness-lab-continuation/002-tool-terminal-truth-native-to-acp.md`. Correct the *native event source* or its authoritative projection so failed calls remain failed all the way to ACP and Python. Add failing-first tests for success, structured failure, denied/cancelled, replay, no source payload leaks and no double tool terminal. Do not relabel an agent failure as a tool failure.

**D003 — Sustained-run event-integrity parity and rate qualification** (infrastructure/capability; ready): `plans/implementation/harness-lab-continuation/003-sustained-trial-gap-and-rate-qualification.md`. Reuse C006's actual `codeggEventGap`, client typed lag, cumulative deadline and measurement-retry contracts, instrument `sustained` and score the same missing-observation semantics as `autonomy`. Demonstrate deterministic gap injection and cancellation/cleanup. Live sampling is **optional and separately budget-admitted**; qualify an upper bound only on a preregistered sample size, not 0/14 as a rate. Avoid requiring costly trials just to declare closure of deterministic code.

**D004 — Recovery diagnostics and false-stall control** (polish/invariant; existing-owner plan ready): `plans/implementation/agent-recovery-escalation-authority/002-observation-diagnostics-and-stall-control.md`. The existing Agent Recovery C002 owns this. Preserve ADR-0040 Option B: confirmed stagnation alone, not simple identical reads or mere unknown progress, may terminate; if unreachable `Stalled` is dead API under the decision, remove/relabel with tests instead of fabricating events. The Harness Lab taxonomy should test the **documented** terminal under that decision, not an impossible synthetic expectation. Stop for ADR if actual production authority is being changed.

## 5. Operational controls, no new spending

All provider-backed testing requires fresh operator-approved model/route and enforceable hard budget preflight per closed Harness Lab C001. Historical approval is evidence, not a transferable authorization to run more calls. Normal CI remains credential-free. Every tested lane names binary SHA, source SHA, effective policy, daemon generation, fixture digest and model; unobserved output/cost is null.

## 6. Completion and closure

Each plan requires a new, independently reviewed closure record with implementation SHA, exact-head CI, failing-first proof, negative/security tests, correlation/replay and any deferred provider evidence. Historical M001–M005 and post-pilot C001–C007 closures are not overwritten. D001–D003 may close separately; C006/C004 conditional verdicts only change in a **separate reconciliation** with evidence that meets their original gates. CI exact-head failures must not be claimed green merely because planning changed.

## 7. Milestone status

| Milestone | Status | Plan | Hard blocker |
|---|---|---|---|
| D001 | closed | `implementation/harness-lab-continuation/001-mainline-headless-accepted-fix-disposition.md`; `closure/harness-lab-continuation/001-status.md` | Exact-head CI run 40383901325 passed |
| D002 | closed | `implementation/harness-lab-continuation/002-tool-terminal-truth-native-to-acp.md`; `closure/harness-lab-continuation/002-status.md` | Exact-head CI run 40383901325 passed |
| D003 | closed | `implementation/harness-lab-continuation/003-sustained-trial-gap-and-rate-qualification.md`; `closure/harness-lab-continuation/003-status.md` | 63-trial live sample passed the ≤5% one-sided 95% upper-bound target for the recorded profile, conditional on trial independence; evidence and receipts at `closure/harness-lab-continuation/003-live-event-gap-sample-20261027.md` |
| Recovery C002 (D004 cross-reference) | closed in owning subsystem | `implementation/agent-recovery-escalation-authority/002-observation-diagnostics-and-stall-control.md`; `closure/agent-recovery-escalation-authority/002-status.md` | ADR-0040 Option B unchanged |

## Closure verification (2026-10-26)

D001–D003 implementation closures and the Recovery C002 cross-reference were recorded against source head `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6`. Exact-head `CI / verify` run [40383901325](https://github.com/dbowm91/codegg/actions/runs/40383901325) passed. A separately authorized D003 sample then completed 63/63 instrumented trials with no observed gaps and a one-sided 95% upper bound of 4.643845%; the scoped result, raw receipts and independence caveat are recorded in `plans/closure/harness-lab-continuation/003-live-event-gap-sample-20261027.md`. C006's live-rate condition is closed for that preregistered profile, conditional on trial independence.


## Handoff after D003 (2026-10-27)

D001–D003 implementation closed and D003's scoped 63-trial live event-loss measurement passed its preregistered threshold. **These closures do not mean the continuation source is merged into main or that a prolonged coherent coding session has been tested.** The separately owned successor is `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`, with H001 exact resulting-main integration, H002 ADR-0040-consistent versioned control, H003 deeper Rust/job fixtures, and H004 a separately authorized real long-session qualification.

**Provider constraint:** exactly one CodeGG provider integration, `opencode_go`, throughout the successor. Its upstream model-serving organizations are not extra CodeGG providers, and no route/provider diversity comparison is required. H004 requires fresh operator-approved budget authority and remains blocked until H001–H003 close. Historical D003 63/63 event integrity and cost-null caveats are unchanged.
