# Harness Lab Post-Pilot C003 — Live OpenCode Proposal, Compile Correction, and Accepted-Defect Proof

Status: blocked — hard dependencies: C001 and C002 strict closure
Repository baseline: `main@2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59` (2026-10-21; refresh at handoff).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`.
Corrects M004 `plans/closure/harness-lab/004-status.md` F005, and completes M005's operational evidence linkage without overwriting historical records.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`.
Primary class: capability
Accepted authorities: existing CodeGG daemon + ACP v1, dev-only Python `dev/harness_lab/improve.py`, OpenCode optional subprocess API. No new ADR unless runtime ownership or default production authorization must change.

## 1. Objective

Produce verified, redacted end-to-end receipts for a **real authenticated OpenCode proposal** against a candidate-only worktree and a **real CodeGG ACP build/test/evaluation** of the proposed patch; prove a bounded compile-correction retry; and demonstrate at least one independently accepted, genuinely defect-correcting candidate. Do not confuse synthetic controller proof with genuine improvement. Do not auto-push/merge accepted code.

## 2. Why blocked / prior gaps

C001 must first provide failure-path observability and *enforceable live provider admission*, otherwise an OpenCode or CodeGG failure might incur unmeasured spend. C002 must qualify the revised score contract and interrupted candidate/outer resume, otherwise “accepted” is not an audited improvement. M004 already has OpenCode v1.19.0 CLI adapter, `--auto --format json` parser, `opencode_propose`, scoped worktree, changelog, cargo build and bounded retries, but only fake CLI tests and no authenticated proposal or accepted correctness-fix trace. M005 has a real model pilot for CodeGG **subject**, not OpenCode **proposer**. Do not represent those as the same evidence.

## 3. Invariants/non-goals

- OpenCode is the optional **patch proposer only**; the Python controller is independent, immutable for each experiment, and performs the score, budget, scheduling, scope check and acceptance itself.
- Candidate may change only approved agent-harness production paths (e.g. bounded `src/agent/`, `src/acp.rs`, specific tool adapters); independently held fixture/test/verifier/controller/ledger and baseline revision remain immutable. An approved candidate can add its own regression test if the external verifier still fails/passes independently.
- Explicit `yolo` for approved CodeGG subject; `full-host` or `workspace-write` remains exactly recorded; OpenCode CLI `--auto` does not bypass explicit deny. Trusted development workstation; no mandatory VM/container.
- Every actual provider request requires user-approved model route/credentials **and C001-verified spend/usage authority**. Do not assume CLI exit 0 proves a model patch passed; no silent retries on transport/provider uncertainty.
- Patch SHA, local branch/worktree, exact CodeGG binary hash, suite/test digest, trial IDs, tool/projection failure data and candidate changelog persist. Proposer cannot hide acceptance regressions. If no genuine harness defect is found, no “accepted improvement” may be claimed.
- Never modify default `main`, automatic merge/push, release, provider keys, user default daemon or untrusted workspaces.

## 4. Scope

In: real OpenCode CLI qualification (pin/refuse unsupported version, log redacted failure categories), verified proposal and compile-correction; real native ACP candidate qualifications under bounded live budget; independently identified defect fixture and accepted/rejected proof; interruption and successful resume with C002; operator guide. No change to broad app feature set.

Out: OpenCode plugin rewrite, new local model provider, training/evolutionary search, network sandbox, any automatic remote Git change, unbounded model retries, full production release, long-horizon/suite expansion (C004).

## 5. Work packages

**A — Preflight and ground-truth defect.**
- On refreshed main, inspect real documented issues (agent stall false positives, repeated read truncation, background job/context recovery, ACP/permission diagnostics, true agent inability to resume after fault). Identify *one currently reproducible* defect affecting agent use—not a fabricated change to baseline expected outcomes.
- Freeze baseline source SHA and failing independent native tool/projection/verifier case with explicit assertion and case hash. Prove failure in base binary and designate success condition before any proposal; separate protected negative controls and hidden holdout. If no real defect exists, the agent must **stop** and mark live-acceptance evidence blocked rather than plant a bug.
- Run deterministic **seeded historical regression** separately if needed to prove mechanical controller acceptance without claiming a production gain; clearly label the synthetic branch, source, and fixture.

**B — OpenCode live proposer integration.**
- Pin actual installed OpenCode CLI version and `run --auto --format json` semantics from documented version; replay fake fixture and run one authenticated bounded proposal in a dedicated linked worktree and private config. Prefer CLI subprocess; no persistent server necessary.
- Select approved model/provider; verify actual provider hard cap/token admission contract and maximum CLI process time; log proposer model version, provider route, native error/usage or unavailable reason and process cleanup. No plaintext keys/responses or hidden chain-of-thought in reports.
- Proposer instruction points to the failing test, allowed touched paths, risk and changelog requirements. Persist a digest of prompt, not credential context. Confirm diff/changelog outside-CI guard and exact baseline.

**C — Controlled compile-correction and acceptance gates.**
- Demonstrate an actual **OpenCode-generated compile-error correction** with maximum retry 1 or 2 in a sacrificial candidate; validate each retry is separately counted and gated and that subsequent compile/test re-run occurs. A deliberately injected compile fault is allowed **only** for this mechanics qualification and must be labeled a fault injection, never a real fix.
- Test separate genuine source candidate: cargo fmt/Clippy/focused Rust tests, fully isolated candidate build, original failing verifier, protected regressions, hidden holdout, native ACP test suite, paired complete score and expected improvement. A compile correction and an accepted real fix may happen in separate runs; both receipts required for full C003 closure.
- Verify real candidate acceptance only for measured defect remediation; if OpenCode proposes a plausible but failing patch, record `REJECTED`, not a success by narration. Do not turn a negative-control `execution_failed` into a false performance improvement.

**D — Interrupted-run recovery and finalized report.**
- During actual candidate qualification, kill a supervised process after at least one persisted scored trial (not just a fake Python state test). Resume with identical immutable inputs, prove prior row reused without a second request/duplicate, original deadline retained, stale daemon generation fenced, and exact task denominator unchanged.
- Add operator command example with `max_iterations`, `max_candidate_retries`, `max_wallclock`, `max_proposer_calls`/budget limits and `--resume`; if existing CLI cannot express promised bounds, reconcile before claiming usability.

## 6. Failure, cancellation, contention, retry

A missing OpenCode binary must leave manual proposer mode working. Any OpenCode Ask/explicit Deny, malformed event stream, unsupported version, provider transport fault, unreported failure usage or script timeout must yield **distinct** failed/blocked receipts, reaping only owned processes. An unfinished ambiguous proposal must not be auto-resubmitted. Compile retries consume cumulative budget; no out-of-allowlist modification can be accepted; candidate build/artifact paths must not collide with other candidates; paused/subprocess-killed ACP tool work must not escape controller cleanup.

## 7. Compatibility and migration

No Rust shipped Python/OpenCode dependency, production database migration, ACP major version change or enforced model subscription. New comparator/ledger version uses explicit compatibility with historical M005 receipts and original pilot evidence. Do not alter prior closure records to erase blocked history.

## 8. Required tests/verification

- Offline: fake version mismatches, Ask/Deny, provider failure, timeout and malformed JSON; test `ManualProposer` still works; check changelog, diff-scope, non-main target, failed build retries and acceptance/denial matrix.
- Real local: fresh private CodeGG daemon and ACP for old/candidate binaries, failed reproduction against baseline, compiled corrected patch, fixed task success plus protected controls, full paired denominator.
- Provider-backed opt-in: at least one actual bounded OpenCode proposal and compile-correction attempt, and an actual legitimate accepted correction only if demonstrably available; record model usage provenance or enforceably capped profile.
- Command evidence (run actual supported flags): `python3 -m unittest discover -s dev/harness_lab/tests -v`, `cargo test -p codegg --test acp_stdio`, `cargo test -p codegg --lib acp`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`, documented `python3 -m dev.harness_lab optimize ...` with 1–2 iterations on frozen suite. CI remains provider-free; hosted exact-head CI required for code changes.

## 9. Acceptance and stop conditions

C003 **strict closure** requires: one legitimate frozen failing baseline behavior corrected and independently verified with baseline/candidate binary receipts; an actual authenticated OpenCode proposer invocation with scope-preserving patch; bounded real compile correction (may be fault-injected/labeled separately), full candidate scored ACP run with paired nonregression and protected controls, interrupted/resumed proof, required changelog and budget accounting, no merge/push.

If provider spend cannot be enforced, a real defect does not exist, or OpenCode access is missing, preserve a **conditional/blocked** disposition with precise evidence. A deterministic seeded-regression success closes *mechanical scoring* only, never the external live autonomous improvement claim.

Closure: `plans/closure/harness-lab-post-pilot-corrective/003-status.md` including two types of proof (synthetic controller qualification vs genuine fixed defect), prompts/config hashes, build/test stdout digests, requested/effective authorization, nonregression and holdout comparison, retry/interruption IDs, remaining conditions, status of M004 and C004 unblock.
