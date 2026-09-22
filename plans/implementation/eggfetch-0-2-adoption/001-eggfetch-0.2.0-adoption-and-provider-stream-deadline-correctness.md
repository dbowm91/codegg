# Eggfetch 0.2 Adoption M001 — Eggfetch 0.2.0 Adoption and Provider Stream-Deadline Correctness

Status: implemented

Closure: `plans/closure/eggfetch-0-2-adoption/001-status.md` (implementation `653e7abb`)

Repository baseline reviewed: `198524aa4ff8656928c86cf36168892532f2e29c`

Source subsystem roadmap:

- `plans/subsystems/eggfetch-0-2-adoption-roadmap.md`

Predecessor closure evidence:

- `plans/closure/http-client-consolidation/003-status.md`
- `plans/closure/http-client-maintenance-consolidation/001-status.md`
- `plans/closure/dependency-security-workspace-consolidation/006-status.md`

Primary class: infrastructure + invariant.

## 1. Objective

Adopt the published crates.io `eggfetch-core 0.2.0` release with the smallest attributable dependency change, qualify its changed request-total timeout semantics against CodeGG's provider streaming architecture, and correct timeout layering so long-running healthy model streams are not terminated by the historical provider-client `total(60s)` setting.

Preserve the existing HTTP ownership model:

- Eggfetch remains the sole generic HTTP transport owner.
- CodeGG remains the owner of provider retry, stream setup/idle liveness, cancellation, SSRF policy, redirect policy decisions and secret-safe domain errors.
- No new HTTP facade, hidden retry layer or transport feature family is introduced.

This is not a general HTTP modernization milestone.

## 2. Researched upstream compatibility facts

At plan creation, Eggfetch 0.2.0 is published and its release notes state:

- MSRV remains Rust 1.89;
- no intentional public Rust/Python/C/CLI/HTTPX API break from the 0.1.7 surface;
- the existing `http1`, `tls-rustls`, `json` feature names remain supported;
- 0.1.6 introduced explicit HTTPS→HTTP redirect downgrade policy and opt-in environment proxy resolution;
- 0.1.7 fixed `Timeout.total` so it is one absolute deadline through response-body EOF/trailers, including `bytes()`, `text()`, `json()`, `bytes_stream()` and raw streaming paths;
- 0.1.7 also added bounded reuse for identical resolved-target routes;
- 0.2.0 includes the issue #24 streaming decompression chunk-boundary fix;
- the 0.2.0 `http1` compatibility alias still includes advanced routing, high-level URL, logical-retry capability types, redirect capability and Basic-auth capability, while no retry policy is active unless configured.

The published `RequestBuilder::timeout(Timeout)` overrides client timeout fields only where the request timeout contains a value. It cannot be used to erase an inherited client-level `total = Some(...)` merely by providing `None`. Therefore a streaming request that shares a client carrying `total(60s)` remains subject to that absolute total deadline.

Implementation MUST re-check the packaged crates.io artifact before editing and stop if these facts differ materially.

## 3. Current CodeGG evidence

At the reviewed baseline:

### 3.1 Dependency/trust profile

Workspace:

```toml
eggfetch-core = { version = "0.1.5", default-features = false }
```

Root and `codegg-providers`:

```toml
features = ["http1", "tls-rustls", "json"]
```

EggLSP:

```toml
features = ["http1", "tls-rustls"]
```

The approved graph intentionally excludes:

- `tls-native-roots`;
- HTTP/2 and HTTP/3;
- proxy;
- Eggfetch compression features;
- cookies;
- multipart.

The Rust 1.89 workspace floor already matches Eggfetch 0.2.0.

### 3.2 Provider transport policy

`crates/codegg-providers/src/provider_core.rs::create_http_client()` currently configures:

- connect = 10s;
- total = 60s;
- max idle connections per host = 32;
- idle pool timeout = 30s;
- redirects enabled;
- max redirects = 10.

Provider adapters for OpenAI, Anthropic, Google, Azure, OpenRouter, OpenAI-compatible providers, Bedrock, OpenCode Zen and Responses API consume long-lived model output through `Response::bytes_stream()`.

### 3.3 Higher-level stream liveness/retry policy

`src/agent/provider_turn.rs` currently owns:

- `STREAM_SETUP_TIMEOUT = 120s`;
- `STREAM_IDLE_TIMEOUT = 90s`;
- caller cancellation;
- bounded provider retry attempts;
- retry-chain deadlines;
- transient/permanent error classification;
- attempt identity;
- visible-output tracking;
- no transparent replay after visible text/reasoning/tool-call output.

That agent-level policy must remain authoritative for long model streams.

## 4. Required invariants

1. A healthy provider stream that continues yielding events inside CodeGG's idle bound must not fail solely because 60 seconds elapsed since HTTP dispatch.
2. Provider connection establishment remains bounded.
3. Provider stream establishment remains bounded by the existing agent-level setup timeout.
4. A stalled provider stream remains bounded by the existing agent-level idle timeout.
5. Cancellation remains prompt.
6. Non-streaming provider requests that currently rely on the 60-second total bound remain finitely bounded after the shared streaming-client correction.
7. A timeout after visible output is never transparently replayed.
8. A transient timeout before visible output may retry only inside the existing CodeGG retry budget.
9. Eggfetch automatic/logical retry policy is not configured for provider/model traffic.
10. WebFetch/research/MCP validated destination snapshots cannot silently fall back to a second DNS resolution.
11. Logical Host/TLS identity remains separate from the physical resolved destination.
12. Existing body-size limits remain fail-closed.
13. Existing redirect behavior remains unchanged in this milestone.
14. Existing WebPKI trust-store policy remains unchanged.
15. Transport/domain errors remain secret-safe.
16. `codegg-core` remains transport-neutral.

## 5. Scope and non-goals

In scope:

- published-artifact/package metadata verification for Eggfetch 0.2.0;
- targeted workspace dependency update to 0.2.0;
- narrow lockfile reconciliation;
- provider streaming total-deadline regression fixture;
- provider timeout-policy correction required by 0.2.0 semantics;
- request-level total timeout restoration for non-streaming provider operations that need it;
- focused provider retry/cancellation/visible-output verification;
- pinned-routing/body-limit/EggLSP regression verification;
- dependency graph audit, including `base64`, Rustls and feature ownership;
- active documentation and planning-state reconciliation;
- closure evidence.

Out of scope unless required to preserve an existing contract:

- enabling new Eggfetch features;
- changing provider retry counts/backoff/fallback/circuit behavior;
- changing the 120s setup or 90s idle policy merely because this upgrade exists;
- HTTP/2/HTTP/3 adoption;
- environment proxy adoption;
- compression adoption;
- native-root adoption;
- broad dependency modernization;
- a new HTTP abstraction layer;
- replacement of SSE parsers;
- WebFetch/research SSRF redesign;
- HTTPS→HTTP redirect-policy hardening;
- binary-size optimization unrelated to an attributable graph change.

## 6. Ordered work packages

### WP1 — Freeze the current owner/feature/timeout baseline

Before production edits, record:

```bash
cargo tree -i eggfetch-core --locked
cargo tree -e features -i eggfetch-core --locked
cargo tree -i rustls --locked
cargo tree -i rustls-webpki --locked
cargo tree -i base64 --locked
cargo tree -d --locked
cargo audit
```

Also perform a source census for:

```text
eggfetch_core
create_http_client
bytes_stream()
.timeout(
Timeout::builder
follow_redirects
max_redirects
resolved_addresses
max_decoded_body_size
RetryPolicy
proxy_environment
```

Record which provider requests are:

- long-lived streaming model requests;
- bounded non-streaming model/catalog/discovery requests;
- setup/error paths completed before a stream is returned.

Do not infer that every request made by a provider object should share the same total-timeout semantics.

### WP2 — Verify the crates.io 0.2.0 artifact and update narrowly

Verify the packaged release, not only GitHub main:

```bash
cargo info eggfetch-core@0.2.0
```

Confirm:

- rust-version 1.89;
- required features are present;
- no Git/path override is needed.

Then update the workspace requirement from 0.1.5 to 0.2.0 and resolve the lockfile narrowly. Prefer:

```bash
cargo update -p eggfetch-core --precise 0.2.0
```

or the exact Cargo selector required by the current lock.

Do not run an unscoped `cargo update`.

Reject unrelated lockfile churn before continuing.

### WP3 — Add a deterministic total-deadline regression fixture

Add a local loopback fixture that exposes the semantic difference relevant to CodeGG.

The fixture must:

1. accept a request locally;
2. begin a streaming response successfully;
3. emit valid body/SSE chunks at intervals shorter than the read/idle threshold;
4. continue past a deliberately short configured `Timeout.total`;
5. prove Eggfetch 0.2.0 terminates the stream with `TimeoutPhase::Total` even though chunks continue arriving.

Use short test-only durations (for example tens/hundreds of milliseconds), not a real 60-second sleep.

This is qualification evidence, not a test of whether upstream behavior is "wrong": the upstream behavior is intentional. The test establishes why CodeGG's existing provider-client policy must be layered differently.

If 0.2.0 does not exhibit the documented absolute-total behavior in the packaged artifact, stop and re-evaluate the plan rather than applying the timeout redesign blindly.

### WP4 — Correct provider timeout layering

The preferred target, subject to WP3 evidence and current source census, is:

#### Shared streaming-capable provider client

Change `provider_core::create_http_client()` so it retains:

- connect = 10s;
- max idle connections per host = 32;
- idle pool timeout = 30s;
- redirects enabled;
- max redirects = 10;

but does not impose a client-level absolute `total(60s)` across every response body.

Do not replace the removed total deadline with an unbounded application policy. Long-stream liveness remains bounded by `provider_turn.rs`:

- 120s setup;
- 90s next-event idle;
- cancellation;
- retry-chain deadline/budget.

#### Non-streaming provider operations

Census every provider request that fully buffers/decodes a finite response outside the long model-stream lifecycle (for example model discovery/listing and other ordinary provider metadata calls).

Preserve a finite total deadline for those operations using the narrowest explicit request/application boundary available. Eggfetch 0.2.0 exposes request-level:

```rust
RequestBuilder::timeout(Timeout)
```

Use a shared private constant/helper only if multiple non-streaming call sites genuinely share the same policy. Do not create a second HTTP abstraction.

The expected default is to retain the historical 60-second total for ordinary provider metadata requests unless existing owner-specific evidence establishes a different bound.

#### Stream request rule

Do not attach a request-level total timeout to long-lived model SSE requests merely to reproduce the old client setting.

#### Error/setup paths

Provider `stream()` setup and any error body consumed before returning the EventStream remain bounded by the outer 120-second setup timeout. Verify this with existing or focused tests rather than adding another hidden transport deadline.

If implementation cannot distinguish streaming and non-streaming request ownership cleanly without broad provider API churn, stop and report the blocker. Do not retain a known accidental 60-second stream cutoff merely to avoid touching explicit metadata call sites.

### WP5 — Prove retry, visible-output and cancellation semantics

Add or reuse deterministic provider/agent fixtures covering:

1. **Long active stream:** continues beyond the test-equivalent legacy total interval when chunks arrive within idle bounds.
2. **Idle stream:** still fails through CodeGG's idle-timeout owner.
3. **Setup stall:** still fails through CodeGG's setup-timeout owner.
4. **Cancellation:** cancels promptly during setup and streaming.
5. **Pre-visible transient transport timeout:** remains eligible only for bounded CodeGG retry.
6. **Post-visible timeout/interruption:** publishes/records the attempt as visible/superseded and does not transparently replay.
7. **No second Eggfetch retry owner:** source/feature/config evidence shows no configured `RetryPolicy` for provider/model traffic.

Do not weaken the visible-output gate to make a timeout test pass.

### WP6 — Requalify pinned routing, body limits and ordinary HTTP owners

Run focused tests for:

- WebFetch `resolved_addresses()` destination pinning;
- URL research resolved routing;
- remote MCP static routing where present;
- Host/TLS identity preservation;
- no second DNS fallback;
- exact-limit/under-limit/oversized body behavior;
- chunked/unknown-length oversized body behavior;
- ordinary root-client redirect following and ten-hop bound;
- EggLSP download redirect/raw-byte/archive-safety behavior.

Eggfetch 0.1.7+'s resolved-route cache reuse is acceptable only if these CodeGG invariants remain green. Do not add CodeGG cache logic around it.

Issue #24 decompression behavior does not require a new CodeGG production feature. Instead prove via the feature tree that no Eggfetch `compression-*` feature became active incidentally.

### WP7 — Reconcile dependency/security graph

After the 0.2.0 resolution, inspect:

```bash
cargo tree -i eggfetch-core --locked
cargo tree -e features -i eggfetch-core --locked
cargo tree -i rustls --locked
cargo tree -i rustls-webpki --locked
cargo tree -i base64 --locked
cargo tree -d --locked
cargo audit
```

Required checks:

- Eggfetch resolves from crates.io, not Git/path;
- Rust 1.89 remains sufficient;
- Rustls does not regress below the accepted patched security floor;
- WebPKI roots remain the intended trust source;
- `tls-native-roots` remains absent;
- HTTP/2/3, proxy, compression, cookies and multipart remain absent unless already independently owned elsewhere;
- no new advisory is introduced;
- no audit ignore is added merely to close this work.

#### Base64 convergence decision

Eggfetch 0.2.0's compatibility `http1` profile enables Basic-auth capability, which may introduce `base64 0.23` while CodeGG currently owns `base64 0.22`.

If the resolved graph contains both 0.22 and 0.23:

- determine the exact owner paths;
- if CodeGG's direct 0.22 uses compile unchanged or with trivial source-compatible edits against 0.23, a narrow workspace convergence to 0.23 is permitted as directly attributable upgrade cleanup;
- record the exact source/test evidence;
- otherwise retain both and document why; do not widen M001 into unrelated dependency modernization.

Dependency convergence is desirable but not a correctness gate for Eggfetch adoption unless duplicate versions create a security or build problem.

### WP8 — Broad verification and active documentation

Run the repository's current equivalent of:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test -p codegg-providers --all-features --locked -- --test-threads=1
cargo test -p egglsp --all-features --locked -- --test-threads=1
cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
cargo audit
```

Use narrower named test commands first when triaging failures.

Update active documentation that currently describes:

- Eggfetch supported floor 0.1.5;
- provider client as carrying a 60-second client-global total if that is no longer true;
- timeout ownership inconsistent with the implemented post-0.2 policy.

Expected active documents include:

- `docs/dependency-maintenance.md`;
- `architecture/provider.md`;
- any current architecture/error/client document whose statements become stale;
- `plans/registry.md` at closure.

Do not rewrite historical 0.1.4/0.1.5 plans or closure records; they remain accurate evidence of their baselines.

### WP9 — Closure

Create:

- `plans/closure/eggfetch-0-2-adoption/001-status.md`

The closure record must include:

- implementation commit(s);
- exact Eggfetch package/version provenance;
- before/after feature and reverse-dependency trees;
- lockfile delta and companion package explanation;
- provider timeout-policy before/after table;
- the short deterministic absolute-total streaming fixture result;
- long-stream/setup/idle/cancellation/retry evidence;
- visible-output no-replay evidence;
- non-streaming finite-total evidence;
- pinned-routing/no-second-DNS evidence;
- body-limit evidence;
- EggLSP evidence;
- Rustls/base64/security graph disposition;
- broad verification results;
- active-document reconciliation;
- unresolved findings by severity;
- final disposition: closed, conditionally closed, corrective pass required, or blocked.

## 7. Migration and compatibility semantics

No durable storage/config/schema migration is expected.

The intended runtime behavior change is narrow and corrective:

- provider streams may legitimately live longer than 60 seconds as long as CodeGG's setup/idle/cancellation/deadline policies allow;
- ordinary non-streaming provider operations remain finitely bounded;
- no provider payload or event schema changes;
- no user-visible retry semantics change;
- no redirect behavior change;
- no trust-root behavior change.

If users previously observed provider streams being cut off around 60 seconds due to transport total timeout, that behavior is not a compatibility contract to preserve.

## 8. Security effects

This upgrade must not weaken:

- TLS certificate/hostname verification;
- WebPKI trust selection;
- SSRF validation;
- resolved-target pinning;
- body-size limits;
- redirect bounds;
- secret redaction;
- cancellation;
- no-replay-after-visible-output safety.

The new `RedirectDowngradePolicy::Deny` capability is intentionally not globally enabled here. Changing artifact-download redirect security policy should be a separately reviewed policy change rather than an incidental dependency-upgrade effect.

Environment proxy support remains opt-in and MUST stay disabled unless another approved plan owns it.

## 9. Performance/resource effects

Eggfetch 0.2.0's resolved-route reuse may reduce repeated pinned-route connection construction. Treat this as upstream behavior to inherit, not a reason to add CodeGG caching.

Do not claim a binary-size or throughput win without same-profile measurements. Optional before/after release artifact size or local micro-observations may be recorded as descriptive evidence only.

Removing the client-global provider total timeout must not create an unbounded stalled-stream resource leak because CodeGG's setup/idle/cancellation/retry-chain controls remain mandatory.

## 10. Acceptance criteria

- Workspace resolves crates.io `eggfetch-core 0.2.0`.
- Rust 1.89 remains the supported floor.
- Existing CodeGG Eggfetch feature/trust policy is preserved.
- A deterministic fixture proves the relevant 0.2.0 absolute-total streaming behavior.
- Provider long streams are not cut off by a fixed legacy 60-second client-global total.
- Provider setup remains bounded at the existing agent layer.
- Provider idle streams remain bounded at the existing agent layer.
- Non-streaming provider operations retain explicit finite total deadlines.
- Cancellation remains prompt.
- Pre-visible transient failures can retry only within CodeGG's existing budget.
- Post-visible failures do not replay.
- No Eggfetch retry policy is configured beneath provider retry ownership.
- Pinned routing/no-second-DNS/Host/TLS invariants remain green.
- Body limits remain fail-closed.
- Ordinary root and EggLSP redirect behavior remains compatible.
- No environment proxy or compression feature is enabled incidentally.
- Rustls security floor/trust profile does not regress.
- Base64 duplicate-version disposition is explicit.
- Strict all-feature Clippy, workspace tests, quick verification and audit pass, or any environmental limitation is recorded without being misrepresented as a pass.
- Active documentation and registry are truthful.
- No unresolved medium-or-higher M001 finding remains.

## 11. Stop conditions

Stop and report rather than improvise if:

- crates.io `eggfetch-core 0.2.0` is unavailable or materially differs from the researched release;
- adoption requires a Git/path override;
- preserving CodeGG behavior requires enabling native roots, HTTP/2/3, proxy, compression or another out-of-scope feature;
- the packaged timeout semantics do not match the documented absolute-total behavior and the provider-policy rationale therefore changes;
- correcting streaming timeout ownership requires broad public provider API redesign rather than explicit request-policy edits;
- removing the shared client total cannot be paired with finite non-streaming bounds;
- pinned routing regresses or gains DNS fallback;
- visible-output retry safety regresses;
- the lockfile update introduces unrelated broad churn that cannot be separated;
- Rustls/trust configuration regresses;
- a new security advisory makes simple 0.2.0 adoption insufficient for truthful closure;
- broad verification exposes an unrelated cross-subsystem defect that cannot be fixed within this bounded workstream.

## 12. Handoff notes

- Preserve unrelated user changes.
- Keep historical 0.1.4/0.1.5 planning/closure evidence immutable.
- Use local loopback fixtures; do not depend on live OpenAI/Anthropic/GitHub/search endpoints.
- Prefer explicit per-request timeout policy over a new provider HTTP facade.
- Do not shorten the 120s setup or 90s idle constants merely to compensate for transport behavior without separate evidence.
- Do not configure Eggfetch retries.
- Keep redirect downgrade hardening deferred unless separately registered.
- Treat dependency-size/performance claims as measured observations, never assumptions.
