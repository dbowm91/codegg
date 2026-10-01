# Provider Wire-Kernel Consolidation Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool` — CodeGG provider abstraction, direct-provider operation, and Eggpool boundary.
- `plans/001-terminology-and-domain-model.md` — provider connection, provider/model identity, session identity, and authority boundaries.
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections` — provider connection and Eggpool integration direction.
- `plans/003-planning-process.md` — bounded cross-repository handoff and closure rules.

Related ADRs:

- None required for the initial workstream. CodeGG already consumes the neutral `eggpool-model-routing` crate from an immutable EggPool revision, and this work preserves the existing ownership choice that CodeGG may call providers directly without running EggPool. If implementation would make the EggPool daemon a required runtime dependency, move credentials/routing authority into EggPool, or create a new public provider protocol, stop and write an ADR.

## 1. Purpose and ownership boundary

This workstream consolidates CodeGG's duplicated OpenAI/Anthropic/Gemini request and streaming protocol implementations behind the reusable `eggpool-wire` sans-I/O kernel while preserving CodeGG's provider abstraction and direct-provider capability.

CodeGG continues to own:

- `Provider`, `ChatRequest`, `ChatEvent`, durable provider connections, credential resolution, and session/model selection;
- provider endpoint construction, authentication/static/session-affinity headers, model discovery, HTTP transport through Eggfetch, deadlines/cancellation, and CodeGG retry/error taxonomy;
- model-specific policy and provider-facing transforms resolved from CodeGG's declarative model adapters;
- application tool authorization/execution and textual tool repair;
- specialized transports/protocols not represented by `eggpool-wire`, notably Bedrock Converse and the higher-level hosted/stateful Responses program subsystem.

The shared kernel owns provider-neutral wire IR, finite codecs, SSE framing/decoding, canonical stream events, usage normalization, adaptation/fidelity, and bounded protocol conformance.

The workstream must not move EggPool account routing, quota, health/quarantine, fallback, provider transport, or runtime configuration into CodeGG.

## 2. Work classification

### Invariants

- CodeGG runs against direct providers without requiring an EggPool process.
- Provider credentials and endpoint/session-affinity policy remain CodeGG-owned and never enter `eggpool-wire`.
- One CodeGG semantic request is encoded once into the selected target grammar; provider payloads are never translated through another provider format.
- Private reasoning history is never returned to a provider unless the resolved CodeGG model/wire policy explicitly allows that provider-private round trip.
- Provider/model-specific transforms have one CodeGG authority; provider modules must not independently re-resolve model names into a second policy table.
- Tool calls, usage, finish/error outcomes, cancellation, and retry classification preserve CodeGG semantics.
- Shared-kernel dependency is immutable-pinned and cannot silently track EggPool `main`.
- Bedrock and hosted/stateful Responses remain operational if shared-wire adoption does not cover them.

### Capabilities

- Existing direct OpenAI/OpenAI-compatible, Anthropic-compatible, and Google provider connections continue to stream agent responses/tools.
- Existing Eggpool-as-provider connections continue to use the CodeGG provider interface.
- Existing provider/model discovery and `/connect` workflows remain unchanged.

No new user-visible provider is claimed by this roadmap.

### Infrastructure

- A narrow CodeGG ↔ `eggpool-wire` semantic bridge.
- A provider-facing resolved wire-policy projection from CodeGG model adapters without a `codegg-providers -> codegg-core` dependency cycle.
- Shared protocol encoding/stream decoding for standard provider families.
- Differential and conformance fixtures proving parity before deletion.
- Thin transport/provider wrappers after cutover.

### Polish

- Removal of duplicated provider serializers/SSE state machines once no production consumer remains.
- Architecture/provider documentation updated to the single shared protocol owner.
- Later provider-profile metadata deduplication may be planned after wire cutover; it is not part of this roadmap's implementation milestones.

## 3. Non-goals

- Do not replace `Provider`, `ProviderConnection`, credential stores, `/connect`, or session model-selection APIs with EggPool runtime types.
- Do not import EggPool root/runtime crates; only the neutral `eggpool-wire` package is eligible.
- Do not move CodeGG model adapters or tool policy into EggPool.
- Do not add Bedrock Converse to `eggpool-wire` merely to make the initial migration universal.
- Do not migrate hosted-program continuation/scheduler state into `eggpool-wire`.
- Do not remove CodeGG retry/error handling as part of wire consolidation.
- Do not fold the currently stale provider fallback/circuit implementation into this workstream; resilience-owner cleanup requires separate evidence.
- Do not consolidate provider endpoint/auth/model-catalog metadata until the shared wire path is closed and stable.
- Do not change public provider IDs or durable connection storage keys merely to simplify implementation.

## 4. Current state

At baseline `daaed6cee4697f7f565084e44629e186a941f093`:

- `crates/codegg-providers/src/provider_core.rs` defines CodeGG's semantic request/event/provider contracts and durable provider registration.
- `openai.rs`, `openai_compatible.rs`, `anthropic.rs`, `google.rs`, `azure.rs`, `openrouter.rs`, and several wrappers/factories independently serialize provider requests.
- `sse_parser.rs` plus provider-local stream parsers independently decode OpenAI/Anthropic/Google stream dialects and assemble tool-call arguments/usage.
- `openai_compatible.rs` contains a second hard-coded Laguna request policy for tool aliases, argument aliases, private `reasoning_content`, and thinking parameters even though `codegg-core` already owns declarative model adapters.
- CodeGG's direct OpenAI path explicitly requests `stream_options.include_usage`; this must not be lost during migration.
- Existing transcript tests cover tools, interrupted tool history, private reasoning, and provider-specific request bodies, but they exercise only CodeGG's local serializers.
- `codegg-core` already pins `eggpool-model-routing` from an immutable EggPool commit, establishing a sibling-crate dependency precedent.
- EggPool planning commit `597d522dd2b7f1281c5a793ad8a107547360ea79` registers request-admission-wire M006 to close the external semantic-producer gaps in `eggpool-wire`. CodeGG M001 remains hard-blocked until that upstream milestone closes and records the immutable consumer revision.

## 5. Target architecture

```text
CodeGG Agent / Session
        |
        v
ChatRequest + resolved provider wire policy
        |
        v
codegg-providers::wire bridge
        |
        +--> canonicalize CodeGG semantics
        |      -> eggpool_wire::CanonicalRequest
        |
        +--> eggpool-wire finite codec
        |      -> provider request JSON
        |
        v
CodeGG provider transport
 endpoint + auth + headers + Eggfetch + deadlines
        |
        v
provider SSE bytes
        |
        v
eggpool-wire StreamEventDecoder
        |
        v
eggpool-wire completed-tool-call accumulator
        |
        v
codegg-providers::wire event/usage bridge
        |
        v
ChatEvent
```

Model-specific policy remains outside the shared kernel:

```text
codegg-core ResolvedModelAdapter
        |
        v
bounded provider-facing WirePolicy projection
        |
        +--> tool/argument aliases
        +--> private-reasoning round-trip permission
        +--> closed post-encode field transforms
        +--> tool-choice/parallel controls
```

Provider modules configure transport, not protocol grammar.

## 6. Dependency graph

```text
EggPool request-admission-wire M006
 external semantic-producer consumer contract
             |
             | hard: closed + immutable revision recorded
             v
CodeGG M001 — shared-wire bridge and parity qualification
             |
             | hard: closure with provider parity matrix
             v
CodeGG M002 — OpenAI-family cutover
             |
             | hard: closure with compatible-family evidence
             v
CodeGG M003 — Anthropic/Gemini cutover and duplicate parser retirement
```

Provider-profile metadata consolidation is deliberately deferred until M003 closure so endpoint/auth/catalog data work cannot obscure wire-parity evidence.

## 7. Milestones

### Milestone 001 — Shared-wire bridge and parity qualification

Class: infrastructure

Objective: pin the qualified upstream `eggpool-wire` revision, build the CodeGG semantic/request/event/policy bridge, and prove differential behavior without switching production providers.

Dependencies:

- hard: EggPool request-admission-wire M006 closed with an immutable downstream revision.
- interface: existing CodeGG `Provider`/`ChatRequest`/`ChatEvent` contracts remain stable.

Deliverable boundary:

- direct `codegg-providers` dependency on the immutable shared crate;
- request/event/usage/error bridge plus bounded model-wire-policy projection;
- conformance-vector consumption and transcript differential harness;
- explicit migration-disposition matrix for every provider family and every known intentional difference;
- no production provider cutover.

Exit conditions:

- no dependency cycle is introduced;
- private reasoning remains fail-closed by default;
- system/tool/history/structured-output/reasoning/usage semantics have differential evidence;
- CodeGG can decode shared-kernel streams into complete `ChatEvent::ToolCall` events;
- known divergences such as OpenAI token-control naming, streaming usage, Anthropic system handling, and Laguna transforms are classified before M002;
- closure accepted.

### Milestone 002 — OpenAI-family shared-wire cutover

Class: infrastructure

Objective: move OpenAI Chat and OpenAI-compatible production backends onto the qualified shared codec/stream path while retaining CodeGG transport/auth/discovery identity.

Dependencies:

- hard: M001 closed positively.

Deliverable boundary:

- native OpenAI, generic OpenAI-compatible, Azure OpenAI, OpenRouter, OpenCode Zen/Go, and OpenAI-compatible additional/wrapper providers use the shared OpenAI Chat codec/decoder path where their protocol actually matches;
- model-specific aliases/private reasoning/thinking quirks are driven from the CodeGG resolved wire policy, not hard-coded model matching inside providers;
- provider endpoint/auth/static/session-affinity headers and model discovery remain CodeGG-owned;
- old OpenAI serialization/SSE code is removed only when unreferenced.

Exit conditions:

- all affected provider transcript/stream tests pass through the shared implementation;
- direct OpenAI streaming usage remains present where CodeGG requested it;
- session-affinity headers remain transport-only;
- retry/status/error classification remains CodeGG-owned;
- no Anthropic/Google/Bedrock/hosted-Responses behavior changes;
- closure accepted.

### Milestone 003 — Anthropic/Gemini cutover and duplicate wire retirement

Class: infrastructure

Objective: migrate the remaining standard shared-kernel families and remove CodeGG's duplicate OpenAI/Anthropic/Gemini wire machinery.

Dependencies:

- hard: M002 closed.

Deliverable boundary:

- Anthropic and Anthropic-compatible providers, including MiniMax where its endpoint contract matches, use `AnthropicMessages`;
- Google native provider uses the qualified Gemini surface selected by current semantics, with explicit function-call/result and multimodal parity;
- known system-prompt behavior is corrected only with a dedicated regression and documented compatibility disposition;
- duplicate provider request encoders/SSE parsers/helpers are deleted or reduced to compatibility wrappers when no longer authoritative;
- Bedrock and hosted/stateful Responses remain separate specialized paths.

Exit conditions:

- no standard OpenAI/Anthropic/Gemini provider path maintains a second SSE parser or finite request grammar;
- CodeGG provider architecture docs identify `eggpool-wire` as the shared protocol owner and CodeGG as transport/policy owner;
- full provider/root/default-feature qualification passes;
- closure accepted.

## 8. Cross-cutting requirements

### Storage and migration

No database migration or durable provider-connection identity change. Existing provider IDs, connection kinds, account IDs, endpoints, and secret references remain valid.

### Protocol and compatibility

Provider-facing JSON/SSE behavior is qualified before cutover. Byte identity is not required where the shared codec is semantically equivalent, but differences affecting model behavior, accounting, tool identity, reasoning privacy, or provider acceptance require explicit tests and disposition.

### Security and authorization

Credentials, auth headers, session-affinity values, prompt content, and tool arguments must not appear in diagnostics. Tool aliases never bypass canonical tool authorization: responses are mapped back to canonical tool names before broker/permission handling. Private reasoning remains non-user-visible and provider-return eligibility is explicit.

### Concurrency, cancellation, and recovery

The shared kernel is per-request/sans-I/O. CodeGG keeps current HTTP stream cancellation/idle bounds and retry budget. A mid-stream failure must not be retried after uncertain side effects merely because the parser changed. No global stream accumulator is allowed.

### Observability and audit

Preserve CodeGG usage/finish/error events and accounting. Shared adaptation notices may be projected as bounded diagnostics, never raw payload logs.

### Performance and resource use

Avoid serialize-deserialize bridges between CodeGG and `eggpool-wire` merely to cross the crate boundary. Stream decoding remains incremental. Dependency/binary-size deltas are recorded but are not acceptance thresholds unless material regression appears.

### Documentation and operations

Keep `architecture/provider.md`, resilience docs, and model-adapter docs aligned with actual ownership. Do not instruct operators to run EggPool for direct-provider use.

## 9. Verification strategy

Use three layers:

1. shared-kernel conformance vectors consumed directly by CodeGG;
2. differential transcript fixtures comparing current CodeGG serializers/parsers with the new bridge before cutover;
3. production-path provider tests after each family migrates.

The parity corpus must include system/developer placement, interrupted tool history, multiple/interleaved tools, tool-result pairing, images, structured output, reasoning effort/budget/private history, provider-specific aliases, session affinity, usage/cache/reasoning counters, finish/error mapping, malformed/incomplete streams, arbitrary chunk splits, and terminal evidence.

No live credentialed provider call is required for implementation closure unless repository evidence shows an existing live qualification contract already depends on one.

## 10. Risks and decision points

- A `codegg-providers -> codegg-core` dependency would create an architectural cycle. Resolve model policy through a narrow provider-facing projection from the existing core authority instead.
- CodeGG's existing provider builders are not uniformly equivalent; generic OpenAI-compatible paths intentionally omit or transform controls for some serving stacks. Do not force every compatible endpoint to identical optional fields.
- The current Anthropic builder can lose a system instruction after the agent loop moves `request.system` into a `Message::System`. Treat any shared-codec correction as an explicit compatibility fix with regression evidence rather than demanding byte-identical old output.
- CodeGG `ChatEvent::ToolCall` is complete-call oriented while the shared kernel emits incremental canonical tool events. M001 must consume the upstream bounded completed-call accumulator rather than recreating a second provider-specific state machine.
- If an affected provider relies on syntax outside the shared surface, retain a narrow specialized wrapper and record why rather than adding provider-name branches to `eggpool-wire`.

## 11. Completion definition

This roadmap closes when CodeGG's ordinary OpenAI/OpenAI-compatible, Anthropic-compatible, and Gemini provider paths share `eggpool-wire` for finite protocol encoding and streaming decode, CodeGG retains transport/auth/model-policy authority, duplicate standard wire parsers/serializers are retired, and Bedrock/hosted Responses remain clearly specialized.

Provider-profile metadata deduplication is a separate potential successor and is not required for this roadmap to close.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 001 — shared-wire bridge and parity qualification | closed | `plans/implementation/provider-wire-kernel-consolidation/001-shared-wire-bridge-and-parity-qualification.md` | `plans/closure/provider-wire-kernel-consolidation/001-status.md` | — |
| 002 — OpenAI-family shared-wire cutover | closed | `plans/implementation/provider-wire-kernel-consolidation/002-openai-family-shared-wire-cutover.md` | `plans/closure/provider-wire-kernel-consolidation/002-status.md` | — |
| 003 — Anthropic/Gemini cutover and duplicate wire retirement | closed | `plans/implementation/provider-wire-kernel-consolidation/003-anthropic-gemini-cutover-and-wire-retirement.md` | `plans/closure/provider-wire-kernel-consolidation/003-status.md` | — |
