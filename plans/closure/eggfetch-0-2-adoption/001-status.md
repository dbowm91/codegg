# Eggfetch 0.2 Adoption M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/eggfetch-0-2-adoption/001-eggfetch-0.2.0-adoption-and-provider-stream-deadline-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/eggfetch-0-2-adoption-roadmap.md`

Repository baseline reviewed: `239c51d1`

Implementation commit:

- `653e7abb` — eggfetch 0.2 adoption: provider stream-deadline correctness (workspace dependency, absolute-total qualification fixture, shared-client total removal, explicit non-streaming request totals, docs)

Predecessor closure evidence (immutable, not rewritten):

- `plans/closure/http-client-consolidation/003-status.md`
- `plans/closure/http-client-maintenance-consolidation/001-status.md`
- `plans/closure/dependency-security-workspace-consolidation/006-status.md`

## 1. Executive finding

M001 is complete and closed. The workspace resolves published crates.io
`eggfetch-core 0.2.0` under the existing minimal HTTP/1 + Rustls/WebPKI
feature/trust profile with no Git/path override. The material
compatibility risk is corrected: the shared streaming-capable provider
client no longer imposes a client-global absolute `total(60s)` across
every response body, so healthy long model streams governed by the
agent-level 120s setup / 90s idle / cancellation / retry-chain policy
are not terminated solely because 60 seconds elapsed since dispatch.
Bounded non-streaming provider operations retain explicit finite totals
at the request boundary. No new HTTP facade, retry owner, proxy,
compression, native-root, HTTP/2/3, cookie, or multipart surface was
introduced. No unresolved medium-or-higher finding remains.

## 2. Requirement-to-evidence matrix (plan §10 acceptance criteria)

| Acceptance criterion | Evidence | Result | Notes |
|---|---|---|---|
| Workspace resolves crates.io `eggfetch-core 0.2.0` | `cargo info eggfetch-core@0.2.0`; `Cargo.toml` workspace `0.2.0`; `Cargo.lock` `6cd254b8`; `cargo tree -i eggfetch-core --locked` | pass | MIT, rust-version 1.89, crates.io source, no Git/path override. |
| Rust 1.89 remains the supported floor | `cargo info` rust-version + workspace `rust-version = "1.89"` + `cargo ck` green | pass | Eggfetch 0.2.0 floor equals CodeGG floor. |
| Existing feature/trust policy preserved | `cargo tree -e features -i eggfetch-core --locked`; member manifests unchanged | pass | Root/providers `http1,tls-rustls,json`; EggLSP `http1,tls-rustls`; `tls-native-roots` absent; HTTP/2/3, proxy, compression, cookies, multipart absent. |
| Deterministic fixture proves 0.2.0 absolute-total streaming behavior | `provider_core::tests::absolute_total_deadline_terminates_healthy_stream` | pass | Chunked loopback, 30ms-spaced chunks, 250ms total → ≥1 chunk then `TimeoutPhase::Total`. |
| Long streams not cut off by legacy 60s client-global total | `provider_core::tests::shared_streaming_client_permits_long_active_stream` | pass | 20 chunks (~600ms) via `create_http_client()` complete with no error. |
| Setup remains bounded at agent layer | `src/agent/provider_turn.rs` `STREAM_SETUP_TIMEOUT = 120s` unchanged; `stream_once` wraps `provider.stream()` in `tokio::time::timeout(STREAM_SETUP_TIMEOUT)` | pass | No transport change; error-body reads before stream return stay inside setup bound. |
| Idle streams remain bounded at agent layer | `STREAM_IDLE_TIMEOUT = 90s` unchanged; per-event `tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next())` | pass | Transport carries no read/total; stall bound is agent-owned. |
| Non-streaming ops retain explicit finite totals | `non_streaming_timeout()` (60s total-only) + `openai_compatible::models`, `opencode_zen::discover_models` request-level use; `ResponsesTransport::create_response` request-level `config.request_timeout`; `non_streaming_timeout_carries_sixty_second_total` test | pass | Historical 60s preserved for metadata; Responses default 120s preserved. |
| Cancellation remains prompt | `ResponsesTransport::transport_cancel_and_check`; `EggpoolProbe` cancellable paths; `provider_turn` cancel checks before/during/after attempts | pass | No transport change to cancellation. |
| Pre-visible transient failures retry only within budget | `provider_timeout_is_transient_before_visible_output` (Total → Transient); `m001_transient_503_retries_before_visible_output`, `m001_transient_ratelimit_retries_before_visible_output` | pass | Timeout disposition Transient; retry budget/chain unchanged. |
| Post-visible failures do not replay | `m001_midstream_text_failure_does_not_replay`; `provider_turn.rs` visible-output gate unchanged | pass | Superseded path, no transparent replay. |
| No Eggfetch retry policy beneath provider retry | `rg RetryPolicy crates/codegg-providers` empty; `rg \.retry\(` empty for provider traffic; feature tree shows `logical-retry` capability present but no policy configured | pass | Retry ownership stays with CodeGG. |
| Pinned routing / no-second-DNS / Host/TLS green | `pinned_address_ignores_later_dns_and_preserves_host_header`; `mcp_snapshot_addresses_keep_logical_url_separate_from_wire_destination`; research/MCP suites | pass | §5. |
| Body limits fail-closed | `eggfetch_limit_*` (exact/under/declared-over/chunked-over/chunked-under) | pass | §5. |
| Ordinary + EggLSP redirect behavior compatible | `ordinary_builder_follows_redirects_and_enforces_ten_hop_bound`; `shared_client_follows_redirects_and_enforces_ten_hop_bound`; EggLSP download/archive-safety suites | pass | §5. |
| No env proxy or compression enabled incidentally | Feature-tree grep for compression/proxy/http2/http3/cookies/multipart empty; `rustls-native-certs` absent from graph; `rg proxy_environment` empty in HTTP paths | pass | Opt-in proxy stays disabled. |
| Rustls floor / trust profile does not regress | `cargo tree -i rustls` 0.23.45; `rustls-webpki` 0.103.15; WebPKI roots unchanged | pass | Accepted patched floor retained. |
| Base64 disposition explicit | §6 | pass | Dual versions retained with rationale; no convergence churn. |
| Strict Clippy, workspace tests, quick verify, audit | §7 command table | pass | All green; `--all-features` not used for workspace sweeps per repo policy (real-server tests excluded). |
| Active docs and registry truthful | §8 | pass | Floor + timeout ownership reconciled; history untouched. |
| No unresolved medium-or-higher finding | §9 | pass | — |

## 3. Package / version provenance

`cargo info eggfetch-core@0.2.0` (run before editing):

```text
eggfetch-core #http #https #client #async #tls
version: 0.2.0
license: MIT
rust-version: 1.89
features include: http1 (= native-http1 + high-level-url + logical-retry
  + redirects + basic-auth), tls-rustls, json, plus opt-in proxy,
  compression-*, cookies, http2, http3, multipart (all absent from
  CodeGG resolution); default = [http1, tls-rustls, tls-native-roots]
  (CodeGG uses default-features = false)
required features http1, tls-rustls, json present;
no Git/path override needed
```

Upstream semantics verified against the packaged artifact source
(`~/.cargo/registry/src/.../eggfetch-core-0.2.0/src/timeout.rs`,
`request.rs`, `error.rs`):

- `Timeout.total` is one absolute wall-clock deadline from logical
  request start through response-body EOF/trailers, including
  `bytes()`, `text()`, `json()`, `bytes_stream()`, and raw streaming
  paths; it never resets on chunk arrival.
- `RequestBuilder::timeout(Timeout)` merges per-field: request `Some`
  replaces the client field, request `None` keeps the client value. A
  streaming request therefore cannot erase an inherited client-level
  `total = Some(...)` by providing `None` — the client-level total must
  be removed for streaming-capable clients.

## 4. Before / after trees and lockfile delta

Before (baseline `239c51d1`):

```text
eggfetch-core v0.1.5
├── codegg
├── codegg-providers
└── egglsp
features: http1, tls-rustls (+json for root/providers)
eggfetch deps included base64 0.22.1 and dashmap
```

After (`653e7abb`):

```text
eggfetch-core v0.2.0
├── codegg
├── codegg-providers
└── egglsp
features: http1 (+ advanced-routing/native-http1/standard-route/
  transport-http1, basic-auth, high-level-url, logical-retry,
  redirects), tls-rustls (hyper-rustls), json (root/providers only)
```

Lockfile delta (`git diff Cargo.lock`):

```diff
 name = "eggfetch-core"
-version = "0.1.5"
+version = "0.2.0"
-checksum = "8415bc16..."
+checksum = "6cd254b8..."
 dependencies = [
- "base64 0.22.1",
+ "base64 0.23.1",
  "bytes",
- "dashmap",
  "futures-core",
```

Companion explanation:

- `base64 0.22.1 → 0.23.1` on the eggfetch edge: 0.2.0's `http1`
  compatibility profile enables Basic-auth capability (`dep:base64`);
  the resolved base64 moves to 0.23.1, converging with the existing
  `eggsact 1.2.5 → base64 0.23.1` edge. CodeGG's direct workspace
  `base64 = "0.22"` is retained (§6).
- `dashmap` removed from eggfetch's dependency list: upstream cleanup
  in 0.2.0; no CodeGG code depended on it transitively. No other
  lockfile entries changed; no unscoped `cargo update` was run.

## 5. Provider timeout-policy before / after

| Owner | Before | After |
|---|---|---|
| `provider_core::create_http_client()` (shared streaming-capable) | connect 10s; **total 60s (absolute through body EOF)**; 32 idle/host; 30s idle-pool; redirects true/10 | connect 10s; **no client-level total**; 32 idle/host; 30s idle-pool; redirects true/10 |
| Long SSE `stream()` requests (OpenAI, Anthropic, Google, Azure, OpenRouter, OpenAI-compatible, Bedrock, Zen) | subject to accidental 60s absolute body lifetime | **no request-level total**; bounded by agent 120s setup / 90s idle / cancellation / retry-chain |
| `openai_compatible::models()` (`GET /models` + buffered `json()`) | inherited client 60s total | explicit request `.timeout(non_streaming_timeout())` = total 60s |
| `opencode_zen::discover_models()` (`GET /models` + buffered `json()`) | inherited client 60s total | explicit request `.timeout(non_streaming_timeout())` = total 60s |
| `ResponsesTransport` shared client | connect 10s; total `config.request_timeout` (default 120s) across both streaming and non-streaming | connect 10s; **no client-level total** |
| `ResponsesTransport::create_response` (buffered `json()`) | inherited client total (120s default) | explicit request total = `config.request_timeout` (default 120s) |
| `ResponsesTransport::create_response_stream` (`bytes_stream()`) | inherited client total (120s default) — same accidental cap | **no request-level total**; bounded by agent policy + transport `stream_idle_timeout` (default 60s) |
| `stream()` error paths (`text()` before returning `EventStream`) | inherited client 60s total | no hidden transport deadline; bounded by outer 120s setup timeout (plan WP4 rule) |
| `catalog::fetch_live()` | own client, total 10s | unchanged |
| `EggpoolProbe` | own client, connect + total = options, `follow_redirects(false)`, buffered `bytes()` | unchanged |

Shared helper (no second HTTP abstraction):

```rust
pub const NON_STREAMING_PROVIDER_TOTAL: Duration = Duration::from_secs(60);
pub fn non_streaming_timeout() -> eggfetch_core::Timeout // total-only
```

## 6. Fixture and focused-test evidence

WP3 absolute-total qualification —
`provider_core::tests::absolute_total_deadline_terminates_healthy_stream`:
local tokio loopback, chunked `text/event-stream`, 30 chunks at 30ms
spacing (~900ms server intent), client total 250ms → first chunks
arrive, then `Error::Timeout { phase: Total }`. Result: pass (0.25s).
This is qualification evidence that the upstream behavior is
intentional, not an upstream bug report.

WP4/WP5 long-stream correction —
`provider_core::tests::shared_streaming_client_permits_long_active_stream`:
same harness via corrected `create_http_client()`, 20 chunks at 30ms
(~600ms, well past the 250ms WP3 total) → all 20 chunks arrive, no
error. Result: pass.

Non-streaming finite bound —
`provider_core::tests::non_streaming_timeout_carries_sixty_second_total`:
helper carries `total = 60s`, all other phases `None` (inherit client
connect/pool via merge). Result: pass.

Retry disposition —
`provider_core::tests::provider_timeout_is_transient_before_visible_output`:
`Timeout{Total}` → `RetryDisposition::Transient`. Result: pass.
`error::tests::retry_taxonomy_matrix`: pass.
`responses_api::tests::transport_cancel_and_check`: pass.

Agent-level retry / visible-output / cancellation (unchanged owners,
reused as WP5 evidence):

- `m001_midstream_text_failure_does_not_replay` — pass
- `m001_transient_503_retries_before_visible_output` — pass
- `m001_transient_ratelimit_retries_before_visible_output` — pass

No second Eggfetch retry owner: `rg RetryPolicy crates/codegg-providers`
empty; `rg \.retry\(` empty for provider traffic; `rg proxy_environment`
empty in HTTP paths; feature tree shows `logical-retry` capability types
present under `http1` but no policy configured (upstream default is no
retry unless configured).

Pinned routing / identity:

- `tool::webfetch::tests::pinned_address_ignores_later_dns_and_preserves_host_header` — pass
- `mcp::remote::tests::mcp_snapshot_addresses_keep_logical_url_separate_from_wire_destination` — pass
- research suites (164 tests matching research/url) — pass
- `core::eggpool` + provider `eggpool::tests` (probe auth/summary, redirect redaction, oversized chunked, cancellation/overall-timeout) — pass

Body limits (fail-closed):

- `eggfetch_limit_accepts_body_exactly_at_limit` — pass
- `eggfetch_limit_accepts_body_under_limit` — pass
- `eggfetch_limit_accepts_chunked_body_under_limit` — pass
- `eggfetch_limit_rejects_chunked_body_crossing_limit` — pass
- `eggfetch_limit_rejects_declared_body_over_limit` — pass

Redirects (ordinary + provider + EggLSP):

- `http_client::tests::ordinary_builder_follows_redirects_and_enforces_ten_hop_bound` — pass
- `provider_core::tests::shared_client_follows_redirects_and_enforces_ten_hop_bound` — pass (one transient WouldBlock flake observed on first post-upgrade run, passed on immediate rerun; pre-existing racy `read_http_request` helper, unrelated to 0.2.0 — see §9)
- EggLSP `download::tests` archive-safety (traversal/symlink/absolute/hardlink rejection, valid zip) — 14 pass

Provider suites:

- `cargo test -p codegg-providers --all-features --locked` — 163 pass
- `cargo test -p egglsp --features lsp-test-support --locked` — all suites pass (1000 + 19 + 11 + 14 + …)
- `cargo test -p egglsp --locked` — pass
- `cargo test --lib -- webfetch/http_client/resolved/pinned/mcp::remote/eggpool/research` — all pass

## 7. Dependency / security graph disposition

After resolution (`--locked`):

- `cargo tree -i eggfetch-core` → `0.2.0` from crates.io for root, providers, EggLSP. No Git/path override.
- `cargo tree -e features -i eggfetch-core` → `http1` (+ advanced-routing, native-http1, standard-route, transport-http1, basic-auth, high-level-url, logical-retry, redirects), `tls-rustls` (hyper-rustls), `json` (root/providers). No compression-*, proxy, http2/http3, cookies, multipart, `tls-native-roots`.
- `cargo tree -i rustls` → `0.23.45` (accepted patched floor, no regress).
- `cargo tree -i rustls-webpki` → `0.103.15` (unchanged trust source).
- `rustls-native-certs` → not in graph (`tls-native-roots` absent).
- `cargo tree -d` → pre-existing third-party duplicate majors only; no new duplication introduced by this work.
- `cargo audit` → 11 allowed warnings (same set as baseline: bincode, instant, paste, unic-*, yaml-rust, spin), no errors, no new advisory, no ignore added.

Base64 convergence decision (plan WP7): retain both versions explicitly.

- `base64 0.22.1` owners: `codegg` direct, `codegg-core`, `plist→syntect`, `sqlx-core`, `tiktoken→eggcontext`. CodeGG direct uses only stable `Engine::{encode,decode}` with `STANDARD` / `URL_SAFE_NO_PAD`.
- `base64 0.23.1` owners: `eggfetch-core 0.2.0` (via Basic-auth capability) + `eggsact 1.2.5`. The upgrade moves the eggfetch edge 0.22 → 0.23, converging it with the pre-existing eggsact edge.
- Workspace `base64 = "0.22"` is intentionally retained. Bumping the workspace to 0.23 would be unrelated dependency modernization beyond M001's attributable scope; dual versions are pre-existing, create no security or build problem, and `cargo ck` / Clippy / tests are green. Convergence remains desirable but is not a correctness gate.

## 8. Broad verification and active documentation

Commands (all green):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg-providers --all-features --locked -- --test-threads=1   # 163 pass
cargo test -p egglsp --features lsp-test-support --locked -- --test-threads=1
cargo test -p egglsp --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1       # 4871 lib pass + all suites pass
scripts/verify.sh quick
cargo audit
```

Notes:

- Workspace `--all-features` sweeps are intentionally not used per repo
  policy (`AGENTS.md`): they drag in `lsp-real-server-tests`, which need
  installed servers. `verify.sh quick` + the targeted feature sets above
  are the canonical evidence; real-server tests were never in default
  sweeps.
- `cargo clippy -p codegg-providers --all-targets --locked -- -D warnings`
  also green (covered by the workspace Clippy run).

Active documentation reconciled (historical 0.1.4/0.1.5 plans and closure
records left immutable):

- `docs/dependency-maintenance.md` — supported Eggfetch floor `0.1.5` → `0.2.0`.
- `architecture/provider.md` — `create_http_client()` snippet now documents no client-global total + `non_streaming_timeout()` (60s) per-request rule; transport `0.1.5` → `0.2.0`.
- `architecture/client.md` — health-check client `0.1.5` → `0.2.0`.
- `plans/registry.md` — M001 ready → closed; workstream active → closed (this closure).
- `plans/subsystems/eggfetch-0-2-adoption-roadmap.md` — Status active/M001 ready → closed/M001 closed (this closure).

## 9. Unresolved findings by severity

- High: none.
- Medium: none.
- Low: `provider_core::tests::shared_client_follows_redirects_and_enforces_ten_hop_bound`
  (and its `src/http_client.rs` twin) uses a nonblocking-listener +
  blocking-read test helper (`read_http_request`) that can race with
  `WouldBlock` and fail the server thread (`IncompleteMessage` on the
  client). Observed once immediately post-upgrade, passed on immediate
  rerun with no code change; the helper predates M001 and is unrelated
  to Eggfetch 0.2.0 behavior. No action in M001; a future test-hardening
  pass may make the helper WouldBlock-tolerant.
- Informational: binary-size / throughput effects of 0.2.0 (resolved-route
  reuse) are inherited upstream behavior and intentionally unclaimed
  without same-profile measurements.

## 10. Migration and compatibility

No durable storage, config, schema, or protocol migration. Intended
runtime behavior change is narrow and corrective:

- provider streams may live longer than 60 seconds while agent
  setup/idle/cancellation/retry-chain policy allows;
- ordinary non-streaming provider operations remain finitely bounded
  (60s metadata, 120s Responses default);
- no payload/event schema, retry-count, redirect, or trust-root change.

A pre-existing ~60s provider-stream cutoff caused by the transport total
is not a preserved compatibility contract.

## 11. Future work and unblocking

The `eggfetch-0-2-adoption` roadmap contains only M001; no M002 or
successor milestone exists. No other active or blocked plan declares a
hard dependency on this workstream. Closing M001 therefore closes the
workstream and unblocks no future plan — this is terminal infrastructure
correctness work, not a predecessor gate.

The roadmap notes one explicitly deferred candidate (not registered as a
plan): a later HTTPS→HTTP `RedirectDowngradePolicy::Deny` hardening pass
for artifact-acquisition callers. M001 evidence gives no reason to open
it now; register separate work only if future evidence justifies it.

## 12. Final disposition

Closed. All acceptance criteria pass, broad verification is green, and
no medium-or-higher finding remains.
