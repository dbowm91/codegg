# HTTP Client Consolidation and Eggfetch Adoption Roadmap

Status: active; M001 is in progress now that `eggfetch-core 0.1.4` is available on crates.io

Repository baseline reviewed: `a848872573c0604eb52a3ae1f46b6edef0c3eb29`

Canonical long-term references:

- `plans/000-long-term-specification.md` §4.2, explicit ownership;
- `plans/000-long-term-specification.md` §4.7, correctness before transparent magic;
- `plans/000-long-term-specification.md` §11, daemon-owned provider connections and Eggpool;
- `plans/002-long-term-roadmap.md` Phase 2, Eggpool and daemon-owned provider connections;
- `plans/003-planning-process.md`, bounded implementation and closure rules.

Related ADRs: none required. The transport selection is an explicit product/maintenance decision: replace CodeGG's direct `reqwest` dependency with the Eggstack-owned `eggfetch-core` crate once version `0.1.4` is available on crates.io. Open an ADR only if implementation discovers a need for a new cross-workspace HTTP service/trait, a new proxy policy, or another architectural choice not decided by this roadmap.

## 1. Purpose and ownership boundary

This subsystem owns the bounded migration of CodeGG's existing outbound HTTP call sites from `reqwest` to `eggfetch-core 0.1.4`, followed by retirement of CodeGG's direct `reqwest` dependency.

The migration is justified by transport ownership, control, security semantics, and consolidation inside the Eggstack ecosystem. It is **not** justified as a binary-size reduction: Eggfetch's own aligned Rustls qualification records a larger stripped embedded artifact than reqwest. CodeGG must not claim a footprint win unless a later CodeGG-specific measurement actually demonstrates one.

This subsystem does not own provider protocol formats, MCP protocol semantics, search ranking, research extraction, EggLSP archive policy, or Eggfetch upstream development. Those behaviors remain owned by their existing subsystems and must be preserved while the HTTP engine changes underneath them.

## 2. Classification

Primary classification: **infrastructure**.

Security-sensitive portions of M001 are also **invariant** work because CodeGG's validated-destination / DNS-rebinding protections must not regress during the transport swap.

## 3. Non-goals

This roadmap does not authorize:

- a generic CodeGG-wide HTTP abstraction or service locator merely to hide Eggfetch;
- a new crate whose only purpose is wrapping `eggfetch-core`;
- a Git/revision dependency on Eggfetch in lieu of the crates.io `0.1.4` release;
- HTTP/2, HTTP/3, proxy, cookie, multipart, or compression expansion;
- automatic request retries where CodeGG does not already perform them;
- replacement of provider SSE/domain parsers with transport-layer parsing;
- provider protocol, authentication, tool-call, or model-catalog redesign;
- changing WebFetch/research URL SSRF allow/deny policy;
- changing MCP protocol negotiation or OAuth/session semantics;
- changing EggLSP archive extraction policy;
- platform-specific raw socket code to emulate reqwest's duration-valued TCP keepalive setting;
- a DashMap major-version migration or unrelated dependency modernization;
- new CI lanes, scheduled network tests, binary-size gates, dependency bots, or release automation;
- rewriting historical plans or closure records that accurately mention reqwest.

## 4. Current state

At the reviewed baseline CodeGG has four direct reqwest ownership points:

1. the root package enables `stream`, `json`, and `rustls-tls` with defaults disabled;
2. `codegg-providers` enables the same transport surface;
3. `egglsp` enables `stream` and `rustls-tls` for server downloads;
4. `codegg-core` depends on reqwest without transport features, primarily for `Url` and the `AppError::Http(reqwest::Error)` coupling.

The production use surface is broader than provider calls. It includes:

- provider streaming/model discovery across OpenAI-compatible, OpenAI Responses, Anthropic, Google, Azure, OpenRouter, Bedrock, Eggpool and related adapters;
- built-in search and research source clients;
- WebFetch and direct URL research fetching;
- remote MCP JSON/SSE traffic;
- remote SDK health calls;
- image-generation HTTP calls;
- update checks;
- EggLSP binary/archive downloads;
- URL parsing/normalization paths that currently use `reqwest::Url` without performing I/O;
- error conversions and active architecture/dependency documentation.

Two security-sensitive call paths currently validate a URL and pin the validated address set through reqwest's resolver override: WebFetch and direct URL research. Their no-second-DNS/Host-preservation behavior is already regression-tested and is a hard migration invariant.

Remote MCP currently validates/revalidates DNS candidates before sending, but the subsequent ordinary reqwest request performs its own connection-time resolution. Eggfetch 0.1.4's request-scoped static routing allows the migration to close that validation-to-connect gap by connecting only to the revalidated snapshot.

## 5. Upstream Eggfetch 0.1.4 contract used by this roadmap

The implementation plans may rely on the following capabilities present in the Eggfetch tree intended for 0.1.4:

- Rust MSRV 1.80, below CodeGG's Rust 1.89 floor;
- `default-features = false` with explicit `http1`, `tls-rustls`, and `json` ownership;
- `tls-rustls` using the deterministic packaged WebPKI trust set without requiring `tls-native-roots`;
- request/response JSON helpers behind the opt-in `json` feature;
- streaming `Response::bytes_stream()`;
- numeric `Response::content_length()`;
- phase-aware `Timeout`, including `connect`, `read`, and optional wall-clock `total`;
- idle connection caps and idle timeout controls;
- request-scoped `resolved_addresses()` that connects only to supplied `SocketAddr` values, preserves logical Host/TLS identity, survives retries and same-origin redirects, and fails closed for incompatible routes;
- redirects disabled by default, with explicit follow/max controls.

Execution must verify these capabilities against the published `0.1.4` crate rather than assuming the pre-release repository state is identical to the packaged artifact.

## 6. Dependency and behavior mapping

CodeGG should use a narrow Eggfetch feature profile rather than Eggfetch defaults:

```toml
eggfetch-core = { version = "0.1.4", default-features = false, features = ["http1", "tls-rustls", "json"] }
```

`egglsp` does not require native JSON and should use only `http1,tls-rustls` unless implementation demonstrates a real JSON consumer.

Do **not** add `tls-native-roots` merely because it is part of Eggfetch's default profile. CodeGG's current reqwest `rustls-tls` configuration uses the packaged WebPKI-root path; widening trust-store behavior during a transport migration is unnecessary semantic drift.

Other compatibility points that must be mapped explicitly:

- reqwest follows redirects by default; Eggfetch does not. Ordinary CodeGG clients that previously inherited reqwest defaults must explicitly use follow-redirects with the reqwest-equivalent bound (10) where behavior should remain unchanged. Security-sensitive clients that currently disable redirects must remain no-follow.
- reqwest `Client::new()` has transport defaults that are not identical to Eggfetch's unbounded native defaults. Each construction site must preserve its existing explicit timeout, and sites that relied on reqwest defaults must receive an intentional CodeGG timeout policy rather than silently becoming unbounded.
- provider client pool configuration maps directly to Eggfetch idle-per-host and idle-timeout controls.
- reqwest's `tcp_keepalive(Duration::from_secs(30))` has no exact typed Eggfetch equivalent. Eggfetch can enable portable `SO_KEEPALIVE` through socket options but does not expose the keep-idle duration. This migration must not add platform-specific raw socket configuration merely to mimic the value. Existing provider stream/read timeout behavior remains the correctness mechanism; a future typed upstream keepalive enhancement requires separate evidence if needed.
- Eggfetch `RequestBuilder::json()` returns `Result<RequestBuilder>` and `Response::bytes_stream()` returns `Result<Stream>` from a mutable response. Call sites must handle those explicit error points rather than mechanically translating reqwest chains.
- Eggfetch transport errors do not carry reqwest's attached URL object. CodeGG-owned errors must remain sanitized and transport-neutral rather than recreating raw URL coupling.

## 7. Target architecture

After this roadmap closes:

- CodeGG root, `codegg-providers`, and `egglsp` directly use the crates.io `eggfetch-core 0.1.4` surface appropriate to their needs.
- `codegg-core` is transport-neutral: it uses `url::Url` for URL parsing and CodeGG-owned error data instead of depending on either reqwest or Eggfetch solely for types.
- provider code retains its existing shared `create_http_client()` construction point; unrelated root consumers are not forced behind a new common wrapper.
- security-sensitive HTTP requests attach validated/revalidated address snapshots with `resolved_addresses()` and cannot silently return to system DNS.
- ordinary clients preserve their prior redirect and timeout behavior explicitly.
- provider SSE, MCP SSE, bounded-body collection, cancellation, and higher-level protocol parsing remain CodeGG-owned.
- no production CodeGG source or manifest directly depends on reqwest.

## 8. Dependency graph and milestone order

```text
crates.io eggfetch-core 0.1.4 publication
        |
        v
M001 transport boundary + pinned/security-sensitive adoption
        |
        v
M002 provider streaming + Eggpool adoption
        |
        v
M003 remaining consumers + reqwest retirement + documentation/closure audit
```

M001 is the only milestone externally blocked at roadmap creation. M002 and M003 are internally blocked on predecessor closure because they reuse the error, response-stream, timeout, and manifest decisions proven by earlier work.

## 9. Milestones

### M001 — Transport boundary and pinned/security-sensitive adoption

Plan: `plans/implementation/http-client-consolidation/001-transport-boundary-and-pinned-http-adoption.md`

Goals:

- admit the published Eggfetch dependency without claiming full migration;
- remove reqwest type ownership from `codegg-core`;
- migrate the shared bounded-body helper;
- migrate WebFetch and direct URL research with strict validated-address pinning;
- migrate remote MCP and bind its actual connection to the revalidated address snapshot;
- preserve URL/Host/TLS identity, no-follow redirects, bounded bodies, and application-level retry semantics.

Status: **active**; the published crate resolves with the required API/feature surface.

### M002 — Provider streaming and Eggpool adoption

Plan: `plans/implementation/http-client-consolidation/002-provider-streaming-and-eggpool-adoption.md`

Goals:

- replace reqwest throughout `codegg-providers`;
- preserve 60s request / 10s connect policy, idle pool behavior, explicit status mapping, JSON bodies, model discovery and all provider-specific headers/signing;
- preserve streaming chunk timeout, cancellation and SSE parser behavior;
- remove the provider crate's direct reqwest dependency.

Status: **blocked on M001 closure**.

### M003 — Remaining HTTP consumers and reqwest retirement

Plan: `plans/implementation/http-client-consolidation/003-remaining-http-consumers-and-reqwest-retirement.md`

Goals:

- migrate remaining built-in search/research clients, SDK, image, upgrade and EggLSP download paths;
- preserve ordinary redirect/default-timeout semantics explicitly;
- update source guards and active documentation;
- remove root and EggLSP reqwest declarations and verify no direct production use remains;
- record dependency-tree/footprint observations without turning them into a gate.

Status: **blocked on M002 closure**.

## 10. Cross-cutting verification

Every milestone must run focused local tests for its consumers, then the repository's normal broad local posture when the milestone is otherwise clean:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Network-dependent external-provider tests are not required for CI. Prefer loopback fixtures that prove request headers, bodies, redirects, streaming, cancellation and static routing deterministically.

M003 may use temporary census commands (`rg`, `cargo tree`, release artifact inspection) as closure evidence. It must not add a permanent dependency/size scanner or binary-size gate.

## 11. Primary risks

1. **Pre-release packaging drift.** The current Eggfetch repository still identifies the crate as 0.1.3 while the user is preparing 0.1.4. The migration must consume the published artifact and stop if its surface differs materially.
2. **Redirect-default drift.** A mechanical client swap would change ordinary reqwest follow-redirect behavior because Eggfetch defaults to no-follow.
3. **Timeout-default drift.** A mechanical `Client::new()` swap could make previously bounded calls unbounded.
4. **Streaming API shape.** Eggfetch requires mutable responses and fallible stream extraction.
5. **Error coupling.** reqwest-specific URL/status extraction must not be replaced with leaked raw endpoints or a new core dependency on Eggfetch.
6. **Provider keepalive knob.** The exact 30s TCP keep-idle setting is not expressible by Eggfetch's current typed API.
7. **Security regression.** Static-routing call sites must prove no second DNS lookup and no pool/redirect escape.
8. **False footprint claim.** Eggfetch's isolated measurement is not a size win; CodeGG-specific consolidation should be described by measured results, not assumptions.

## 12. Completion criteria

This roadmap is complete only when:

- all three implementation milestones have accepted closure records;
- CodeGG uses the published crates.io `eggfetch-core 0.1.4`, not a Git override;
- no production source or active manifest directly references reqwest;
- `codegg-core` has no transport-client dependency solely for URL/error types;
- WebFetch, direct URL research and remote MCP have deterministic tests proving validated destination routing cannot fall back to a second DNS resolution;
- provider streaming/model discovery and Eggpool probes retain their existing functional contracts;
- ordinary root and EggLSP clients explicitly preserve redirect/timeout semantics that previously came from reqwest defaults;
- active dependency/architecture docs describe Eggfetch truthfully while historical records remain intact;
- broad local verification is green;
- any remaining transitive `reqwest` package, if one exists through an unrelated third party, is recorded accurately rather than misrepresented as CodeGG-owned HTTP transport;
- no medium-or-higher migration finding remains open.
