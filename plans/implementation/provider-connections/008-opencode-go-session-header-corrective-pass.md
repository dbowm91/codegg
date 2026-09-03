# Provider Connections Milestone 008 — OpenCode Go Stable Session Header Corrective Pass

Status: ready for handoff

Repository baseline: `fca5b5278873c12ea5f2d5ca15a24247d4bf019b`

Source corrective roadmap:

- `plans/subsystems/provider-opencode-session-affinity-corrective-addendum.md`

Original milestone and closure preserved by this pass:

- M007: `plans/implementation/provider-connections/007-independent-closure-ratification-and-governance-reconciliation.md`
- M007 closure: `plans/closure/provider-connections/007-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md` — canonical session identity, provider connection, secret-reference boundaries
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`
- `plans/003-planning-process.md`

Applicable architecture and implementation surfaces:

- `architecture/provider.md`
- `architecture/core.md`
- `crates/codegg-core/src/context.rs`
- `crates/codegg-providers/src/provider_core.rs`
- `crates/codegg-providers/src/openai_compatible.rs`
- `crates/codegg-providers/src/additional.rs`
- `src/agent/turn_runtime.rs`
- `src/agent/loop.rs`
- `src/exec.rs`

Primary class: corrective provider compatibility / request transport

Operational note: the provider notice received on 2026-09-03 states that OpenCode Go requests missing `x-opencode-session` may begin failing on 2026-09-06. This deadline raises implementation priority but does not justify bypassing the repository's session-identity or provider-ownership boundaries.

## 1. Objective

Correct CodeGG's direct OpenCode Go request path so every conversation-bound inference carries `x-opencode-session` with the stable CodeGG session identity for that conversation, while preserving that identity across turns, continuations, retry/fallback paths, and repeated provider calls.

At the same time, repair the adjacent generic transport defect where `OpenAiCompatibleConfig::extra_headers` is declared but never emitted.

The implementation must introduce a bounded request-context seam, not a free-form header passthrough. CodeGG already owns the canonical session identity; M008 must project that identity into provider transport metadata without allowing model text, tools, plugins, frontends, arbitrary HTTP headers, or provider configuration to invent conversation authority.

M008 is intentionally narrow. It must not redesign provider storage, Eggpool routing, authentication, model adaptation, the daemon session model, or the whole provider trait unless current repository evidence proves the narrow seam cannot be implemented safely.

## 2. Corrective findings

### 2.1 OpenCode Go currently has no session-affinity header

Current factory:

```rust
pub fn create_opencode_go(credential: Credential) -> impl Provider {
    OpenAiCompatibleProvider::simple_with_credential(
        "opencode_go",
        "OpenCode Go",
        credential,
        "https://opencode.ai/go/v1",
    )
}
```

`simple_with_credential()` constructs `OpenAiCompatibleConfig` with an empty `extra_headers` vector and no dynamic request-header policy.

`OpenAiCompatibleProvider::stream()` currently constructs the network request with authentication and `Content-Type` only. There is no `x-opencode-session` branch anywhere in CodeGG.

### 2.2 Canonical CodeGG session identity is lost before `Provider::stream()`

CodeGG already has the correct identity at higher layers:

- `TurnRunInput` contains `session_id: String`;
- `AgentLoop` retains `session_id` for the conversation runtime;
- `src/exec.rs` also has a stable execution/session identity;
- `codegg_core::context::SessionId` defines bounded canonical session validation (`MAX_SESSION_ID_LENGTH`, nonempty, no NUL/control characters).

However, `crates/codegg-providers::ChatRequest` contains only request-body/model controls:

```text
messages
model
tools
system
temperature
top_p
max_tokens
response_format
thinking_budget
reasoning_effort
```

No request context/session identity reaches `Provider::stream()`. The provider therefore cannot correctly construct a conversation-affinity header even though the daemon already knows the value.

### 2.3 `extra_headers` is a dead configuration surface

`OpenAiCompatibleConfig` declares:

```rust
pub extra_headers: Vec<(String, String)>
```

but the request builder does not apply it. At least one existing provider configuration uses that field (`Editor-Version` in the Copilot provider), so this is an independent correctness defect in the same transport seam.

Dynamic `x-opencode-session` must not be implemented by placing one value into `extra_headers`: static provider configuration cannot represent one different stable value per conversation.

## 3. Why M007 verification did not catch this

Provider M007 was an independent closure-ratification milestone for an earlier storage-layout/test discrepancy. Its expected production diff was empty. It verified provider migration idempotency, CRUD/revision behavior, closure lineage, and hosted repository evidence.

M007 did not claim to verify every upstream provider-specific request header. Existing provider tests do not capture an OpenCode Go request on a local fake endpoint and assert session affinity. Existing OpenAI-compatible request tests primarily validate JSON body construction and response normalization. The static-header field also lacks a positive network-bound assertion.

The corrective lesson is specific: provider compatibility that depends on HTTP metadata needs a wire-bound test at the transport seam; body/transcript coverage alone is insufficient.

## 4. Invariants that must not regress

### 4.1 Session identity invariants

- The authoritative source is CodeGG's existing canonical session identity, not a newly generated provider identity.
- The same CodeGG session must produce the same upstream OpenCode session value across all turns in that session.
- A turn ID, request ID, tool-call ID, provider connection ID, project ID, workspace ID, daemon instance ID, or model ID must not substitute for conversation identity.
- Different CodeGG sessions must not be collapsed to one constant/global value.
- Provider transport must not generate a random UUID per request.
- Session identity is transport metadata only and must not enter the model-visible request body, prompt, messages, tool definitions, tool results, or reasoning content.
- The provider layer must not persist session identity into provider-connection storage or credential storage.

### 4.2 Provider ownership invariants

- `OpenAiCompatibleProvider` remains the transport owner for OpenCode Go.
- OpenCode Go-specific header behavior must be configured explicitly by the OpenCode Go factory or a typed transport policy; do not scatter `if self.id == "opencode_go"` checks through generic request code if a narrow configuration/builder seam can express the contract.
- Non-OpenCode compatible providers do not receive `x-opencode-session` by default.
- Static provider `extra_headers` remain provider-owned configuration, not a caller-controlled arbitrary header map.
- Authentication and `Content-Type` retain explicit transport ownership.

### 4.3 Security and privacy invariants

- Authorization values, API keys, stored credentials, and secret references remain unchanged and never appear in logs.
- `x-opencode-session` is not a credential, but it is a correlation identifier; log only whether the required header was attached, never its raw value.
- Do not expose arbitrary frontend/inbound headers to upstream providers.
- Header names/values must be validated using the HTTP library's typed header parsing before network send.
- Static extras must not silently override or duplicate transport-reserved authorization/content/session headers.

### 4.4 Compatibility and scope invariants

- Request JSON and streaming response semantics remain unchanged.
- Existing OpenAI-compatible providers continue to compile and behave as before except that their already-configured `extra_headers` are finally emitted.
- Provider M001-M007 storage/lifecycle/selection behavior remains unchanged.
- No database migration or protocol version change is expected.
- No new CI lane, live external-provider test, benchmark, scanner, or release automation is added.

## 5. Required production changes

### 5.1 Add bounded provider request context

Extend the canonical provider request boundary with explicit request metadata carrying optional conversation identity.

Preferred shape:

```rust
#[derive(Debug, Clone, Default)]
pub struct ProviderRequestContext {
    pub session_id: Option<Arc<str>>,
}

pub struct ChatRequest {
    // existing fields ...
    pub context: ProviderRequestContext,
}
```

An equivalent small typed shape is acceptable. Do not add `HashMap<String, String>`, `headers: Vec<_>`, or provider-specific `x_opencode_session` fields to `ChatRequest`.

The request-context type is an internal provider transport projection, not a second domain identity system. Where the upper layer has a raw string, validate it against the existing `codegg_core::context::SessionId` contract before projecting its string value into provider request context. Do not make `codegg-providers` depend on `codegg-core` if that violates crate layering or creates a dependency cycle; validation should remain at the owning upper-layer boundary in that case.

Required semantics:

- conversation-bound agent turns populate `context.session_id` from the canonical turn/session identity;
- subsequent tool-call turns and continuations reuse that same value;
- provider wrappers/fallback code pass the `ChatRequest` context through unchanged;
- test/diagnostic/one-shot requests that genuinely lack conversation context may use `None` for providers that do not require it;
- an owning one-shot production surface that intentionally supports OpenCode Go must establish one invocation-scoped stable session identity before provider transport, rather than relying on the provider to synthesize one per request.

### 5.2 Populate context at the primary agent-turn boundary

`src/agent/turn_runtime.rs` already receives `TurnRunInput.session_id` before the `ChatRequest` is created. Validate/project that canonical identity there and attach it to the request.

Audit `src/agent/loop.rs` to ensure context planning, message compaction, tool continuation, steering, and repeated provider calls mutate only body/history fields and do not replace/drop request context.

The stable session identity must survive:

```text
TurnRunInput.session_id
        -> ChatRequest.context.session_id
        -> AgentLoop repeated provider turns
        -> Provider::stream(&ChatRequest)
        -> OpenAI-compatible transport policy
        -> x-opencode-session
```

Do not source the value from the current turn ID. A session can contain many turns.

### 5.3 Audit every `ChatRequest` constructor

Because adding typed request context creates a compile-visible seam, inspect every production constructor rather than blindly setting `None` everywhere.

At minimum classify and update constructors in:

- `src/agent/turn_runtime.rs` — conversation-bound; MUST carry canonical session identity;
- `src/exec.rs` — already owns `self.session_id`; SHOULD carry it;
- `src/research/llm.rs` — determine whether the request is session-bound; use the caller's canonical session when available, otherwise explicitly classify as standalone;
- `src/tool/review.rs` and `src/tool/commit.rs` — if these run inside an agent/session tool context, propagate that existing session identity rather than creating a new value; if current APIs do not expose it, do not introduce a broad refactor solely for optional OpenCode Go use without evidence;
- `src/main.rs` direct/diagnostic provider requests — explicitly standalone unless an existing session exists;
- provider fallback/tests/transcript fixtures — preserve supplied request context or use default `None` intentionally.

If any production path can select `opencode_go` while remaining `None`, decide at its ownership boundary whether it is genuinely one-shot or missing required context. For a genuinely one-shot supported invocation, create one stable invocation/session ID once and reuse it for all requests in that invocation. For an accidental missing context, correct the caller.

### 5.4 Add an explicit OpenAI-compatible session-header policy

Do not encode dynamic session affinity in static `extra_headers`.

Preferred implementation shape is a small provider-owned policy/builder on `OpenAiCompatibleProvider`, for example:

```rust
session_affinity_header: Option<HeaderName>
```

with a constructor/builder equivalent to:

```rust
.with_session_affinity_header("x-opencode-session")
```

Then configure only the OpenCode Go factory:

```rust
OpenAiCompatibleProvider::simple_with_credential(...)
    .with_session_affinity_header("x-opencode-session")
```

The exact API may differ, but the ownership must remain explicit in provider construction rather than hidden in generic model-name/provider-name heuristics.

When a provider configures a required session-affinity header:

- read the value only from typed `ChatRequest` request context;
- convert it to `HeaderValue` using normal HTTP validation;
- attach exactly one header value;
- if context is absent or invalid, fail locally before network I/O with a non-retryable `ProviderError::Api` code such as `missing_session_context` / `invalid_session_context`, unless the owning caller established a valid one-shot identity earlier;
- do not retry a missing-context error as though it were a network failure.

A provider without a configured session-affinity policy ignores `context.session_id` at HTTP header construction.

### 5.5 Make `extra_headers` real

Refactor the request-builder path so `OpenAiCompatibleConfig::extra_headers` is actually applied.

Required behavior:

1. construct the POST request;
2. attach the configured authentication header;
3. attach `Content-Type` or rely on `.json()` only if current behavior/tests prove equivalence;
4. validate/apply static configured extra headers;
5. apply required dynamic session-affinity metadata from request context;
6. serialize/send the unchanged JSON body.

Header ownership must be deterministic. At minimum reject or otherwise explicitly guard static extras that collide case-insensitively with:

- the configured authentication header;
- `Content-Type`;
- the configured dynamic session-affinity header.

Do not silently send duplicate conflicting values. A narrow internal helper for typed header parsing/application is appropriate if it removes duplication, but do not generalize this into an arbitrary provider-header DSL.

### 5.6 Preserve retry/fallback semantics

Inspect provider wrappers such as `crates/codegg-providers/src/fallback.rs` and any retry/circuit path that reuses or clones `ChatRequest`.

Required behavior:

- retries for the same logical provider request reuse the exact same session context;
- fallback between providers does not mutate the session identity;
- a fallback provider that does not define a session header simply ignores the metadata;
- no retry path calls a random session generator;
- missing required session context is non-retryable and should not burn fallback attempts unless existing fallback policy explicitly treats local request-contract errors differently.

No broad retry redesign is required unless the new test reveals context loss in an existing wrapper.

### 5.7 Do not blindly forward inbound OpenCode headers

Search server/ACP/CLI compatibility surfaces for any current inbound `x-opencode-session` handling.

If none exists, do not add generic passthrough plumbing.

If a compatibility route already maps an OpenCode client session to a CodeGG session, preserve the canonical CodeGG session identity at that mapping boundary. Raw arbitrary request headers must not bypass CodeGG's session model and flow directly into provider transport.

This avoids two sources of truth for conversation identity.

## 6. Required tests and regression evidence

### 6.1 OpenCode Go wire-capture test

Add a deterministic local HTTP test server or use the repository's existing provider fake-server seam. The test must inspect the actual request received by the server, not merely a helper-returned header map.

Required assertions:

- request 1 for session `S1` contains exactly one `x-opencode-session: S1`;
- request 2 for the same session `S1` contains the same value;
- a request for session `S2` contains `S2` and is different from `S1`;
- the request JSON body does not contain `S1`/`S2` merely because session metadata was attached;
- raw session values are not required in test logs.

The test may construct an OpenAI-compatible provider pointed at the local server using the same session-affinity policy as `create_opencode_go()`; do not require live OpenCode Go access.

### 6.2 Missing-session negative test

For a provider configured with required session affinity:

- send a `ChatRequest` with no session context;
- assert a typed/non-retryable local provider error;
- assert the fake server received zero requests.

This prevents a future refactor from restoring silent omission.

### 6.3 Non-OpenCode isolation test

Use the same `ChatRequest` containing a session ID against a normal OpenAI-compatible provider without session-affinity policy.

Assert that `x-opencode-session` is absent.

This proves request metadata does not become a global header leak.

### 6.4 `extra_headers` positive and collision tests

Add coverage proving:

- a configured benign static header such as `Editor-Version` or test equivalent reaches the fake server;
- header-name/value validation failures are returned locally;
- static extras cannot silently override/duplicate authorization, content type, or the configured session-affinity header.

Do not weaken existing Copilot/provider behavior to make this pass.

### 6.5 Agent-turn propagation test

Add or extend a turn-runtime/agent harness test that captures the `ChatRequest` delivered to a mock provider and asserts that the request context contains the canonical session ID supplied to the turn runtime.

A second turn in the same session should prove the same context value is retained. If existing harness structure makes two-turn setup expensive, one propagation assertion plus the provider wire test may suffice provided the closure record explains why `AgentLoop` reuses the same `ChatRequest` metadata across turns.

### 6.6 Retry/fallback preservation test when needed

If inspection shows any wrapper reconstructs `ChatRequest`, add a focused test proving session context survives that path. If wrappers pass `&ChatRequest` unchanged, record that evidence and avoid redundant test scaffolding.

## 7. Ordered work packages

### WP1 — Establish exact baseline and request-path inventory

Before editing:

1. record `git rev-parse HEAD`;
2. inspect `ChatRequest`, `Provider::stream`, `OpenAiCompatibleProvider::stream`, `create_opencode_go`, and all production `ChatRequest` constructors;
3. identify which constructors are conversation-bound versus standalone;
4. inspect fallback/retry wrappers for reconstruction or cloning;
5. confirm no current `x-opencode-session` code exists;
6. confirm `extra_headers` is not applied elsewhere by a wrapper.

If repository reality has materially changed from the baseline, update the plan/closure evidence rather than implementing against stale assumptions.

### WP2 — Add typed/bounded request context

Implement the smallest provider-request metadata type that carries optional session identity without exposing arbitrary headers.

Update production constructors deliberately. Use the canonical CodeGG session identity where available.

Compile early after this step so exhaustive `ChatRequest` construction failures expose every remaining call site.

### WP3 — Configure OpenCode Go session-affinity policy

Add the generic, explicit session-header policy/builder at the OpenAI-compatible provider transport seam and enable it only in `create_opencode_go()`.

Add missing/invalid-context failure behavior before network send.

### WP4 — Repair static `extra_headers`

Apply configured static headers with typed validation and deterministic reserved-header collision handling.

Keep this logic in the existing OpenAI-compatible request builder; do not add another HTTP client layer.

### WP5 — Add wire and propagation regression tests

Implement the tests in section 6. Prefer existing local fake-server/harness utilities where practical.

The key evidence must cross the real request-builder/network seam.

### WP6 — Documentation and closure preparation

Update `architecture/provider.md` to document:

- `ChatRequest` request context/session metadata;
- OpenCode Go's required session-affinity header;
- the fact that session metadata is not serialized into model request JSON;
- static `extra_headers` execution and reserved-header ownership.

If another architecture document already owns provider request context, update that one instead of duplicating prose.

Prepare the eventual closure record at `plans/closure/provider-connections/008-status.md` only after implementation/verification is complete. Do not mark this plan implemented/closed merely because planning files landed.

## 8. Storage, protocol, migration, and compatibility effects

### Storage

No schema change is expected. Do not store `x-opencode-session` in provider connection records, credential tables, model catalogs, or new transport tables.

### Protocol

No public protocol field is expected. CodeGG already has session identity in daemon-owned turn/runtime state. If implementation discovers a real frontend-to-daemon path that loses canonical session identity before turn construction, stop and classify that as a separate protocol/session defect rather than adding a free-form header field.

### Migration

None.

### Provider compatibility

- OpenCode Go gains the required session-affinity header.
- Other OpenAI-compatible providers remain header-isolated unless explicitly configured.
- Existing static extra headers begin working as their config surface already promises.
- JSON body shape and SSE parsing remain unchanged.

### Backward behavior

Do not preserve the previous silent omission for OpenCode Go as a compatibility mode. Once a request path selects a provider that requires stable session context, missing context should be explicit and actionable rather than sending a known-incomplete request upstream.

## 9. Failure, concurrency, cancellation, and restart semantics

### Failure

- missing required session context fails before network I/O;
- invalid HTTP header encoding fails before network I/O;
- static reserved-header collision fails deterministically rather than creating duplicate values;
- these local contract failures are non-retryable.

### Concurrency

The session ID is immutable request metadata. Concurrent turns/subagents with different canonical sessions may share the same provider instance safely because the value is read from each `ChatRequest`; it must not be stored in mutable provider-global state.

Do not implement session affinity by mutating `OpenAiCompatibleProvider.config.extra_headers` per request. A shared provider instance would then race and leak identities between sessions.

### Cancellation

No cancellation behavior changes. Header construction occurs before send and must not introduce blocking work.

### Restart

No new persistence is required. A restored CodeGG session already has its canonical session ID; newly reconstructed provider requests must project that same ID after restart.

## 10. Security review requirements

Before closure, explicitly verify:

- no session value is included in tracing/debug body previews;
- no raw header map containing authorization/session values is logged;
- static extra headers remain internal provider configuration and are not sourced from untrusted prompt/tool/front-end input;
- header parsing rejects CR/LF/control-character injection;
- authorization ownership is unchanged;
- session metadata never enters credential persistence;
- no generic inbound-header passthrough was introduced.

If a test/debug server records headers for assertions, keep fixtures synthetic and do not use real credentials/session IDs.

## 11. Verification commands

Run the smallest commands that prove the changed ownership boundaries. Exact test names may differ after implementation; record the real commands in closure evidence.

Focused provider tests, for example:

```bash
cargo test -p codegg-providers openai_compatible -- --test-threads=1
cargo test -p codegg-providers opencode -- --test-threads=1
```

Focused application propagation test, selecting the smallest owning test target:

```bash
cargo test --locked <turn-runtime-or-agent-session-propagation-test> -- --test-threads=1
```

Static/type/lint hygiene:

```bash
cargo fmt --all -- --check
cargo clippy -p codegg-providers --all-targets -- -D warnings
git diff --check
```

Repository quick verification:

```bash
scripts/verify.sh quick
```

If the focused Clippy command is not the repository's supported package shape, use the closest existing quick-verification equivalent and record the exact substitution. Do not add a new CI lane solely for M008.

Live requests to OpenCode Go are optional operator smoke evidence, not required automated closure evidence. Unit/integration closure must be deterministic and network-independent.

## 12. Static guards and documentation

No new shell guard is required by default. Prefer compile-visible typed request context plus regression tests over another repository-wide grep script.

Required durable guards are:

- typed request context rather than a free-form header map;
- explicit provider session-affinity configuration;
- missing-context pre-network test;
- non-OpenCode negative header test;
- real-wire `extra_headers` test;
- architecture documentation reflecting the implemented ownership boundary.

Add a small static guard only if implementation introduces a pattern that cannot be reliably protected by compilation/tests. Do not expand `scripts/verify.sh` or CI topology for this narrow correction.

## 13. Explicit non-goals

M008 must not:

- implement all current OpenCode proprietary/request metadata headers;
- add `x-opencode-project`, `x-opencode-request`, or `x-opencode-client` without separate evidence and scope;
- forward arbitrary client headers upstream;
- create a provider-header plugin API;
- add provider-specific session records to SQLite;
- change provider connection lifecycle/storage migrations;
- alter Eggpool routing, health, rotation, or credential semantics;
- change model profiles/adapters;
- redesign fallback policy beyond preserving request context;
- unify all provider HTTP clients;
- add external/live provider tests to CI;
- add release automation or change manual release cadence.

## 14. Acceptance criteria

M008 is implementation-complete only when all are true:

1. `ChatRequest` or an equivalent typed provider request context carries optional stable session identity without arbitrary headers.
2. The primary agent-turn path populates that context from CodeGG's canonical session identity.
3. `src/exec.rs` and every other production `ChatRequest` constructor are deliberately classified and updated rather than accidentally defaulted.
4. OpenCode Go explicitly configures `x-opencode-session` as a required dynamic session-affinity header.
5. Generic OpenAI-compatible code does not string-match OpenCode provider IDs to decide affinity when an explicit configuration seam can express it.
6. Same-session requests emit the same header value; different sessions emit different values.
7. Missing OpenCode Go session context fails locally and non-retryably before network I/O, or the owning standalone invocation establishes one stable ID before transport.
8. Non-OpenCode providers do not receive `x-opencode-session` merely because request context exists.
9. `extra_headers` is emitted on the real network request path.
10. Static extra headers cannot silently collide with authorization/content/session-owned headers.
11. No session value is added to model JSON, prompt/history, storage, or ordinary logs.
12. Retry/fallback paths preserve request context unchanged.
13. Focused tests, formatting, relevant Clippy, diff hygiene, and `scripts/verify.sh quick` pass.
14. `architecture/provider.md` or the canonical owning architecture document is updated.
15. A new closure record, not M007 history, owns the M008 disposition.

## 15. Stop conditions

Stop implementation and report/register a narrower follow-up rather than broadening M008 if any of the following is discovered:

- providing session identity requires a breaking public protocol redesign rather than using existing daemon session context;
- the only viable implementation requires a new `codegg-core` <-> `codegg-providers` dependency cycle;
- current OpenCode Go semantics require several additional headers with materially different identity/privacy ownership;
- upstream requires the header value to be something other than stable conversation/session identity;
- implementing static extra headers exposes a broader untrusted-header injection surface that requires a security design decision;
- a provider wrapper reconstructs requests in a way that requires broad provider-trait redesign rather than a small metadata-preservation fix;
- unrelated existing test failures prevent attribution after focused M008 tests pass.

Do not weaken tests, silently revert to random per-request IDs, add global mutable session state to providers, or bypass canonical session ownership to avoid a stop condition.

## 16. Required closure evidence

Create `plans/closure/provider-connections/008-status.md` after implementation with:

- exact baseline and implementation commit(s);
- changed-file list and request-path ownership summary;
- requirement-to-evidence matrix for all acceptance criteria;
- wire-capture evidence showing same-session stability and different-session separation;
- missing-session zero-network evidence;
- non-OpenCode header-isolation evidence;
- `extra_headers` positive/collision evidence;
- agent-turn request-context propagation evidence;
- retry/fallback inspection/test evidence;
- security/logging review;
- storage/protocol/migration statement;
- exact focused verification and `scripts/verify.sh quick` outputs;
- unresolved findings by severity;
- recommendation: closed, conditionally closed, corrective pass required, or blocked.

Provider M007 must remain unchanged as historical strict closure for its original scope. M008 becomes the current strict disposition only for the newly discovered OpenCode Go session-affinity and generic extra-header transport defects.