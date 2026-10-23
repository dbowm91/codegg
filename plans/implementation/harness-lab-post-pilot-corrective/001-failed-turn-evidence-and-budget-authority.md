# Harness Lab Post-Pilot C001 — Failed-Turn Evidence and Budget Authority

Status: ready for handoff
Repository baseline: `main@2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59` (2026-10-21; refresh HEAD before editing).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`
Corrects: Harness Lab M005 `plans/closure/harness-lab/005-status.md` (F001/F002), preserving M002's strict `plans/closure/harness-lab/002-status.md`.
Long-term authority: `plans/000-long-term-specification.md` §§23, 24, 26–29; `plans/001-terminology-and-domain-model.md`.
Primary class: invariant; secondary: developer capability.
Applicable ADRs: existing ACP/daemon authorization and audit decisions; do not introduce a new scheduler, provider API or policy authority.

## 1. Objective

Explain and accurately classify a **real native provider-backed `TurnFailed`** without exposing secrets, and establish an enforceable, admission-time hard usage/spend policy for a live Harness Lab trial. Preserve the existing automatic stop if any authoritative usage or ceiling cannot be established. The user-approved trusted development workstation remains the target; no VMs, containers, extra prompts or changes to normal daemon permissions.

## 2. Why ready / findings

M001–M003 strictly closed; the live 2026-10-21 Task 1 Muse Spark 1.3 run has an ACP `HarnessTurnFailure`, seven tool completions, unchanged source, unavailable terminal usage, unknown dollar cost and no diagnostic cause. Existing M005 pilot stopped before Task 2. This is a concrete negative reproduction.

Current `src/acp.rs` produces `-32010 native turn failed` with only `kind`, `sessionId`, `turnId`; `terminal_usage()` inspects `CoreEvent::TurnCompleted` / `ProjectionEvent::TurnCompleted`, not `TurnFailed`. `dev/harness_lab/ledger.py::record` accurately marks missing tokens and provider cost unavailable. `src/core/daemon_*`, agent/provider adapters, native run store/protocol need a focused trace audit before assuming the provider reported terminal usage at all. M002's deterministic fixture proved failure semantics and Deny, but not failure-path provider metrics.

## 3. Invariants and non-goals

**Must preserve:** ACP v1 standard terminal reasons and one terminal per turn, correlation/visibility filtering, native daemon policy ownership, explicit Deny under yolo, distinct full-host/workspace-write, no default-daemon attachment, bounded stdout/error frames, no plaintext credentials/headers/prompts/private reasoning in receipts, accurate nulls and fail-closed resource admission.

**Out of scope:** automatically recovering unknown spend; assuming a subscription plan constitutes a spending cap; fabricated provider token counts, scraped untrusted pricing, new OpenCode credentials/configuration, changing provider business logic merely to increase telemetry, running Task 2 before a proven budget, live model calls during CI, or an isolation/VM requirement.

## 4. Required production changes

1. **Source-of-truth audit.** Trace native `TurnFailed` creation, provider transport error classes and any usage accumulated over attempts. Enumerate what is known at each layer: provider attempt ID, message/finish reason, native turn ID, tool/agent last action, time elapsed, provider model route, accumulated reported tokens, internal retry counts, error category. Separate `not provided`, `not observed`, `lost at projection`, `provider-transport failure`, `hard-denied`, `cancelled`, `budget_exceeded`; never infer error cause from the final tool name.
2. **Bounded failure receipt.** If source has a safe typed error, project allowlisted category + correlation through native authorized event/receipt and ACP `-32010` JSON-RPC error `data` using optional namespaced fields. If richer native metadata is unavailable, explicitly keep `native_failure_unspecified`; document the gap and the owning production layer. Continue to reject unknown code values and preserve success/failure/cancellation distinction.
3. **Usage provenance on every terminal.** When native source actually knows partial/aggregated provider usage on a failed or cancelled turn, deliver it without double-counting across retries. If it cannot, `provider_usage: null` plus reason remains mandatory. Add tests distinguishing successful terminal with usage, provider failure after at least one metered attempt, failure before any provider response, failure with no provider usage support, and projection disconnection. Do not change ACP v1 `stopReason` enum.
4. **Budget admission contract.** Introduce explicit controller preflight for `token_limit`, `cost_limit_usd`, `max_provider_requests`, wallclock/trial caps and **provider-authoritative or otherwise demonstrably enforceable quota** where a hard monetary cap is required. Token counts observed only *after* provider responses are telemetry, not hard caps. If a provider cannot cap failure-path spend and the operator requested a hard spending limit, **do not launch**, returning structured `budget_unverifiable` and missing source. If a suitable capped subscription/route is independently verified, document what it enforces and what it does not; no guessed price `cost_usd`. Prefer authoritative provider quota/cost reporting to post-hoc local estimation.
5. **Stop-rule reproducibility.** One failing trial without usage must terminate only the bounded experiment according to its stated admission contract; record retained completed prior trials, unattempted cases, reason, daemon identity, requested/effective policy, and resource caps. No uncontrolled retry/provider fallback. Replay and report distinguish `missing_usage` from consumed-zero.
6. **Operator documentation.** Update `architecture/acp.md`, `dev/harness_lab/README.md`, diagnostic CLI, plus `docs/daemon.md` only if native contract changed. Include an explicit example of a preflight refusal with no provider calls and safe instructions to qualify a supported budget source. Preserve the historical pilot evidence untouched.

## 5. Ordered work packages

A. **Audit and gap classification.** Locate actual native error/usage path, obtain redacted fixture for the failed Muse category, write a short source-to-ACP mapping and confirm which missing fields are truly unobservable. Stop if this requires leaking private provider payloads or bypassing authorization.

B. **Truthful telemetry projection.** Implement minimal typed/additive fields and safe ACP emission, if supported, plus Python decoding. Test valid/invalid/absent usage and correlation. No provider-specific magic parsing.

C. **Admitted-budget guard.** Build an explicit provider profile preflight with a typed trusted source descriptor and fail-closed hard ceiling semantics; assert a missing cap prevents a live trial *before the first request*. Verify controller cancellation, zero retry and ledger integrity on failure.

D. **Provider-free end-to-end regression.** Native test fixture produces a failure after tool use; assert JSON-RPC code, exact redacted category, valid usage/missing-reason, no stdout pollution, correct stop. A manual authenticated recheck is **optional**, not closure evidence for a code-only correctness gate; strictly label any actual provider calls.

## 6. Failure, cancellation, restart and contention

Crash between last provider receipt and ACP terminal yields **unknown** usage unless an authoritative durable receipt confirms otherwise. Partial accumulation must be keyed by provider attempt/turn and idempotent across event replay; neither duplicate terminal nor a restarted controller may count twice. If policy snapshot differs at admission, reject. A headless permission pending remains a native denial/error, never auto-granted in full-host mode if explicit Deny. Multi-daemon experiments are separately keyed and capped; client/daemon teardown remains owned and bounded.

## 7. Compatibility and migration

Existing ACP clients ignore additive `error.data` fields; never change successful v1 result shape except optional `codeggUsage` semantics with explicit null handling. Avoid production DB changes; if a durable aggregation is required, first document one stable canonical owner and compatibility proof. Version new Python ledger fields; old stored pilot data remains readable with `usage_unavailable` and no unearned token estimate.

## 8. Required tests and verification

- Rust unit: all terminal variants, detailed category sanitization/size bound, distinct cancelled/failed, multiple provider attempts, no reuse of stale turn metadata, secret-key/non-public payload exclusion, request-denied path unchanged.
- Integration: real `codegg acp` -> native daemon -> local provider fixture simulating success with usage, fault after usage, transport failure with no usage and explicit denial. No live calls.
- Python: preflight rejection with missing hard cap and **zero** outgoing provider requests; no double count after replay/retry; two private lanes; stopped experiment resumes only safely; JSONL/SQLite integrity and reason accuracy.
- Commands: `cargo test -p codegg --lib acp`, `cargo test -p codegg --test acp_stdio`, `python3 -m unittest discover -s dev/harness_lab/tests -v`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `scripts/verify.sh quick`. Record actual runs; a filter matching zero tests is not proof.

## 9. Acceptance, stop, closure

**Accept:** an offline failure after real native tool calls produces safe reason/provenance and truthful usage or an explicit unavailable status; a hard cap with no trusted enforcement is rejected before any live provider call; both ACP policy profiles and explicit Deny retain their original semantics; no credentials are stored; replay remains exact. A later real provider trial may remain blocked despite code closure; do not represent conditional M005 live closure as satisfied on fixture tests.

**Stop:** no authoritative usage exists but implementation tries to invent it; requires broad provider rewrite or ACP v2; cap cannot be enforced yet tasks are proposed; error details contain sensitive values.

**Closure artifact:** `plans/closure/harness-lab-post-pilot-corrective/001-status.md` with implementation SHAs, source-to-client error/usage field matrix, provider failure reproduction, negative spending proof, actual tests/results, compatibility, live-qualification eligibility and next milestone unblock audit.
