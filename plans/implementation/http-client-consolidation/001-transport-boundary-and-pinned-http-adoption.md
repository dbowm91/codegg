# HTTP Client Consolidation M001 — Transport Boundary and Pinned HTTP Adoption

Status: active

Repository baseline reviewed: `a848872573c0604eb52a3ae1f46b6edef0c3eb29`

Source subsystem roadmap:

- `plans/subsystems/http-client-consolidation-roadmap.md`

Canonical references:

- `plans/000-long-term-specification.md` §4.2 — explicit ownership;
- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/000-long-term-specification.md` §11 — bounded provider/network behavior;
- `plans/002-long-term-roadmap.md` Phase 2 — daemon-owned provider connections.

## 1. Objective

Establish the Eggfetch transport boundary in CodeGG using the published crates.io `eggfetch-core 0.1.4`, remove reqwest type ownership from `codegg-core`, and migrate the security-sensitive outbound HTTP paths first: bounded untrusted response collection, built-in WebFetch, direct URL research, and remote MCP.

The milestone succeeds only if CodeGG's existing validated-destination guarantees are preserved or strengthened. The migration must never turn URL validation into a check that is followed by an unrelated DNS resolution at connection time.

## 2. Readiness and hard dependency gate

This plan is technically specified but must not execute until all of the following are true:

1. `eggfetch-core 0.1.4` is available from crates.io;
2. Cargo resolves the published crate without a `[patch]`, Git dependency, local path override, or vendored copy;
3. the published crate exposes the intended `http1`, `tls-rustls`, `json`, `resolved_addresses`, response streaming/content-length, and timeout APIs;
4. its declared MSRV remains compatible with CodeGG's Rust 1.89 floor;
5. its license remains acceptable to the repository.

Suggested preflight after publication:

```bash
cargo info eggfetch-core@0.1.4
```

If the packaged artifact differs materially from the reviewed upstream tree, stop and update this plan rather than coding around an unpublished API assumption.

## 3. Current evidence

### Manifest/type ownership

- root `Cargo.toml` directly owns reqwest with `stream,json,rustls-tls` and defaults disabled;
- `crates/codegg-core/Cargo.toml` directly owns reqwest with defaults disabled even though core does not own general HTTP I/O;
- `crates/codegg-core/src/provider_connections.rs` uses `reqwest::Url` for parsing;
- `crates/codegg-core/src/error.rs` stores `reqwest::Error` in `AppError::Http`;
- root `src/error.rs` implements a reqwest-specific Axum conversion and reads status from that error type.

### Security-sensitive HTTP

`src/tool/webfetch.rs` and `src/research/sources/url.rs` both:

1. validate the logical URL and resolve/validate the host;
2. disable redirects;
3. disable proxies;
4. configure reqwest with `resolve_to_addrs()` using the validated `SocketAddr` set;
5. stream the body through `read_body_bounded()`.

WebFetch intentionally performs a **fresh validation/resolution** before its application-level 403/503 retry. That is a security contract, not redundant work.

`src/security/untrusted_http.rs` provides the shared bounded collector. It checks declared `Content-Length` early and then enforces the cap again while consuming streamed chunks. Its regression `pinned_address_ignores_later_dns_and_preserves_host_header` proves the production pinning property.

### Remote MCP

`src/mcp/remote.rs` validates the host, stores validated IPs, revalidates the DNS set before JSON requests, disables redirects, and then dispatches through an ordinary reqwest client. The validation and actual connection therefore do not share one caller-controlled destination snapshot. Eggfetch's static routing allows this milestone to make the actual connection use the revalidated address set.

## 4. Invariants

- No validated untrusted request may silently perform a second system DNS resolution after the accepted address snapshot is selected.
- The logical URL remains authoritative for HTTP Host, HTTPS SNI/certificate verification, origin policy, diagnostics and user-visible URL reporting.
- WebFetch's explicit 403/503 retry re-runs `validate_url_target()` and uses the new snapshot; it must not reuse the first attempt's addresses by accident.
- WebFetch and direct URL research continue to disable redirects.
- Remote MCP continues to disable HTTP redirects unless its protocol code explicitly handles a URL transition and validates the new target.
- Static routing must not silently fall back to proxy/H3/UDS behavior. The CodeGG Eggfetch profile in this milestone does not enable those features.
- The cumulative body limit remains enforced while streaming even if an upstream `Content-Length` is absent or false.
- `codegg-core` must remain transport-neutral. Do not replace its reqwest dependency with `eggfetch-core` merely for URL/error types.
- Existing secret redaction rules remain intact; HTTP errors must not start embedding credential-bearing query strings or authorization values.
- Historical plans/closure records are not rewritten.

## 5. Scope and non-goals

In scope:

- root and `codegg-core` manifest changes needed for M001;
- transport-neutral core URL/error ownership;
- `src/security/untrusted_http.rs`;
- `src/tool/webfetch.rs` built-in fallback path;
- `src/research/sources/url.rs`;
- `src/mcp/remote.rs`;
- URL-only `reqwest::Url` uses in files touched by this milestone where moving to `url::Url` is necessary to remove core coupling;
- focused security and response-body regression tests.

Out of scope:

- provider crate migration — M002;
- remaining search/research/image/update/SDK/EggLSP clients — M003;
- new SSRF blocklists, resolver traits, DNS caches, DoH/DoT, proxy routing, or H3;
- a new shared HTTP facade crate;
- generalized automatic retry policy;
- provider/MCP protocol redesign.

## 6. Production changes

### 6.1 Admit Eggfetch with parity-oriented features

Add root Eggfetch ownership using the published crate:

```toml
eggfetch-core = { version = "0.1.4", default-features = false, features = ["http1", "tls-rustls", "json"] }
```

Do not enable `tls-native-roots`, proxy, HTTP/2, HTTP/3, cookies, multipart, or compression during this migration. `tls-rustls` is the closest match to CodeGG's current deterministic reqwest Rustls/WebPKI trust profile.

Retain root reqwest temporarily while unmigrated M002/M003 call sites still require it.

### 6.2 Remove transport ownership from `codegg-core`

Replace core-only URL parsing with direct `url = "2"` ownership and migrate `reqwest::Url` consumers to `url::Url`.

Replace `AppError::Http(reqwest::Error)` with CodeGG-owned data. Preserve the useful information, not the transport type. The preferred shape is a sanitized message plus optional numeric HTTP status if a caller actually has one. Do not make `codegg-core` depend on Eggfetch simply to store `eggfetch_core::Error`.

Update root Axum conversion/mapping accordingly. Because current CodeGG does not use reqwest `error_for_status()`, ordinary transport failures should continue to map to a gateway/server error rather than inventing status data that the transport error does not possess.

### 6.3 Adapt bounded untrusted-body collection

Change `read_body_bounded()` to consume an Eggfetch response while preserving both checks:

1. early reject from `Response::content_length()` when present and over limit;
2. incremental cumulative cap while consuming `Response::bytes_stream()`.

Account for Eggfetch API shape:

- response must be mutable;
- `bytes_stream()` itself is fallible;
- each stream item is still fallible.

Map both stream acquisition and chunk failures into `BoundedBodyError::Stream` using an Eggfetch-owned source/error representation or a transport-neutral message. Do not weaken `TooLarge { limit, observed }` behavior.

### 6.4 Migrate WebFetch with request-scoped static routing

Use one normally configured Eggfetch client per tool instance or request flow, not one client per validated address set solely to carry DNS overrides. Configure:

- explicit wall-clock timeout matching `self.timeout`;
- redirects disabled;
- no proxy feature in the dependency profile.

For each attempt:

1. `validate_url_target(url)`;
2. build the GET request against the original logical URL;
3. attach `.resolved_addresses(target.addresses().iter().copied())`;
4. add the existing browser/User-Agent headers;
5. send and process the mutable response through the bounded collector.

For the 403/503 retry, call `validate_url_target(url)` again and attach the **new** address snapshot. Do not use Eggfetch automatic retry to replace this application-level behavior.

### 6.5 Migrate direct URL research identically

Use the same security model as WebFetch:

- original logical URL;
- redirects off;
- request-scoped validated addresses;
- configured 30s timeout;
- bounded 5 MiB response collector;
- unchanged content-type extraction/hash/extraction behavior.

Do not create a generic HTTP abstraction solely to make these two files look identical. A small security-local helper is acceptable only if it owns a real invariant such as constructing the standard no-follow timeout policy or attaching validated addresses.

### 6.6 Bind remote MCP connection to the revalidated snapshot

Replace `reqwest::Url` with `url::Url` and the reqwest client with Eggfetch.

Preserve the existing MCP validation lifecycle:

- validate host/address during construction/initialization;
- revalidate the DNS set before each outbound JSON request according to current semantics.

After successful revalidation, convert the accepted `IpAddr` values plus the logical URL's effective port into `SocketAddr` values and attach them via `resolved_addresses()` to the **actual** POST request. This closes the gap between revalidation and transport resolution.

Preserve:

- `Content-Type` and `Accept`;
- configured headers with CR/LF rejection;
- OAuth bearer header;
- legacy MCP session header;
- modern protocol/method/name headers;
- session-ID capture from response headers;
- JSON-RPC response/error handling;
- SSE response parsing and the bounded 1 MiB stream buffer;
- shutdown/reconnect/cancellation behavior.

Do not expose Eggfetch static routing through model-visible MCP metadata; it is an internal transport invariant.

## 7. Ordered work packages

### WP1 — Published-crate preflight and manifest boundary

Verify the crates.io 0.1.4 artifact, add root Eggfetch, add direct `url` ownership to `codegg-core`, and keep reqwest only where later milestones still require it.

### WP2 — Transport-neutral core errors and URLs

Remove `reqwest::Url` and `reqwest::Error` from `codegg-core`; repair root Axum/error adapters and focused error tests.

### WP3 — Bounded response helper migration

Port `untrusted_http` to Eggfetch and make its loopback body-limit fixtures use Eggfetch. Re-establish the no-second-DNS/Host-header regression using `resolved_addresses()`.

### WP4 — WebFetch and URL research static routing

Migrate both direct fetch paths, preserving no-follow redirects, fresh WebFetch retry validation, body limits and content handling.

### WP5 — MCP pinned transport migration

Port JSON/SSE traffic and make every revalidated MCP request connect only to the accepted snapshot.

### WP6 — Focused closure audit

Run security/MCP tests, search touched source for accidental reqwest coupling, run broad local verification, and write the M001 closure record. Do not mark M002 ready until M001 has no unresolved medium-or-higher finding.

## 8. Failure, cancellation, restart, and contention semantics

- Eggfetch transport errors are mapped into the existing Tool/Research/MCP error domains; no background retry task is added.
- WebFetch's explicit retry remains exactly one fresh application-level attempt under current conditions.
- Dropping an Eggfetch streaming response must release transport resources naturally; do not retain response streams after cancellation.
- MCP heartbeat/reconnect ownership remains unchanged. Transport replacement must not spawn a second reconnect loop.
- The static address snapshot is immutable for one request attempt. A later logical attempt may intentionally validate again and receive a new snapshot.
- Empty or port-mismatched resolved address sets fail before network I/O.

## 9. Compatibility and migration behavior

No user-facing configuration migration is expected. The only intended behavior change is a security improvement for remote MCP: the actual socket destination is constrained to the already revalidated address set rather than relying on a later system DNS lookup.

Trust-store behavior should remain aligned with current packaged Rustls roots; do not widen to native system roots in M001.

## 10. Required tests

At minimum add/adapt deterministic tests for:

- bounded body exactly at limit;
- declared body over limit rejected early;
- chunked/streamed body crossing limit rejected cumulatively;
- pinned `.invalid` host connects to supplied local address with zero DNS fallback and preserves Host;
- failed supplied address set does not fall back to DNS;
- WebFetch first attempt and 403/503 retry each use independently validated snapshots;
- direct URL research uses the accepted snapshot and does not follow redirects;
- MCP JSON request preserves Host/header/session/protocol semantics while using the revalidated addresses;
- MCP request fails rather than escaping to DNS when its accepted destination is unreachable;
- MCP SSE stream remains bounded/cancellable;
- transport-neutral `AppError::Http` maps to the same external Axum class expected before the migration;
- no secret-bearing header/query material appears in new error formatting.

Prefer loopback fixtures and `.invalid` logical hostnames; do not require external network access.

## 11. Verification commands

Focused commands may be adjusted to existing test target names, but closure must include the equivalent of:

```bash
cargo test --lib security::untrusted_http -- --test-threads=1
cargo test --lib tool::webfetch -- --test-threads=1
cargo test --lib research -- --test-threads=1
cargo test --lib mcp::remote -- --test-threads=1
cargo test -p codegg-core --all-features
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Also record:

```bash
cargo tree -p codegg-core | rg 'reqwest|eggfetch|url'
cargo tree -p codegg | rg 'eggfetch-core|reqwest'
```

These are closure evidence, not new CI gates.

## 12. Documentation updates

M001 should update active architecture text only where the transport/error/security contract changes materially. Do not perform the broad dependency-documentation sweep reserved for M003.

At minimum ensure any active MCP/security architecture page does not describe a connection-time DNS behavior that is no longer true.

## 13. Acceptance criteria

- Published crates.io `eggfetch-core 0.1.4` is the resolved dependency; no Git/path override.
- `codegg-core` has no direct reqwest or Eggfetch dependency and owns URL/error data through transport-neutral crates/types.
- Root may still contain reqwest solely for M002/M003 unmigrated consumers.
- `read_body_bounded()` preserves early and streamed size enforcement on Eggfetch responses.
- WebFetch and direct URL research connect only to the address snapshots returned by CodeGG validation.
- WebFetch's 403/503 retry performs a fresh validation and pins the new result.
- Remote MCP connects only to its revalidated address snapshot and cannot silently re-resolve after acceptance.
- Logical Host/TLS identity remains based on the original URL.
- Security-sensitive redirects remain disabled/fail closed.
- Focused tests and broad local verification pass.
- M001 closure contains no unresolved medium-or-higher finding.

## 14. Stop conditions

Stop and record a blocker if:

- crates.io `0.1.4` is unavailable or lacks a reviewed required API;
- the packaged feature graph requires enabling proxy/H3/native-roots or another unrelated capability to obtain static routing/JSON/streaming;
- Eggfetch static routing cannot prove no second DNS lookup for CodeGG's validated-target flow;
- preserving MCP protocol behavior would require a new generic resolver/service architecture;
- transport-neutral core error ownership would require a public breaking API that has not been approved;
- existing focused security tests cannot be made deterministic without weakening the invariant.

## 15. Closure evidence requirements

Create `plans/closure/http-client-consolidation/001-status.md` only after implementation. Record:

- published Eggfetch version and resolved checksum/lockfile state;
- implementation commit;
- focused security/MCP test results;
- broad verification results;
- direct dependency tree excerpts for core/root;
- explicit disposition of every acceptance criterion and any residual finding.

## 16. Handoff summary

Begin only after `eggfetch-core 0.1.4` is published. Establish the transport-neutral core boundary first, then migrate bounded untrusted HTTP, WebFetch/direct URL research, and MCP static routing. Treat destination pinning as the hard correctness gate. Do not touch provider streaming or remaining ordinary clients until this milestone closes cleanly.
