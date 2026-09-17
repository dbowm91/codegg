# HTTP Client Maintenance Consolidation M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/http-client-maintenance-consolidation/001-eggfetch-0.1.5-policy-and-body-ownership.md`

Source subsystem roadmap:

- `plans/subsystems/http-client-maintenance-consolidation-roadmap.md`

Repository baseline reviewed: `cb15b096`

Implementation commit:

- `72f6610d` — Eggfetch 0.1.5 policy and body ownership consolidation (workspace dependency, WebFetch/research body-limit transfer with `untrusted_http` deletion, Eggpool cancellable transfer, ordinary builder helper, docs)

## 1. Executive finding

M001 is complete and closed. The workspace resolves published crates.io
`eggfetch-core 0.1.5` under the existing minimal feature/trust profile with
no Git/path override. Both CodeGG-owned generic bounded-body accumulators
are removed: WebFetch/direct URL research enforce their 5MB limits through
request-scoped `max_decoded_body_size()` on the same request that carries
the validated `resolved_addresses()` snapshot and collect via Eggfetch
`Response::bytes()`; Eggpool enforces `response_byte_limit` the same way
under its existing cancellation owner with `DecodedBodyTooLarge -> Oversized`
preserved. Ordinary root redirect policy is consolidated into one
transparent `ClientBuilder` helper without a new HTTP facade, singleton,
retry owner, or transport-type wrapper. `send_detailed()` was evaluated and
explicitly deferred. No behavior change is intended and no footprint win is
claimed.

## 2. Requirement-to-evidence matrix

| Requirement (plan §13) | Evidence | Result | Notes |
|---|---|---|---|
| Workspace resolves crates.io `eggfetch-core 0.1.5` | `cargo info eggfetch-core@0.1.5`; `Cargo.toml:66`; `Cargo.lock`; `cargo tree -i eggfetch-core --locked` | pass | Version `0.1.5`, MIT, rust-version `1.89`; no Git/path override. |
| Minimal feature/trust profile retained | `cargo tree -e features -i eggfetch-core --locked`; manifests | pass | Root `http1,tls-rustls,json`; EggLSP `http1,tls-rustls`; providers use workspace union; `tls-native-roots` absent; no HTTP/2/3, proxy, compression, cookie, multipart. |
| Rust 1.89 MSRV satisfied | `cargo info` rust-version + workspace `rust-version = "1.89"` + `cargo ck` | pass | Eggfetch 0.1.5 floor equals CodeGG floor. |
| WebFetch/research enforce limits through Eggfetch on pinned request | `src/tool/webfetch.rs` (both attempts set `.max_decoded_body_size(MAX_RESPONSE_SIZE)` alongside `.resolved_addresses()`); `src/research/sources/url.rs` (same for `MAX_RESPONSE_BYTES`); `collect_bounded_body` / direct `bytes()` mapping | pass | Status/content-type handling still precedes collection; redirects stay disabled on both. |
| Oversized fails closed with/without trustworthy Content-Length | `tool::webfetch` loopback tests: exact-limit, under-limit, declared-over-limit, chunked-over-limit, chunked-under-limit | pass | 10/10 webfetch tests pass; over-limit yields `DecodedBodyTooLarge` mapped to owner body-limit error. |
| Generic root accumulator removed | `src/security/untrusted_http.rs` deleted; `src/security/mod.rs` module removed; `rg read_body_bounded src crates` empty | pass | No production `read_body_bounded` remains; `bytes_stream()` remains only for SSE/streaming (provider/MCP), which the plan permits. |
| Eggpool accumulator removed; Oversized/Cancelled preserved | `crates/codegg-providers/src/eggpool.rs`: request-level limit, `collect_body_cancellable` via `tokio::select!` around `response.bytes()`, `classify_body_error` maps `DecodedBodyTooLarge -> Oversized`; early Content-Length check retained as non-authoritative fast path | pass | 8/8 eggpool tests pass including new chunked-oversized and stalled-body cancellation tests. |
| No second retry owner | No `RetryPolicy` enabled; `rg RetryPolicy` shows only Eggfetch-internal/test references, no provider/model POST usage | pass | Provider logical retries untouched. |
| SSRF/no-second-DNS retained and tested | WebFetch `pinned_address_ignores_later_dns_and_preserves_host_header` (moved from deleted helper to owner); `mcp::remote` snapshot test green | pass | Logical Host/TLS identity authoritative; physical destination only via snapshot. |
| Ordinary policy consolidated transparently; no facade | `src/http_client.rs::ordinary_http_client_builder(Timeout) -> ClientBuilder` + migration of 17 root call sites; provider `create_http_client()` and EggLSP downloader untouched | pass | Helper owns only timeout + follow-true/10-hop; no retry/endpoint/auth/parsing/response logic; no singleton. |
| `send_detailed()` narrow-or-deferred | §5 evaluation, no production call-site change | pass | Deferred: no current decision needs DNS-vs-refused detail; adoption would require domain-API widening and broad churn. |
| No reqwest reintroduced | `cargo tree -i reqwest --locked` errors (no package); `rg reqwest` only scanner literals/history | pass | — |
| Active docs accurate | `architecture/security.md`, `architecture/provider.md`, `architecture/client.md`, `docs/dependency-maintenance.md` | pass | Historical plan/closure 0.1.4 references untouched. |
| Focused tests + strict Clippy + quick verify | §4 command table | pass | All green. |
| Footprint claim | None claimed | pass | Neutral/unmeasured; no size gate added. |
| No unresolved medium-or-higher finding | §6 | pass | — |

## 3. Production baseline and resolved package evidence

Before (plan baseline `0349cb2c`, working baseline `cb15b096`):

```text
eggfetch-core v0.1.4 (Cargo.lock 442000edd076a6f101931292d4a159d5f2b4cec29c6acd010d558af00430b327)
```

`cargo info eggfetch-core@0.1.5` (run before editing):

```text
eggfetch-core #http #https #client #async #tls
version: 0.1.5
license: MIT
rust-version: 1.89
features include: http1, tls-rustls, tls-native-roots (default),
  compression-*, cookies, http2, http3, json, multipart, proxy,
  test-util, tracing
required features http1, tls-rustls, json exist;
default-features = false usable;
public ClientBuilder, request-level max_decoded_body_size(),
Response::bytes(), resolved_addresses(), ordinary Error,
opt-in RequestFailure, typed NetworkFailureKind present
(verified against registry src for 0.1.5)
```

After:

```text
eggfetch-core v0.1.5 (Cargo.lock 8415bc169d247e90aa25890b37b45275f3e4449e6aeda0946c022aa3e4713fc1)
```

`cargo tree -i eggfetch-core --locked` (after):

```text
eggfetch-core v0.1.5
├── codegg v0.1.0
├── codegg-providers v0.1.0
│   ├── codegg v0.1.0
│   └── codegg-core v0.1.0
│       └── codegg v0.1.0
└── egglsp v0.1.0
    ├── codegg v0.1.0
    └── codegg-core v0.1.0 (*)
```

`cargo tree -e features -i eggfetch-core --locked` (after): `http1`
consumed by root/providers/egglsp; `tls-rustls` by all three;
`json` by root + providers only. No new feature enabled.

`cargo tree -i reqwest --locked` (after): `error: package ID
specification 'reqwest' did not match any packages`.

`cargo tree -d --locked` (after): unchanged duplicate families
(`base64`, `plist`/`syntect`, `sqlx-core`, `tiktoken`, etc.); no new
duplicate introduced by this change.

Lockfile/manifest summary: `Cargo.toml` workspace floor `0.1.4` →
`0.1.5` (one line); `cargo update -p eggfetch-core --precise 0.1.5`
narrowly refreshed `Cargo.lock` (eggfetch checksum + version plus the
required `windows-sys 0.60.2 → 0.61.2` companion entries; 14
insertions/14 deletions total, all attributable to the version update).
No Git/path patch. No unrelated dependency modernization.

## 4. Verification executed

| Command | Result |
|---|---|
| `cargo info eggfetch-core@0.1.5` | pass — 0.1.5/MIT/MSRV 1.89/feature/API surface confirmed before editing |
| `cargo test --lib tool::webfetch --locked -- --test-threads=1` | pass — 10 passed (5 new Eggfetch-limit + pinned-address + 5 pre-existing) |
| `cargo test --lib research --locked -- --test-threads=1` | pass — 123 passed |
| `cargo test --lib security:: --locked -- --test-threads=1` | pass — 369 passed (no `untrusted_http` module remains; SSRF/sandbox/policy/service/workflow/runtime green) |
| `cargo test -p codegg-providers --lib --locked -- --test-threads=1` | pass — 146 passed (includes `shared_client_follows_redirects_and_enforces_ten_hop_bound`, retry taxonomy, and 8 eggpool tests) |
| `cargo test --lib mcp::remote --locked -- --test-threads=1` | pass — 1 passed (`mcp_snapshot_addresses_keep_logical_url_separate_from_wire_destination`) |
| `cargo test --lib http_client --locked -- --test-threads=1` | pass — ordinary builder redirect/bound + distinct-timeout tests (plus matched scanner test) |
| `cargo test -p egglsp --all-features --locked -- --test-threads=1` | pass — 14 passed, 0 doc-tests |
| `cargo fmt --all -- --check` | pass (after `cargo fmt --all`) |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | pass (after one `redundant_guards` fix in `url.rs`) |
| `scripts/verify.sh quick` | pass — fmt, agent schema, core-boundary, sandbox, execution-ownership, tui-authority, workspace check |

Static/census evidence:

```text
rg -n 'read_body_bounded' src crates
# (no output — both generic accumulators removed)

rg -n 'max_redirects\(10\)' src crates
# src/http_client.rs:24 (the single ordinary-policy seam)
# crates/egglsp/src/download.rs:92 (independent per plan)
# crates/codegg-providers/src/provider_core.rs:34 (independent per plan)
# crates/codegg-providers/src/responses_api.rs:1231 (provider-owned)
# crates/codegg-providers/src/catalog.rs:43 (provider-owned)
# → no remaining repeated root literals; all 17 migrated root
#   call sites go through the helper

rg -n '\breqwest\b' Cargo.toml crates/*/Cargo.toml src crates --glob '!plans/**'
# src/tool/lsp_security.rs:181,511 only (scanner string literals
# matching "reqwest::" paths, not a dependency)

rg -n 'eggfetch-core.*0\.1\.4|eggfetch-core 0\.1\.4' Cargo.toml crates architecture docs .opencode --glob '!plans/**'
# (no output — active current-state text reconciled; historical
# plans/closures intentionally retain 0.1.4)

rg -n 'bytes_stream\(\)' src crates
# only provider SSE/streaming paths + mcp/remote streaming —
# legitimate per plan; bounded collection uses bytes()
```

No public-network test was added. All new fixtures are loopback.

## 5. Work-package dispositions

- **A (dependency):** implemented as specified. Packaged artifact
  qualified before editing; narrow lockfile refresh; feature/trust
  graph materially unchanged.
- **B (WebFetch/research):** implemented. Both requests set the
  request-level limit on the pinned request; collection is Eggfetch
  `bytes()` with `DecodedBodyTooLarge` mapped to the owner body-limit
  error (`response body exceeds <limit> byte limit`) and other
  transport text passed through unchanged. `untrusted_http.rs`
  deleted; its unique pinned-address regression moved to
  `tool::webfetch` owner tests. Exact/under-limit succeed;
  declared-over-limit and chunked-over-limit fail closed (proven).
- **C (Eggpool):** implemented. Request-level limit; cancellable
  `response.bytes()` via `tokio::select!` so cancellation drops the
  body future and releases transport resources with no detached
  background work; `DecodedBodyTooLarge -> Oversized`; early
  Content-Length check retained as a non-authoritative fast path;
  Timeout/TLS/auth/redirect/JSON/empty/count/string limits and
  redaction unchanged. Manual chunk loop deleted. Chunked-oversized
  and stalled-body cancellation tests added and green.
- **D (ordinary builder policy):** implemented (not rejected as
  over-abstraction). Census confirmed the repeated
  timeout + follow-true + 10-hop contract across 17 root call sites
  (key-based search providers, built-in search sources, research
  GitHub/docs.rs/crates.io/advisory, upgrade checker, image tool,
  server SDK, plugin installer, MCP OAuth). One private
  `ordinary_http_client_builder(Timeout) -> ClientBuilder` seam in
  `src/http_client.rs` replaces the repetition; callers keep
  user-agent/headers/auth. Pinned/no-follow (WebFetch, URL research,
  MCP remote), provider `create_http_client()`, provider catalog /
  Responses-API clients, and the EggLSP downloader stay independent.
  Redirect follow + ten-hop cap proven by loopback test; distinct
  timeouts preserved by caller-supplied argument (10s/15s/20s/30s/120s
  call sites unchanged); invalid auth/header construction still fails
  at build (`rejects_invalid_authorization_header_at_construction`
  green); helper contains no retry/endpoint/auth/parsing/response
  logic and no global state.
- **E (`send_detailed()`):** explicitly deferred without penalty.
  Current provider conversion (`From<eggfetch_core::Error>`) keeps
  only secret-safe transport classes (`Timeout`, transient
  `Transport{kind}`, stable `Api(kind)`); Eggpool distinguishes
  Timeout/TLS with generic `Unreachable` otherwise. No current
  user/domain contract requires DNS-vs-refused detail, and no narrow
  caller becomes materially simpler or more correct: adoption would
  require touching every provider request call site and widening
  domain/public error contracts without a consumer. `RequestFailure`
  / `NetworkFailureKind::{Dns, ConnectionRefused, Connect}` remain
  available upstream for a future plan with a concrete consumer.
- **F (docs/closure):** implemented in this record. Active docs
  reconciled (below); no new CI lane, scanner, or size gate.

## 6. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | — | — | — |

No low findings are withheld: the `windows-sys` companion bump is
classified above as attributable lockfile churn, not a finding. The
`observed` byte-count diagnostic from the deleted helper is
intentionally not preserved per plan §6.2 (not a security invariant;
no tested external contract depended on it).

## 7. Documentation and operations

Updated active current-state text (historical plans/closures untouched):

- `docs/dependency-maintenance.md` — supported Eggfetch floor `0.1.5`
  and Eggfetch-owned decoded-body limiting vs CodeGG-owned SSRF /
  static routing / error projection;
- `architecture/security.md` — Eggfetch-owned body-limit section
  replaces `untrusted_http.rs` API; bounded-body invariant and test
  command updated; module table entry removed;
- `architecture/provider.md` — transport baseline `0.1.4` → `0.1.5`;
- `architecture/client.md` — client transport baseline `0.1.4` →
  `0.1.5`.

No new crate, facade, singleton, retry owner, TLS mode, protocol
feature, permanent scanner, CI lane, size gate, or release automation
was added. Existing source/network guards continue to recognize
Eggfetch as network-capable code (`scanner_catches_eggfetch_and_legacy_http_clients`
green).

## 8. Roadmap disposition

M001 is closed with accepted evidence. The HTTP Client Maintenance
Consolidation roadmap closes with this record if the roadmap owner
accepts it (single-milestone workstream; M001 was its only milestone).

Registry audit for unblocking: no other registered implementation plan
lists this milestone as a hard/interface dependency. The subsystem
roadmap has no M002. The unrelated blocked/conditional items remain
unchanged (dependency-security M005 generalized updater interface;
architecture-convergence M009 compatible-host evidence; runtime-safety
C002 supported-Linux evidence). No future plan status transition is
required beyond marking this M001 closed.

## 9. Registry updates

- Marked `plans/implementation/http-client-maintenance-consolidation/001-eggfetch-0.1.5-policy-and-body-ownership.md`
  `implemented` (status line updated separately if the registry
  convention requires it).
- Marked subsystem `HTTP client maintenance consolidation` M001 `closed`
  with this closure record and implementation commit
  `72f6610d`.
- Recorded that no future plan was unblocked by this closure.
- Preserved unrelated blockers.

## 10. Recommendation

**Closed.** All acceptance criteria are met with evidence; no
medium-or-higher finding remains; verification is green without new
continuous gates.
