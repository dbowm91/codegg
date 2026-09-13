# HTTP Client Consolidation M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/http-client-consolidation/001-transport-boundary-and-pinned-http-adoption.md`

Source subsystem roadmap:

- `plans/subsystems/http-client-consolidation-roadmap.md#9-milestones`

Repository baseline reviewed: `a848872573c0604eb52a3ae1f46b6edef0c3eb29`

Implementation commits or pull requests:

- `756e036` — `feat: adopt eggfetch for pinned HTTP paths`
- `2a37be3` — `test: cover MCP pinned transport snapshot`

## 1. Executive finding

M001 is complete and closed. The published crates.io `eggfetch-core 0.1.4` artifact is used directly by the root crate with the scoped `http1`, `tls-rustls`, and `json` features. `codegg-core` no longer owns reqwest URL or error types. The bounded untrusted-body collector, built-in WebFetch, direct URL research, and remote MCP now use Eggfetch; validated WebFetch/research/MCP address snapshots are attached to the actual requests, while logical URL Host/TLS identity remains authoritative.

The application-level WebFetch retry still performs a fresh validation, redirects remain disabled on the security-sensitive paths, and the existing body limits and protocol/error behavior remain in place. No unresolved medium-or-higher finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Published `eggfetch-core 0.1.4` is usable | `cargo info eggfetch-core@0.1.4`; locked registry dependency | pass | Published crates.io artifact; MIT; MSRV 1.80; required `http1`, `tls-rustls`, `json`, static-routing, stream, content-length, timeout, and redirect APIs present. |
| Core remains transport-neutral | `crates/codegg-core/Cargo.toml`, core URL/error code, core tests, dependency census | pass | Core owns `url = "2"` and CodeGG-owned `HttpError`; it has no direct reqwest or Eggfetch dependency. |
| Bounded bodies reject early and while streaming | `security::untrusted_http` focused suite: 6 passed | pass | Declared `Content-Length` and cumulative streamed chunk limits are both retained. |
| WebFetch uses validated request-scoped routing | `src/tool/webfetch.rs`; focused WebFetch suite: 4 passed | pass | Original URL is retained; retry revalidates and pins a fresh target; redirects are disabled. |
| Direct URL research uses validated request-scoped routing | `src/research/sources/url.rs`; research suite: 123 passed | pass | 30-second total timeout, no redirects, bounded 5 MiB body, and accepted address snapshot are preserved. |
| MCP actual JSON requests use the revalidated snapshot | `src/mcp/remote.rs`; MCP focused test: 1 passed | pass | The POST request receives the revalidated IP/port snapshot; the new deterministic test verifies logical URL, wire target, and session header separation. |
| Logical Host/TLS identity remains authoritative | `.invalid` pinned-address regression and MCP request-builder regression | pass | Requests retain the logical hostname while static routing controls only the socket destination. |
| Redirects remain disabled | Eggfetch client construction in WebFetch, URL research, and MCP | pass | No automatic redirect path was introduced. |
| Transport-neutral HTTP error mapping is preserved | `codegg-core` targeted `HttpError` test; root Axum adapter | pass | Numeric status is preserved when available; ordinary transport errors remain gateway/server class without raw URL coupling. |
| Broad repository posture is green | `cargo clippy ... -D warnings`; `scripts/verify.sh quick`; workspace/core tests | pass | See exact commands and results below. |
| No unresolved medium-or-higher finding | Closure review of code, tests, dependency tree, and security invariants | pass | Remaining provider and ordinary-consumer reqwest ownership is explicitly M002/M003 scope. |

## 3. Production implementation evidence

- Root `Cargo.toml` directly owns `eggfetch-core 0.1.4` with `default-features = false` and only `http1`, `tls-rustls`, and `json`. Root reqwest remains temporarily for unmigrated M002/M003 consumers.
- `crates/codegg-core` directly owns `url = "2"`, uses `url::Url`, and stores sanitized `HttpError { message, status }` data rather than a transport error object. The root error adapter remains a compatibility boundary for unmigrated reqwest callers and strips reqwest's attached URL before mapping.
- `src/security/untrusted_http.rs` consumes mutable Eggfetch responses, checks `content_length()` before reading, then handles fallible stream acquisition and per-chunk cumulative limits.
- `src/tool/webfetch.rs` uses one Eggfetch client per request flow, explicit total timeout, no redirects, and `.resolved_addresses()` on each attempt. Its 403/503 retry calls URL validation again.
- `src/research/sources/url.rs` uses the same accepted-address model, explicit timeout, no redirects, and the existing bounded content extraction/hash behavior.
- `src/mcp/remote.rs` uses Eggfetch for JSON/SSE. Before each JSON POST it revalidates DNS, converts accepted IPs plus the logical effective port to socket addresses, and attaches those addresses to the actual request. Existing headers, OAuth/session handling, protocol metadata, heartbeat/reconnect ownership, and bounded SSE parsing remain intact.
- Active architecture documentation was updated for core transport neutrality, sanitized error ownership, MCP static routing, and security-sensitive Eggfetch behavior. Historical plans and closure records were not rewritten.

## 4. Verification executed

### Commands run

The repository commands below were executed through the local `rtk` command wrapper. Root tests and verification used the host-compatible environment override shown below because the default macOS pkg-config selection pointed at an incompatible architecture-specific `liblzma` dylib.

```bash
cargo info eggfetch-core@0.1.4
cargo check -p codegg-core --all-features
cargo check -p codegg
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0 cargo test -p codegg --lib security::untrusted_http -- --test-threads=1
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0 cargo test -p codegg --lib tool::webfetch -- --test-threads=1
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0 cargo test -p codegg --lib research -- --test-threads=1
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0 cargo test -p codegg --lib mcp::remote -- --test-threads=1
cargo test -p codegg-core --all-features
cargo test -p codegg-core http_error_keeps_status_without_transport_or_url_coupling
cargo fmt --all
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0 cargo clippy --workspace --all-targets --all-features -- -D warnings
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0 scripts/verify.sh quick
cargo tree -p codegg-core | rg 'reqwest|eggfetch|url'
cargo tree -p codegg | rg 'eggfetch-core|reqwest'
```

### Results

- `eggfetch-core@0.1.4` resolved from crates.io. `Cargo.lock` records checksum `442000edd076a6f101931292d4a159d5f2b4cec29c6acd010d558af00430b327` from the crates.io registry.
- Core and root `cargo check` passed.
- Focused suites passed: untrusted HTTP 6 tests, WebFetch 4 tests, research 123 tests, MCP 1 test, and the targeted core HTTP-error test.
- `cargo test -p codegg-core --all-features` passed all 632 tests.
- Workspace Clippy with all targets/features and `-D warnings` passed.
- `scripts/verify.sh quick` passed all configured guards and workspace locked check.
- The core dependency census shows direct `url` ownership and no direct reqwest/Eggfetch edge for `codegg-core`; the root census shows direct `eggfetch-core 0.1.4` and the intentionally retained root reqwest edge for later milestones.
- The MCP focused test initially found and then fixed a test-fixture port mismatch; the final run passed. This confirmed Eggfetch fails closed for a resolved target whose port differs from the logical URL's effective port.

## 5. Invariant review

- Validated untrusted requests no longer silently perform a second DNS lookup after their accepted snapshot is attached. WebFetch and research attach the snapshot to each request; MCP revalidates immediately before each JSON POST and attaches that result to the POST.
- Logical URL identity remains separate from static routing: Host, TLS/SNI, origin policy, diagnostics, and user-visible URL data continue to come from the original URL.
- WebFetch's retry is still one explicit fresh validation/attempt, not a reused address list or generic transport retry.
- WebFetch, direct URL research, and MCP redirects are disabled.
- No proxy, H2/H3, native-roots, cookies, multipart, or compression features were enabled for the M001 Eggfetch profile.
- Body caps remain enforced both from declared length and during streamed accumulation.
- `codegg-core` does not depend on either HTTP client merely for URL/error types.
- Error formatting remains sanitized; the new core error stores only message/status data and the root reqwest adapter removes its URL before conversion.

## 6. Failure and recovery review

Eggfetch failures are mapped into the existing Tool, Research, and MCP error domains. No new background retry loop or reconnect owner was added. WebFetch retains its existing single application-level 403/503 retry and revalidation. Dropping a response stream releases the transport naturally; MCP heartbeat, reconnect, shutdown, and cancellation ownership were not moved. A request attempt uses an immutable address snapshot, while a later logical retry may intentionally obtain a new snapshot. Empty or port-incompatible resolved destinations fail before network I/O through Eggfetch's request validation.

Duplicate delivery, durable persistence, daemon restart, and lease semantics are not applicable to this transport-only milestone.

## 7. Migration and compatibility review

No user-facing configuration or schema migration is required. Existing reqwest ownership remains only in root and other unmigrated crates needed by M002/M003. `codegg-core`'s public error behavior remains status/message based, with the transport-specific type removed. Rustls/WebPKI trust behavior remains the deterministic packaged profile. The only intentional runtime behavior improvement is that remote MCP's actual socket destination is constrained to the address set that was revalidated for that request.

## 8. Security review

The pinned-address regression uses a `.invalid` logical hostname and verifies that the supplied destination is used while the logical Host remains unchanged. The MCP regression likewise uses a documentation-only address and performs no external network access. Static routing has no DNS fallback for the request snapshot, and port mismatch is rejected. Redirects and proxy features are disabled in the M001 profile. Body collection remains bounded under absent or misleading `Content-Length`. Error conversion strips attached reqwest URLs and does not introduce credentials, authorization values, or query-bearing raw transport errors into the core error model.

## 9. Documentation and operations

Updated active architecture documentation:

- `architecture/codegg_core.md` — transport-neutral core URL/error ownership;
- `architecture/error.md` — `HttpError` and the root legacy adapter;
- `architecture/mcp.md` — revalidated socket snapshot on actual MCP requests;
- `architecture/security.md` — Eggfetch request-scoped static routing and response handling.

The existing static guards and `scripts/verify.sh quick` remain the operational checks. The closure record records the host-specific linker/pkg-config workaround needed for this local macOS environment; it does not change repository configuration or CI policy.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | This host's default pkg-config selection chooses an incompatible architecture-specific `liblzma` during root linking. | Local verification needs `PKG_CONFIG_PATH=/usr/local/lib/pkgconfig MACOSX_DEPLOYMENT_TARGET=14.0`. | None for M001; retain as an operator/environment note. |

There is no unresolved medium-, high-, or critical-severity finding. Remaining direct reqwest consumers are planned M002/M003 scope, not a M001 defect.

## 11. Roadmap disposition

M001 is closed with accepted evidence. M002 may proceed: its provider migration can use the proven Eggfetch 0.1.4 feature profile, transport-neutral core error conventions, and stream/timeout handling established here. M003 remains blocked on M002 closure because it depends on provider migration and its accepted conventions.

## 12. Registry updates

- Marked the M001 implementation plan `implemented`.
- Added this closure record at `plans/closure/http-client-consolidation/001-status.md`.
- Marked M001 `closed` and M002 `ready` in `plans/subsystems/http-client-consolidation-roadmap.md`.
- Marked `plans/implementation/http-client-consolidation/002-provider-streaming-and-eggpool-adoption.md` `ready for handoff`.
- Updated `plans/registry.md` so M002 is the dependency-ready plan and M003 remains explicitly blocked on M002 closure.
