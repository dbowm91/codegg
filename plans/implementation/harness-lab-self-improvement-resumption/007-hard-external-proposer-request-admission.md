# Harness Lab Self-Improvement S007 — Hard External Proposer Request Admission

Status: **closed — offline request-level admission qualified; live route still requires fresh authorization**

Repository baseline: `e8dbb8af3793acf46741eae89b3c5509fc952e47` (PR #219)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S007.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md`.
Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`.
Primary class: **invariant**.

## 1. Objective

Enforce the OpenCode CLI proposer's request ceiling at the provider HTTP boundary. Request N may be forwarded; request N+1 must be refused before upstream transport. Record a bounded, credential-free receipt. Qualification uses only a loopback HTTP fixture; this plan authorizes no real model/provider request.

## 2. Why this corrective is ready

S002 proved CodeGG subject-side admission but explicitly left OpenCode CLI request accounting unproven. S005 records per-invocation fake-CLI tests as insufficient. The installed OpenCode 1.19.3 CLI can be directed to a custom local provider endpoint using its supported config, so the request boundary can be exercised offline with the actual CLI and a local fixture.

## 3. Current implementation evidence

`dev/harness_lab/improve.py::run_opencode` launches the external CLI directly. `Budget` and `SpendGuard` count proposer invocations, not HTTP calls made inside one invocation. The existing `ProposerRouteAndBudgetTests` fake CLI verifies invocation N+1 only. OpenCode configuration supports a provider `baseURL`; the local experiment against v1.19.3 confirmed that one CLI invocation can issue multiple chat-completion requests (including title generation) to that endpoint.

## 4. Invariants that must not regress

- Every external proposer HTTP request is admitted before forwarding; every refused or uncertain request consumes the local ceiling and is never retried upstream by the gateway.
- The CLI cannot silently fall back to its user/default provider endpoint. Use an isolated, pinned configuration, disable plugins, validate the configured route, and fail before launch if the gateway is unavailable.
- Receipts contain request counts, status, timing, endpoint class and payload digests only; never request/response bodies, authorization headers, keys or private prompts.
- A request without observable usage remains unknown; the request cap is independent of USD reporting.
- Offline fixture traffic cannot escape loopback. No real provider call occurs in S007.

## 5. Scope

### In scope

- A bounded loopback HTTP admission gateway for the supported OpenCode Go HTTP wire shape(s).
- A temporary OpenCode config that routes the exact pinned proposer model through the gateway and disables plugins/default-route fallback.
- A provider boundary receipt and fail-closed cleanup for timeout, cancellation, rejected N+1, malformed routes and child exit.
- Controlled local HTTP tests that invoke the actual supported OpenCode CLI and prove multi-request accounting.

### Explicitly out of scope

- Any live model/provider traffic or credentialed validation.
- New CodeGG provider adapters, scheduler/daemon changes, or a general-purpose proxy.
- Treating CLI invocation count, tool-call count, token estimates or subscription limits as provider request admission.

## 6. Required production changes

Keep ownership in `dev/harness_lab`; do not alter CodeGG subject admission. The gateway binds only to `127.0.0.1`, accepts only the configured API method/path, bounds request and response bytes and time, and checks the ceiling under a lock before opening an upstream connection. Fixture mode returns controlled API responses without any upstream. Live-capable forwarding remains disabled unless a specific upstream is explicitly configured and validated; this plan tests fixture mode only.

`run_opencode` must pass the isolated config and gateway address to the child, use the pinned model, disable plugins, and refuse any configuration that would let the CLI contact its default route directly. Preserve existing return receipt fields while adding the gateway receipt.

## 7. Ordered work packages

### A — Define the transport contract

Specify supported request paths, counting point, redirect/retry behavior, size/time bounds, refusal status, and sanitized receipt schema.

Acceptance evidence: tests show every admitted request is counted exactly once and no path reaches a non-loopback endpoint in fixture mode.

### B — Route the CLI through the gate

Create a temporary config for the exact provider/model, route it to loopback, use isolated fixture credentials, and disable plugin/network fallback.

Acceptance evidence: actual OpenCode v1.19.3 CLI requests hit only the fixture; a two-request normal run has both calls attributed to the same invocation.

### C — Enforce request N+1 and terminal evidence

Run with ceiling 1 against a fixture that observes the CLI's first request and rejects its next LLM request before any upstream action. Verify CLI termination, counts and a complete receipt, including timeout/cancel and malformed response cases.

Acceptance evidence: upstream/fixture forwarding count is exactly N; at least one additional CLI request is refused; receipt contains no prompt, body, header or credential values.

## 8. Failure, cancellation, restart, and contention semantics

The gateway has one owner per proposer invocation, a monotonic request count and no automatic retry. Child cancellation closes the server and sockets, records an interrupted/unknown terminal, and preserves the consumed count. A crash or lost receipt is a refusal to resume or reissue; it never resets the ceiling. Concurrent proposer invocations have independent caps and receipts.

## 9. Compatibility and migration

Keep manual proposer and no-proposer optimize flows unchanged. Existing receipts remain readable; new gateway evidence is additive and versioned. OpenCode CLI version remains pinned to 1.19.3 until separately requalified.

## 10. Required tests

- Actual OpenCode CLI against loopback fixture with one and multiple requests.
- N accepted / N+1 refused before fixture forwarding; disabled or unreachable gateway refuses before CLI launch.
- Provider path, method, redirect, oversized body and timeout fail closed.
- Cancellation closes the gateway; concurrent invocations do not share counters.
- Receipt redaction tests prove no body, prompt, auth header, token or key survives serialization.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_proposer_gateway dev.harness_lab.tests.test_improve
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
```

## 12. Documentation updates

Update the Harness Lab README and S002/S005/S006 handoff so external HTTP requests are distinct from invocation counts. State that no live proposer route is unblocked by offline fixture qualification alone.

## 13. Acceptance criteria

The actual CLI's request N+1 is refused before transport, the complete sanitized receipt identifies all observed admissions/refusals, fixture mode makes zero upstream calls, and no default-route fallback is possible. Live readiness remains blocked until a separately authorized exact-route configuration check.

## 14. Stop conditions

Stop if the pinned CLI cannot be forced to use the local gateway without an unmetered side channel, if the required OpenCode Go wire protocol cannot be tested offline, or if proving containment requires changing CodeGG daemon/provider architecture. Do not use a real model to work around a failed fixture.

## 15. Closure evidence required

Record actual CLI version, fixture request sequence and receipt digest, N/N+1 forwarding counts, refusal cases, focused and full Harness Lab results, quick and hosted exact-head CI, and residual live limitations.

## 16. Handoff notes

Use loopback only and synthetic credentials. Never print the user's OpenCode auth store or request payload. Monitor test-created child/server cleanup; do not use the user's dirty fixture files as test fixtures.
