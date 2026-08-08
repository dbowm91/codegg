# Post-Audit Correctness, Simplification, and Footprint Milestone 001 — Untrusted HTTP Safety and Bounded Streaming

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 001

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: security correctness and bounded-resource infrastructure

Dependencies:

- hard: none
- soft: none

Target closure record:

- `plans/closure/post-audit-correctness-simplification/001-status.md`

## 1. Objective

Correct the built-in untrusted HTTP fetch boundary so that:

- framework output limits are actually enforced;
- response bodies are bounded while being streamed rather than after whole-body accumulation;
- DNS/SSRF validation constrains the address used by the actual connection;
- the built-in web-fetch and research URL consumers share only the small mechanics that are genuinely identical.

This milestone is intentionally narrower than a networking subsystem rewrite.

## 2. Explicit non-goals

Do not:

- redesign provider HTTP clients, MCP remote transport, upgrade fetching, or all reqwest ownership;
- add a crawler, browser, JavaScript engine, anti-bot bypass, proxy pool, cookie jar, or redirect-following framework;
- change the external `webfetch` tool schema unless required to make an existing documented limit truthful;
- permit private/special-use destinations for convenience;
- weaken existing scheme restrictions;
- introduce a DNS cache/service or asynchronous resolver framework unless reqwest's supported resolver/address override APIs cannot safely pin the validated destination;
- create a large generic HTTP abstraction merely to remove a few repeated lines;
- add a new CI lane or network-dependent hosted test suite.

## 3. Current implementation evidence

Inspect at minimum:

- `src/tool/webfetch.rs`;
- `src/research/sources/url.rs`;
- `src/security/ssrf.rs`;
- `src/search_backend/` dispatch/framing paths that cap model-facing output;
- root reqwest/url feature configuration;
- focused tests for webfetch, research sources, SSRF, and output truncation.

Known defects at the reviewed baseline:

1. `execute_builtin()` calculates `effective_max` using `max_length.min(max_output_chars.max(max_length))`; this is algebraically always `max_length`, so the framework cap is ignored.
2. WebFetch calls `response.bytes().await` and rejects `bytes.len() > MAX_RESPONSE_SIZE` only after the complete body has been collected.
3. `UrlSource::fetch_url()` checks declared `Content-Length`, but still calls `response.bytes().await`; chunked/misreported bodies can exceed the cap in memory before truncation.
4. `validate_host_ip()` and `revalidate_dns()` prove properties about resolver results, but reqwest resolves the hostname again when opening the connection. The validation set is not bound to the actual socket destination.

## 4. Invariants that cannot regress

- Only HTTP(S) URLs are accepted by this untrusted-fetch boundary.
- Internal/private/link-local/loopback/multicast/special-use addresses remain rejected according to the accepted SSRF policy.
- Every address in a resolved candidate set must be safe; do not accept a hostname merely because one returned address is public when another is disallowed.
- The actual connection attempt must be restricted to validated addresses for that request attempt.
- TLS certificate/SNI and HTTP Host semantics continue to use the original hostname.
- Automatic redirects remain disabled for built-in WebFetch unless explicitly handled with a fresh validation cycle.
- Body bytes retained in memory must never exceed the configured hard cap by more than one bounded incoming chunk needed to detect overflow.
- Character/model output limits are separate from network/body byte limits and remain hard upper bounds.
- Existing external-untrusted provenance/trust labeling remains unchanged.
- Network failures remain ordinary typed tool/research errors and must not panic the process.

## 5. Expected production-code changes

Preferred shape:

- keep URL policy in `src/security/ssrf.rs`, but return a request-attempt structure or validated address set that can be consumed by the HTTP client;
- use supported reqwest client-builder resolver/address override behavior or an equivalent narrow connector so the actual destination is selected from the validated set;
- add a small bounded response-body helper, local to the untrusted-fetch domain, that iterates the response byte stream and fails immediately on cumulative overflow;
- use the helper from `src/tool/webfetch.rs` and `src/research/sources/url.rs` where semantics match;
- correct `effective_max` so the user/requested limit cannot exceed the framework limit;
- keep HTML extraction and image base64 behavior where currently supported, applying the byte limit before decoding/encoding;
- preserve content-type-specific output behavior.

Avoid exporting a new public abstraction unless another production consumer already needs it.

## 6. Address-pinning design requirements

The implementation must establish this sequence per request attempt:

1. parse URL;
2. extract hostname/port;
3. resolve hostname;
4. reject the request if the set is empty or any candidate violates the SSRF policy;
5. build/send the request such that connection resolution cannot escape the approved set;
6. retain original hostname for TLS/SNI/Host validation;
7. if an explicit retry is performed, repeat the safety validation/pinning step rather than reusing assumptions after an arbitrary delay.

If the selected reqwest API cannot safely pin multiple addresses while preserving normal retry/failover semantics, using one validated address for the attempt is acceptable. Retry may choose another already-validated address or perform a fresh resolve/validate/pin cycle. Do not connect to a newly resolved address without validation.

The implementation agent must verify the exact reqwest API semantics against the locked/upstream version before coding. Do not infer resolver behavior from method names alone.

## 7. Bounded-body design requirements

The helper should conceptually provide:

```text
read_body_bounded(response, max_bytes) -> bytes | BodyTooLarge
```

Required behavior:

- preflight-reject `Content-Length` values greater than the limit when available;
- still enforce the limit while streaming because Content-Length can be absent or untrustworthy;
- accumulate chunks only until the next chunk would cross the limit;
- stop reading and return a typed error when the cap is exceeded;
- reject a zero/invalid configured limit according to existing caller semantics rather than silently disabling protection;
- avoid duplicate allocation proportional to the full response where possible;
- preserve exact bytes for permitted image/base64 and text decoding behavior.

Do not add complex ring buffers or spill-to-disk because current consumers require only bounded in-memory bodies.

## 8. Ordered work packages

### Work package A — Prove current contracts

1. inspect callers of `execute_builtin()` and determine exact semantics of `max_output_chars`;
2. enumerate WebFetch retry behavior and redirect policy;
3. inspect research URL source requirements and whether it currently relies on redirects;
4. identify tests that can exercise body limits without external Internet access;
5. confirm the current SSRF special-address policy and add missing boundary fixtures only when relevant to the pinning work.

### Work package B — Fix output-cap arithmetic

1. replace the ineffective expression with an actual outer cap;
2. add focused tests for requested less-than, equal-to, and greater-than framework limits;
3. test UTF-8-safe truncation at the final output boundary.

### Work package C — Add bounded body collection

1. implement the narrow streaming helper;
2. convert WebFetch text and image paths;
3. convert research URL collection;
4. test declared-too-large, chunked-over-limit, exactly-at-limit, and under-limit bodies using a local fixture server;
5. verify the fixture cannot accidentally contact external hosts.

### Work package D — Bind SSRF validation to the connection

1. resolve and validate address candidates once per attempt;
2. configure the actual request connection to use only approved addresses;
3. add a deterministic resolver/fixture seam if needed for tests;
4. prove that a hostname whose later resolver result changes to loopback/private cannot cause the connection to use that new address;
5. verify normal public-host TLS/Host behavior remains correct through focused tests or a narrow manual smoke.

### Work package E — Consolidate only genuine duplication

1. share bounded-body mechanics between WebFetch and UrlSource;
2. keep trust/provenance/research-record construction in their owning modules;
3. avoid pulling research types into tool modules or vice versa;
4. remove obsolete DNS-revalidation code only if the pinned connection makes it redundant and tests cover the stronger invariant.

## 9. Storage, protocol, migration, and compatibility effects

Storage: none.

Protocol: none.

CLI/tool schema: no breaking change expected.

Compatibility:

- permitted public URLs should behave as before;
- oversized responses may now fail earlier instead of being fully downloaded and then rejected/truncated;
- requests relying on DNS rebinding or special-use destinations are intentionally rejected;
- output may be more strictly truncated where the documented framework cap was previously ineffective.

## 10. Focused verification

Use deterministic local fixture tests. At minimum run the affected tests plus:

```bash
cargo test --lib security::ssrf
cargo test --lib tool::webfetch
cargo test --lib research::sources
scripts/verify.sh quick
```

Adjust exact test selectors to repository reality. Do not introduce Internet-dependent tests.

If reqwest feature configuration changes, also run:

```bash
cargo tree -e features -i reqwest
cargo check --workspace --all-targets --locked
```

The workspace check is justified only if manifests/features change; otherwise quick verification is sufficient.

## 11. Static guards

Do not add a regex guard for `response.bytes()` globally.

Prefer direct unit/integration tests around the shared bounded helper and address-pinning seam. A small compile-time ownership boundary is acceptable if the new helper must remain private to untrusted-fetch consumers, but no new guard is required by default.

## 12. Acceptance criteria

M001 closes only when:

- the framework output cap is demonstrably effective;
- WebFetch and research URL bodies are streamed under a hard cumulative byte limit;
- oversized chunked responses fail before full-body accumulation;
- SSRF validation constrains the address used by the actual connection attempt;
- a deterministic test demonstrates that a post-validation DNS change cannot redirect the connection to a forbidden address;
- original hostname/TLS/Host behavior is preserved;
- existing content-type behavior remains available;
- no public network tests, crawler/browser expansion, redirect framework, or generalized provider HTTP rewrite is introduced;
- focused tests and `scripts/verify.sh quick` pass;
- closure evidence records the chosen reqwest pinning mechanism and why it binds validation to connection semantics.

## 13. Stop conditions

Stop and report rather than broadening scope if:

- the locked reqwest version cannot safely bind the connection to validated addresses without replacing the connector stack;
- fixing the issue would require changing global provider networking behavior;
- the existing SSRF policy intentionally permits address classes that conflict with the roadmap invariant;
- output-limit semantics are externally documented differently than inferred from current call sites.

In those cases, record the exact API/contract blocker. Do not weaken the safety requirement to make the milestone pass.

## 14. Required closure evidence

`plans/closure/post-audit-correctness-simplification/001-status.md` must include:

- implementation commit/PR;
- exact output-cap semantics before/after;
- address-pinning mechanism and test evidence;
- bounded-body helper behavior and over-limit fixture results;
- focused commands and outcomes;
- compatibility/security review;
- unresolved findings by severity;
- explicit confirmation that no broader networking subsystem was introduced.
