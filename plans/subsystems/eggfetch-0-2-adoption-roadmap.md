# Eggfetch 0.2 Adoption Roadmap

Status: closed; M001 closed

Repository baseline reviewed: `198524aa4ff8656928c86cf36168892532f2e29c`

Canonical long-term references:

- `plans/000-long-term-specification.md` §4.2, explicit ownership;
- `plans/000-long-term-specification.md` §4.7, correctness before transparent magic;
- `plans/000-long-term-specification.md` §11, daemon-owned provider connections and Eggpool;
- `plans/003-planning-process.md`, bounded implementation and closure rules.

Predecessor evidence:

- `plans/subsystems/http-client-consolidation-roadmap.md` — closed M001-M003 reqwest → Eggfetch adoption;
- `plans/subsystems/http-client-maintenance-consolidation-roadmap.md` — closed M001 Eggfetch 0.1.5 maintenance consolidation;
- `plans/closure/http-client-maintenance-consolidation/001-status.md` — accepted current HTTP-maintenance baseline;
- `plans/closure/dependency-security-workspace-consolidation/006-status.md` — accepted Rustls 0.23.45 security floor and WebPKI trust profile.

Related ADRs: none required. This work does not change the selected HTTP transport, retry ownership, trust model, or provider abstraction. If implementation discovers that a new cross-cutting HTTP service/facade or retry authority is required, stop and open an ADR instead of widening this workstream.

## 1. Purpose

Adopt the published crates.io `eggfetch-core 0.2.0` release while preserving CodeGG's existing HTTP ownership boundaries and correcting one material compatibility risk introduced by Eggfetch's fixed total-timeout semantics for streaming responses.

The upgrade is intentionally narrow. CodeGG already uses Eggfetch throughout the active outbound HTTP surface. This work is therefore a dependency/runtime-semantics qualification pass, not another reqwest migration.

The principal correctness question is provider streaming. Current `codegg-providers::provider_core::create_http_client()` sets:

- connect timeout: 10 seconds;
- total timeout: 60 seconds;
- 32 idle connections per host;
- 30-second idle-pool timeout;
- redirects enabled, capped at 10.

Eggfetch 0.1.7+, including 0.2.0, makes `Timeout.total` one absolute request-lifecycle deadline through response-body EOF, including `Response::bytes_stream()`. Provider adapters use `bytes_stream()` for long-running model SSE. CodeGG's agent layer separately owns a 120-second stream-setup timeout and 90-second per-event idle timeout plus visible-output retry safety.

The upgrade must therefore prove and, if necessary, correct the provider-client timeout layering so a healthy long model stream is not terminated solely because 60 seconds elapsed since dispatch.

## 2. Upstream release facts relevant to CodeGG

Published Eggfetch 0.2.0 (2026-09-22):

- keeps MSRV at Rust 1.89;
- states no intentional public Rust/Python/C/CLI/HTTPX API break from the 0.1.7 surface;
- retains the `http1`, `tls-rustls`, `json` compatibility profile used by CodeGG;
- includes the 0.1.6 redirect-downgrade policy and opt-in environment proxy support;
- includes the 0.1.7 absolute total-deadline fix and bounded resolved-route client reuse;
- includes issue #24 streaming decompression corruption fixes.

CodeGG does not currently enable Eggfetch compression features, so issue #24 is inherited but not directly exercised by the supported CodeGG feature graph. Environment proxy resolution also remains opt-in and must not become active incidentally.

## 3. Ownership boundary

CodeGG continues to own:

- provider request/response protocol and SSE parsing;
- provider logical retry budgets, attempt identity, visible-output no-replay safety, fallback and circuit policy;
- agent-level stream setup, idle timeout and cancellation;
- SSRF policy and accepted-address snapshots;
- the decision to follow or reject redirects at each caller;
- secret-safe domain error projection;
- operator/application timeout policy.

Eggfetch continues to own:

- HTTP framing and body transport;
- connection pooling;
- TLS and WebPKI trust;
- phase-aware transport timeout enforcement;
- resolved-target routing mechanics and cache reuse;
- redirect mechanics when enabled;
- request-scoped response body limits;
- transport error classification/evidence.

No second retry owner, HTTP facade, or process-global client service is introduced.

## 4. Current supported profile

At the reviewed CodeGG baseline:

- workspace MSRV is Rust 1.89;
- workspace `eggfetch-core` requirement is 0.1.5 with `default-features = false`;
- root crate enables `http1,tls-rustls,json`;
- `codegg-providers` enables `http1,tls-rustls,json`;
- EggLSP enables `http1,tls-rustls`;
- `tls-native-roots`, HTTP/2, HTTP/3, proxy, compression, cookies and multipart are not part of the approved CodeGG Eggfetch profile;
- WebFetch and URL research use `resolved_addresses()` plus request-scoped decoded-body limits;
- provider SSE paths use `Response::bytes_stream()`;
- `src/agent/provider_turn.rs` owns `STREAM_SETUP_TIMEOUT = 120s`, `STREAM_IDLE_TIMEOUT = 90s`, cancellation, bounded retries and the visible-output no-replay gate.

## 5. Target architecture after M001

After M001 closes:

- root, providers and EggLSP resolve crates.io `eggfetch-core 0.2.0`;
- the existing minimal feature/trust profile remains unchanged unless a separately justified dependency-convergence edit is required;
- provider streaming is not subject to an accidental fixed 60-second absolute body-lifetime deadline;
- non-streaming provider requests that need a finite total deadline retain one explicitly at the request/application-policy boundary;
- CodeGG's 120-second stream setup and 90-second idle timeout remain the primary long-stream liveness controls;
- provider errors caused by transport timeouts remain secret-safe and correctly classified;
- visible-output failures are never transparently replayed;
- WebFetch/research static resolved routing retains no-second-DNS behavior while benefiting from Eggfetch's bounded resolved-route reuse;
- ordinary root clients and EggLSP preserve their existing explicit timeout/redirect behavior;
- no environment proxy behavior is enabled;
- no compression feature is enabled merely to exercise issue #24;
- active dependency/provider documentation states the 0.2.0 baseline and actual timeout ownership.

## 6. Milestones

### M001 — Eggfetch 0.2.0 adoption and provider stream-deadline correctness

Status: closed.

Implementation plan:

- `plans/implementation/eggfetch-0-2-adoption/001-eggfetch-0.2.0-adoption-and-provider-stream-deadline-correctness.md`

Closure record:

- `plans/closure/eggfetch-0-2-adoption/001-status.md` (implementation `653e7abb`)

Class: infrastructure + invariant.

Objective:

Adopt Eggfetch 0.2.0, qualify the changed total-timeout semantics against CodeGG's long-running provider streams, correct timeout layering if required, preserve pinned-routing/retry/security behavior, and reconcile the dependency graph and active documentation.

Hard dependencies:

- closed original HTTP-client consolidation;
- closed Eggfetch 0.1.5 maintenance consolidation;
- Rust 1.89 workspace baseline;
- published crates.io Eggfetch 0.2.0.

No external blocker is known at plan creation.

## 7. Deferred/non-goal work

The following are explicitly outside M001 unless required to preserve existing behavior:

- enabling Eggfetch HTTP/2, HTTP/3, proxy, compression, native roots, cookies or multipart;
- adopting Eggfetch logical retries for provider/model requests;
- moving provider SSE parsing into Eggfetch;
- redesigning provider retry/fallback/circuit policy;
- changing the user-visible timeout model;
- broad dependency modernization;
- converting all HTTP consumers to `standard-http1` or another new feature recipe;
- changing WebFetch/research SSRF policy;
- enabling environment proxy resolution;
- broad redirect-policy hardening.

Eggfetch 0.1.6 introduced `RedirectDowngradePolicy::Deny`. Artifact-acquisition callers such as EggLSP/plugin installation may merit a later HTTPS→HTTP downgrade-hardening pass, but M001 must not silently change redirect policy while qualifying the version upgrade. Register separate work only if post-M001 evidence justifies it.

## 8. Verification strategy

M001 must use deterministic local fixtures and dependency inspection. Required evidence includes:

- published crates.io `eggfetch-core 0.2.0` metadata and feature census;
- a local streaming fixture that demonstrates the 0.2.0 total-deadline behavior with chunks/events continuing beyond a deliberately short total deadline;
- a corresponding CodeGG provider-policy fixture proving the selected correction permits an active stream beyond that short total interval while still enforcing setup/idle/cancellation authority;
- non-streaming provider total-timeout coverage after any timeout-layering change;
- visible-output timeout/interruption proof that no transparent replay occurs;
- pre-visible-output transient timeout proof that bounded retry behavior remains correct;
- WebFetch/research resolved-address/no-second-DNS tests;
- response-size enforcement tests;
- provider SSE, Eggpool and cancellation suites;
- ordinary-client redirect-bound tests;
- EggLSP redirect/raw-byte/archive-safety tests;
- dependency/feature graph evidence proving no unintended Eggfetch features;
- targeted lockfile-diff review;
- `cargo audit`, strict all-feature Clippy and `scripts/verify.sh quick`.

No public provider/network endpoint is required for committed tests.

## 9. Completion definition

This workstream closes when M001 has accepted closure evidence proving:

- CodeGG resolves crates.io Eggfetch 0.2.0 on Rust 1.89;
- the intended HTTP/1 + Rustls/WebPKI + JSON-where-needed profile remains intact;
- provider streams are governed by explicit CodeGG liveness policy rather than an accidental 60-second absolute transport lifetime;
- bounded non-streaming provider operations still have finite total deadlines;
- retries do not multiply below CodeGG's retry budget;
- visible-output streams are not replayed;
- pinned routing, body limits, redirects, cancellation and EggLSP behavior remain compatible;
- dependency/security state is reconciled without unrelated churn;
- active docs/registry are accurate;
- no unresolved medium-or-higher finding remains.

## 10. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 Eggfetch 0.2.0 adoption and provider stream-deadline correctness | closed | `plans/implementation/eggfetch-0-2-adoption/001-eggfetch-0.2.0-adoption-and-provider-stream-deadline-correctness.md` | `plans/closure/eggfetch-0-2-adoption/001-status.md` | — |
