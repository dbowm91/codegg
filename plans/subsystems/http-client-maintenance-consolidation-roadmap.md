# HTTP Client Maintenance Consolidation Roadmap

Status: closed; M001 is closed with accepted closure

Canonical long-term references:

- `plans/000-long-term-specification.md` §4.2, explicit ownership;
- `plans/000-long-term-specification.md` §4.7, correctness before transparent magic;
- `plans/000-long-term-specification.md` §11, daemon-owned provider connections and Eggpool;
- `plans/003-planning-process.md`, bounded implementation and closure rules.

Related completed work:

- `plans/subsystems/http-client-consolidation-roadmap.md` — closed M001-M003 migration from reqwest to Eggfetch;
- `plans/closure/http-client-consolidation/003-status.md` — accepted final reqwest-retirement evidence;
- `plans/subsystems/dependency-security-workspace-consolidation-roadmap.md` — dependency graph convergence and Rust 1.89 baseline.

Related ADRs: none required. This roadmap does not change the selected HTTP transport or introduce a new cross-workspace HTTP service/trait. If implementation discovers that such an abstraction is required, stop and open an ADR rather than expanding this maintenance workstream.

## 1. Purpose and ownership boundary

The original HTTP-client consolidation is complete: CodeGG-owned outbound HTTP uses `eggfetch-core 0.1.4`, direct reqwest ownership is retired, `codegg-core` remains transport-neutral, and security-sensitive WebFetch/research/MCP routes preserve validated-address/static-routing invariants.

This follow-on owns a smaller maintenance-consolidation pass made possible by current `eggfetch-core 0.1.5`. Its purpose is to reduce CodeGG-owned HTTP plumbing where Eggfetch now already owns the same generic transport concern, while preserving CodeGG's application policy boundaries.

This workstream owns:

- adopting the published crates.io `eggfetch-core 0.1.5` release under the existing minimal feature profile;
- transferring generic response-size enforcement from CodeGG-owned streaming loops to Eggfetch's request/client decoded-body limit where behavior is equivalent;
- removing duplicate bounded-body collection code once no longer needed;
- consolidating repeated ordinary-client construction policy only at the builder-policy level, without hiding Eggfetch behind a new request/response abstraction;
- measuring dependency/artifact effects truthfully rather than assuming a size win.

CodeGG continues to own:

- SSRF hostname/IP allow/deny policy and DNS revalidation;
- the decision to attach accepted address snapshots through `resolved_addresses()`;
- provider protocol serialization/parsing, SSE/domain parsing and model semantics;
- logical/provider retry budgets, side-effect safety, `Retry-After`, circuit breakers and fallback policy;
- cancellation and higher-level workflow ownership;
- secret-safe domain error projection.

Eggfetch owns generic HTTP framing, connection pooling, TLS, phase-aware transport timeouts, request routing mechanics, response-body enforcement, and transport error evidence.

## 2. Work classification

### Invariants

- validated untrusted destinations cannot silently fall back to a second DNS lookup after CodeGG accepts an address snapshot;
- logical URL/Host/TLS identity remains distinct from the physical resolved destination;
- provider retries remain CodeGG-controlled and do not multiply invisibly through a newly enabled Eggfetch retry policy;
- transport errors crossing provider/domain boundaries remain secret-safe;
- security-sensitive clients remain redirect-disabled where currently required;
- `codegg-core` remains transport-neutral.

### Capabilities

No new user-visible capability is introduced. Existing HTTP-backed capabilities must remain behaviorally compatible.

### Infrastructure

- `eggfetch-core 0.1.5` dependency baseline;
- Eggfetch-owned decoded-body limits for bounded response collection;
- a narrow ordinary-client builder-policy seam for repeated root-client defaults, if current call-site census confirms the shared contract.

### Polish

- deletion of duplicate body-accumulation code and tests that merely re-prove Eggfetch internals;
- reduction of repeated redirect/client setup literals;
- active documentation/version reconciliation;
- optional artifact/dependency measurements.

## 3. Non-goals

This roadmap does not authorize:

- reopening or rewriting the closed reqwest-migration history;
- adding a CodeGG HTTP facade with custom request/response types;
- adding a new crate whose purpose is wrapping Eggfetch;
- process-global HTTP singletons or service locators;
- moving SSRF validation policy into Eggfetch;
- enabling `tls-native-roots`, HTTP/2, HTTP/3, proxy, cookie, multipart, compression or other new Eggfetch features merely because they exist;
- enabling Eggfetch automatic retries for provider/model POST requests;
- changing provider retry budgets, side-effect reconciliation, circuit breakers or fallback order;
- replacing provider/MCP SSE parsers with transport-layer parsing;
- changing EggLSP archive/extraction policy;
- redesigning search/research source behavior;
- treating `send_detailed()` or typed DNS provenance as mandatory without a concrete consumer benefit;
- new CI lanes, public-network tests, dependency bots or permanent binary-size gates;
- unrelated dependency modernization.

## 4. Current state

At repository baseline `0349cb2cdcc638958b034cc6187eb73c39f55b6f`:

- workspace MSRV is Rust 1.89;
- `[workspace.dependencies]` pins `eggfetch-core = 0.1.4` with `default-features = false`;
- the root package enables `http1,tls-rustls,json`;
- EggLSP enables `http1,tls-rustls` without JSON;
- provider clients use the shared `provider_core::create_http_client()` policy with 10s connect, 60s total, bounded idle pooling and ten-hop redirects;
- direct reqwest ownership is absent from active CodeGG transport paths;
- CodeGG-owned DashMap is already on major version 6, so the historical DashMap 5/6 duplicate noted during earlier closure is no longer a current Eggfetch consolidation target.

Two remaining maintenance patterns are material.

First, `src/security/untrusted_http.rs::read_body_bounded()` manually checks `Content-Length`, opens `Response::bytes_stream()`, counts chunks, aborts at a CodeGG-owned byte limit and maps stream errors. WebFetch and direct URL research call this helper. The Eggpool probe in `crates/codegg-providers/src/eggpool.rs` contains a second manual bounded-body streaming accumulator with cancellation around each chunk.

Current Eggfetch already exposes `RequestBuilder::max_decoded_body_size()` and the corresponding client setting. The limit applies to ordinary identity bodies and decoded compressed bodies, applies to streaming and buffered consumption, remains authoritative when `Content-Length` is absent or incorrect, and terminates with `Error::DecodedBodyTooLarge`. This is the generic transport/resource invariant CodeGG's manual loops are currently duplicating.

Second, ordinary root clients repeat the same explicit Eggfetch policy across many call sites: bounded owner-specific timeout, `follow_redirects(true)`, and `max_redirects(10)`, followed by call-site-specific user agent/auth/header configuration. Provider clients are already centralized separately and must remain so; security-sensitive pinned routes intentionally use different redirect policy.

`eggfetch-core 0.1.5` raises its MSRV to Rust 1.89, matching CodeGG, and adds opt-in detailed native request failures with typed `Dns`, `ConnectionRefused`, and generic `Connect` provenance while preserving the ordinary `Error` API. Its direct dependency and feature layout remains compatible with CodeGG's existing narrow feature selections. Detailed failure adoption is useful only where CodeGG can consume the structured evidence without widening domain APIs or exposing secrets.

## 5. Target architecture

After M001 closes:

- root, providers and EggLSP resolve crates.io `eggfetch-core 0.1.5` under the same intentionally narrow feature sets they use today;
- CodeGG does not maintain a generic chunk-counting loop solely to enforce maximum HTTP response bytes when Eggfetch's decoded-body limit provides the same safety property;
- WebFetch and direct URL research set the body bound on the actual request that also carries the accepted resolved-address snapshot;
- Eggpool retains its stable redacted `Oversized`/`Cancelled` result contract but uses Eggfetch's body bound rather than a second accumulator;
- the no-second-DNS/static-routing tests remain attached to actual CodeGG owners even if `security::untrusted_http` becomes unnecessary and is removed;
- ordinary root HTTP clients share only transparent construction policy where useful; call sites still use `eggfetch_core::Client`/`RequestBuilder` directly and own their auth, user agent, endpoint and parsing behavior;
- provider `create_http_client()` remains its own policy boundary and is not folded into a root helper;
- EggLSP remains independently configured because its downloader lifecycle and feature surface differ from root/provider clients;
- Eggfetch retries remain disabled unless already explicitly configured by an owner;
- `send_detailed()` remains deferred unless implementation identifies a narrow current caller whose behavior becomes simpler or more correct by consuming typed network provenance.

## 6. Dependency graph

```text
Closed HTTP-client consolidation M001-M003
        |
        +-- CodeGG Rust 1.89 baseline
        |
        +-- published eggfetch-core 0.1.5
        |
        v
M001 Eggfetch 0.1.5 maintenance consolidation
        |
        v
closure evidence
```

Dependencies:

- closed HTTP-client consolidation: **hard**, satisfied;
- Rust 1.89 workspace baseline: **hard**, satisfied;
- published crates.io `eggfetch-core 0.1.5`: **hard**; implementation must verify the packaged artifact with Cargo before editing;
- provider retry architecture: **interface**, stable and closed; M001 must not change it.

## 7. Milestones

### M001 — Eggfetch 0.1.5 policy and body ownership consolidation

Class: infrastructure + invariant + polish.

Implementation plan: `plans/implementation/http-client-maintenance-consolidation/001-eggfetch-0.1.5-policy-and-body-ownership.md`

Objective:

Adopt `eggfetch-core 0.1.5`, delete CodeGG-owned generic bounded-body accumulation where Eggfetch can enforce the same limit, centralize only genuinely shared ordinary-client construction policy, and requalify transport/security behavior without broadening the feature graph or retry ownership.

Dependencies:

- original HTTP-client consolidation closed;
- Eggfetch 0.1.5 packaged artifact available;
- no unresolved ADR required.

Deliverable boundary:

One coherent maintenance pass covering dependency version, bounded response ownership, repeated ordinary-client policy, focused regression tests, active documentation, and closure evidence.

User or operator value:

No intended behavior change. The value is lower CodeGG maintenance/security surface and one fewer implementation of generic HTTP body limiting, with transport hardening inherited from Eggfetch 0.1.5.

Exit conditions:

- 0.1.5 resolves from crates.io with existing feature/trust policy;
- manual generic bounded-body loops are removed or retained only where a caller-specific semantic cannot be represented by Eggfetch;
- oversized responses still fail closed on both declared and chunked/unknown-length bodies;
- cancellation remains prompt for Eggpool and other cancellable owners;
- validated-address/static-routing invariants remain green;
- ordinary redirect/timeout semantics remain explicit and bounded;
- no hidden transport retry layer is enabled;
- active docs are truthful;
- broad verification passes and closure contains no unresolved medium-or-higher finding.

Deferred work:

- typed `send_detailed()` adoption unless a current narrow consumer demonstrates concrete value;
- any new protocol feature or trust-root mode;
- transport-level retries;
- binary-size optimization unrelated to demonstrated duplication.

## 8. Cross-cutting requirements

### Storage and migration

No database, durable state, configuration schema or migration is expected.

### Protocol and compatibility

No wire protocol, provider payload, MCP framing, search/research output, SDK protocol or EggLSP archive format changes are authorized.

### Security and authorization

SSRF policy remains CodeGG-owned. Static resolved routing must continue binding the physical connection to the address set accepted by CodeGG while preserving logical Host/TLS identity. Body limits must remain fail-closed. Secret-bearing provider URLs/errors must not cross redaction boundaries.

### Concurrency, cancellation, and recovery

Eggfetch response limits must release/drop the response transport when exceeded. Eggpool cancellation must still be able to interrupt body collection rather than waiting for the full response. No background retry/recovery owner is introduced.

### Observability and audit

Existing domain error classes remain authoritative. Typed detailed network provenance may be evaluated, but must not force a public/domain taxonomy expansion merely because Eggfetch exposes more detail.

### Performance and resource use

The principal resource goal is deletion of duplicate buffering logic, not a promised binary reduction. Closure should record dependency graph and, when reproducible on the same host/profile, release artifact bytes before/after as descriptive evidence only.

### Documentation and operations

Update active dependency/security/provider documentation where it names 0.1.4 or describes CodeGG as owning generic bounded-body accumulation. Preserve historical plans/closures as historical evidence.

## 9. Verification strategy

Use deterministic loopback fixtures. Required evidence includes:

- oversized declared-length response rejection;
- oversized chunked/unknown-length response rejection through Eggfetch's configured limit;
- exact/under-limit body success;
- Eggpool cancellation during body collection;
- WebFetch/research accepted-address routing with no DNS fallback and preserved Host/TLS identity;
- provider client timeout/pool/redirect regressions;
- ordinary root-client redirect bound tests where policy is consolidated;
- EggLSP focused downloader tests to prove the dependency bump does not alter download semantics;
- dependency/feature census proving no new Eggfetch feature or reqwest edge;
- normal workspace formatting, Clippy and quick verification.

No public network is required for committed tests.

## 10. Risks and decision points

1. **Body-limit semantic mismatch.** If Eggfetch's packaged 0.1.5 body limit does not enforce the same effective bound for identity/chunked bodies, retain the CodeGG helper and record the gap rather than weakening safety.
2. **Error-shape coupling.** `DecodedBodyTooLarge` does not carry CodeGG's current `observed` byte count. Preserve the stable domain failure class; do not keep a second accumulator solely for a non-contract diagnostic unless evidence shows a consumer depends on it.
3. **Cancellation drift.** Replacing an explicit chunk loop with `Response::bytes()` must remain interruptible by the owner's `tokio::select!`/future cancellation.
4. **Over-abstraction.** A small builder-policy helper can reduce repeated literals; a new HTTP service/facade would increase coupling and requires an ADR.
5. **Retry multiplication.** Eggfetch retry support must remain disabled for provider/model traffic unless a future plan integrates it with CodeGG's unified attempt budget.
6. **False footprint claims.** 0.1.5 has substantially the same direct feature/dependency shape as 0.1.4; any artifact change must be measured rather than inferred.

## 11. Completion definition

This roadmap closes when M001 has an accepted closure record showing that CodeGG consumes crates.io Eggfetch 0.1.5 with unchanged minimal feature/trust policy, duplicate generic body-limit maintenance has been removed where safe, ordinary client policy is no more abstract than necessary, all security/retry/cancellation invariants remain green, active documentation is reconciled, and no unresolved medium-or-higher finding remains.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 Eggfetch 0.1.5 policy and body ownership consolidation | closed | `plans/implementation/http-client-maintenance-consolidation/001-eggfetch-0.1.5-policy-and-body-ownership.md` | `plans/closure/http-client-maintenance-consolidation/001-status.md` | — |
