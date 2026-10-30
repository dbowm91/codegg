# Harness Lab — Autonomous Self-Improvement Resumption Roadmap

Status: **S001–S009 have closure records; S005/S008 remain conditional**. S001/S002/S004/S007/S009 closed; S003 closed as `no_reproducible_defect`; S005/S008 remain conditional without claiming `stable_offline`; S006 is `stable_live_not_established`. No provider-backed campaign was authorized or run for S007–S009.

Reviewed baseline: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (2026-10-30), merge commit for H007 PR #217. All seven H007 receipts are on main. Recheck source and exact resulting-mainline CI at implementation; PR #219 is the active planning handoff, not implementation evidence.

Long-term references: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`; `plans/003-planning-process.md`. Related accepted decision: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (Option B). Source predecessor: `plans/subsystems/harness-lab-roadmap.md`, `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`, `plans/subsystems/harness-lab-continuation-corrective-addendum.md`, `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`.

## 1. Purpose and ownership

**Resume repeatable, independently evaluated improvements to CodeGG's agent-facing harness**, not simply more long-duration benchmark attempts. The baseline is M001–M005's existing Python Harness Lab controller, its C001–C007 and D001–D003 corrective closures, C003's real accepted candidate, and H001–H006 mainline qualification. H007 supplies the latest, deeper *negative* live evidence, not a new hard gate requiring 30-minute task completion.

The `dev/harness_lab` Python supervisor owns private subject lanes, task/fixture identities, immutable evidence, proposal iteration, budgets, candidate qualification and accept/reject decisions. The production CodeGG daemon/broker owns tool execution, permission authority, ACP and job lifecycle. OpenCode CLI may propose scoped patches; it never evaluates or accepts itself. `opencode_go` is the **single CodeGG provider adapter** in scope; its upstream model IDs are routes, not separate provider integrations. A different route is an optional independently approved measurement, never a prerequisite for this line.

The desired deliverable is an operator-controlled local assess → diagnosis → bounded proposal → independent build/test → paired replay/holdout → accept/reject loop that can be repeated on *real, pre-registered harness defects*. An accepted candidate remains unmerged/unpushed until human review.

## 2. Evidence and non-equivalences

- PR #141 merged the two-lane ACP evaluation/ledger/optimization foundation; M005 closed the bounded optimize/checkpoint/restore controller. C003 accepted a genuine headless workspace-binding correction from an authenticated OpenCode proposal: 13/13 paired cases, one correctness improvement, zero regressions, negative controls held. D001 subsequently qualified the headless binding in production. These are **one successful demonstration**, not a repeatability estimate.
- H006's latest live result solved its task and hidden verifier, completed two repair cycles and observed one native job ID running/completed, but lasted only 10m43s. The 30-minute endurance requirement remained unmet; no job/ACP malfunction was demonstrated.
- H007's seven frozen live attempts (now merged through PR #217) consumed three explicit authorizations. Two had typed `provider_timeout` outcomes; others failed implementation/verifier/repair or job-overlap requirements. No run achieved 30 minutes; all observed ACP tool starts matched terminals with zero recorded gaps. The sanitized summary receipts do **not** supply a complete causal model of why an agent stopped after a partial repair. Treat route quality, prompt/fixture design, and harness defects as distinct hypotheses.
- H007's merged implementation separates `task_outcome` and `horizon_outcome`: a fast correct result is a task pass even when a long-horizon observation is unavailable. PR #217 merged as `99cecc050e`; its description remains an earlier planning-only snapshot, while the seven-run closure is authoritative. Branch CI success is not a substitute for checking resulting-mainline CI.
- H007's 30-minute duration qualification is **not** a hard prerequisite for a normal-duration paired self-improvement trial. An independently managed endurance track can continue later with genuinely dependent work, never sleeps or fabricated prolonged activity.

## 3. Invariants and non-goals

**Invariant:** the candidate cannot change its own evaluator or frozen test/verifier corpus; every accepted outcome has a genuinely improved pre-registered correctness criterion, all protected controls retained and paired baseline/candidate evidence complete. Unknowns, missing events or test setup failures cannot become either a task pass or a candidate regression by default. Historical receipts/authorization are immutable.

**Invariant:** exact candidate/baseline Git SHA, binary digest, frozen suite/case digest, model route, policy, daemon generation, provider admissions, run ID and credential-free evidence distinguish experiments. Two concurrent subject daemons have separate mutable roots, sockets, workspaces and Cargo targets. Shared provider credentials are never copied into receipts. All request budgets are enforced *before transport*, including the proposer separately from the CodeGG subject when applicable; unsupported budget paths fail closed.

**Invariant:** trusted development workstation with intentional `yolo` approval and explicit `full-host` or `workspace-write` policy; explicit Deny, native sandbox restrictions, scoped paths and production permissions remain authoritative. Human-owned merge/push; no self-modification of the stable Python controller in a candidate; no hidden reuse of a production daemon.

**Non-goals:** a new harness runtime or scheduler, universal model ranking, new CodeGG provider adapters, automatic Git merge/push, autonomous unbounded provider spending, required VM/container, SBC benchmarking, training a local decision model, UI redesign, rewriting ADR-0040 recovery, or making endurance duration a condition for a correctness-fix acceptance. Delegation/LSP/MCP benchmark expansion requires separate qualified fixtures and is not a gate here.

## 4. Dependency graph and classifications

```text
Existing M/C/D and H001–H007 evidence
                 |
                 v
S001 — H007 attribution (offline; ready)
     |                            |
     v                            v
S002 — current-main optimize       S004 — defect intake
qualification (offline)            (S001 hard; S002 interface)
     |             |
     |             +----------> S005 — campaign fault matrix
     v                          (S002 hard; S003/S004 interface)
S003 — first causal campaign           |
(S002 hard; live approval)             |
     |                                 |
     +----------------+----------------+
                      |
                      v
S006 — distinct real campaigns, repeatability and human review
(S003 + S004 + S005 hard; fresh live approval)

Corrective work (all offline):
S007 — external proposer HTTP admission (S002 hard)
S008 — complete S005 fault matrix (S005 + S007 hard)
S009 — generic positive intake (S004 interface; independent of live defects)
```

H007's 30-minute sustained live success is **not** a self-improvement dependency. S001 can work against the merged seven-run receipt corpus now. S002 must confirm resulting-mainline verification and close S001, not await another H007 trial. S004 is independently executable after S001, consuming S002's manifest interface when available. S005 is offline and may follow S002 independently of S003's provider authorization. S006 requires the completed causal campaign, intake and fault-tolerance contracts. Live execution at either S003 or S006 requires separately approved operator/provider authority; none is implied.

| Milestone | Class | Status | Principal acceptance |
|---|---|---|---|
| **S001 — Trial attribution and evidence sufficiency** | infrastructure | **closed** | Seven H007 receipts classified with pinned evidence, uncertainty and no unsupported CodeGG defect attribution |
| **S002 — Mainline optimizer and candidate-gate requalification** | invariant | **closed for offline qualification; live proposer blocked** | Isolated subjects, native allow/deny tool path, request N/N+1 subject cap, offline checkpoint/score tests, exact-main CI and quick verification |
| **S003 — Bounded causal self-improvement campaign** | capability | **closed: `no_reproducible_defect`** | Frozen H007 evidence and S004 dossier refused eligibility; zero live calls; no positive improvement claimed |
| **S004 — Reproducible defect intake and campaign eligibility** | infrastructure | **closed; no eligible current dossier** | Deterministic, digest-bound H007 intake and truthful ineligibility result |
| **S005 — Campaign fault tolerance and repeatable offline qualification** | invariant | **conditional closure; `stable_offline` not established** | Subject/provider admission, deterministic controller and cleanup tests pass; full transition-by-transition process-kill injection and sustained optimizer saturation remain uncovered |
| **S006 — Real campaign repeatability and stable operator handoff** | capability | **closed conditionally: `stable_live_not_established`** | Zero of two required new independent accepted campaigns; no defect or fresh approval; historical C003 excluded |
| **S007 — Hard external proposer request admission** | invariant | **closed** | Actual OpenCode CLI request N+1 refused pretransport through a controlled loopback HTTP gateway; no live calls; live route still needs fresh authorization |
| **S008 — Complete offline campaign fault qualification** | invariant | **closed conditionally; `stable_offline` not established** | Two repeated 162-test focused runs, real private-daemon parent-death cleanup and bounded parallel Cargo checks passed; phase-by-phase process death and sustained optimizer stress remain open |
| **S009 — Positive generic defect intake** | infrastructure | **closed** | Generic positive synthetic dossier path and CLI pass; synthetic evidence is always campaign-ineligible and H007 remains ineligible |

Implementation plans: `plans/implementation/harness-lab-self-improvement-resumption/001-*.md` through `006-*.md`; use exact filenames in the registry and closure records `plans/closure/harness-lab-self-improvement-resumption/NNN-status.md`.

## 5. Exit criteria by stage

**After S001:** a machine-readable, independently reviewed failure/evidence matrix makes no claim not supported by H007 telemetry. Defects and weak model/fixture outcomes are separated; each proposed CodeGG fix has a locally runnable failing-first regression. A provider timeout is `provider_boundary` unless an independently demonstrated CodeGG provider bug exists. Do not generalize a zero-gap observation into zero true event loss.

**After S002:** a current-main operator can run the existing controller in manual/offline mode without a provider credential, reproduce expected negative controls, reject a cosmetic/incomplete candidate, qualify a seeded *historical* improvement as explicitly synthetic, and recover after interruption without duplicate provider requests or candidate effects. Any gap in proposer-specific spend admission is recorded as a hard live stop, not bypassed via CodeGG's subject-only cap.

**After S003:** at least one whole bounded assess/propose/qualify/accept-or-reject **campaign** has an auditable report. This is an operational capability even if no candidate is accepted, provided the objective no-defect/no-improvement result and all stop conditions are honored. Claim repeatability or improvement only to the extent preregistered non-synthetic data support it; separate task quality, reliability, provider timeout, and long-horizon observation in reports.

**After S004:** subsequent campaigns can consume a deterministic frozen defect dossier with an independently reproduced CodeGG harness red case and explicit no-defect/insufficient-evidence rejection; source/user-task/model mistakes cannot become automatic candidate eligibility.

**After S005:** repeated deterministic offline campaigns, crashes and policy/budget/ownership failures result in typed, safe terminals with unchanged score/authorization denominators, no duplicate effects and verifiable private-root cleanup. A green fake provider does not establish live reliability.

**After S006:** claim `stable_offline` only with S004/S005 proof; claim limited `stable_live_evidenced` only after two *new*, independent genuine current-CodeGG fixes pass held-out verification and negative controls plus a rejected/no-defect control. If authorization or genuine defects are absent, record `stable_live_not_established` without inventing evidence, ranking models or lowering thresholds.

S007–S009 are corrective follow-ups to the named S002/S004/S005 gaps. S007 and S008 qualify only against offline HTTP fixtures and deterministic subjects. S009 proves the generic positive intake mechanics using synthetic test evidence; it does not add a real defect to the S006 denominator. S006's next genuine improvement campaign remains gated on a new, independently reproduced current CodeGG harness defect and fresh campaign authorization.

## 6. Cross-cutting failure, migration and safety

Version only genuinely new diagnostic/result schemas; do not mutate H005–H007 original case, receipt, hidden verifier or closure history. Migration reads old records and represents missing fields as unavailable. Freeze candidate scope, verifier and holdout hashes *before* proposal. A measurement-incomplete pair cannot be silently rerun with changed denominator. Resume is bound to source/case/route/policy/budget and original cumulative deadline. Kill abandoned child process groups, stop private daemons, release worktrees/Cargo targets without touching operator artifacts, and preserve a sanitized refusal receipt when any preflight fails.

Provider budget approvals are non-transferable and **none are granted here**. The subject's `opencode_go` provider-boundary cap is not proof of a cap on a separate OpenCode CLI proposer process. Proposer requests need their own demonstrable pre-transport cap/receipt or an explicit stop; neither a subscription's advertised allowance nor quota-derived units count as billed USD. All reports carry unknown billed cost when unavailable.

## 7. Risks, deferred work and closure

Primary risks: H007's summarized receipts are insufficient for causal attribution; overfitting fixes to one model's prompt style; incomplete baseline/candidate pairing; acceptance driven by timing, relabeled failure or fixture leakage; provider-side timeout obscuring agent outcomes; causal-evidence scarcity; and developer-machine RAM/disk exhaustion from parallel Rust builds. Preserve per-lane targets and configurable parallelism; no synthetic performance gain through cache ownership differences.

If attribution reveals no reproducible CodeGG defect, S001/S004 do not invent one. S002/S005 can still establish instrument integrity; S003 may close as a correctly refused/no-defect campaign but **cannot** report an accepted real improvement. S006 must keep the live-stability verdict conditional until multiple distinct genuine current fixes are independently evidenced. If extending runtime, tool policy or provider interface becomes necessary, stop and open a narrow finding in that owning subsystem; do not expand this roadmap speculatively.

Closure records belong to `plans/closure/harness-lab-self-improvement-resumption/001-status.md` through `006-status.md`, with implementation SHA, passing/failing tests, immutable evidence IDs, negative cases, explicit budget truth and residual severity. Register each milestone status only after evidence is gathered. No canonical long-term files or accepted ADRs are changed.
