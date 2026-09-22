# Eggfetch 0.2 Adoption — Post-Closure Maintenance Corrective Addendum

Status: active; C001 ready for handoff

Repository baseline reviewed: `926a6e5bac8e3b2968679ec9e1fbf6242798ebde`

Canonical references:

- `plans/000-long-term-specification.md` §4.2, explicit ownership;
- `plans/000-long-term-specification.md` §4.7, correctness before transparent magic;
- `plans/003-planning-process.md` §7, corrective passes;
- `plans/subsystems/eggfetch-0-2-adoption-roadmap.md` — closed predecessor roadmap;
- `plans/closure/eggfetch-0-2-adoption/001-status.md` — accepted M001 closure evidence.

Related ADRs: none required. This corrective is polish-only and does not change HTTP architecture, timeout semantics, retry ownership, transport features, or public product behavior.

## 1. Why this corrective exists

Eggfetch 0.2 adoption M001 closed correctly at implementation `653e7abb`. Post-closure review identified two residual maintenance findings that were intentionally non-blocking for M001:

1. `codegg-providers` exposes `NON_STREAMING_PROVIDER_TOTAL` and `non_streaming_timeout()` through its public crate surface even though the policy is an internal transport-construction detail and all known consumers are within the crate.
2. The redirect fixtures in both `src/http_client.rs` and `crates/codegg-providers/src/provider_core.rs` combine a nonblocking listener with a request reader that treats every `read()` error as fatal. M001 closure observed one transient `WouldBlock` failure that passed immediately on rerun.

Neither finding invalidates M001's functional closure. This addendum owns a narrow maintenance pass so the historical M001 closure remains immutable.

## 2. Corrective milestone

### C001 — Provider policy API and redirect-fixture hardening

Status: ready.

Implementation plan:

- `plans/implementation/eggfetch-0-2-adoption-maintenance-corrective/001-provider-policy-api-and-redirect-fixture-hardening.md`

Primary class: polish.

Hard dependencies:

- Eggfetch 0.2 adoption M001 closed — satisfied.
- No external dependency or ADR required.

Objective:

Reduce accidental public API surface around the provider timeout policy and make the duplicated redirect fixtures deterministic under nonblocking I/O without changing production redirect, timeout, retry, TLS, or provider-stream behavior.

## 3. Ownership and invariants

This corrective must preserve:

- crates.io `eggfetch-core 0.2.0`;
- Rust 1.89 MSRV;
- HTTP/1 + Rustls/WebPKI feature/trust policy;
- no client-global provider stream total deadline;
- explicit finite timeout policy for bounded provider metadata/Responses operations;
- provider retry and visible-output no-replay ownership;
- ordinary/provider redirect-following behavior and ten-hop limit;
- all production HTTP behavior.

The provider timeout helper is an internal implementation policy, not a supported external crate API. Narrowing its visibility is permitted only after repository-wide consumer census confirms no in-repo external caller.

The redirect-fixture change is test-only. It must not alter production socket mode, connection behavior, Eggfetch configuration, or redirect policy.

## 4. Non-goals

C001 does not authorize:

- changing provider timeout durations;
- changing Responses API stream-idle/setup semantics;
- changing redirect policy or enabling `RedirectDowngradePolicy::Deny`;
- changing Eggfetch version/features;
- refactoring production HTTP clients;
- adding a generic HTTP test framework;
- introducing new dependencies;
- modifying historical M001 closure evidence;
- broad test-suite cleanup unrelated to the two recorded findings.

## 5. Verification strategy

C001 must prove:

- the timeout constant/helper are no longer exported from the public `codegg-providers` facade while all intended intra-crate call sites still compile;
- no repository consumer depends on the removed re-export;
- both redirect fixtures tolerate an intentionally induced transient `WouldBlock` condition;
- redirect success and ten-hop overflow behavior remain unchanged;
- focused provider/root HTTP tests pass repeatedly without rerun-dependent success;
- normal repository verification remains green.

The regression test should deliberately induce the I/O condition rather than depend on probabilistic repetition.

## 6. Completion definition

C001 closes when:

- internal provider timeout-policy symbols have the narrowest visibility needed by their current consumers;
- public crate re-exports for those internal symbols are removed;
- both redirect request readers use a bounded, deterministic strategy that treats transient `WouldBlock`/equivalent retryable readiness conditions correctly;
- a deterministic regression proves the previous failure mode;
- existing redirect-bound tests pass;
- no production HTTP behavior changes;
- focused and canonical verification passes;
- closure evidence records all changes and residual findings.

## 7. Status table

| Corrective | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 Provider policy API and redirect-fixture hardening | ready | `plans/implementation/eggfetch-0-2-adoption-maintenance-corrective/001-provider-policy-api-and-redirect-fixture-hardening.md` | pending `plans/closure/eggfetch-0-2-adoption-maintenance-corrective/001-status.md` | — |
