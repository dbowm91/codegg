# Harness Lab — Autonomous Self-Improvement Resumption Roadmap

Status: **active planning**; S001 ready for offline handoff; S002 blocked on S001 closure and H007 mainline integration; S003 blocked on S002 closure and fresh explicit provider authorization. No self-improvement campaign is authorized by this plan.

Reviewed baseline: `main@4aea3df5e93c60aa85f55e805f2a8e9e947ab083` (2026-10-29) and open PR #217 branch `plans/h007-workload-calibration-20261029@ab22d66f28681321b8d2b46001ed6698764c9484` (2026-10-29). Recheck live refs at implementation. The S001 evidence corpus is already on the PR branch; later code qualification requires a reconciled mainline.

Long-term references: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/002-long-term-roadmap.md`; `plans/003-planning-process.md`. Related accepted decision: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (Option B). Source predecessor: `plans/subsystems/harness-lab-roadmap.md`, `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md`, `plans/subsystems/harness-lab-continuation-corrective-addendum.md`, `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`.

## 1. Purpose and ownership

**Resume repeatable, independently evaluated improvements to CodeGG's agent-facing harness**, not simply more long-duration benchmark attempts. The baseline is M001–M005's existing Python Harness Lab controller, its C001–C007 and D001–D003 corrective closures, C003's real accepted candidate, and H001–H006 mainline qualification. H007 supplies the latest, deeper *negative* live evidence, not a new hard gate requiring 30-minute task completion.

The `dev/harness_lab` Python supervisor owns private subject lanes, task/fixture identities, immutable evidence, proposal iteration, budgets, candidate qualification and accept/reject decisions. The production CodeGG daemon/broker owns tool execution, permission authority, ACP and job lifecycle. OpenCode CLI may propose scoped patches; it never evaluates or accepts itself. `opencode_go` is the **single CodeGG provider adapter** in scope; its upstream model IDs are routes, not separate provider integrations. A different route is an optional independently approved measurement, never a prerequisite for this line.

The desired deliverable is an operator-controlled local assess → diagnosis → bounded proposal → independent build/test → paired replay/holdout → accept/reject loop that can be repeated on *real, pre-registered harness defects*. An accepted candidate remains unmerged/unpushed until human review.

## 2. Evidence and non-equivalences

- PR #141 merged the two-lane ACP evaluation/ledger/optimization foundation; M005 closed the bounded optimize/checkpoint/restore controller. C003 accepted a genuine headless workspace-binding correction from an authenticated OpenCode proposal: 13/13 paired cases, one correctness improvement, zero regressions, negative controls held. D001 subsequently qualified the headless binding in production. These are **one successful demonstration**, not a repeatability estimate.
- H006's latest live result solved its task and hidden verifier, completed two repair cycles and observed one native job ID running/completed, but lasted only 10m43s. The 30-minute endurance requirement remained unmet; no job/ACP malfunction was demonstrated.
- H007's seven live attempts on the PR #217 branch consumed three explicit authorizations. Two had typed `provider_timeout` outcomes; others failed implementation/verifier/repair or job-overlap requirements. No run achieved 30 minutes; all observed ACP tool starts matched terminals with zero recorded gaps. The sanitized summary receipts do **not** supply a complete causal model of why an agent stopped after a partial repair. Treat route quality, prompt/fixture design, and harness defects as distinct hypotheses.
- The H007 branch also separates `task_outcome` and `horizon_outcome`. Fast correctness must stay a task pass even if a long-horizon observation is unavailable. PR #217 remains open at review. Its description and some source registry/roadmap summaries lag the seven-attempt closure; do not mistake passing branch CI for merged production.
- H007's 30-minute duration qualification is **not** a hard prerequisite for a normal-duration paired self-improvement trial. An independently managed endurance track can continue later with genuinely dependent work, never sleeps or fabricated prolonged activity.

## 3. Invariants and non-goals

**Invariant:** the candidate cannot change its own evaluator or frozen test/verifier corpus; every accepted outcome has a genuinely improved pre-registered correctness criterion, all protected controls retained and paired baseline/candidate evidence complete. Unknowns, missing events or test setup failures cannot become either a task pass or a candidate regression by default. Historical receipts/authorization are immutable.

**Invariant:** exact candidate/baseline Git SHA, binary digest, frozen suite/case digest, model route, policy, daemon generation, provider admissions, run ID and credential-free evidence distinguish experiments. Two concurrent subject daemons have separate mutable roots, sockets, workspaces and Cargo targets. Shared provider credentials are never copied into receipts. All request budgets are enforced *before transport*, including the proposer separately from the CodeGG subject when applicable; unsupported budget paths fail closed.

**Invariant:** trusted development workstation with intentional `yolo` approval and explicit `full-host` or `workspace-write` policy; explicit Deny, native sandbox restrictions, scoped paths and production permissions remain authoritative. Human-owned merge/push; no self-modification of the stable Python controller in a candidate; no hidden reuse of a production daemon.

**Non-goals:** a new harness runtime or scheduler, universal model ranking, new CodeGG provider adapters, automatic Git merge/push, autonomous unbounded provider spending, required VM/container, SBC benchmarking, training a local decision model, UI redesign, rewriting ADR-0040 recovery, or making endurance duration a condition for a correctness-fix acceptance. Delegation/LSP/MCP benchmark expansion requires separate qualified fixtures and is not a gate here.

## 4. Dependency graph and classifications

```text
Existing M/C/D closure + H001–H006 implementation/evidence
             |
             +-- H007 PR #217: offline/deeper fixture + seven live receipts
             |             | (mainline merge needed for S002 integration)
             v             v
S001 [infrastructure] Offline H007 failure attribution + evidence adequacy
             |
             +---------> S002 [invariant] Requalify existing optimize/acceptance
                                on reconciled main (offline; no provider calls)
                                       |
                                       v
                         S003 [capability] Bounded genuine self-improvement
                         campaign (live only under fresh operator authorization)
```

H007 *sustained live success* is **not** a dependency. S001's evidence corpus is readable on the PR branch now; if a source receipt/trace is absent, classify it unmeasured and close with a specific evidence limitation rather than guessing. S002 has a **hard** mainline integration and S001 closure dependency. S003 has a **hard** S002 closure dependency, plus an **operational** live-provider permission/budget gate.

| Milestone | Class | Status | Primary acceptance |
|---|---|---|---|
| **S001 — Trial attribution and evidence sufficiency** | infrastructure | **ready** (offline) | Versioned read-only classification of all seven H007 receipts, three-way observed/unobservable/contradicted outcomes, unambiguous ownership routing, and at least one genuinely reproducible *harness* defect or an explicit no-defect disposition |
| **S002 — Mainline optimizer and candidate-gate requalification** | invariant | **blocked** (S001 closure + PR #217 mainline integration) | Native two-lane deterministic evaluate → bounded proposer seam → isolated build/qualify → correct accept/reject, SIGKILL/resume and no-regression proofs on exact mainline |
| **S003 — Controlled repeated self-improvement campaign** | capability | **blocked** (S002 closure; live operational approval) | Pre-registered genuine failing behavior, bounded independent proposals and holdout/paired trials, measured improvement or honest rejected/no-defect outcome; never automatic merge |

Source plans: `plans/implementation/harness-lab-self-improvement-resumption/{001,002,003}-*.md`.

## 5. Exit criteria by stage

**After S001:** a machine-readable, independently reviewed failure/evidence matrix makes no claim not supported by H007 telemetry. Defects and weak model/fixture outcomes are separated; each proposed CodeGG fix has a locally runnable failing-first regression. A provider timeout is `provider_boundary` unless an independently demonstrated CodeGG provider bug exists. Do not generalize a zero-gap observation into zero true event loss.

**After S002:** a current-main operator can run the existing controller in manual/offline mode without a provider credential, reproduce expected negative controls, reject a cosmetic/incomplete candidate, qualify a seeded *historical* improvement as explicitly synthetic, and recover after interruption without duplicate provider requests or candidate effects. Any gap in proposer-specific spend admission is recorded as a hard live stop, not bypassed via CodeGG's subject-only cap.

**After S003:** at least one whole bounded assess/propose/qualify/accept-or-reject **campaign** has an auditable report. This is an operational capability even if no candidate is accepted, provided the objective no-defect/no-improvement result and all stop conditions are honored. Claim repeatability or improvement only to the extent preregistered non-synthetic data support it; separate task quality, reliability, provider timeout, and long-horizon observation in reports.

## 6. Cross-cutting failure, migration and safety

Version only genuinely new diagnostic/result schemas; do not mutate H005–H007 original case, receipt, hidden verifier or closure history. Migration reads old records and represents missing fields as unavailable. Freeze candidate scope, verifier and holdout hashes *before* proposal. A measurement-incomplete pair cannot be silently rerun with changed denominator. Resume is bound to source/case/route/policy/budget and original cumulative deadline. Kill abandoned child process groups, stop private daemons, release worktrees/Cargo targets without touching operator artifacts, and preserve a sanitized refusal receipt when any preflight fails.

Provider budget approvals are non-transferable and **none are granted here**. The subject's `opencode_go` provider-boundary cap is not proof of a cap on a separate OpenCode CLI proposer process. Proposer requests need their own demonstrable pre-transport cap/receipt or an explicit stop; neither a subscription's advertised allowance nor quota-derived units count as billed USD. All reports carry unknown billed cost when unavailable.

## 7. Risks, deferred work and closure

Primary risks: H007's summarized receipts are insufficient for causal attribution; overfitting fixes to one model's prompt style; incomplete baseline/candidate pairing; acceptance driven by timing, relabeled failure or fixture leakage; provider-side timeout obscuring agent outcomes; stale PR #217 status; and developer-machine RAM/disk exhaustion from parallel Rust builds. Preserve per-lane targets and configurable parallelism; no synthetic performance gain through cache ownership differences.

If attribution reveals no reproducible CodeGG defect, S001 does not invent one. S002 can still establish instrument integrity; S003 may close as a correctly refused/no-defect campaign but **cannot** report an accepted real improvement. If extending the runtime, tool policy or provider interface becomes necessary, stop and open a narrow finding in that owning subsystem; do not expand this roadmap speculatively.

Closure records belong to `plans/closure/harness-lab-self-improvement-resumption/001-status.md` through `003-status.md`, with implementation SHA, passing/failing tests, immutable evidence IDs, negative cases, explicit budget truth and residual severity. Register each milestone status only after evidence is gathered. No canonical long-term files or accepted ADRs are changed.
