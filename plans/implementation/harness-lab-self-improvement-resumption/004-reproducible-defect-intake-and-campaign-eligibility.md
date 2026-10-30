# Harness Lab Self-Improvement S004 — Reproducible Defect Intake and Campaign Eligibility

Status: **blocked on S001 closure**; the intake/reporting contract can be drafted against S001's frozen schema. Offline only, no provider calls authorized.

Reviewed baseline: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (H007 PR #217 merged 2026-10-30); reconcile newer main at implementation.
Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S004.
Primary class: **infrastructure**. Hard dependency: S001 attribution/adequacy closure. Interface dependency: S002's immutable experiment manifest and score identities; integrate after S002. S003 is a consumer, not a requirement to begin.

Authority: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`. Related existing evidence: C003's accepted workspace-binding fix, H006/H007 closures, and S001/S002/S003 plans.

## 1. Objective

Replace manual one-off selection of a purported "harness defect" with a deterministic, **evidence-limited and reusable intake gate** for candidate self-improvement campaigns. Consume existing immutable Harness Lab receipts and reproducible native ACP/test evidence; produce a bounded defect dossier or an explicit `no_reproducible_defect` / `insufficient_evidence` disposition. Do not convert model mistakes, provider timeouts, failing user-task code or missing telemetry into CodeGG bugs.

## 2. Current implementation evidence and why additional scope exists

S001 analyzes seven historical H007 receipts; S003 consumes one preregistered defect. Neither establishes a repeatable *between-campaign* process for deciding whether a future observed failure is actually a CodeGG-owned defect and safe to propose against. Existing `dev/harness_lab/{ledger,scoring,autonomy_run,improve}.py` provide receipts and candidate evaluation; do not add a parallel ledger, report warehouse or inference dependency. H007's task-vs-horizon split stays authoritative. Confirm actual current modules and S001's output before editing.

## 3. Invariants and non-goals

- Candidate eligibility requires a reproducible **CodeGG agent-harness** red condition with independent expected behavior and at least one plausible disconfirming control. An observed task failure alone never satisfies eligibility.
- Input evidence is immutable, digest-bound and versioned; unknown remains unknown. Historical H005–H007 receipts, verifiers and closure records are read-only. A fake replay can prove machinery but cannot retroactively upgrade live evidence.
- Eligibility is decided by deterministic rules and independent tests, not an LLM's narrative; an LLM MAY propose a hypothesis, but cannot certify it or edit evaluator, contract or protected controls.
- No background crawling, public telemetry service, persistent daemon schema, new model provider adapter, automatic auth, mandatory VM, automatic Git mutation or new tool protocol.
- Respect existing operator permission scopes. Never auto-promote a candidate into a paid S003 attempt.

## 4. Intended interfaces and production changes

Add a small developer-only `dev/harness_lab` intake/diagnostics helper (or extend S001's module) accepting one pinned receipt group and optional independent reproduction artifacts. Emit `defect_dossier_v1`: evidence digests/identity, symptom, observed and unobserved claims, independent reproduction command and output hash, suspected CodeGG owner, alternatives and falsifiers, test+holdout eligibility, allowed candidate source scope, protected negative controls and disposition. Separate `observed_failure`, `reproduced_harness_defect`, `route_failure`, `fixture_or_evaluator_issue`, `unattributed`, `no_defect`.

Require an explicit unique defect identifier based on stable evidence and repro contract, not a natural-language title; identical receipts must not produce duplicate campaign eligibility. Disallow changed candidate baseline, fixture, verifier, policy, route or red-test digests from silently reusing eligibility. Keep raw trajectories bounded and redacted. Prefer a CLI report mode alongside the existing Python controller, not a standalone service. If S001 already supplies a sufficient typed artifact, reuse it rather than fork schema.

## 5. Ordered work packages

**A — Inspect and freeze interfaces.** Read S001 closure, S002 expected manifest and current `ledger/scoring/improve` paths. Inventory available native ACP and headless fixture controls. Record a minimal schema and deterministic mapping from historical receipts to intake states, retaining provenance and missing evidence explicitly.

**B — Reproduce before eligibility.** Build a bounded offline fixture set: historical C003 genuine CodeGG defect (labelled historical/synthetic for present exercise), H007 insufficient-repair and typed timeout negatives, failing user Rust implementation negative, one protected Deny/control failure, and an unmeasured verifier case. The positive requires a failing-first **CodeGG-owned** native/ACP contract and an independent fixed-behavior expectation; never manufacture a new real defect.

**C — Bind eligibility to campaign identity.** Produce the preregistration input S003/S005 can consume: exact baseline SHA/binary, frozen verifier/control/holdout digests, allowable source paths, experiment type `historical_fixture` vs `genuine_current_defect`, comparable policy/route constraints, and bounded risk classification. If a real current defect cannot be reproduced, produce a non-eligible dossier with a bounded diagnostic next action.

**D — Validate isolation and negative cases.** Falsify eligibility by changing verifier hash, case ID, source SHA, evidence digest, policy, ownership scope or an unobserved result; prove candidate cannot modify dossier, holdout or control. Deduplicate identical inputs without silently merging heterogeneous trials.

**E — Document handoff.** Add one credential-free operator command path to `dev/harness_lab/README.md`; record typed refusals, example dossier and ownership routing. Do not initiate proposals, builds or provider-backed calls from intake alone.

## 6. Failure, restart, contention and security

Malformed or partial receipts fail closed to `insufficient_evidence` with a saved sanitized refusal, not a guessed eligible defect. Crash between read and dossier write produces an idempotent atomic re-run; no effect on original ledgers. Concurrent intake uses unique content-addressed outputs and does not compete for candidate Cargo targets or mutate worktrees. Redact provider responses, credentials, private paths and prompt bodies. Keep hidden verifier outside candidate-writable path; no follow-up when the independent reproduction cannot be made deterministic.

## 7. Protocol, storage and compatibility

No CodeGG production RPC, database migration or session protocol. Use a versioned local JSON artifact and explicit readers for legacy receipts; missing historical fields are `unknown`, not `false`. Original S001/S003 schema remains authoritative; any adapter must be additive and tested against pinned historical artifacts. If a new core API appears necessary, stop and route a narrowly evidenced defect to its owning subsystem.

## 8. Required verification

Focused: negative taxonomy for H007 timeout/repair/fixture cases; genuine-vs-historical source labels; unchanged historical input digests; duplicate intake idempotency; missing event/verifier and tamper refusals; correct owner scope and no false promotion. Integration: one native ACP red-and-control reproduction in a private lane, a positive historical dossier and a negative no-defect dossier; unchanged original checkout and no external model calls. Adversarial: symlink escape and candidate-write attempt on frozen dossier/holdout.

Commands (select exact existing test modules after inspection):

```bash
python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_scoring dev.harness_lab.tests.test_ledger
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# If native Rust sources change: scripts/verify.sh full and exact-head hosted CI
```

## 9. Acceptance, stops and closure

Accept only if a frozen, independently reproduced CodeGG defect is eligible and the H007 negatives remain ineligible, with the same decision after replay/restart and explicit evidence completeness. Do not claim discovery of a fresh real defect from seeded C003 material. Stop if S001 is unclosed, a verifier is candidate-writable, provenance is missing, the only failure is provider/model/user-task behavior, or a new storage/protocol architecture is required.

Closure `plans/closure/harness-lab-self-improvement-resumption/004-status.md` must include implementation SHA, requirement-to-evidence table, positive/negative dossiers and digests, complete tests/CI, security/compatibility/contention evidence, typed unknowns, severity of residual findings and readiness for S006. A no-current-defect disposition can close the deterministic intake capability but does not prove autonomous self-improvement.
