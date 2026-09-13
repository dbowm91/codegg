# HTTP Client Consolidation M002 — Provider Streaming and Eggpool Adoption

Status: implemented

Repository baseline reviewed: `a848872573c0604eb52a3ae1f46b6edef0c3eb29`

Source subsystem roadmap:

- `plans/subsystems/http-client-consolidation-roadmap.md`

Predecessor:

- `plans/implementation/http-client-consolidation/001-transport-boundary-and-pinned-http-adoption.md`
- required closure: `plans/closure/http-client-consolidation/001-status.md`

Canonical references:

- `plans/000-long-term-specification.md` §4.2 — explicit ownership;
- `plans/000-long-term-specification.md` §11 — daemon-owned provider connections and Eggpool;
- `plans/002-long-term-roadmap.md` Phase 2 — provider connection runtime.

## 1. Objective

Replace reqwest throughout `codegg-providers` with the crates.io `eggfetch-core 0.1.4` transport established by M001 while preserving provider request formatting, authentication, signing, model discovery, rate-limit/error classification, streaming/cancellation behavior, SSE parsing, and Eggpool probe bounds.

This is a transport substitution, not a provider redesign.

## 2. Readiness

Do not begin until M001 closes with:

- published `eggfetch-core 0.1.4` proven usable in CodeGG;
- transport/error conversion conventions established;
- broad verification green;
- no unresolved medium-or-higher security finding.

If M001 required a materially different Eggfetch feature set or error model than this plan assumes, update this plan before execution.

## 3. Current evidence

`crates/codegg-providers/Cargo.toml` owns:

```toml
reqwest = { version = "0.12", default-features = false, features = ["stream", "json", "rustls-tls"] }
```

`provider_core::create_http_client()` centralizes the ordinary provider client with:

- 60 second request timeout;
- 10 second connect timeout;
- 32 idle connections per host;
- 30 second idle pool timeout;
- 30 second TCP keepalive request;
- fallback to a default reqwest client if builder construction fails.

Provider modules then layer their own HTTP contracts on that client. Current streaming consumers include OpenAI, OpenAI-compatible adapters, Anthropic, Azure, Google, OpenRouter, Bedrock, OpenCode Zen, Eggpool and the Responses API/shared SSE parser paths. They use combinations of:

- `.json(...)` request bodies;
- provider-specific headers and query parameters;
- status inspection, including explicit 429/rate-limit handling;
- `Response::text()` / `Response::json()` for error/model discovery;
- `bytes_stream()` with CodeGG-owned SSE framing;
- per-chunk idle timeouts and cancellation;
- bounded Eggpool probe body collection.

`ProviderError` currently has a blanket `From<reqwest::Error>` that attaches reqwest's URL object to a CodeGG error. Eggfetch errors do not carry that URL object and CodeGG should not reintroduce raw URL coupling simply to preserve an implementation detail.

## 4. Invariants

- Provider protocol payloads and headers remain byte/JSON compatible except where a pre-existing test already permits variation such as header ordering.
- Credentials are never logged or inserted into unsanitized error URLs/messages.
- Model-visible provider/tool behavior is unchanged.
- Existing provider-specific status mapping remains intact: rate limiting, auth failures, model-not-found behavior, stream errors and circuit behavior are not flattened into one transport error.
- Provider SSE/domain parsing remains CodeGG-owned. Eggfetch supplies body chunks only.
- Existing stream idle/cancellation behavior remains in place; do not replace it with an opaque transport retry.
- No automatic Eggfetch retry policy is added.
- The provider transport remains HTTP/1 + Rustls/WebPKI + JSON for migration parity; no H2/H3/proxy/native-root feature expansion.
- Connection reuse remains enabled and bounded according to the existing provider pool intent.
- Bedrock signing and any provider request-signature/canonicalization logic must be computed over the same effective method/path/query/headers/body as before.

## 5. Scope and non-goals

In scope:

- `crates/codegg-providers/Cargo.toml` and lockfile resolution;
- `provider_core::create_http_client()`;
- `ProviderError` transport conversion;
- every provider source file that directly names reqwest types/status/header/request/response APIs;
- shared SSE/parser helpers that consume reqwest responses/streams;
- Eggpool provider/probe HTTP flows;
- provider-focused tests and active provider architecture docs directly affected by the transport contract.

Out of scope:

- root search/research/image/update/SDK clients — M003;
- EggLSP downloads — M003;
- new provider adapters or model support;
- changes to auth storage/resolution;
- new retry/circuit behavior;
- provider SSE parser redesign;
- TCP keepalive upstream feature work;
- HTTP/2/3 enablement.

## 6. Production changes

### 6.1 Replace the provider manifest dependency

Replace reqwest with:

```toml
eggfetch-core = { version = "0.1.4", default-features = false, features = ["http1", "tls-rustls", "json"] }
```

Add a direct `http = "1"` dependency only if typed `HeaderName`/`HeaderValue` ownership is still useful after migration. Do not preserve reqwest merely for its re-exported `http` types.

Do not add `tls-native-roots`, proxy, H2/H3, cookies, compression or multipart.

### 6.2 Rebuild `create_http_client()` with explicit Eggfetch semantics

Return `eggfetch_core::Client` and explicitly configure the behavior that reqwest previously supplied:

- `Timeout { total: Some(60s), connect: Some(10s), ..Default::default() }` or the closest tested Eggfetch composition preserving the existing total/connect intent;
- `max_idle_connections_per_host(32)`;
- `idle_timeout(30s)`;
- `follow_redirects(true)` and `max_redirects(10)` to preserve ordinary reqwest redirect behavior unless a specific provider client intentionally overrides it.

Eggfetch `ClientBuilder::build()` is infallible. Remove the reqwest builder-error/fallback branch rather than inventing a second hidden default policy.

### 6.3 Explicitly disposition TCP keepalive

Do not reproduce `tcp_keepalive(30s)` with raw platform constants. Eggfetch's public socket option layer can enable portable `SO_KEEPALIVE` but cannot express the duration-valued keep-idle setting used by reqwest.

For this migration:

- retain existing application-level stream/chunk idle timeouts and total/connect deadlines;
- do not force every provider through the advanced direct connector solely for boolean keepalive;
- record the keepalive-duration mismatch in the M002 closure record;
- treat it as a blocker only if focused long-lived provider/stream tests demonstrate a real regression.

A future typed Eggfetch keepalive builder is separate upstream work, not an excuse for platform-specific CodeGG socket code.

### 6.4 Migrate request builders deliberately

Eggfetch request APIs are similar but not source-compatible:

- `.header(name, value)` takes string references and stores header validation errors until build/send;
- `.query(key, value)` appends one pair at a time rather than reqwest's generic serialization helper;
- `.json(&value)` returns `Result<RequestBuilder>` before network I/O;
- `.body(...)`/`.bytes(...)` remain available for pre-serialized/signature-sensitive bodies.

For each provider:

1. preserve the exact URL path/query construction;
2. preserve header validation and reserved-header collision checks;
3. preserve credential/header redaction;
4. handle JSON serialization failure explicitly before `.send()`;
5. avoid serializing a body twice when signatures depend on the original bytes.

OpenAI-compatible session-affinity header validation should continue to use a standards-based header parser (`http` crate or Eggfetch `Headers`) rather than ad hoc CR/LF-only validation.

### 6.5 Migrate mutable response consumption

Eggfetch responses are single-consumption and methods require mutable access:

- status/headers may be read before body consumption;
- `text()` and `json()` take `&mut self`;
- `bytes_stream()` takes `&mut self` and returns `Result<BoxBytesStream>`.

Update provider code accordingly. Stream acquisition failure must be mapped into the same provider stream/transport domain as chunk failures; do not unwrap it.

Keep existing per-chunk timeout loops, SSE buffers, parser state and cancellation/select behavior unchanged unless the API shape forces a narrow mechanical adaptation.

### 6.6 Preserve explicit status/error semantics

Replace `From<reqwest::Error>` with an Eggfetch-aware transport conversion. Use `eggfetch_core::Error::kind()` / timeout variants where they improve existing classification, but do not broaden retryability without a provider-policy decision.

Required behavior:

- explicit HTTP 429 responses still become `ProviderError::RateLimit` where they do today;
- non-success response bodies remain available for provider-specific API errors;
- timeout transport errors should map to `ProviderError::Timeout` where CodeGG already distinguishes timeouts, otherwise preserve existing `request_error` behavior;
- `ProviderError::Api.url` must not be populated with unsanitized credential/query-bearing URLs simply because reqwest previously exposed `Error::url()`;
- call sites that need endpoint context may attach a sanitized origin/path under existing redaction rules.

### 6.7 Preserve provider-specific contracts

Audit each provider family rather than relying only on a compile pass:

- OpenAI / OpenAI-compatible / OpenRouter / OpenCode Zen: streaming chat completion headers, JSON, tool-call/reasoning adapters, 429 handling, model discovery;
- Responses API: SSE event framing, stream idle timeout and buffer cap;
- Anthropic: version/auth headers and tool streaming state;
- Google: query/API-key behavior and streaming event shape;
- Azure: deployment/API-version URL/query behavior;
- Bedrock: SigV4/canonical request and signed headers/body bytes;
- Eggpool: health/model/probe limits, cancellation, bounded body reads and error mapping;
- any additional provider module returned by a final reqwest census.

Do not consolidate provider implementations merely because they now share a transport type.

## 7. Ordered work packages

### WP1 — Provider manifest and shared client

Swap the provider dependency and implement the explicit Eggfetch timeout/pool/redirect configuration. Add focused tests for builder policy where practical.

### WP2 — Shared error and stream adapters

Migrate `ProviderError`, shared SSE parser helpers, body-stream types and common status handling before individual adapters.

### WP3 — OpenAI-family providers

Migrate OpenAI, OpenAI-compatible, OpenRouter, OpenCode Zen and Responses API paths. Run their existing request-capture and streaming tests before proceeding.

### WP4 — Anthropic/Google/Azure/Bedrock

Migrate the remaining major hosted provider protocols, with special attention to typed headers, query encoding and Bedrock signing.

### WP5 — Eggpool and remaining providers

Migrate Eggpool probe/stream paths and every remaining reqwest consumer in the crate. Preserve cancellation and bounded reads.

### WP6 — Provider crate census and closure

Require no direct `reqwest` source/manifest reference under `crates/codegg-providers/`, run focused + broad verification, and create M002 closure evidence.

## 8. Failure, cancellation, restart, and contention semantics

- Transport errors must terminate the affected provider request/stream through existing `ProviderError` channels.
- No hidden Eggfetch retry policy may duplicate CodeGG circuit/retry behavior.
- Dropping a provider stream must drop its Eggfetch body stream and release resources.
- Per-chunk timeout/cancellation remains CodeGG-owned and should still interrupt stalled provider streams.
- Shared `Client` cloning/reuse should continue to share the underlying Eggfetch client/pool rather than constructing one client per token/event.
- Provider connection lifecycle/credential rotation remains owned by existing connection-manager code.

## 9. Compatibility and migration behavior

The expected external behavior is transport-neutral. One known internal transport-policy delta is accepted provisionally: CodeGG cannot express reqwest's 30s TCP keep-idle duration through Eggfetch's current typed API. Existing stream/read deadlines must cover correctness; the closure record must say whether tests found any observable impact.

Redirects must be explicitly enabled with a 10-hop cap in the shared provider client to avoid the silent no-follow change that a mechanical Eggfetch swap would otherwise introduce.

## 10. Required tests

Use existing provider fixtures plus new narrow tests where API differences create risk. At minimum cover:

- shared client total/connect timeout intent;
- shared client redirect-follow behavior and 10-hop bound using loopback redirects;
- OpenAI-compatible request headers/body and session-affinity collision rules;
- OpenAI-family SSE chunk boundary parsing and per-chunk timeout;
- JSON serialization error before network I/O;
- model discovery JSON decode;
- explicit 429 mapping;
- non-success response body mapping;
- Anthropic auth/version headers and tool-event streaming;
- Google/Azure query encoding;
- Bedrock canonical/signature fixtures unchanged;
- Eggpool bounded probe response and cancellation;
- stream acquisition error mapped without panic;
- sanitized transport errors contain no API keys/bearer tokens.

No external provider credentials or Internet access are required for normal verification.

## 11. Verification commands

Use focused test names present in the crate, plus:

```bash
cargo test -p codegg-providers --all-features -- --test-threads=1
cargo clippy -p codegg-providers --all-targets --all-features -- -D warnings
rg -n '\breqwest\b' crates/codegg-providers/Cargo.toml crates/codegg-providers/src
cargo tree -p codegg-providers | rg 'eggfetch-core|reqwest'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

The `rg` command is a temporary closure census, not a new static CI framework.

## 12. Documentation updates

Update active provider architecture documentation where it names `reqwest::Client`, its pool behavior, or reqwest-specific error conversion. Keep protocol/auth documentation unchanged unless implementation actually changes behavior.

The broad repository dependency cleanup remains M003.

## 13. Acceptance criteria

- `codegg-providers` resolves `eggfetch-core 0.1.4` from crates.io with only the approved migration features.
- No direct reqwest dependency or reqwest type remains in the provider crate.
- Shared client preserves 60s total / 10s connect intent, 32 idle-per-host, 30s idle-pool timeout and ordinary 10-hop redirect following.
- The TCP keepalive-duration delta is explicitly recorded and has no demonstrated medium-or-higher regression.
- Provider request JSON/header/query/signing behavior remains covered by deterministic fixtures.
- Streaming uses Eggfetch byte streams while all CodeGG SSE parsers, buffer caps, per-chunk timeouts and cancellation semantics remain intact.
- Rate-limit/auth/model/stream/error classifications remain intentional and tested.
- Eggpool probes remain bounded and cancellable.
- Focused provider tests and broad verification pass.
- M002 closure contains no unresolved medium-or-higher finding.

## 14. Stop conditions

Stop and record a blocker if:

- a provider requires a reqwest behavior that cannot be represented without enabling an out-of-scope Eggfetch feature;
- Bedrock signing or another authenticated protocol changes effective bytes/headers in a way existing fixtures cannot reconcile;
- provider streaming loses cancellation/idle-timeout behavior;
- the keepalive-duration mismatch produces a reproducible operational regression;
- the migration would require automatic transport retries that conflict with CodeGG provider/circuit ownership;
- a proposed shared abstraction starts absorbing provider protocol policy rather than transport construction only.

## 15. Closure evidence requirements

Create `plans/closure/http-client-consolidation/002-status.md` after implementation. Record:

- resolved Eggfetch version;
- implementation commit;
- provider reqwest census result;
- focused provider/SSE/signing/Eggpool test results;
- TCP keepalive-duration disposition;
- broad verification results;
- any remaining transitive reqwest package and why it is not provider-owned.

## 16. Handoff summary

Use M001's proven Eggfetch/error conventions. Migrate the provider shared client first, then common error/stream helpers, then provider families in bounded groups. Preserve all provider-domain parsing and cancellation. Do not proceed to final root/EggLSP reqwest retirement until the provider crate is clean and M002 has accepted closure evidence.
