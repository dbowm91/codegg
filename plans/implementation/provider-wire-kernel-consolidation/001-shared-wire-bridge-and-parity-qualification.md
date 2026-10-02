# Provider Wire-Kernel Consolidation Milestone 001 — Shared-Wire Bridge and Parity Qualification

Status: implemented

Repository baseline: `53dea47f` (rebased on latest `origin/main`)

Source roadmap:

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md#milestone-001--shared-wire-bridge-and-parity-qualification`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md#phase-2--eggpool-and-daemon-owned-provider-connections`

Applicable ADRs:

- None required. This milestone adds a pinned neutral sibling crate and an internal bridge without changing CodeGG's direct-provider/runtime ownership. Stop if implementation would require the EggPool daemon or EggPool root runtime as a dependency.

Primary class: infrastructure

## 1. Objective

Establish the qualified `eggpool-wire` dependency and a single CodeGG semantic bridge, then prove request/stream parity against existing provider behavior before switching any production provider implementation.

The milestone must also eliminate the architectural need for `openai_compatible.rs` to maintain a second model-name policy table by defining a bounded provider-facing projection from CodeGG's existing declarative model-adapter authority.

## 2. Why this milestone is ready

EggPool request-admission-wire M006 is closed. Its closure records the immutable consumer revision `f05b18b7358d9a4125d1e20c491151eec265e403`, which provides canonical-origin requests, consumer-oriented surface encoding/options, and bounded completed-tool-call accumulation. No other hard dependency remains.

## 3. Current implementation evidence

At the baseline:

- `crates/codegg-providers/src/provider_core.rs` owns `Provider`, `ChatRequest`, `Message`, `ContentPart`, `ToolDefinition`, `ChatEvent`, `TokenUsage`, registry/credential contracts, and `ProviderRequestContext`.
- `crates/codegg-providers/src/openai.rs`, `openai_compatible.rs`, `anthropic.rs`, `google.rs`, `azure.rs`, `openrouter.rs`, and `sse_parser.rs` implement overlapping wire grammars that EggPool now centralizes in `eggpool-wire`.
- `tests/provider_transcripts.rs` freezes important CodeGG behavior around assistant/tool serialization, interrupted history, multiple tools, denied/empty tool results, and Laguna private reasoning/tool aliases.
- `crates/codegg-core/src/model_profile/adapter.rs` plus `assets/model-adapters/*.toml` are the declarative model-policy authority. `openai_compatible.rs::request_policy` separately hard-codes Laguna aliases/reasoning/thinking rules because the provider crate cannot depend on `codegg-core`.
- The main agent path moves `request.system` into canonical `Message::System` before provider submission. Existing provider builders do not all interpret that representation identically.
- Direct OpenAI emits `stream_options.include_usage` unless configured otherwise.
- `ChatEvent::ToolCall` expects a complete call with parsed JSON arguments; current SSE code accumulates provider deltas internally.
- `codegg-core` already depends on `eggpool-model-routing` via an immutable EggPool Git revision.

## 4. Invariants that must not regress

- No production provider cutover in M001.
- No dependency from `codegg-providers` back into `codegg-core`.
- Model-specific wire policy has one declarative CodeGG authority; the provider layer consumes a bounded resolved projection and does not infer policy from model names independently.
- Private reasoning history is omitted unless the resolved projection explicitly permits its provider-private round trip.
- Canonical tool authorization remains based on canonical names; wire aliases are reversed before tool execution.
- Credentials, endpoints, headers, session IDs, raw prompts, tool arguments, and provider bodies stay out of logs/diagnostics.
- CodeGG transport, deadlines, cancellation, retry taxonomy, and durable connection identity remain unchanged.
- The EggPool dependency is immutable-pinned.
- Bedrock and hosted/stateful Responses behavior is untouched.

## 5. Scope

### In scope

- Add `eggpool-wire` as a direct dependency of `codegg-providers`, pinned to the immutable EggPool M006 closure revision.
- Add a narrow bridge module converting CodeGG semantic requests to `eggpool_wire::CanonicalRequest` and canonical events/usage/errors back to CodeGG types.
- Define a bounded provider-facing resolved wire-policy projection produced from `ResolvedModelAdapter` by CodeGG's higher layer, with a conservative default for callers that do not supply one.
- Consume the upstream completed-tool-call accumulator and conformance vectors.
- Build differential fixtures comparing legacy CodeGG request/stream behavior with the new bridge without changing the production provider implementations.
- Produce a provider-family migration matrix identifying exact, semantically equivalent, intentionally corrected, provider-specific, and unsupported cases.

### Explicitly out of scope

- Switching a production provider to the shared codec.
- Deleting `sse_parser.rs` or provider builders.
- Provider endpoint/auth/model discovery changes.
- Retry/fallback/circuit changes.
- Bedrock.
- Hosted/stateful Responses program execution.
- Provider-profile metadata extraction.
- Crates.io publication/version policy for `eggpool-wire`.

## 6. Required production changes

### Dependency boundary

Add the shared package only to `crates/codegg-providers/Cargo.toml` unless repository evidence proves another direct consumer. Prefer an immutable Git revision matching EggPool M006 closure for the first integration. Record the revision in closure evidence.

The dependency must not drag in EggPool root/runtime crates, Tokio/HTTP clients, SQLite, routing, account, or server machinery beyond dependencies CodeGG already owns independently.

### Request bridge

Create one provider bridge module responsible for structural conversion. It must project:

- model and streaming intent;
- system/user/assistant/tool history after CodeGG's existing interrupted-tool-history projection;
- text/image content;
- assistant tool calls and tool results with stable IDs;
- tool definitions, descriptions, parameter schemas, defer-loading semantics where representable, and tool-choice policy;
- temperature/top-p/max-output controls;
- `ResponseFormat::{JsonObject, JsonSchema}`;
- thinking budget/reasoning effort;
- no fabricated wire source surface.

Private `ContentPart::Reasoning` must be dropped by default. It may become a canonical reasoning history block only when the resolved provider wire policy explicitly authorizes that private provider round trip.

Do not serialize through JSON merely to convert between CodeGG and shared Rust types.

### Resolved provider wire policy

Remove the architectural need for provider-local model-name matching without adding a dependency cycle.

Keep `ResolvedModelAdapter` and adapter TOML authoritative in `codegg-core`. Project only provider-facing facts into a small type consumable by `codegg-providers`, conceptually including:

- canonical→wire and wire→canonical tool aliases;
- tool argument aliases;
- whether provider-private reasoning history may be returned;
- closed post-encode request transforms required by the selected model/serving stack;
- tool-choice/max-parallel controls where they genuinely affect the wire.

The exact carrier may be part of request context or another typed request field, but it must be immutable per logical request, default-conservative, redaction-safe, and must not contain credentials or arbitrary headers.

Do not duplicate regex/model matching in `codegg-providers`.

### Event/usage bridge

Map canonical stream events to CodeGG semantics:

- text/reasoning deltas;
- completed tool calls from the upstream bounded accumulator;
- usage counters with checked/saturating `u64 -> usize` conversion and an explicit mapping for cached/reasoning counters;
- finish reason;
- provider/stream errors into CodeGG's existing error taxonomy at the transport/provider boundary.

CodeGG-specific tool JSON parsing occurs only after the shared accumulator has produced a complete argument payload. Malformed completed arguments must preserve current fail-closed behavior.

### Differential qualification harness

Extend `tests/provider_transcripts.rs` or add a focused sibling target that can render/parse through both the existing and shared paths.

Classify differences rather than blindly requiring byte identity. The matrix must explicitly cover:

- OpenAI `max_tokens` vs `max_completion_tokens`;
- `stream_options.include_usage`;
- Anthropic system-message projection;
- OpenAI-compatible providers that intentionally omit optional controls;
- Laguna/private reasoning/tool/argument aliases and thinking parameters;
- Google function-call/result and multimodal shapes;
- finish/usage/cache/reasoning counters;
- malformed/incomplete/EOF stream behavior.

Any difference affecting provider acceptance, tool identity, reasoning privacy, usage, or model behavior blocks M002 until resolved.

## 7. Ordered work packages

### Work package A — Pin and boundary guard

Intent: consume only the qualified reusable crate.

Required changes: add immutable dependency, dependency-tree guard/evidence, compile-time imports limited to the neutral package.

Acceptance evidence: `cargo tree -p codegg-providers` shows no EggPool runtime stack; CodeGG MSRV remains 1.89.

### Work package B — Semantic bridge and resolved wire-policy projection

Intent: establish one translation seam and one model-policy authority.

Required changes: request/event/usage conversion, conservative reasoning behavior, upper-layer adapter projection, no provider-local model-name inference on the new path.

Acceptance evidence: focused round-trip/unit tests for every CodeGG request/event variant and policy default/alias case.

### Work package C — Shared stream accumulation/conformance

Intent: prove CodeGG can consume canonical incremental streams without its own provider-specific state machine.

Required changes: adapt upstream completed-call output to `ChatEvent::ToolCall`; replay exported upstream conformance vectors and CodeGG-specific complete-tool-call fixtures.

Acceptance evidence: arbitrary chunk boundaries/interleaved calls yield deterministic CodeGG events; overflow/malformed/incomplete cases fail boundedly.

### Work package D — Differential provider matrix

Intent: make later cutover mechanical and evidence-driven.

Required changes: compare legacy and shared request/stream paths for every targeted provider family; record accepted intentional corrections separately from unresolved divergence.

Acceptance evidence: closure matrix marks each family ready/not-ready for M002/M003 with exact blocking cases; no production provider points to the new bridge yet.

## 8. Failure, cancellation, restart, and contention semantics

The bridge and shared kernel are pure/per-request. They do not own retries, clocks, tasks, provider sockets, or durable state.

Dropping a provider stream drops the CodeGG transport and all per-stream decoder/accumulator state. No background parser task or shared global accumulator is permitted.

Malformed/overflow/incomplete shared-kernel outcomes map to typed CodeGG provider/stream errors; do not convert them into successful EOF. M001 must not alter retry budget/disposition.

## 9. Compatibility and migration

No user/config/storage migration in M001.

Existing provider implementations remain production authority. The new bridge is qualification-only until closure.

If the shared codec exposes a semantically preferable behavior that differs from legacy CodeGG (for example Anthropic system placement), classify it as an intentional correction with a dedicated regression and migration note rather than silently modifying production in this milestone.

## 10. Required tests

Focused tests:

- every `Message`/`ContentPart` variant and interrupted tool history;
- response-format conversion;
- reasoning effort/budget and private reasoning default/opt-in;
- tool/argument aliases and reverse normalization;
- complete/interleaved tool calls;
- usage/cache/reasoning mapping and integer bounds;
- canonical finish/error/incomplete/EOF handling;
- upstream conformance-vector replay.

Differential provider fixtures:

- OpenAI;
- generic OpenAI-compatible including Laguna policy;
- OpenRouter/OpenCode/Azure representative configurations;
- Anthropic;
- MiniMax Anthropic-compatible;
- Google Gemini.

Security/negative tests:

- no private reasoning under conservative/default policy;
- no alias bypass of canonical tool identity;
- malformed tool args fail before broker execution;
- redaction-safe Debug/error output.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test --test provider_transcripts
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo tree -p codegg-providers

cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run current repository verification scripts/feature matrices required by `architecture/testing.md` if the dependency graph or workspace lock changes. Do not claim hosted CI unless it actually ran.

## 12. Documentation updates

- `architecture/provider.md` — document the bridge as qualification-only in M001 and the retained CodeGG ownership boundaries.
- `architecture/model-adapters.md` — document the provider-facing projection and removal target for provider-local model inference.
- roadmap/registry and later closure record.

## 13. Acceptance criteria

- CodeGG builds against the immutable EggPool M006 closure revision.
- `codegg-providers` has no dependency on EggPool root or `codegg-core`.
- Every targeted CodeGG semantic request can be projected into the shared canonical IR without fake wire provenance.
- Private reasoning is conservative by default and explicitly policy-gated.
- Canonical stream events can produce the existing complete CodeGG tool-call events with bounded state.
- Differential evidence classifies every provider-family difference required for M002/M003.
- No production provider implementation has switched yet.

## 14. Stop conditions

Stop and report if:

- EggPool M006 is not closed or has no immutable consumer revision;
- the shared dependency pulls EggPool runtime/network/routing state;
- a dependency cycle is required to obtain model adapter policy;
- parity would require arbitrary provider JSON inside the shared canonical IR;
- private reasoning cannot be proven fail-closed;
- a targeted provider requires a wire grammar not represented by the shared kernel;
- the work would need a production cutover to obtain parity evidence.

## 15. Closure evidence required

Record:

- pinned EggPool revision and upstream M006 closure reference;
- dependency tree/MSRV result;
- bridge/policy API and authority diagram;
- focused/conformance/differential test results;
- provider-family parity matrix with intentional corrections and blockers;
- confirmation of zero production provider cutover;
- security/privacy review for reasoning/tool aliases;
- residual findings by severity and M002/M003 readiness disposition.

## 16. Handoff notes

Preserve unrelated work. The most important architectural constraint is avoiding a `codegg-providers -> codegg-core` cycle: project already-resolved adapter facts downward instead of moving resolution into the provider crate. Do not use live provider credentials for the differential harness.
