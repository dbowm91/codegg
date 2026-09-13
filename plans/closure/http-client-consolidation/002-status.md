# HTTP Client Consolidation M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/http-client-consolidation/002-provider-streaming-and-eggpool-adoption.md`

Source subsystem roadmap:

- `plans/subsystems/http-client-consolidation-roadmap.md`

Repository baseline reviewed: `a848872573c0604eb52a3ae1f46b6edef0c3eb29`

Implementation commits or pull requests:

- `a6057fb05e3dfdd9bc1bf14b36c755c45432a248` — migrate `codegg-providers` and its Eggpool/Responses transport paths to published Eggfetch 0.1.4

## 1. Executive finding

M002 is complete and strictly closed. Every direct provider-crate reqwest
consumer was migrated to crates.io `eggfetch-core 0.1.4`; provider request
formatting, status mapping, SSE/domain parsing, cancellation, bounded Eggpool
probes, and Bedrock signing inputs remain provider-owned. The provider crate
has no direct reqwest manifest or source reference.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Approved Eggfetch dependency profile | `crates/codegg-providers/Cargo.toml`, `Cargo.lock`, `cargo tree -p codegg-providers` | pass | Resolves `eggfetch-core 0.1.4` with `http1`, `tls-rustls`, and `json`; no provider-owned reqwest. |
| Shared timeout/pool/redirect behavior | `provider_core::create_http_client`; `shared_client_follows_redirects_and_enforces_ten_hop_bound` | pass | 60s total, 10s connect, 32 idle connections per host, 30s idle-pool timeout, redirects enabled and capped at 10. |
| Provider request and response migration | provider package tests and package clippy | pass | OpenAI-family, Anthropic, Google, Azure, OpenRouter, OpenCode Zen, Bedrock, model discovery, and shared SSE paths compile and pass. |
| Typed headers and JSON/body error boundaries | OpenAI-compatible tests; fallible `.json()`/`.bytes_stream()` handling | pass | Standards-based header validation and explicit pre-send serialization/stream-acquisition errors are retained. |
| Streaming/cancellation/SSE behavior | provider package test suite; existing SSE and Eggpool cancellation fixtures | pass | Eggfetch byte streams feed unchanged CodeGG parser/buffer/idle-timeout state machines. |
| Eggpool bounds and error redaction | Eggpool loopback tests | pass | Bounded body reads, cancellation, no-follow redirects, status classification, and secret-safe diagnostics remain covered. |
| Transport errors do not leak credentials | `eggfetch_transport_errors_are_secret_safe_and_classified` | pass | Conversion uses stable error kinds and leaves `ProviderError::Api.url` empty. |
| Broad repository health | `scripts/verify.sh quick` | pass | All existing quick guards and workspace all-targets check passed. |

## 3. Production implementation evidence

`codegg-providers` now owns the approved Eggfetch feature profile and uses
Eggfetch clients for all provider families, live model catalog discovery,
Responses API transport, shared SSE response handling, and Eggpool probing.
The shared client explicitly maps the prior timeout, pool, and redirect
intent. Response consumption is mutable and fallible as required by
Eggfetch. The direct `http` dependency remains because provider code still
uses standards-based `HeaderName`, `HeaderValue`, and `StatusCode` types.

No provider protocol abstraction or retry policy was introduced. Bedrock still
serializes and signs the same body bytes and effective headers before sending.

## 4. Verification executed

### Commands run

```bash
cargo check -p codegg-providers --all-features
cargo test -p codegg-providers --all-features -- --test-threads=1
cargo clippy -p codegg-providers --all-targets --all-features -- -D warnings
python3 scripts/check_git_forbidden_patterns.py
cargo fmt --all -- --check
scripts/verify.sh quick
rg -n '\breqwest\b' crates/codegg-providers/Cargo.toml crates/codegg-providers/src
cargo tree -p codegg-providers --locked | rg 'eggfetch-core|reqwest'
cargo tree -i reqwest --locked
```

### Results

- Provider check passed.
- Provider tests passed: 129 tests across 2 suites.
- Provider clippy passed with `-D warnings`.
- Git forbidden-pattern guard passed with 0 findings.
- Formatting and quick verification passed, including workspace all-targets checking.
- Provider reqwest census returned no matches; provider dependency tree returned only `eggfetch-core v0.1.4` for the transport query.
- Remaining reverse-tree reqwest ownership is outside M002: root `codegg`, `egglsp`, and the `codegg-core` compatibility path. Those are the explicitly scoped M003 consumers, not provider-owned transitive leftovers.

## 5. Invariant review

- Provider payloads, headers, query construction, and signing code remain in
  their original provider modules; deterministic provider fixtures pass.
- Provider-domain status mapping remains explicit, including HTTP 429 rate
  limits and non-success response bodies.
- SSE parsing, buffer caps, per-chunk idle timeouts, and cancellation remain
  CodeGG-owned; Eggfetch supplies byte chunks only.
- No automatic Eggfetch retry policy is configured.
- The transport profile remains HTTP/1 + Rustls/WebPKI + JSON, without proxy,
  native roots, H2/H3, cookies, compression, or multipart expansion.

## 6. Failure and recovery review

Transport, JSON-build, response-body, and stream-acquisition failures now
terminate through existing provider error channels without unwraps. Dropping
an Eggfetch stream drops its body stream and releases transport resources.
Existing per-chunk timeout/cancellation loops remain in place. Shared client
cloning preserves pool reuse, and provider connection/credential lifecycle
ownership is unchanged. Eggpool rejects redirects and bounds both declared
and streamed response sizes.

## 7. Migration and compatibility review

The external provider contract is transport-neutral. Eggfetch's fallible
request/JSON/stream APIs were adapted explicitly. Ordinary provider clients
follow redirects with a ten-hop cap, while Eggpool remains no-follow.

The known TCP keepalive-duration mismatch is accepted provisionally: Eggfetch
0.1.4 does not expose reqwest's 30-second keep-idle duration through its typed
API, and this migration did not add platform-specific socket code. Existing
application-level request/stream deadlines cover correctness, and focused
loopback/stream tests show no regression. This is not a medium-or-higher
finding and does not block closure.

## 8. Security review

Authentication headers remain provider-specific and are validated before
send. Transport conversion no longer attaches raw URLs to provider errors;
this prevents query-bearing API keys from entering error data. Eggpool probe
errors remain category-only and redacted. The existing Git secret guard and
all repository quick security-adjacent guards pass.

## 9. Documentation and operations

Updated `architecture/provider.md` to describe Eggfetch ownership, the
explicit feature profile, timeout/pool/redirect behavior, and the keepalive
disposition. No new CI lane or permanent dependency scanner was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Eggfetch 0.1.4 cannot express reqwest's duration-valued TCP keep-idle setting | Long-lived idle TCP behavior is represented by application deadlines rather than that socket knob | Revisit only with a future typed upstream capability and reproducible regression evidence; no M002 action. |

No critical, high, or medium findings remain. The remaining reqwest reverse
dependencies are deliberately owned by M003.

## 11. Roadmap disposition

M002 is closed with accepted evidence. M003 is unblocked: its other hard
dependency, M001, is already closed, and M002's feature, redirect/timeout,
mutable-response, and sanitized-error interfaces are stable.

## 12. Registry updates

- Marked the M002 implementation plan `implemented` and the roadmap milestone
  `closed`.
- Added this accepted closure record and implementation commit reference to
  `plans/registry.md`.
- M003 was audited as the only registered plan blocked on M002. Its blocker is
  fully resolved, so its implementation plan and registry entry are now
  `ready`; architecture-convergence and runtime-safety conditional blockers
  remain unchanged.
