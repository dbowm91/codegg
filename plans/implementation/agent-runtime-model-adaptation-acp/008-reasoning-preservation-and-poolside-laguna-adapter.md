# Agent Runtime, Model Adaptation, and ACP Milestone 008 — Reasoning Preservation and Poolside Laguna Adapter

Status: implemented

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-008--reasoning-preservation-and-poolside-laguna-vertical-slice`

Long-term requirements:

- `plans/000-long-term-specification.md#11-daemon-owned-provider-connections-and-eggpool`
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`

Primary research anchors:

- Poolside Laguna model cards and serving guidance under `https://huggingface.co/poolside`
- Current serving guidance for `poolside_v1` tool/reasoning parsers, automatic tool choice, thinking-enabled templates, and preservation of assistant reasoning between tool calls must be reverified during implementation.

Primary class: capability/compatibility

## 1. Objective

Extend CodeGG's provider-neutral message and turn-history model so provider-required assistant reasoning can be preserved across tool calls without becoming user-visible content. Use that capability to implement the first complete declarative model-adapter vertical slice for Poolside Laguna, including model matching, tool and argument aliases, interleaved reasoning history, thinking controls, parallelism limits, prompt/recovery policy, and serving-requirement diagnostics.

The milestone must preserve existing providers and projection privacy. It must not equate provider-round-trip reasoning state with a frontend-visible chain-of-thought feature.

## 2. Dependencies

Hard dependency:

- M007 declarative adapter registry, schema, typed transforms, resolution, and pinning.

Interface dependencies:

- M001 prompt compiler;
- M002 canonical/wire tool surface;
- M006 recovery-control placement seam where available;
- existing provider event parsing (`ReasoningDelta`), assistant/tool message history, provider request serializers, event/projection visibility policy, and context compaction/history hardening.

No live Laguna endpoint is required for routine closure; golden protocol fixtures and optional manual validation are sufficient.

## 3. Current implementation evidence

Re-audit:

- provider streaming can emit `ChatEvent::ReasoningDelta`;
- provider-neutral `Message::Assistant` currently stores visible content and tool calls but not preserved reasoning state;
- event processor/agent loop can observe reasoning deltas during one response;
- later provider requests rebuild history without a typed private reasoning field;
- session projections intentionally avoid exposing provider-private hidden reasoning;
- model profiles have generic thinking/control fields but no provider-neutral reasoning round-trip contract;
- Laguna guidance requires interleaved reasoning/tool behavior and preservation of prior assistant reasoning for subsequent calls;
- serving implementations may require matching tool/reasoning parsers and automatic tool-choice configuration.

## 4. Invariants

- Preserved reasoning is private provider-round-trip state, not ordinary message content.
- It is not serialized into ACP updates, TUI projections, audit metadata, logs, crash diagnostics, or user-visible artifacts by default.
- Provider adapters may drop private reasoning when unsupported; they must not reinterpret it as visible text.
- Existing provider message serialization remains compatible.
- Tool calls/results retain correct pairing and ordering when reasoning is present.
- Compaction/history hardening does not orphan tool calls or leak private reasoning.
- Model adapter identity and provider capabilities determine whether reasoning is preserved.
- Canonical tool names and permissions remain authoritative; Laguna aliases are wire-only.
- Serving requirements are diagnostics/config assertions, not assumptions that CodeGG can enforce on a remote server.
- Active turns pin adapter/version/behavior.

## 5. Scope

### In scope

- Add provider-neutral private reasoning content to assistant history using a typed visibility-aware representation.
- Capture reasoning deltas and associate them with the correct assistant response/tool-call batch.
- Preserve opaque or textual reasoning according to provider adapter capability.
- Serialize preserved reasoning only through providers/adapters that explicitly support/require it.
- Define compatibility behavior for providers that emit reasoning but do not accept it back.
- Ensure history hardening, retries, compaction, context accounting, and event stores treat private reasoning correctly.
- Implement built-in Laguna adapter TOML(s) for the intended Laguna family/model variants.
- Implement required typed provider transforms:
  - thinking enable/disable request field where supported;
  - interleaved reasoning preservation;
  - tool format/tool-choice policy;
  - canonical `bash`/wire `shell` or verified current alias;
  - canonical `command`/wire `cmd` or verified current argument alias;
  - max parallel tools;
  - control-message placement;
  - recovery hints;
  - model context/output defaults only where verified.
- Add serving-requirement diagnostics for parser/template/auto-tool settings.
- Add captured/golden request-response history fixtures representing at least two tool-call rounds.
- Add negative disclosure tests through projection/ACP seams.

### Out of scope

- User-facing hidden reasoning display.
- Persisting private reasoning indefinitely unless existing session history requires it and retention policy is explicit.
- Supporting every reasoning-capable provider in this milestone.
- Launching or configuring vLLM/SGLang servers automatically.
- Dynamic model probing that sends privileged repository content.
- Automatic server remediation.
- Broad model quality benchmarking.

## 6. Required production changes

### Provider-neutral message model

Add a field/type equivalent to:

```rust
pub struct ReasoningContent {
    pub data: ReasoningData,
    pub visibility: ReasoningVisibility,
    pub provider_format: Option<String>,
}

pub enum ReasoningData {
    Text(Arc<String>),
    Opaque(Arc<String>),
}

Message::Assistant {
    content: Vec<ContentPart>,
    reasoning: Option<ReasoningContent>,
    tool_calls: Vec<ToolCall>,
}
```

Exact shape may differ. Prefer opaque preservation when a provider returns a token/blob that should not be parsed. Apply explicit size bounds and redaction/logging behavior.

### Event processor/history assembly

- collect reasoning deltas separately from visible text;
- attach them to the assistant message for the same response;
- preserve ordering relative to tool calls according to provider contract;
- avoid duplicating reasoning on retry/replay;
- ensure tool-result history includes the prior assistant reasoning when the adapter requires it;
- update history-hardening logic and fixtures.

### Storage/retention

Determine whether current durable provider-message storage serializes assistant messages. If so, add backward-compatible optional fields and a private visibility/retention policy. It is acceptable to retain private reasoning only in active in-memory turn history if provider continuation does not require restart persistence; document the limitation truthfully.

Do not place reasoning bodies in session projections. If durable encrypted/opaque retention is required, stop for an explicit retention/security decision rather than storing plaintext casually.

### Laguna adapter

Reverify current model IDs and serving contract. The adapter should describe model behavior declaratively; only typed serialization transforms belong in Rust.

Example effective behavior:

- match Poolside Laguna instruct/agentic variants and exclude base models;
- tool calling enabled with one parallel call unless verified otherwise;
- explicit structured tool-call contract;
- preserve reasoning in assistant history;
- use user/control role appropriate to serving stack;
- set thinking template field when configured;
- normalize wire tool/argument aliases through M002;
- surface parser/template requirements as warnings/errors based on configured endpoint metadata.

Do not hard-code one provider hostname; support compatible endpoints when model/serving metadata is explicit.

### Disclosure and projection

- projection adapters ignore private reasoning bodies;
- usage/token summaries may include reasoning token counts if providers report them;
- logs show presence/bytes/fingerprint only, not content;
- ACP maps visible message/tool updates only.

## 7. Ordered work packages

### A — Reasoning contract and retention review

- inventory provider event/message/storage/compaction/projection paths;
- define private reasoning types, bounds, visibility, and retention;
- add failing two-round interleaved-reasoning history fixture;
- document provider accept/drop behavior.

### B — Capture and history round trip

- update event processor and assistant message assembly;
- update provider serializers through capability/adapter checks;
- preserve tool-call pairing and retry idempotency;
- update history hardening and compaction behavior.

### C — Negative disclosure boundary

- update projection, event-store, logging, artifact, ACP-preparation, and debug formatting paths;
- add tests proving bodies do not leave the provider-history boundary;
- bound or omit retention according to the accepted decision.

### D — Laguna adapter and typed transforms

- reverify model/serving guidance;
- add adapter TOML and required typed transforms;
- implement tool/argument aliases, thinking field, interleaving policy, parallelism, prompt/recovery settings, and serving diagnostics;
- add effective-adapter inspection output.

### E — Golden fixtures and optional manual validation

- capture representative OpenAI-compatible Laguna request/response fixtures;
- test two or more tool rounds with reasoning preservation;
- document an optional manual vLLM/SGLang validation recipe without making it a routine CI dependency;
- update architecture/provider/model-adapter docs.

## 8. Failure, cancellation, restart, and contention semantics

- Unsupported reasoning serialization fails or drops according to explicit adapter policy; it does not send an unknown field blindly.
- Oversized reasoning is bounded/truncated only if truncation is provider-safe; otherwise stop the turn with a typed context/compatibility error.
- Cancellation discards unpublished active response buffers and preserves no partial visible reasoning.
- Retry does not duplicate prior reasoning/tool-call history.
- Concurrent sessions keep reasoning history isolated.
- Restart behavior matches the chosen retention policy and is documented; do not claim resumable interleaving if private reasoning is only in memory.
- Server-requirement mismatch produces one actionable diagnostic and does not retry indefinitely.

## 9. Compatibility and migration

- Add optional/default fields to serialized assistant messages where possible.
- Existing provider serializers ignore `None` reasoning.
- Existing projection consumers see no new private content.
- Existing visible reasoning/debug behavior, if any, is not broadened.
- Existing model-profile config maps into the adapter or remains a compatible override.
- Laguna adapter schema/version is pinned and changes alter the adapter fingerprint.

## 10. Required tests

Focused:

- reasoning delta capture and assistant association;
- text versus opaque reasoning;
- provider accepts/preserves/drops policy;
- two-round tool history with reasoning;
- retry/history-hardening idempotency;
- compaction behavior;
- serde backward compatibility;
- size bounds;
- adapter matching/exclusion;
- tool and argument alias round trip;
- thinking request transform;
- parser/template requirement diagnostics;
- fingerprint stability.

Production-shaped:

- captured Laguna round 1 reasoning + tool call -> tool result -> round 2 reasoning + tool call/final;
- endpoint metadata reports parser mismatch and receives actionable diagnostic;
- generic OpenAI/Anthropic requests remain unchanged when no reasoning field applies;
- nested Laguna child uses the same pinned adapter/history behavior.

Negative/security:

- private reasoning absent from session projection snapshots/events;
- absent from ACP update fixtures;
- absent from logs/errors/debug formatting and audit metadata;
- one session cannot observe another session's private reasoning buffer;
- tool aliases cannot bypass canonical permissions;
- base Laguna model is not matched as an agentic adapter unless explicitly configured.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test provider::
cargo test agent::processor
cargo test agent::loop
cargo test model_adapter::
cargo test projection::
cargo check --workspace
```

Add one focused Laguna golden-fixture integration target. Run one broad local library suite. Optional live serving validation is manual evidence and must not be required in routine CI.

## 12. Acceptance criteria

- Provider-neutral assistant history can preserve private reasoning when required.
- Two-round interleaved reasoning/tool history is correct and retry-safe.
- Private reasoning does not enter ordinary projections, ACP, logs, or audit metadata.
- Existing providers remain compatible.
- A complete declarative Laguna adapter resolves and applies aliases, thinking, interleaving, parallelism, prompt/recovery, and serving diagnostics.
- Adapter behavior is pinned/fingerprinted per turn.
- Optional manual live validation is documented but not a release/CI blocker.

## 13. Stop conditions

Stop if:

- correct provider continuation requires plaintext durable reasoning retention without an accepted security/retention decision;
- a serving stack uses an undocumented proprietary reasoning format that cannot be represented opaquely;
- provider message schema migration would break existing durable sessions without a compatibility path;
- Laguna behavior cannot be verified from current primary model/serving documentation;
- implementing the adapter requires automatic server launch/configuration;
- scope expands into user-visible chain-of-thought.

## 14. Closure evidence

Include:

- reasoning type/visibility/retention decision;
- two-round golden history fixture;
- retry/compaction/history-hardening evidence;
- negative projection/ACP/log disclosure results;
- Laguna effective adapter and serving diagnostic examples;
- existing-provider compatibility evidence;
- focused and broad local verification results;
- optional live validation outcome if performed, clearly labeled;
- closure recommendation.
