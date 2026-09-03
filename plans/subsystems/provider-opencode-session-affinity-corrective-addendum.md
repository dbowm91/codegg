# Provider Connections — OpenCode Go Session Affinity Corrective Addendum

Status: ready

Repository baseline reviewed: `fca5b5278873c12ea5f2d5ca15a24247d4bf019b`

Parent roadmap and strict historical disposition:

- `plans/subsystems/provider-connections-roadmap.md`
- `plans/subsystems/provider-tool-dvr-independent-closure-ratification-addendum.md`
- Provider M007 strict closure: `plans/closure/provider-connections/007-status.md`

Long-term references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md` — canonical session identity and provider connection boundaries
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`
- `plans/003-planning-process.md`

## 1. Corrective purpose

Provider Connections M007 remains valid historical strict closure for the storage, lifecycle, migration, and governance scope that it reviewed. A later provider-compatibility notice and source audit exposed a separate request-transport defect in the direct OpenCode Go integration.

OpenCode Go now requires `x-opencode-session` to carry one stable identifier per conversation. The upstream OpenCode client sets that header from its canonical session ID for OpenCode-backed requests. The current CodeGG OpenCode Go factory uses the generic OpenAI-compatible provider without any session-affinity configuration, while CodeGG's canonical provider `ChatRequest` does not carry session identity to the provider boundary.

The same audit found an adjacent transport defect: `OpenAiCompatibleConfig::extra_headers` is a declared configuration surface, but `OpenAiCompatibleProvider::stream()` does not apply those headers to the outgoing request.

This addendum preserves M001-M007 history and adds one narrow corrective milestone. It does not reopen provider storage, lifecycle, credential ownership, Eggpool routing, session-selection persistence, or release architecture.

## 2. Discovered corrective findings

### Finding A — OpenCode Go session affinity is dropped before transport

CodeGG already has a stable daemon/session identity at the turn runtime and agent-loop layers. `TurnRunInput` and `AgentLoop` carry a session ID, but the provider `ChatRequest` currently contains only messages, model/tool/body controls, and reasoning controls. The identity therefore disappears before `Provider::stream()`.

`create_opencode_go()` calls `OpenAiCompatibleProvider::simple_with_credential()` with `https://opencode.ai/go/v1`. No provider-specific session-header behavior is configured, and the outgoing request sends authentication plus `Content-Type` only.

The result is that direct CodeGG requests to OpenCode Go omit `x-opencode-session` even though CodeGG has the stable identity required to populate it correctly.

### Finding B — declared static OpenAI-compatible headers are not emitted

`OpenAiCompatibleConfig` contains `extra_headers: Vec<(String, String)>`. Existing provider configuration uses this surface, but the generic transport currently never iterates over or applies the configured entries when building the `reqwest` request.

M008 must make the declared header surface truthful while keeping dynamic conversation affinity separate from static provider configuration.

### Why previous verification did not catch these findings

Provider M007 reviewed a storage-layout assertion, migration semantics, provider CRUD/revision behavior, and closure governance. It made no production transport change and had no reason to exercise OpenCode Go request headers.

Existing OpenAI-compatible tests focus primarily on request bodies, response streaming, model adapters, and transcript normalization. They do not include a wire-capture assertion that a CodeGG session identity survives through the provider boundary into an OpenCode Go request header, and they do not assert that `extra_headers` reaches the network request.

## 3. Corrective invariants

- CodeGG's daemon/session identity remains authoritative. Provider code must not invent a replacement identity when a canonical session exists.
- One CodeGG conversation/session must emit one stable OpenCode session value across turns, tool-call continuations, retries, and provider-layer re-entry.
- Different CodeGG sessions must remain distinguishable; a global daemon ID, connection ID, provider ID, turn ID, request ID, or random-per-request UUID is not an acceptable substitute.
- No random fallback may be generated inside `OpenAiCompatibleProvider::stream()`. A per-request random value would satisfy header presence while violating affinity semantics.
- Session affinity is transport metadata, not model input. It must not be inserted into chat messages, system prompts, tool arguments, request JSON bodies, durable provider-connection metadata, or credential storage.
- OpenCode Go receives `x-opencode-session`; unrelated OpenAI-compatible providers do not receive that header merely because a `ChatRequest` contains session context.
- Arbitrary inbound/frontend HTTP headers are not blindly forwarded upstream. If an existing compatibility surface has a trusted session mapping, it must resolve to CodeGG's canonical session context before provider invocation.
- Header values are never logged in ordinary or diagnostic request logs. Diagnostics may report only presence/absence and the owning provider/header policy.
- Credentials, authorization headers, secret references, provider lifecycle, model selection, and connection scoping remain unchanged.
- `extra_headers` must be applied deterministically as the existing configuration contract promises, without becoming a free-form per-request injection channel.
- Reserved transport headers such as authorization, content type, and the configured session-affinity header must have one unambiguous owner; static extras must not silently create duplicate/conflicting values.
- No storage migration, protocol-version bump, new CI lane, release automation, or broad HTTP-client abstraction is introduced for this correction.

## 4. Corrective milestone

### M008 — OpenCode Go stable session-header propagation and OpenAI-compatible header correctness

Status: ready

Implementation plan:

- `plans/implementation/provider-connections/008-opencode-go-session-header-corrective-pass.md`

Class: corrective provider compatibility / request transport

Dependencies:

- hard: none beyond the historical Provider M001-M007 implementation already on `main`;
- interface: existing canonical CodeGG session identity, `Provider::stream()` / `ChatRequest`, and `OpenAiCompatibleProvider` transport seam;
- operational: OpenCode Go has announced that requests missing `x-opencode-session` may begin failing on 2026-09-06.

Exit conditions:

- the canonical CodeGG conversation/session identity is carried to the provider request boundary through typed/bounded request metadata rather than free-form headers;
- `create_opencode_go()` configures a provider-owned session-affinity header policy for `x-opencode-session` without string-matching the provider ID in generic transport code;
- two requests from the same CodeGG session emit the same `x-opencode-session` value and two different sessions emit different values;
- retries/continuations reuse the original session identity rather than generating a new one;
- an OpenCode Go production inference path cannot silently send a request without a required session identity; missing context produces a local, typed/actionable error or is supplied by the owning invocation boundary before transport;
- non-OpenCode providers do not receive the header;
- configured `extra_headers` are actually emitted and reserved-header collisions have deterministic failure/ownership semantics;
- no session/header value is leaked into request bodies, logs, protocol projections, provider metadata, or credential storage;
- focused wire-capture tests and relevant agent/provider tests pass;
- `cargo fmt --all -- --check`, `cargo clippy -p codegg-providers --all-targets -- -D warnings` (or the repository-equivalent focused Clippy command), `git diff --check`, and `scripts/verify.sh quick` pass;
- closure evidence is recorded at `plans/closure/provider-connections/008-status.md`.

## 5. Why M008 is dependency-ready

This is a bounded transport correction against already-established ownership boundaries. CodeGG already possesses a stable session identity, the turn runtime already knows that identity before provider invocation, and OpenCode Go is already implemented through the generic OpenAI-compatible provider.

No new provider protocol, storage model, credential backend, scheduler primitive, or architecture decision is needed. The corrective task is to preserve existing session identity through the provider request seam, configure one provider-specific affinity policy, make the existing static-header contract executable, and add the missing production-path tests.

An ADR is not required unless implementation discovers that session identity cannot reach the provider boundary without redesigning the `Provider` contract across unrelated backends. If that occurs, stop rather than introducing a broad request-context architecture opportunistically.

## 6. Verification posture

Verification remains intentionally narrow:

- unit coverage for request/session metadata validation and header-policy selection;
- deterministic local wire-capture tests for OpenCode Go and one non-OpenCode compatible provider;
- regression coverage for `extra_headers`;
- focused agent/turn-runtime coverage proving canonical session identity is attached to conversation-bound provider requests;
- the existing quick repository verification posture.

Do not add live OpenCode Go network tests, new hosted workflow lanes, packet capture infrastructure, generic HTTP integration frameworks, benchmarks, coverage gates, or release automation.

## 7. Deferred work remains deferred

M008 does not add or redesign:

- `x-opencode-project`, `x-opencode-request`, `x-opencode-client`, or other optional OpenCode metadata unless a separate compatibility requirement is demonstrated;
- generalized arbitrary-header passthrough from TUI, ACP, HTTP, WebSocket, plugin, MCP, or CLI clients;
- a global provider request-header DSL;
- provider connection storage/schema changes;
- Eggpool routing/session-stickiness behavior;
- broad provider-client unification;
- model-profile/adaptation behavior;
- authentication or credential rotation semantics;
- CI/release policy.

## 8. Closure disposition

Provider M007 remains immutable historical strict closure for its reviewed scope. Until M008 receives an accepted closure record, the direct OpenCode Go request-compatibility claim is not strictly closed.

M008 is the sole active corrective owner for stable `x-opencode-session` propagation and the adjacent `extra_headers` transport defect. A future closure record must identify the exact implementation revision, wire-capture evidence, missing-session behavior, non-OpenCode negative evidence, and quick-verification result.