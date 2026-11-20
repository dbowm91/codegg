# Harness Lab Self-Improvement Resumption S051 — Continuation and Terminal Evidence

Status: **implemented**

Repository baseline: `6ee1e1a1ec1a279125a26c646f949cbf4a428b92`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#45-s051--continuation-layout-and-final-job-projection`

Long-term requirements:

- `plans/000-long-term-specification.md` §§26–29
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: **diagnostic correctness**

## 1. Objective

Correct continuation-layout measurement against the actual public task template, preserve the two required final-job summaries, and expose a bounded terminal state for the serialized agent transcript. Attempt 63 completed 45.48 active minutes and all public task gates, but remained unqualified because the independent verifier failed. It also exposed a second measurement error: pointers are interleaved with their corresponding progress sections, not placed after every progress section. The existing safe result does not contain enough transcript-terminal detail to distinguish an agent turn that ended normally from one that timed out.

## 2. Invariants

- Keep H007 v97 task text, hidden verifier, route, duration criteria, and all run caps unchanged.
- Use only `opencode_go/muse-spark-1.3-contributor` for subsequent authorized attempts.
- Keep exact-ID lifecycle, two repair pairs, active duration, continuation, review, public artifacts, and independent verifier requirements unchanged.
- Never retain continuation body, job IDs, job commands, prompts, raw tool output, hidden verifier assertions, credentials, private receipt, or runner log.
- Preserve only fixed success-job labels, allowed statuses, booleans, and bounded counts.

## 3. Scope

### In scope

- Match the public continuation format's interleaved order: `completed_phases`, `next_phase`, `contract_phases`, `next_contract_phase`, `required_stages`, `next_stage`, `gates`, `next_gate`.
- Add production-shaped layout fixtures with all four progress headers and exact pointer placement/order, plus missing/duplicate/misordered negative cases.
- Safely project the fixed `workspace_tests` and `clippy` summaries: submitted once, documented completed, final observed status, and completed Boolean.
- Carry only the fixed per-job summaries through campaign evidence so an incomplete final gate identifies which label remains incomplete.
- Project only allowlisted serialized-transcript terminal state and stop reason, so the safe record distinguishes `completed` from timeout or another terminal.
- Continue with the unchanged v97 case and existing caps after offline verification and fresh preflight.

### Explicitly out of scope

- Changing task instructions, hidden verifier, job semantics, ACP protocol, daemon, scheduler, provider, model, or caps.
- Claiming attempt 62's remaining task failures were caused only by the layout parser. It also lacked a reviewer result, final job completion, and public artifacts.

## 4. Required implementation

Each progress field is followed by its matching pointer before the next progress field: `completed_phases`/`next_phase`, `contract_phases`/`next_contract_phase`, `required_stages`/`next_stage`, and `gates`/`next_gate`. The line-layout diagnostic must enforce this exact interleaving and must not require the pointers to form a terminal footer; job and review evidence follows them.

For each required final success-job label, preserve only its fixed label and the existing summary fields. Do not expose its job ID, command, arguments, output, or arbitrary label. Campaign projection rejects unknown labels, unknown statuses, private fields, Boolean-as-count, and malformed structures.

## 5. Acceptance evidence

- Regression tests prove the public interleaved progress/pointer layout and reject a pointer that precedes its matching progress section.
- Negative layout tests cover absent/duplicate headers and pointer placement/order.
- Safe projection tests preserve only `workspace_tests` and `clippy` summaries and remove private IDs/commands and unknown fields.
- Transcript diagnostics expose only bounded terminal state and stop reason, and campaign projection removes unknown/private terminal fields.
- Focused Python suites, assertion-quality and duplication guards, Python compilation, and `git diff --check` pass.
- Each live attempt uses the unchanged v97 manifest, paired ARM64 binary/helper, Muse route, exact current authorization, and all existing caps. H007 qualifies only if all current checks and the unchanged independent verifier pass.

Offline implementation status: the parser matches the public interleaved layout; campaign evidence carries only the two fixed final-job summaries and fails closed for missing or invalid status. Transcript diagnostics and public campaign reconciliation carry an allowlisted terminal and stop reason. The five focused Python modules passed (221 tests); both repository test-quality guards, Python compilation, and whitespace checks passed. Attempt 63 was unqualified: 45.48 active minutes, 179 provider requests, two qualified repair pairs, complete event measurement, all 12 gates, both final jobs, review, and public artifacts, but the independent verifier failed. Its pointer-layout failure was a diagnostic false positive. Attempt 64 was cancelled after 34 provider requests and 239 seconds of observed activity because the external controller misread the post-run campaign ledger; this was not a harness/model failure. Attempt 65 ran 35.29 active minutes with 182 provider requests, 201 matched tool starts/terminals, all 7 stages and 12 gates, both repair pairs, final jobs, review, artifacts, and an observed `completed`/`end_turn` transcript terminal. It remains unqualified: public continuation checks passed while the independent verifier reported a continuation-boundary failure, and the lifecycle projection recorded one failed status call with an unobserved request/result correlation. S052 owns those two remaining diagnostic gaps. The external monitor sampled 108 times; minimum free disk was 122,088,800,256 bytes. Private receipts, runner logs, raw transcripts, and hidden-verifier output were not inspected.

Attempt 64 evidence:

- Preflight: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt64-preflight-20261113.json`
- Safe interruption diagnostic: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt64-safe-diagnostic-20261113.json`
- The attempt consumed one campaign/grant/task start and 34 provider requests. It was externally cancelled before the verifier; do not treat it as a model or product failure.

Attempt 65 evidence:

- Preflight: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt65-preflight-20261113.json`
- Safe diagnostic: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt65-safe-diagnostic-20261113.json`
- The live monitor observed 108 samples, 182 provider requests, 201 starts and terminals, and a 122,088,800,256-byte minimum free disk. Active work was 2,117.620 seconds. The unchanged independent verifier did not pass. Its public reconciliation identifies one unbound failed job-status call; the safe evidence does not associate that call with the status error-class histogram. The continuation-domain checks all pass publicly despite the verifier's continuation-boundary failure. S052 owns those unresolved attributions.

## 6. Ordered work packages

### A — Continuation header parity

Align the line-layout parser with the public prompt fields. Keep the projection structural and body-free.

### B — Final required-job detail

Carry the fixed per-label completion summaries through safe campaign evidence using the existing parser output.

### C — Offline verification and continued live qualification

Run focused tests and guards; verify exact manifest and paired artifacts, campaign/grant/task headroom, credential availability without printing the value, and a fresh 30-GiB preflight reserve. Start only after all checks pass. While a run is active, observe request admissions from its lane-local provider-boundary receipt and progress from the bounded causal trace; the campaign ledger is reconciled only after `run_suite` returns. Reconcile the safe transcript terminal and stop reason after each attempt without inspecting private raw artifacts.

## 7. Stop conditions

- Stop if preserving the final-job summary requires raw IDs, commands, or output.
- Stop if preflight disk is below 30 GiB, authorization is exhausted, or any route/model/artifact identity differs.
- Preserve evidence if any existing request, time, inactivity, native-call, disk, or cancellation limit fires.
- Do not call H007 qualified from active duration, complete stages, or partial evidence.

## 8. Closure evidence required

Record each attempt's preflight/safe-evidence paths, route/model, campaign/grant/task counts, request count, layout and per-job summaries, transcript terminal, terminal cause, minimum sampled free disk, and qualification decision. State that private raw artifacts were not inspected.
