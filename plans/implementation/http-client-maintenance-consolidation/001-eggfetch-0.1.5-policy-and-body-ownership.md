# HTTP Client Maintenance Consolidation M001 — Eggfetch 0.1.5 Policy and Body Ownership

Status: ready for handoff

Repository baseline reviewed: `0349cb2cdcc638958b034cc6187eb73c39f55b6f`

Planning-only predecessor commit:

- `1820a825` — adds `plans/subsystems/http-client-maintenance-consolidation-roadmap.md`; no production-code delta.

Source subsystem roadmap:

- `plans/subsystems/http-client-maintenance-consolidation-roadmap.md#7-milestones`

Related completed roadmap and closure evidence:

- `plans/subsystems/http-client-consolidation-roadmap.md`
- `plans/closure/http-client-consolidation/001-status.md`
- `plans/closure/http-client-consolidation/002-status.md`
- `plans/closure/http-client-consolidation/003-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.2 — explicit ownership;
- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/000-long-term-specification.md` §11 — daemon-owned provider connections and Eggpool;
- `plans/003-planning-process.md` — bounded handoff and closure rules.

Applicable ADRs: none. If the work requires a generic CodeGG HTTP service/trait, new retry owner, or new transport/trust policy, stop rather than inventing that architecture inside this milestone.

Predecessors: HTTP-client consolidation M001-M003 are closed. CodeGG's Rust 1.89 workspace baseline already matches Eggfetch 0.1.5's MSRV.

Primary class: infrastructure + invariant + polish.

## 1. Objective

Adopt the published crates.io `eggfetch-core 0.1.5` release and use its existing native response-limit/transport surface to remove CodeGG-owned generic HTTP body-accumulation maintenance where semantics match, while consolidating repeated ordinary-client redirect construction policy without introducing a second HTTP abstraction.

The intended externally visible behavior is unchanged. The success metric is simpler ownership: Eggfetch owns generic response-size enforcement and HTTP transport mechanics; CodeGG owns SSRF policy, provider/domain protocol semantics, logical retries, cancellation policy and secret-safe error projection.

This milestone must not claim a binary-size reduction unless measured on CodeGG itself. The large dependency win — retirement of direct reqwest ownership — is already complete.

## 2. Why this milestone is ready

The required interfaces and dependencies are stable:

- CodeGG already uses `eggfetch-core 0.1.4` throughout root/provider/EggLSP production HTTP paths;
- no active CodeGG-owned reqwest transport remains;
- the root/provider feature profile is already explicit and minimal rather than using Eggfetch defaults;
- WebFetch, direct URL research and remote MCP already use request-scoped accepted-address routing where required;
- provider retry taxonomy/budgets and side-effect reconciliation are closed architecture and must remain above the transport;
- CodeGG already uses Rust 1.89;
- Eggfetch 0.1.5 source exposes the same narrow feature families plus public `ClientBuilder`, request-level `max_decoded_body_size()`, ordinary `Error`, opt-in `RequestFailure`, and typed `NetworkFailureKind`.

Implementation must still verify the actual published 0.1.5 package with Cargo before editing. Source-repository evidence is not a substitute for the crates.io artifact.

## 3. Current implementation evidence

### 3.1 Dependency and feature ownership

Root `Cargo.toml` currently declares:

```toml
eggfetch-core = { version = "0.1.4", default-features = false }
```

The root package enables `http1`, `tls-rustls`, and `json`. EggLSP enables `http1` and `tls-rustls`. Providers use the workspace dependency and existing feature union. `tls-native-roots` is intentionally not selected.

The workspace MSRV is `1.89`, matching the new Eggfetch 0.1.5 floor.

### 3.2 Provider client policy is already correctly centralized

`crates/codegg-providers/src/provider_core.rs::create_http_client()` owns provider HTTP construction with:

- 10-second connect timeout;
- 60-second total timeout;
- 32 idle connections per host;
- 30-second idle timeout;
- redirect following enabled with a ten-hop maximum.

Provider implementations consume this shared client. Do not replace it with a root helper.

### 3.3 Root ordinary-client policy is repeated

Many root consumers repeat the same transport defaults while retaining owner-specific timeout/user-agent/auth behavior. Current examples include:

- `src/client/sdk.rs`;
- built-in search providers (`github`, `hn_algolia`, `openalex`, `arxiv`, `pubmed`, `mojeek`, `wikipedia`, `google_news`, `duckduckgo` and related sources);
- `src/upgrade/mod.rs`;
- `src/tool/image.rs`;
- `src/mcp/auth.rs`;
- several research source clients.

The common sequence is effectively:

```rust
Client::builder()
    .timeout(owner_specific_timeout)
    .follow_redirects(true)
    .max_redirects(10)
    // owner-specific headers/user-agent/etc.
    .build()
```

This repetition is small individually but creates policy drift risk. A narrow builder-policy helper is justified only for this repeated ordinary-client contract. It must return/use Eggfetch's public builder directly rather than introducing CodeGG request/response/error wrappers.

### 3.4 CodeGG duplicates generic bounded-response enforcement

`src/security/untrusted_http.rs::read_body_bounded()` currently:

1. rejects a zero limit;
2. checks declared `Content-Length` as an early failure;
3. obtains `Response::bytes_stream()`;
4. counts each chunk;
5. errors when retained bytes would exceed the limit;
6. otherwise buffers the body into `Vec<u8>`.

WebFetch and direct URL research call this helper after sending requests pinned to CodeGG-validated address snapshots.

`crates/codegg-providers/src/eggpool.rs` independently implements another `read_body_bounded()` loop with a cancellation branch around each streamed chunk. It preserves a stable redacted probe taxonomy including `Oversized` and `Cancelled`.

Current Eggfetch exposes request/client decoded-body limits that enforce cumulative decoded/identity bytes for both buffered and streaming consumption. The stream fuses after `Error::DecodedBodyTooLarge` rather than continuing to poll the transport. The request setting overrides client defaults and remains request-scoped.

### 3.5 0.1.5 structured network failure evidence is additive, not required

Eggfetch 0.1.5 exposes `RequestBuilder::send_detailed()` / `Client::send_detailed()` returning `RequestFailure`, with optional `NetworkFailureKind::{Dns, ConnectionRefused, Connect}` while preserving the existing `Error` surface.

Current CodeGG provider conversion intentionally keeps only secret-safe transport classes. Eggpool currently distinguishes timeout and TLS but exposes generic `Unreachable` for other request failures. There is no current user/domain contract requiring DNS-vs-refused detail.

Therefore this milestone must **not** churn provider call sites merely to consume new detail. Adoption is allowed only if a narrow caller becomes materially simpler or more correct without widening domain/public APIs. Otherwise record it as deferred.

## 4. Invariants that must not regress

- `codegg-core` remains free of an HTTP-client dependency solely for transport types.
- Root/providers/EggLSP use crates.io Eggfetch, not a Git/path override.
- Existing narrow feature selection remains intentional; do not enable Eggfetch defaults.
- `tls-native-roots` remains disabled unless separately authorized.
- WebFetch/direct URL research/remote MCP validated-address behavior cannot fall back to system DNS after CodeGG accepts an address snapshot.
- Logical URL/Host/TLS identity remains authoritative while `resolved_addresses()` controls only the physical destination.
- Security-sensitive routes remain no-follow where currently required.
- Ordinary routes that currently follow redirects remain bounded to ten hops.
- Owner-specific timeout values remain unchanged unless current repository evidence proves the old value is dead/incorrect and the deviation is explicitly recorded.
- Provider logical retries remain owned by CodeGG. Do not enable Eggfetch `RetryPolicy` under provider/model POSTs.
- Provider `Retry-After`, side-effect reconciliation, circuit breaker and fallback behavior remain unchanged.
- Response-size enforcement remains fail-closed when `Content-Length` is absent, misleading, chunked, or exactly at/over the configured boundary.
- Eggpool cancellation can interrupt body collection promptly.
- Secret-bearing URLs, API keys, Authorization headers and response bodies do not become part of transport/domain diagnostics.
- No public-network-dependent test is added to the normal suite.

## 5. Scope

### In scope

- workspace/root/EggLSP/provider dependency resolution from Eggfetch 0.1.4 to 0.1.5;
- preserving current feature/trust profile;
- WebFetch and direct URL research body-limit ownership;
- `src/security/untrusted_http.rs` deletion or contraction if no production responsibility remains;
- Eggpool body-limit/cancellation ownership;
- stable mapping of `DecodedBodyTooLarge` to existing CodeGG/Eggpool domain failure categories;
- a small root ordinary-client **builder policy** helper if it cleanly replaces the repeated follow/redirect-max setup while allowing owner-specific timeout, user-agent and auth configuration;
- focused tests required to preserve body/static-routing/cancellation/redirect semantics;
- dependency/feature/reqwest census;
- active dependency/security/provider documentation reconciliation;
- descriptive release-artifact measurement where reproducible.

### Explicitly out of scope

- a generic `HttpClient` trait or CodeGG request/response facade;
- a new crate for HTTP policy;
- global client registry/singleton work;
- moving SSRF allow/deny or DNS validation into Eggfetch;
- changing MCP/provider/search/research protocols;
- provider retry redesign or enabling Eggfetch retries;
- HTTP/2, HTTP/3, proxies, cookies, multipart, compression or new TLS-root modes;
- replacing SSE parsers;
- changing EggLSP extraction/archive semantics;
- forcing `send_detailed()` adoption without a current consumer need;
- general dependency updates beyond lockfile changes causally required by Eggfetch 0.1.5;
- permanent size gates or new CI topology;
- rewriting historical 0.1.4 migration/closure records.

## 6. Required production changes

### 6.1 Dependency baseline

Before editing, verify the packaged release:

```bash
cargo info eggfetch-core@0.1.5
```

Confirm at minimum:

- version `0.1.5` is available from crates.io;
- Rust version requirement is compatible with CodeGG 1.89;
- required features `http1`, `tls-rustls`, and `json` exist;
- `default-features = false` remains usable;
- `ClientBuilder`, request-level `max_decoded_body_size()`, `Response::bytes()`, `resolved_addresses()`, and ordinary error APIs required by CodeGG are present.

Then update the workspace dependency minimum to `0.1.5` and resolve the lockfile narrowly. Do not use a Git/path patch.

Capture before/after:

```bash
cargo tree -i eggfetch-core --locked
cargo tree -e features -i eggfetch-core --locked
cargo tree -i reqwest --locked
cargo tree -d --locked
```

If 0.1.5 changes the feature/dependency graph materially beyond the researched source tree, classify the delta before proceeding.

### 6.2 WebFetch and direct URL research — move the byte bound onto the request

For each security-sensitive request that currently calls `read_body_bounded()`:

1. retain existing URL validation/revalidation;
2. retain the original logical URL;
3. retain `.resolved_addresses(accepted_socket_addresses)` on the actual request;
4. retain redirects disabled;
5. set `.max_decoded_body_size(existing_limit)` on the same request before send;
6. after status/content-type handling that must inspect response metadata, collect with Eggfetch `Response::bytes()` (or equivalent single-consumption API) rather than a CodeGG-owned stream accumulator;
7. map `eggfetch_core::Error::DecodedBodyTooLarge` to the existing owner-domain oversized/body-limit error without exposing raw transport text.

Do not weaken the limit merely because Eggfetch names it "decoded". Under the approved no-compression feature profile the bound applies to ordinary identity bodies; it must still be proven with loopback chunked/unknown-length fixtures.

The current helper's exact `observed` byte count is not a security invariant. Do not retain a second accumulator solely to preserve this diagnostic unless implementation finds a tested external contract that depends on it.

After migration, inspect `src/security/untrusted_http.rs`:

- if it has no production responsibility, delete it and remove its module declaration;
- move any still-unique static-routing/no-second-DNS regression to the actual WebFetch/research owner, unless equivalent owner-level coverage already exists;
- do not retain a dead helper module only to keep historical tests alive.

### 6.3 Eggpool probe — preserve cancellation and stable reason codes without a second accumulator

In `crates/codegg-providers/src/eggpool.rs`:

- set `max_decoded_body_size(options.response_byte_limit)` on the `/models` request;
- retaining the explicit `Content-Length` early check is acceptable if it preserves a useful immediate `Oversized` classification, but it must no longer be the authoritative bound;
- replace the manual chunk-counting `read_body_bounded()` loop with Eggfetch body collection under the existing cancellation owner;
- use `tokio::select!`/the existing cancellation helper around `response.bytes()` so cancellation wins without waiting for complete body collection;
- map `Error::DecodedBodyTooLarge` specifically to `EggpoolProbeReasonCode::Oversized` rather than generic `Unreachable`;
- preserve `Timeout`, TLS, auth, redirect, invalid JSON, unsupported, empty, model-count and model-string limits exactly;
- preserve redaction: no endpoint/API key/body material is added to `EggpoolProbeError`.

Once the replacement is proven, delete the second generic accumulator.

### 6.4 Ordinary root-client construction — consolidate policy, not transport types

Perform a fresh census of root `Client::builder()` / `eggfetch_core::Client::builder()` call sites.

Create one private builder-policy seam only if the current repeated contract still holds. The preferred shape is conceptually:

```rust
pub(crate) fn ordinary_http_client_builder(
    timeout: eggfetch_core::Timeout,
) -> eggfetch_core::ClientBuilder {
    eggfetch_core::Client::builder()
        .timeout(timeout)
        .follow_redirects(true)
        .max_redirects(10)
}
```

The exact module/name may follow current repository organization. Required properties are more important than the example:

- returns Eggfetch's public builder directly;
- owns only repeated ordinary redirect policy plus the caller-supplied timeout;
- callers remain free to set user agent/default headers/auth before `.build()`;
- no custom request/response/error type;
- no global singleton;
- no hidden retry policy;
- no security-sensitive pinned/no-follow caller is forced through this helper;
- provider `create_http_client()` and EggLSP downloader construction stay independent.

Use the helper where at least the repeated redirect contract is genuinely the same. Do not force one-off clients into it merely to maximize call count.

If the census shows that timeout/redirect semantics are materially different enough that a helper would obscure ownership, skip this work package and record that repeated explicit policy is clearer than abstraction. That is an acceptable closure outcome; do not create a helper for its own sake.

### 6.5 Detailed failure API evaluation — evidence gate only

After upgrading to 0.1.5, inspect the existing request-error projection sites, especially provider and Eggpool paths.

Do not mechanically switch them to `send_detailed()`.

Adopt the detailed API only if all of the following are true for a narrow caller:

- typed DNS/refused/connect provenance changes a currently useful CodeGG decision or diagnostic;
- the caller can use it without exposing secret URL/error text;
- it does not require changing every provider request call site;
- it does not alter retryability or public/domain error contracts without separate authorization;
- focused tests demonstrate the improvement.

Otherwise leave ordinary `send()`/`Error::kind()` behavior unchanged and record typed detailed failures as deferred upstream capability.

### 6.6 Documentation and static ownership checks

Update current-state documentation that names the Eggfetch version or attributes generic bounded-body accumulation to CodeGG. Expected candidates include:

- `docs/dependency-maintenance.md`;
- `architecture/security.md`;
- `architecture/provider.md`;
- other active architecture/current-state docs found by census.

Historical implementation/closure records that truthfully describe the 0.1.4 migration remain untouched.

Do not add a new permanent dependency scanner. Existing source/network guards should continue recognizing Eggfetch as network-capable code.

## 7. Ordered work packages

### Work package A — Qualify and adopt the packaged 0.1.5 dependency

Intent: move the supported Eggfetch floor to the current release without feature/trust drift.

Required changes:

- run `cargo info eggfetch-core@0.1.5`;
- update workspace dependency minimum to 0.1.5;
- narrowly refresh `Cargo.lock`;
- inspect Eggfetch reverse/feature trees and lockfile diff;
- confirm Rust 1.89 compatibility.

Acceptance evidence:

- crates.io 0.1.5 resolved;
- no Git/path override;
- no new Eggfetch feature enabled;
- no direct/transitive reqwest reintroduced by this change;
- lockfile churn is attributable to the version update.

### Work package B — Transfer untrusted response-size enforcement to Eggfetch

Intent: delete the root generic body accumulator while preserving security invariants.

Required changes:

- configure request-level byte limits on WebFetch/direct URL research;
- consume bounded responses through Eggfetch;
- preserve pinned resolved targets, no redirects, status/content-type behavior and owner errors;
- delete/contract `security::untrusted_http` after caller migration;
- retain/move only owner-level regressions that still prove CodeGG policy.

Acceptance evidence:

- exact-limit and under-limit bodies succeed;
- over-limit declared-length and chunked/no-length bodies fail closed;
- no-second-DNS/preserved Host regression remains green;
- no production `read_body_bounded` helper remains unless a documented caller-specific need survives.

### Work package C — Transfer Eggpool body-size enforcement while preserving cancellation

Intent: remove the second accumulator without collapsing Eggpool's domain taxonomy.

Required changes:

- request-level Eggfetch limit;
- cancellable `Response::bytes()` collection;
- specific `DecodedBodyTooLarge -> Oversized` mapping;
- delete manual chunk loop;
- preserve other reason codes and redaction.

Acceptance evidence:

- oversized `Content-Length` response -> `Oversized`;
- oversized chunked/unknown-length response -> `Oversized`;
- cancellation while body is stalled/incomplete -> `Cancelled` promptly;
- timeout/TLS/unreachable classification remains unchanged where detailed evidence is not adopted.

### Work package D — Consolidate ordinary root builder policy where transparent

Intent: reduce repeated redirect-policy literals without creating an HTTP facade.

Required changes:

- census current ordinary client builders;
- introduce one private `ClientBuilder` policy helper only if the shared contract is still real;
- migrate compatible root callers;
- leave pinned/no-follow, provider and EggLSP owners independent;
- keep owner-specific timeout/user-agent/auth at the call site.

Acceptance evidence:

- ordinary redirect-following clients still follow redirects and stop after ten hops;
- distinct timeout values remain distinct;
- invalid auth/header construction behavior remains unchanged;
- helper contains no retry, endpoint, auth, parsing or response logic.

### Work package E — Evaluate but do not force structured network failure detail

Intent: prevent future string parsing without causing broad churn.

Required changes:

- inspect current consumers after the dependency upgrade;
- adopt `send_detailed()` only under the evidence gate in §6.5;
- otherwise make no production change and record the deferral.

Acceptance evidence:

- no domain/retry behavior changes merely because new transport detail exists;
- any adopted use has focused secret-safety and classification tests.

### Work package F — Reconcile docs, dependency evidence and closure readiness

Intent: prove the maintenance pass reduced ownership without hidden regressions.

Required changes:

- update active current-state docs;
- run dependency/source census;
- run focused and broad tests;
- optionally measure same-profile release artifact bytes before/after;
- prepare closure record at `plans/closure/http-client-maintenance-consolidation/001-status.md`.

Acceptance evidence:

- all required commands that are applicable are recorded truthfully in closure;
- no new CI lane or permanent size gate;
- unresolved findings classified by severity.

## 8. Failure, cancellation, restart, and contention semantics

This milestone is HTTP transport maintenance; it introduces no durable state or restart protocol.

Failure semantics:

- request construction failures remain local/non-retryable unless existing owner policy says otherwise;
- `DecodedBodyTooLarge` must become the owner's existing body-limit/oversized class, not `Unreachable`;
- Eggfetch transport error text must not leak secret-bearing URLs or credentials into provider/domain errors;
- if the 0.1.5 update causes a request/TLS/redirect semantic regression that cannot be fixed narrowly, stop rather than widening transport features.

Cancellation semantics:

- dropping WebFetch/research futures continues to cancel work naturally;
- Eggpool's explicit cancellation token must race both send and body collection;
- a cancellation branch must drop the response/body future and release transport resources;
- no detached background body collection is allowed.

Contention semantics:

- no new global singleton/shared mutable HTTP policy state is introduced;
- provider connection pooling remains Eggfetch/provider-client owned;
- root builder-policy consolidation is pure construction policy, not a shared mutable client registry.

Restart semantics: none beyond existing owning task/process behavior.

## 9. Compatibility and migration

No user configuration, database schema, protocol version or persisted record changes are expected.

Compatibility expectations:

- `eggfetch-core 0.1.5` is a pre-1.0 dependency update but CodeGG's Rust floor already matches its new MSRV;
- existing feature/trust selection is retained explicitly;
- body-limit error wording may become less specific because Eggfetch does not expose the manual helper's exact observed byte count; preserve failure category/safety rather than diagnostic implementation detail;
- ordinary client callers still receive raw Eggfetch types and retain their local behavior;
- provider public/domain errors and retry disposition remain compatible;
- historical planning records retain 0.1.4 references as historical truth.

Rollback is a normal code/manifest/lockfile revert if focused compatibility verification exposes an upstream regression.

## 10. Required tests

### Focused unit tests

Retain/add deterministic tests for:

- body exactly at limit;
- body below limit;
- declared length above limit;
- chunked/unknown-length body crossing limit;
- Eggpool `DecodedBodyTooLarge -> Oversized`;
- Eggpool cancellation during body collection;
- ordinary builder helper follows redirects and enforces the ten-hop cap if the helper is introduced;
- owner-specific timeout configuration remains represented where current tests expose it.

### Integration tests

- WebFetch static resolved-address request reaches the accepted socket while preserving logical Host;
- direct URL research retains accepted-address behavior;
- remote MCP static-routing tests remain green even though it is not otherwise changed;
- provider client/model/stream suites remain green;
- EggLSP downloader suite remains green after the dependency bump.

### Restart and recovery tests

Not applicable; no durable/restart ownership changes.

### Contention and cancellation tests

- Eggpool cancellation while response body is pending/stalled;
- existing provider stream/cancellation tests remain green.

### Security and negative tests

- over-limit identity/chunked bodies fail closed;
- pinned security-sensitive requests do not perform fallback DNS;
- logical Host/TLS identity remains distinct from physical destination;
- no auth/URL secrets appear in changed error mappings;
- security-sensitive request paths do not inherit the ordinary redirect helper.

### Migration and compatibility tests

- current workspace resolves Eggfetch 0.1.5 with expected features;
- no CodeGG-owned reqwest dependency/use returns;
- active docs/current-state code do not incorrectly claim 0.1.4 after the upgrade, excluding historical plan/closure evidence.

## 11. Required verification commands

Use actual current target/test names where the repository has changed. Narrow tests first. Expected command set:

```bash
cargo info eggfetch-core@0.1.5

cargo tree -i eggfetch-core --locked
cargo tree -e features -i eggfetch-core --locked
cargo tree -i reqwest --locked
cargo tree -d --locked

cargo test --lib security:: --locked -- --test-threads=1
cargo test --lib tool::webfetch --locked -- --test-threads=1
cargo test --lib research --locked -- --test-threads=1
cargo test -p codegg-providers --lib --locked -- --test-threads=1
cargo test --lib mcp::remote --locked -- --test-threads=1
cargo test -p egglsp --all-features --locked -- --test-threads=1

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
scripts/verify.sh quick
```

If `cargo test --lib security::` is not a valid/current filter after deleting the helper module, replace it with the concrete owning WebFetch/research security tests and record the exact commands actually run.

Static/census evidence:

```bash
rg -n 'read_body_bounded' src crates
rg -n 'max_redirects\(10\)' src crates
rg -n '\breqwest\b' Cargo.toml crates/*/Cargo.toml src crates \
  --glob '!plans/**'
rg -n 'eggfetch-core.*0\.1\.4|eggfetch-core 0\.1\.4' \
  Cargo.toml crates architecture docs .opencode \
  --glob '!plans/**'
```

Interpret searches semantically. `bytes_stream()` remains legitimate for SSE/streaming protocols; this plan does not prohibit streaming. `max_redirects(10)` may remain in provider/EggLSP/security-specific owners when intentional.

Optional descriptive footprint evidence, only if reproducible under one host/toolchain/profile:

```bash
cargo build --release --locked
# record byte size of the same final binary before/after; no pass/fail threshold
```

Do not add `cargo-bloat` or another analysis dependency to the repository solely for closure.

## 12. Documentation updates

At minimum inspect and update where current-state text is affected:

- `docs/dependency-maintenance.md` — supported Eggfetch floor/profile;
- `architecture/security.md` — owner of response-size enforcement and retained SSRF/static-routing policy;
- `architecture/provider.md` — 0.1.5 baseline only if version is stated; provider client/retry ownership remains CodeGG/provider-owned;
- active contributor/upgrade guidance that names 0.1.4 as the current dependency.

Do not edit the closed HTTP-client-consolidation roadmap or closure records merely to replace historical `0.1.4` text.

## 13. Acceptance criteria

- Workspace-supported builds resolve crates.io `eggfetch-core 0.1.5`.
- Root/providers/EggLSP retain the existing minimal feature/trust profile; no native roots or new HTTP protocol feature is introduced.
- Rust 1.89 remains the CodeGG MSRV and satisfies Eggfetch.
- WebFetch and direct URL research enforce their existing response byte limits through Eggfetch on the same request that owns static resolved routing.
- Oversized responses fail closed with or without trustworthy `Content-Length`.
- The generic root `read_body_bounded()` accumulator is removed unless closure documents a concrete caller-specific semantic Eggfetch cannot provide.
- Eggpool no longer manually accumulates/counts response chunks solely for byte limiting; its `Oversized` and `Cancelled` contracts remain correct.
- No second hidden retry owner is enabled beneath CodeGG provider retry budgets.
- SSRF validation and no-second-DNS behavior remain CodeGG-owned and tested.
- Ordinary root client policy is consolidated only if it remains transparent; no custom HTTP facade/service is introduced.
- `send_detailed()` is either adopted narrowly with evidence or explicitly deferred without penalty.
- No CodeGG-owned reqwest dependency/use is reintroduced.
- Active current-state documentation is accurate.
- Focused tests, all-feature workspace Clippy and `scripts/verify.sh quick` pass.
- Any binary/dependency reduction claim is backed by measured same-profile evidence; otherwise closure states that footprint was neutral/unmeasured.
- No unresolved medium-or-higher milestone finding remains.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- `eggfetch-core 0.1.5` is not actually available from crates.io or the packaged API materially differs from the required researched surface;
- adopting 0.1.5 requires a Git/path override;
- preserving current behavior requires enabling native roots, HTTP/2/3, proxy, compression or another out-of-scope feature;
- Eggfetch's body limit does not protect ordinary identity/chunked bodies equivalently to the current CodeGG helper;
- moving body collection into `Response::bytes()` makes Eggpool cancellation non-prompt and cannot be corrected narrowly;
- an existing external/public contract demonstrably depends on `BoundedBodyError`'s exact observed-byte diagnostic;
- ordinary-client policy cannot be consolidated without obscuring materially different timeout/redirect/auth semantics;
- detailed failure adoption would require broad provider call-site churn or retry-taxonomy changes;
- a security regression appears in static routing, Host/TLS identity, redaction or redirect policy;
- the dependency update introduces unrelated broad lockfile churn that cannot be separated;
- broad verification exposes a cross-subsystem defect that cannot be fixed within this maintenance boundary.

## 15. Closure evidence required

Create `plans/closure/http-client-maintenance-consolidation/001-status.md` after implementation. It must contain:

- implementation commit(s)/PR(s);
- exact production baseline and resolved Eggfetch 0.1.5 package evidence;
- before/after Eggfetch reverse/feature tree and reqwest census;
- lockfile/manifest summary and any companion package changes;
- requirement-to-evidence matrix;
- proof for exact-limit, declared-over-limit and chunked/unknown-length over-limit behavior;
- Eggpool cancellation and `Oversized` evidence;
- WebFetch/research/MCP static-routing/no-DNS-fallback evidence;
- provider client/retry non-regression evidence;
- disposition of ordinary builder-policy consolidation (implemented or explicitly rejected as over-abstraction, with evidence);
- disposition of `send_detailed()` evaluation (narrow adoption or deferred);
- EggLSP focused dependency-bump evidence;
- formatting, strict all-feature Clippy and quick verification results;
- active-document reconciliation summary;
- optional same-profile artifact-size observation, clearly marked descriptive/non-gating;
- unresolved findings classified by severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- Preserve unrelated user changes and all closed-plan history.
- Use loopback fixtures; do not depend on public provider/search/GitHub endpoints for tests.
- The previous macOS closure work sometimes required a host-specific `PKG_CONFIG_PATH=/usr/local/lib/pkgconfig` for link-heavy tests because of an architecture-mismatched native library. Treat this as an environment note, not repository configuration, and only use it if the current host reproduces that issue.
- Run dependency/package verification before production edits so a packaging mismatch fails early.
- Prefer deletion over introducing another helper when Eggfetch now owns the generic concern.
- Do not optimize for line-count reduction at the expense of clear security/application ownership.
