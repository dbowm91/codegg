# Harness Lab Self-Improvement S005 — Campaign Fault Tolerance and Repeatable Offline Qualification

Status: **blocked on S002 strict closure**. Offline qualification only; no proposer/provider requests authorized.

Baseline: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (2026-10-30); pin a newer exact main before work.
Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S005.
Primary class: **invariant**. Hard dependency: S002 current-main acceptance and restart qualification. Interface dependencies: S003 campaign manifest/receipt contract, S004 dossier schema. S003's live authorization/positive outcome is **not** a prerequisite to run this offline milestone.

Authorities: `plans/000-long-term-specification.md` §§23–24, 26–29, `plans/001-terminology-and-domain-model.md`, `plans/003-planning-process.md`, accepted ADR-0040, and S001–S004 plans/closures.

## 1. Objective

Prove the **whole bounded improvement campaign**, not just an isolated trial, remains truthful and recoverable through process failure, partial work, ambiguous provider usage, duplicate requests and stale state. Reuse `dev/harness_lab` controller, stable native CodeGG ACP path, existing checkpoints/ledger and two private subjects. Make the offline ready/not-ready conclusion deterministic; never infer live reliability from mocks.

## 2. Why this is separate from S002 and S003

S002 requalifies the existing candidate gate and one SIGKILL/resume path on integrated main. S003 preregisters and attempts one real fix if authorized. Neither closes repeated *campaign-level* state-machine correctness under failures between phases or under two sequential candidates. The test target here is safe automation and honest stop behavior, not adding a new optimizer, a public scheduler or a second provider adapter. Inspect existing `improve.py`, `budget.py`, `supervisor.py`, `ledger.py` and S002 evidence before writing tests to avoid recreating already closed cases.

## 3. Invariants and non-goals

- The controller alone selects state transitions and accepts candidates. Candidate cannot mutate evaluator, protected controls, original baseline, controller, ledger, budget or holdout; no automatic merge/push.
- Immutable manifest binds candidate/source/binary/fixture/route/permissions, run IDs, retry and request ceilings, cumulative deadline and frozen expected outcomes. Mutation or ambiguous ownership fails closed.
- A prior observed request whose transport/accounting is uncertain **cannot** be replayed as free. Neither new process nor restart creates fresh authority; no double acceptance or duplicate scored rows.
- A timeout, model fault, missing verifier, lost ACP terminal, resource cap or bad telemetry stays a typed failure/inconclusive outcome, not a candidate regression or a pass.
- All private daemon/data/session/worktree/Cargo paths belong to the experiment; never fall back to the user's ordinary daemon. Do not weaken native Deny/approval/sandbox rules.
- No new system service, long-horizon duration requirement, model ranking, inference-driven scoring, mandatory VM or production schema migration.

## 4. Scope and intended code

Extend existing developer-only controller tests and, **only where a failing-first reproduction exists**, narrow process/manifest/ledger code in `dev/harness_lab`. Add a deterministic offline fault driver using a local fake ACP/provider/proposer with strictly counted attempted transports and content-addressed receipts. Reuse existing `run_optimizer` and checkpoint restoration; avoid replacing the state machine. The driver must exercise real CodeGG binary/tool path for selected boundaries, not imply a fake-provider test verifies real upstream behavior.

Keep a versioned offline qualification report: scenario IDs, expected/actual terminal, immutable manifest digest, event and budget completeness, number of persisted pairs, process/child cleanup outcome, retry count and stable pass/fail/unknown classification. An "unknown" cannot satisfy readiness. No changes to production agent scheduler or ACP protocol without a separately documented owner defect.

## 5. Ordered work packages

**A — Build the fault inventory.** Examine S002 results and list uncovered state transitions: before provider admission, after admission but before known transport outcome, after proposer output, during patch validation, after candidate build, during independent verifier, after first stored baseline/candidate pair, before final score publication, during cleanup, and during restart with stale daemon generation. Mark existing tests that already prove a case; add only absent cases. Include `no_reproducible_defect`, `REJECTED`, `ACCEPTED`, `BLOCKED`, and `INTERRUPTED_UNKNOWN` terminals.

**B — Prove multi-iteration integrity.** Exercise at least two sequential **synthetic** candidates, one rejected and one qualifying only on a real correctness predicate, with identical frozen inputs, isolated builds and stable denominators. Re-run the same deterministic scenario and require identical receipt identities and results modulo explicitly nondeterministic process data. Reject no-op, changed expected outcome, missing negative control, evaluator edit, invalid build and mixed model/policy cohorts.

**C — Crash and ambiguity matrix.** Inject SIGKILL or controlled fault at each genuinely unqualified transition, then restart using the same manifest/deadline. Prove exactly-once scored trial IDs, no duplicate candidate application or accepted result, refusal of changed fingerprint/route/budget, bounded child teardown and `interrupted_unknown` when dispatch cannot be reconstructed. Test N+1 refused at both CodeGG subject provider boundary and any external proposer pretransport seam; where hard proposer admission is unavailable, preserve a hard `live_blocked` result rather than test-only optimism.

**D — Resource/containment matrix.** Exercise disk-floor breach, CPU/Cargo target contention, parallel private subject isolation, cancellation during a managed test job, stalled ACP events, timeout and incomplete verifier. Verify that one private lane's shutdown does not alter another lane or the operator's user daemon, and that partial cleanup is discoverable and bounded on next run. Avoid sleep-based duration padding.

**E — Reuse guard/CI path.** Add fixed-seed, credential-free smoke under existing `dev/harness_lab/tests`, documented opt-in native fault sweep, and concise operator report. Keep slow/native tests separated from ordinary quick unit CI if they exceed established runtime budgets. Guard against replacing the real controller with a mock-only green path.

## 6. Restart, failure and security

Before retrying, reconcile append-only ledger tail against checkpoint pointer and immutable manifest. If the write boundary is uncertain, quarantine the attempt and require manual disposition; no hidden retries. Cancel/kill only positively identified owned PIDs/process groups and daemon generation. Private roots and symlink containment must be asserted before deletion. Cap event/tool bodies and redact credentials/prompts; test stored receipts for leaks. Keep exception terminals and refusal receipts even if no candidate was produced.

## 7. Storage, protocol and migration

Prefer no changes to CodeGG product. Add test-only fixture and versioned report schema in `dev/harness_lab` if necessary; old C003 and H005–H007 receipts remain byte-identical. On reading old schemas, mark missing retry/usage/horizon evidence as unknown. No broad on-disk migration; a current-main reproducible incompatibility requires a bounded adapter plus compatibility tests.

## 8. Required verification

Use genuine non-vacuous fault injection and state checkpoint comparisons; proof includes baseline and candidate SHA/binary hashes, unique trial IDs and no repeat tool/provider admission after resume. Include private two-daemon ACP smoke, request N/N+1 guard, no-op/relabel acceptance falsifiers, timeout/incomplete verifier and cleanup after forced child death.

Expected commands (adapt to real targets and record outcomes):

```bash
cargo build --locked --bin codegg --bin codegg-sandbox-helper
python3 -m unittest dev.harness_lab.tests.test_improve dev.harness_lab.tests.test_budget dev.harness_lab.tests.test_ledger dev.harness_lab.tests.test_scoring
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
scripts/verify.sh quick
git diff --check
# Run full workspace/strict Clippy and exact-head hosted CI if code changes.
# No authenticated model or provider calls in this milestone.
```

## 9. Acceptance, stop conditions and closure

Close only when every preregistered state/fault scenario has a verifiable expected terminal, no duplicate score/admission, no scope escape or orphaned owned process, stable budget/deadline after restart, complete independent negative controls and a credential-free exact-main integration receipt. Document any unproven transition as a named blocker rather than averaging it into a success rate.

Stop on unclosed S002, inability to enforce future proposer admission, evidence of a production permission bypass, non-reproducible checkpoint semantics, candidate-controlled scoring, or a need to change CodeGG daemon/scheduler policy. File narrow owner-specific corrective work if necessary.

Closure: `plans/closure/harness-lab-self-improvement-resumption/005-status.md` with requirement/evidence matrix, fault IDs and repeated trial counts, code and fixture digests, CI/tests, isolation/cleanup and security proofs, precise live-admission readiness, known limitations and blockers to S006. **Offline closure proves controller robustness only; not real-world improvement repeatability.**
