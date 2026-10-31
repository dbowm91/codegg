# Harness Lab — Autonomous Self-Improvement Resumption Roadmap

Status: **S001–S012 offline closed; `stable_offline` established; `stable_live_not_established`**. S001/S002/S004/S007/S009 retain their historical dispositions; S003 and S011 found no eligible current defect. S010 closes the remaining composed native optimizer, crash/restart and prolonged-pressure gates that S005/S008 had left conditional. S013/S014 remain blocked; no live provider campaign is authorized by this status.

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
| **S005 — Campaign fault tolerance and repeatable offline qualification** | invariant | **offline matrix complete; `stable_offline` not established** | Six phase SIGKILL/recovery boundaries, pointer/ledger faults, N/N+1 admission, native private-daemon cleanup, and optimizer progress during two bounded parallel Cargo rounds are tested. The repeated optimizer campaign uses synthetic qualification; no prolonged saturation claim. |
| **S006 — Real campaign repeatability and stable operator handoff** | capability | **closed conditionally: `stable_live_not_established`** | Zero of two required new independent accepted campaigns; no defect or fresh approval; historical C003 excluded |
| **S007 — Hard external proposer request admission** | invariant | **closed** | Actual OpenCode CLI request N+1 refused pretransport through a controlled loopback HTTP gateway; no live calls; live route still needs fresh authorization |
| **S008 — Complete offline campaign fault qualification** | invariant | **closed conditionally; `stable_offline` not established** | Two repeated 190-test focused runs, six durable-phase SIGKILL/recovery tests, private-daemon parent-death cleanup and bounded optimizer/Cargo overlap passed; an optimizer-integrated paired native subject campaign and prolonged saturation remain unqualified. |
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

## 8. Successor qualification milestones S010–S014 (registered 2026-10-30)

S001–S009 historical closures and their immutable receipts remain in force; this section registers **new handoffs rather than rewriting old results**. S005/S008 retain their historical conditional closure labels, and S006's historical live disposition remains `stable_live_not_established`. The later S010/S011 evidence now satisfies the offline operational-readiness standard in S006; it does not create live-model authorization or imply that an eligible current defect exists. The user wants stable autonomous **harness self-improvement**, not a 30-minute gating condition, a public release or an additional provider integration.

### 8.1 Dependency graph

```text
S002 + S005/S007/S008 (closed, offline evidence)
                    |
                    v
S010 — real optimizer + paired native ACP offline test (ready)
                    |                                  S001/H007 (closed)
S004 + S009 (closed) |                                        |
          |          |                                        v
          v          |                              S012 — causal trace (ready)
S011 — trusted real-defect intake (ready)                    |
          |          |                                        |
          +----------+----------------------------------------+
                                 |
                                 v
              S013 — one eligible real current-CodeGG defect
                     + fresh live operator authorization
                                 |
                                 v
              S014 — second independent eligible real defect
                     + fresh authorization + rejected control
```

S010, S011 and S012 have completed their offline implementations and evidence. S010's integrated native-trial SIGKILL/restart gate passed, so the offline operational-readiness criteria are now met; S011 found zero eligible current defects; S012 provides the fresh causal trace and attribution report. S013 remains blocked on independently reproducing a genuine current CodeGG defect and separately approving live admission for the subject and external proposer. S014 is blocked until S013 has a **positive** accepted real defect (a truthful rejected/no-defect S013 result leaves the dependency unsatisfied), another distinct defect, and separate approval. Credential access, paid calls, automated source promotion or work outside scoped repo roots are not approved by the planning branch.

| Milestone | Primary class | Status | Evidence to close |
|---|---|---|---|
| **S010 — Integrated native offline optimizer** | invariant | **closed offline** | Two complete actual paired `run_optimizer` campaigns, prolonged resource pressure, and the resumed native ACP trial after an observed SIGKILL passed. See `plans/closure/harness-lab-self-improvement-resumption/010-status.md` and its crash/recovery receipt. |
| **S011 — Current real-defect intake authority** | infrastructure | **closed offline** | Two private native current-source reproductions and fresh dossier validation passed; `eligible_real_defects=0`, and synthetic/historical inputs remain ineligible. See `011-status.md`. |
| **S012 — Redacted causal trace** | infrastructure | **closed offline** | Fresh 67-event native ACP trace passed boundedness, correlation, repair, continuation, and attribution checks; see `012-status.md`. |
| **S013 — First authorized causal campaign** | capability | **blocked** | One independently reproduced eligible current CodeGG defect and fresh operator-approved bounded external-proposer/subject live experiment, with truthful accepted/rejected/inconclusive outcome and human review packet |
| **S014 — Independent live repeatability** | capability | **blocked** | Two **new** accepted, distinct real current-defect campaigns plus correctly rejected/no-defect control; complete receipts and manual review; limited `stable_live_evidenced` label only if actually met |

Plan handoffs: `plans/implementation/harness-lab-self-improvement-resumption/010-*.md` through `014-*.md`. Closure records: `plans/closure/harness-lab-self-improvement-resumption/010-status.md` through `014-status.md` **only after execution**, never created as fake closures by the planning branch.

### 8.2 Promotion standards, out-of-scope and risk

`stable_offline` is established from the S005/S008 matrix, S010's composed paired native campaign/crash-restart/prolonged-pressure evidence, and S011's trusted current-source intake/refusal gates. This label is offline operational readiness, not a measured real-model improvement rate. `stable_live_evidenced` remains unestablished: it requires two independently accepted **genuine current** CodeGG fixes and a rejection/no-defect control as defined in S006, S013 and S014. The candidate does not merge or push itself. Provider request ceilings are independently enforced for the CodeGG `opencode_go` subject and the external OpenCode CLI proposer. Preserve unknown cost/usage, exact source/binary identities, explicit Deny, default daemon isolation and full test/verifier integrity.

H007's 30-minute sustained horizon remains **independent** in `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`. Do not register H008 or rerun H007 solely to unblock a correctness-oriented improvement campaign; any new sustained live attempt needs its own future plan and fresh authorization. Keep a single CodeGG `opencode_go` adapter; upstream route choice is a separately approved experiment, not integration of multiple CodeGG providers. No new VM requirement, new scheduler, Rust rewrite, remote resource scheduler, always-on autonomous service or automatic Git promotion. Risk: native Rust builds and parallel lanes exhaust disk/CPU, and incomplete causal data can misattribute task/model failure to CodeGG; stop or close conditionally rather than weakening guards.
