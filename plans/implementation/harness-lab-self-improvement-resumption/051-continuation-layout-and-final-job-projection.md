# Harness Lab Self-Improvement Resumption S051 — Continuation Layout and Final Job Projection

Status: **active**

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

Correct the continuation line-layout projection to recognize the exact public field `contract_phases`, and preserve the status of the two fixed required final jobs (`workspace_tests`, `clippy`) in safe campaign evidence. Attempt 62 confirms the layout mismatch: the task format uses `contract_phases`, while the line-layout parser counts `completed_contract_phases`. It also ended with required success jobs incomplete, but the safe campaign projection only exposed the aggregate false result.

## 2. Invariants

- Keep H007 v97 task text, hidden verifier, route, duration criteria, and all run caps unchanged.
- Use only `opencode_go/muse-spark-1.3-contributor` for attempt 63.
- Keep exact-ID lifecycle, two repair pairs, active duration, continuation, review, public artifacts, and independent verifier requirements unchanged.
- Never retain continuation body, job IDs, job commands, prompts, raw tool output, hidden verifier assertions, credentials, private receipt, or runner log.
- Preserve only fixed success-job labels, allowed statuses, booleans, and bounded counts.

## 3. Scope

### In scope

- Replace the stale `completed_contract_phases` line-layout key with `contract_phases`, matching `documented_contract_phases` and the public continuation format.
- Add production-shaped layout fixtures with all four progress headers and exact pointer placement/order, plus missing/duplicate/misordered negative cases.
- Safely project the fixed `workspace_tests` and `clippy` summaries: submitted once, documented completed, final observed status, and completed Boolean.
- Carry only the fixed per-job summaries through campaign evidence so an incomplete final gate identifies which label remains incomplete.
- Run the unchanged v97 case once as attempt 63 after offline verification and a fresh preflight.

### Explicitly out of scope

- Changing task instructions, hidden verifier, job semantics, ACP protocol, daemon, scheduler, provider, model, or caps.
- Claiming attempt 62's remaining task failures were caused only by the layout parser. It also lacked a reviewer result, final job completion, and public artifacts.

## 4. Required implementation

The progress field order is `completed_phases`, `contract_phases`, `required_stages`, `gates`; pointer fields remain `next_phase`, `next_contract_phase`, `next_stage`, `next_gate`. The line-layout diagnostic must derive order from those exact field names.

For each required final success-job label, preserve only its fixed label and the existing summary fields. Do not expose its job ID, command, arguments, output, or arbitrary label. Campaign projection rejects unknown labels, unknown statuses, private fields, Boolean-as-count, and malformed structures.

## 5. Acceptance evidence

- Regression tests prove the public `contract_phases` header produces an ordered four-section layout and pointers after all progress sections.
- Negative layout tests cover absent/duplicate headers and pointer placement/order.
- Safe projection tests preserve only `workspace_tests` and `clippy` summaries and remove private IDs/commands and unknown fields.
- Focused Python suites, assertion-quality and duplication guards, Python compilation, and `git diff --check` pass.
- Attempt 63 uses the unchanged v97 manifest, paired ARM64 binary/helper, Muse route, exact current authorization, and all existing caps. H007 qualifies only if all current checks and the unchanged independent verifier pass.

Offline implementation status: the parser now recognizes `contract_phases`; campaign evidence carries only the two fixed final-job summaries and fails closed for missing or invalid status. The five focused Python modules pass (220 tests). Both repository test-quality guards pass, as do Python compilation and whitespace checks. Attempt 63 remains pending a pushed exact head and fresh preflight.

## 6. Ordered work packages

### A — Continuation header parity

Align the line-layout parser with the public prompt fields. Keep the projection structural and body-free.

### B — Final required-job detail

Carry the fixed per-label completion summaries through safe campaign evidence using the existing parser output.

### C — Offline verification and attempt 63

Run focused tests and guards; verify exact manifest and paired artifacts, campaign/grant/task headroom, credential availability without printing the value, and a fresh 30-GiB preflight reserve. Start only after all checks pass.

## 7. Stop conditions

- Stop if preserving the final-job summary requires raw IDs, commands, or output.
- Stop if preflight disk is below 30 GiB, authorization is exhausted, or any route/model/artifact identity differs.
- Preserve evidence if any existing request, time, inactivity, native-call, disk, or cancellation limit fires.
- Do not call H007 qualified from active duration, complete stages, or partial evidence.

## 8. Closure evidence required

Record tests, attempt 63 preflight/safe-evidence paths, route/model, campaign/grant/task counts, request count, layout and per-job summaries, terminal cause, minimum sampled free disk, and qualification decision. State that private raw artifacts were not inspected.
