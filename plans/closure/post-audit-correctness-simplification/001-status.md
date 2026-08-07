# Post-Audit Correctness, Simplification, and Footprint Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-correctness-simplification/001-untrusted-http-safety-and-bounded-streaming.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Implementation commits or pull requests:

- `94532c650e838b65c1f06c83d5d7809f1a291f3f` — bounded untrusted HTTP bodies, effective output caps, and SSRF address pinning.

## 1. Executive finding

M001 is complete. Built-in WebFetch and research URL collection now resolve and validate each request target once, connect through reqwest's validated-address override with direct connections, and collect bodies under a cumulative streaming byte cap. The framework output cap is now an actual outer bound. No generalized provider HTTP abstraction, protocol change, storage change, or network-dependent test was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Framework output cap is effective | `effective_output_limit()` and less-than/equal/greater-than tests in `src/tool/webfetch.rs` | pass | Uses `requested.min(framework)`; UTF-8 boundary test also passes. |
| WebFetch body is bounded while streaming | `read_body_bounded()` used by text and image paths; helper fixture tests | pass | No `response.bytes()` whole-body collection remains in the M001 consumers. |
| Research URL body is bounded while streaming | `UrlSource::fetch_url()` uses the shared helper | pass | Declared and chunked bodies are rejected before full accumulation. |
| Declared oversized body is rejected early | Content-Length fixture test | pass | Preflight rejects a 6-byte body against a 5-byte limit. |
| Chunked over-limit body is rejected cumulatively | Chunked local fixture test | pass | The second chunk crosses the limit and returns a typed error. |
| Exact and under-limit bodies remain intact | Exact and chunked-under-limit local fixture tests | pass | Byte-for-byte results are preserved. |
| SSRF validation covers all candidates | Mixed public/loopback candidate test plus existing SSRF suite | pass | Any forbidden candidate rejects the target. |
| Actual connection is pinned to validated addresses | Reqwest 0.12.28 `ClientBuilder::resolve_to_addrs`; `.no_proxy()`; local `.invalid` hostname fixture | pass | The fixture is reachable only through the pinned socket and preserves the original Host header; a second DNS lookup would fail. |
| Retry revalidates and repins | Fresh `validate_url_target()` and client construction in the 403/503 retry path | pass | No first-attempt DNS assumption is reused. |
| Redirect policy remains safe | WebFetch and research clients use `Policy::none()` | pass | No redirect-following framework was added. |
| Content-type behavior and provenance remain available | Existing image base64, HTML extraction, text decoding, and structured provenance paths remain in place | pass | Only body acquisition changed. |

## 3. Production implementation evidence

`src/security/ssrf.rs` now provides the private `ValidatedUrlTarget`, which stores the normalized host and complete validated socket-address set. The M001 clients pass that set to reqwest 0.12.28's `resolve_to_addrs`; they disable proxies so the validated direct destination controls the actual socket path while the original URL continues to supply TLS/SNI and HTTP Host semantics.

`src/security/untrusted_http.rs` owns the small private bounded-body helper and typed errors. It rejects invalid zero limits, preflight-rejects oversized Content-Length values, and checks each streamed chunk before extending the retained vector. WebFetch and `UrlSource` retain their own output decoding, source records, and provenance responsibilities.

## 4. Verification executed

### Commands run

```bash
rtk cargo check --lib
rtk cargo test --lib security::ssrf -- --test-threads=1
rtk cargo test --lib security::untrusted_http::tests -- --test-threads=1
rtk cargo test --lib tool::webfetch -- --test-threads=1
rtk cargo test --lib research::sources -- --test-threads=1
rtk cargo test --test ssrf -- --test-threads=1
rtk git diff --check
rtk scripts/verify.sh quick
```

### Results

All commands passed. Focused results were 19 SSRF tests, 6 bounded-body/pinning tests, 4 WebFetch tests, 24 research-source tests, and 29 standalone SSRF integration tests. `scripts/verify.sh quick` passed its formatting, generated-asset, static-guard, and `CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked` stages.

## 5. Invariant review

- HTTP and HTTPS are the only accepted schemes in the new target validator; existing scheme restrictions remain unchanged.
- Loopback, private, link-local, multicast, CGNAT, mapped, and other special-use addresses remain rejected. Every resolved candidate is checked.
- The request client uses the already validated address set and no proxy path; it does not perform a fresh hostname resolution. Retries perform a fresh resolve/validate/pin cycle.
- The original URL is sent unchanged, preserving hostname-based TLS/SNI and Host behavior. The local fixture asserts the Host header.
- Automatic redirects are disabled for both in-tree untrusted URL consumers; no redirect can inherit trust from a prior target.
- Body retention is capped before each chunk is appended. A crossing chunk is observed and rejected without being appended.
- Body byte limits and post-decoding output-character/byte limits remain separate; the requested output limit cannot exceed the framework limit.
- Existing external-untrusted trust/provenance labeling remains unchanged.
- Errors are typed at the helper boundary and mapped to ordinary tool/research errors; no panic path was added.

## 6. Failure and recovery review

There is no persistent state, delivery protocol, daemon restart, or migration surface in M001. Malformed URLs, empty resolutions, forbidden addresses, invalid body limits, oversized declared bodies, streamed overflow, transport errors, and disabled redirects all fail as ordinary typed tool/research errors. A retry is treated as a new request attempt and repeats validation and pinning.

## 7. Migration and compatibility review

No storage schema, protocol, CLI/tool schema, or configuration migration is required. Public URL bodies within the limit retain their existing decoding and extraction behavior. Oversized bodies now fail before full buffering rather than being downloaded and rejected afterward. Research URL redirects are now rejected because following an unvalidated redirect would violate the actual-destination invariant; this is an intentional fail-closed compatibility change, not a redirect expansion.

## 8. Security review

The primary security correction is closing the DNS-validation/connection-resolution TOCTOU boundary. Reqwest 0.12.28 documents `resolve_to_addrs` as the per-domain DNS override; passing the complete validated set to a client built for that attempt prevents reqwest from consulting a changed hostname result. Disabling proxies prevents an environment proxy from becoming an unvalidated alternate connection path. Body limits bound denial-of-service memory exposure for both Content-Length and chunked responses. No secrets are logged or introduced.

## 9. Documentation and operations

Updated the M001 implementation status, subsystem milestone status, active registry, and this closure record. The closure records the locked reqwest mechanism and local deterministic fixture evidence. No new static guard or CI lane was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Other legacy HTTP consumers in the repository still use their pre-existing validation/revalidation paths and were outside M001's explicit scope. | They do not affect the two M001 consumers, but they do not inherit the new pinned-target abstraction. | Revisit only if a future plan expands the untrusted HTTP boundary; no M001 action is required. |

No critical or high-severity M001 finding remains.

## 11. Roadmap disposition

Milestone closed. M002-M007 were already independently ready and remain ready. M008 is not unblocked: its hard dependency still includes closure of M002-M007. No future plan became dependency-ready as a result of M001; the registry and roadmap now record M001 as closed and M008 as blocked only on the remaining milestones.

## 12. Registry updates

- Marked the implementation plan `implemented`.
- Removed M001 from the dependency-ready table and recorded its closure record.
- Marked M001 closed in the subsystem roadmap.
- Kept the post-audit subsystem active, with M002-M007 ready and M008 blocked on those remaining milestones.
