# Harness Lab Mainline Qualification H002 — Versioned ADR-0040 Control Replacement

Status: **closed** — see `plans/closure/harness-lab-mainline-qualification/002-status.md`.
Source roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`.
Reviewed baseline: `plans/harness-lab-continuation-20261026@cd9fde42491c85941c31efbc2d5d200e8db4d638`. Refresh on entry.
Historical inputs: C004 `plans/closure/harness-lab-post-pilot-corrective/004-status.md` §5(4); C005 `plans/closure/harness-lab-post-pilot-corrective/005-status.md` §10; accepted `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`; `plans/closure/agent-recovery-escalation-authority/002-status.md`. Long-term: `plans/000-long-term-specification.md` §§26–29. Primary class: invariant.

## 1. Objective

Replace the benchmark's obsolete **“identical failed read must produce terminal stalled”** requirement with a versioned, deliberately exercised control that measures CodeGG's **current, accepted evidence-gated recovery contract**. The old control is not proof of a defect: C005 ran five real model trials where the agent explored after a failed read, never repeated it, and completed; after Agent Recovery C002, `RecoveryDecision::Stalled` is intentionally removed. An instrument demanding that terminal is invalid under ADR-0040, not a license to reintroduce false stalls.

This milestone repairs **the benchmark contract**, not production recovery policy or the closed recovery controller. It must not convert old “never exercised” evidence into a pass.

## 2. Unclosed requirement and why the old verifier missed it

Current `dev/harness_lab/fixtures/agent_autonomy/cases.jsonl` supports protected `repeated-identical-failure-stall` and `repeated-identical-failure-injected`; both `expected_terminal` definitions name legacy stall behavior or presume a model repeats. C004's paired live sample had no repeated call; `control.no_regression` for that branch was **not exercised**. The score contract can reject a candidate that wins only via advisory latency, but cannot attest a control that the agent never invoked. Merely running more sampled models or using a second provider doesn't fix this conceptual gap.

The authoritative contract is ADR-0040 Option B: repeated identity alone not terminal; changing workspace/job/child state or unknown observation is not confirmed stagnation; graduated guidance is allowed; the existing hard run/turn budget, cancellation, and permission denial remain typed independent terminating authorities.

## 3. Invariants and limits

- **Do not edit** original v1 case manifest, fixture bodies, original expected terminals, historical closures, score receipts or stored digests. Add a new versioned suite/manifest (e.g. `agent_autonomy_v2`), scorer/loader with explicit suite-version/digest and migration notes.
- V2 control must never claim a `stalled` terminal if the product doesn't produce one. Inspect current `src/agent/progress_recovery.rs`, `src/agent/loop.rs`, ACP terminal taxonomy and score contracts. Select only actual typed terminals and evidence. If a new terminal is desired, that is a **new architectural authority** and must go through an ADR, not this plan.
- Distinguish `control_exercised=true/false`, `control_held=true/false/unknown`, actual repeated identical-call count, exact-call SHA, progress evidence class, recovery rung and **actual stop owner**. `not_exercised` is not counted as passed or failed and cannot contribute to positive candidate acceptance.
- Existing D002 typed tool failures and D003 event-gap `unmeasured` remain authoritative; an error-looking string does not prove tool failure. A missing ACP terminal or gap produces unknown, not a false negative.
- An eligible test must include at least one **deterministic, model-independent** fake provider that genuinely issues N identical tool arguments over the same state; prompt wording alone is not a control instrument. The same fixture must show a genuinely progressing repeated read (changed file/state) is not falsely classified stagnant.
- No expensive multi-provider comparisons: all optional live runs through single CodeGG `opencode_go` integration, model route selected per separately admitted experiment; no live call required to qualify offline contract.

## 4. Dependencies

Ready on closed Recovery C002/ADR-0040; can be implemented before H001 merge in a separate fixture-worktree. Soft interface to H003 scoring and H004 live report. No hard external model-provider, delegated-agent, MCP, LSP or SBC dependency.

## 5. Ordered implementation

A. **Truth-table audit:** read original manifest, `faults.py`, `autonomy_run.py`, scorer, `ProgressObservation`, current ACP terminals. Write a small table comparing v1 expected v. actual status for (i) absent file followed by exploration, (ii) repeated exact failed read and unchanged state, (iii) repeated read after a real change, (iv) unknown state, (v) cancel/Deny, and (vi) hard budget. Include provenance SHA.

B. **New frozen v2 taxonomy:** version a *new* manifest and verifier under independent path. V2 protected controls test **false-stall avoidance** for read-heavy exploration, **bounded continued responses** to confirmed identical failure without claiming a non-existent terminal, and **separate budget-stop truth** when hard limits are reached. Give explicit `expected_observation` and `expected_stop_owner` schema; a model that never repeats must be marked **not_exercised**, not coerced to fail. Existing C005 fault injection remains historical; use an actual stubbed model/tool path to guarantee branch coverage locally.

C. **Independent verifier:** keep holdout/verifier outside editable workspace; count canonical full call (tool + normalized arguments + typed failure + state revision), not tool name alone. Classify response guidance, reroutes, valid progress, cancellation and timeouts separately. Make no new claims from timestamps, volatile stdout or model assertions of progress; verify evidence from native ledger/ACP.

D. **Score/admission integration:** protected controls that execute and violate ADR-0040 reject a candidate. Controls not entered are unresolved and cannot silently contribute to `control.no_regression`; score version change must reject mixed v1/v2 pairs and immutable-digest mismatches. Preserve the C002 substantive-correctness acceptance contract and old immutable receipt replay.

E. **Test matrix:** failing-first proof against old v1-only assumption, fake model repeated-call branch entered, positive changed-state no-stall control, repeated absent file and documented run-budget termination, interrupted/retried ACP with event gaps unmeasured, probe that editing frozen fixture or verifier is refused. Test both baseline and candidate path; no re-labeling to create an artificial score gain.

F. **C004 conditional reconciliation:** create **new** `plans/closure/harness-lab-mainline-qualification/002-status.md` with the old-control unsatisfiable-vs-ADR finding, v2 actual behavior and negative controls, exact SHA/CI, and an explicit recommendation: (1) supersede the old C004 control requirement by accepted ADR plus qualified v2 control; or (2) keep C004 conditional for historical non-exercise if repository rules prohibit that supersession. Do not silently rewrite `004-status.md` or C005 closure as if the former terminal was observed. Update registry/roadmap only after review.

## 6. Verification

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo test -p codegg --lib progress_recovery
cargo test -p codegg --test acp_stdio
scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Focused counts must be nonzero, with two failure-first controls tested against the pre-v2 version. Verify case checksum and score version are recorded in receipts; new fake provider never reaches public network. A CI test not executed is not a pass.

## 7. Security, concurrency and migration

No new permissions or outgoing network; maintain `workspace-write`, denied paths, and no replay of acknowledged mutating commands. Tool action fingerprints are redacted/bounded. Concurrent lanes cannot leak recovery state, and a restarted lane does not inherit unrelated previous-turn incident counters. Preserve v1 fixtures/legacy digests and supply an explicit reader for old receipts without mutating them.

## 8. Acceptance and stop conditions

H002 closes only if the deterministic control **really exercises** its targeted behavior, ADR-0040-consistent expected outcome matches, distinct/changed-state work does not generate false stalls, all protected negative controls are non-vacuous, and score/ledger rejects tampering/mixed schema. The C004 milestone disposition is a **separate reviewed reconciliation**; historical live non-exercise remains a fact. Stop for product stall policy change, required new terminal or pressure to change frozen v1 expectations.
