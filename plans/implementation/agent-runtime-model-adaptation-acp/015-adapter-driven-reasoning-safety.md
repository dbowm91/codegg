# Agent Runtime, Model Adaptation, and ACP Milestone 015 — Adapter-Driven Reasoning Safety

Status: implemented — closure record: `plans/closure/agent-runtime-model-adaptation-acp/015-status.md`

Repository baseline: `81b46de801137df605ce302dccff6f258c99fae1`

Source corrective addendum:

- `plans/subsystems/agent-runtime-model-adaptation-acp-corrective-closure-addendum.md#milestone-015--adapter-driven-reasoning-safety`

Historical plans corrected by this milestone:

- `plans/implementation/agent-runtime-model-adaptation-acp/007-declarative-model-adapter-registry.md`
- `plans/implementation/agent-runtime-model-adaptation-acp/008-reasoning-preservation-and-poolside-laguna-adapter.md`

Primary class: provider/correctness

## 1. Objective

Make provider-private reasoning accumulation safe for arbitrary UTF-8 and make provider request behavior derive from the resolved model adapter rather than raw model-name substring checks. The resolved adapter must own whether private reasoning is preserved, which request field carries it, how thinking is enabled, which tool/argument aliases apply, and which serving requirements are diagnosed.

The milestone must preserve private reasoning as opaque provider round-trip state. It must not expose chain-of-thought to users, ACP, projections, logs, diagnostics, artifacts, or generic serialization.

## 2. Dependencies

Hard dependency:

- Milestone 014 strict closure, so prompt/context/cache identity already incorporates resolved adapter and reasoning mode before provider request transforms are corrected.

Existing foundations:

- strict built-in model-adapter TOML compiled into Rust by `codegg-core/build.rs`;
- `ResolvedModelAdapter` contains adapter ID/version/fingerprint, tool and argument aliases, recovery policy, serving requirements, and request transforms;
- Laguna adapter TOML declares `reasoning_content`, thinking enablement, parser requirements, tool aliasing, and single-tool parallelism;
- `ContentPart::Reasoning` has private visibility and skipped serialization;
- `EventProcessor` accumulates reasoning and turns it into private assistant content;
- OpenAI-compatible request construction currently injects Laguna behavior through model-name substring checks;
- provider transcript fixtures exist.

No live Laguna server or external model is required for routine closure.

## 3. Current implementation evidence

Re-audit at implementation time. At the reviewed baseline:

- `EventProcessor::process(ChatEvent::ReasoningDelta)` attempts to fit text under `MAX_REASONING_BYTES` by repeatedly slicing one byte from the end;
- Rust string slicing requires a UTF-8 character boundary, so multibyte text near the limit can panic;
- `ContentPart::Reasoning` correctly skips serde text and redacts `Debug`, but generic provider serializers must continue to omit it unless explicitly adapter-supported;
- `OpenAiCompatibleProvider::build_body` checks `request.model.to_ascii_lowercase().contains("laguna")` to add `reasoning_content` and `chat_template_kwargs.enable_thinking`;
- the Laguna TOML already declares transforms and serving requirements, but those transforms are descriptive rather than the sole production authority;
- custom model aliases or exact IDs resolved to the Laguna adapter but lacking the literal substring may not receive required behavior;
- model names containing `laguna` could receive Laguna behavior even if excluded or matched to another adapter;
- request transform `op` values are represented as strings and require validation/application discipline.

## 4. Invariants that must not regress

- Private reasoning never becomes user-visible assistant text.
- Private reasoning is not serialized by generic message/protocol/projection/ACP paths.
- Reasoning bounds are enforced in bytes without invalid UTF-8 slicing or panic.
- Truncation is deterministic and never splits a code point.
- Resolved adapter identity, not model-name heuristics, selects reasoning/thinking transforms.
- Adapter transforms are a closed typed set; unknown transforms fail build/config validation.
- Adapter data cannot execute code, grant permissions, add tools, bypass the tool broker, or change workspace authority.
- Canonical tool names remain internal; aliases apply only at provider wire boundaries and reverse-map before permission/execution.
- Unknown models use conservative generic behavior with no private-reasoning round-trip assumption.
- Serving-requirement diagnostics are bounded and contain no credentials or reasoning bodies.
- Context/cache identity separates reasoning-enabled and reasoning-disabled behavior.

## 5. Scope

### In scope

- Implement UTF-8-safe byte-budget accumulation/truncation for reasoning deltas.
- Add focused boundary tests with multibyte Unicode and fragmented streaming deltas.
- Replace stringly request-transform operations with a validated typed enum or equivalent generated representation.
- Apply adapter-selected request transforms through a common provider request context.
- Remove Laguna model-name substring checks from OpenAI-compatible request construction.
- Support adapter-selected private reasoning field, thinking parameter, tool aliases, and argument aliases.
- Ensure reverse alias mapping occurs before canonical permission/tool execution.
- Wire serving-requirement diagnostics to the configured provider/server metadata seam where available.
- Preserve reasoning privacy across serde, debug, projections, ACP, logs, artifacts, and context diagnostics.
- Update provider transcript and adapter-resolution fixtures.

### Explicitly out of scope

- Exposing reasoning to users or adding a “show chain-of-thought” feature.
- Dynamic executable adapter plugins or scripting.
- Automatic model benchmarking or learned adaptation.
- Rewriting every provider into one universal wire protocol.
- Requiring live Laguna/vLLM/SGLang in CI.
- Broad model-card refresh unrelated to the corrected behavior.
- Provider authentication or routing redesign.

## 6. Required production changes

### UTF-8-safe reasoning accumulator

Introduce a reusable helper that appends as much of a delta as fits within a byte budget while preserving valid UTF-8, for example by:

- accepting the whole delta when it fits;
- finding the largest `is_char_boundary` index at or below remaining bytes;
- appending the valid prefix;
- recording a truncated flag/count without retaining omitted content.

Do not loop byte-by-byte over large strings if a bounded boundary search can be used. The helper must handle zero remaining bytes, ASCII, multibyte Unicode, combining characters, fragmented code points as valid Rust strings, and very large deltas.

The accumulator should expose bounded metadata such as `was_truncated` or omitted byte count only if needed for provider/runtime behavior. Do not log the reasoning text.

### Typed adapter transforms

Replace raw `RequestTransform { op: String, ... }` execution with a closed enum such as:

```rust
#[serde(tag = "op", rename_all = "snake_case")]
enum RequestTransform {
    PrivateReasoningField { field: String },
    ThinkingParameter { field: String, value: AdapterScalar },
}
```

The exact schema may preserve compatibility with existing TOML keys. Build-time validation must reject:

- unknown operations;
- missing required fields;
- duplicate/conflicting transforms;
- unsafe arbitrary nested field paths if the provider request builder cannot support them safely;
- transforms that attempt to alter authorization, endpoint, headers, tools, or permission policy.

### Resolved provider request context

Pass the resolved adapter or a bounded derived request policy to provider request builders. The provider should not independently re-resolve by model substring. The policy should include only needed wire behavior:

- adapter ID/version/fingerprint;
- private reasoning preservation enabled/disabled;
- private reasoning output field name;
- thinking parameter field/value/default;
- tool format/choice/parallel limit;
- canonical-to-wire tool aliases and argument aliases;
- serving requirements/diagnostics metadata.

Avoid placing the full config/adapter source in every request if a compact resolved structure is sufficient.

### OpenAI-compatible request construction

- use adapter request policy to include `reasoning_content` only when enabled;
- include it only on assistant messages and only from private reasoning parts;
- use adapter policy to set thinking parameters;
- omit both fields for generic/non-supporting adapters;
- apply canonical-to-wire tool and argument aliases exactly once;
- preserve OpenAI-compatible tool-call/result structure and assistant text;
- never send private reasoning to a provider that did not opt into preservation.

### Inbound provider events

- parse reasoning deltas only when the provider/adapter contract identifies them;
- keep the existing private event type;
- map inbound wire tool aliases back to canonical names before resolved-surface/permission checks;
- reject unknown/unmappable wire names with bounded recovery behavior.

If the existing SSE/parser layer already emits reasoning generically, add adapter gating at the provider/event boundary rather than exposing it publicly.

### Serving diagnostics

Use `serving_requirement_diagnostics` or its replacement with explicit configured server metadata. Diagnostics should identify missing parser/auto-tool-choice requirements and adapter identity. They should be warnings/errors according to whether missing configuration makes tool/reasoning parsing unsafe. Do not infer server configuration from model name.

### Privacy/static guards

Retain or add focused tests/guards that ensure:

- reasoning content is skipped by generic serde;
- `Debug` redacts content;
- projection/ACP mapping ignores reasoning;
- context diagnostics use counts/hashes/placeholders only;
- error paths do not include request bodies containing reasoning.

## 7. Ordered work packages

### Work package A — Safe accumulator and privacy tests

- implement byte-budget UTF-8 helper;
- use it in event processing;
- add ASCII/multibyte/boundary/truncation tests;
- audit debug/serde/log/error behavior.

Acceptance evidence:

- no valid UTF-8 input can panic at the reasoning limit;
- accumulated text is valid UTF-8 and at most the configured byte limit;
- omitted reasoning is not logged or serialized.

### Work package B — Typed transform schema and build validation

- define typed transform enum/scalars;
- update TOML parsing/code generation;
- validate conflicts and forbidden fields;
- preserve deterministic adapter fingerprinting/package inventory.

Acceptance evidence:

- unknown/stringly transform fails build validation;
- Laguna TOML compiles to typed behavior;
- generic adapter has no reasoning transform.

### Work package C — Provider request policy integration

- carry resolved request policy to provider builder;
- remove model substring checks;
- apply private reasoning/thinking/tool/argument transforms;
- reverse-map inbound aliases before execution.

Acceptance evidence:

- custom alias matched to Laguna adapter receives behavior;
- model containing “laguna” but excluded from adapter does not;
- non-supporting provider never receives private reasoning;
- aliases cannot bypass canonical permission checks.

### Work package D — Serving diagnostics and transcript fixtures

- wire parser/auto-tool-choice requirements to configured serving metadata;
- add captured multi-round reasoning/tool transcripts;
- add malformed/missing parser diagnostic cases;
- verify reasoning-enabled/disabled cache identity separation from M014.

Acceptance evidence:

- correct Laguna round trip preserves private reasoning across tool rounds;
- disabled thinking omits/sets the correct parameter per adapter;
- diagnostics are bounded and adapter-specific.

### Work package E — Documentation and closure handoff

- update model-adapter/provider/reasoning architecture;
- create M015 closure record only after independent review;
- promote M016 only on strict closure.

## 8. Failure, cancellation, restart, and contention semantics

- Invalid built-in adapter transform fails build/package validation.
- Invalid user override fails with source-aware configuration diagnostics before provider invocation.
- Missing required serving parser configuration produces a clear bounded failure or warning according to the adapter's declared safety requirement; it must not silently parse tool calls as text and continue if correctness is impossible.
- Reasoning truncation does not fail the turn; the private state is safely bounded and optionally marked truncated.
- Cancellation drops in-flight private buffers with the ordinary request/turn lifecycle.
- Concurrent turns own independent reasoning buffers and resolved adapter policies.
- Restart does not persist hidden reasoning beyond existing provider message/session policy; no new durable reasoning store is added.

## 9. Compatibility and migration

- Preserve existing adapter TOML syntax where it can map unambiguously to typed transforms; otherwise update built-ins and reject invalid legacy values with source-aware diagnostics.
- Generic provider/request behavior remains unchanged when no transform is selected.
- Existing public protocol/message serialization remains compatible because private reasoning text is still skipped.
- No durable storage migration is required.
- Existing canonical tool permissions/logs retain canonical names.
- Provider-specific request builders may gain an internal resolved-policy argument without changing user configuration semantics.

## 10. Required tests

### Reasoning bound tests

- exact byte limit ASCII;
- one byte below/above limit;
- two-, three-, and four-byte Unicode crossing the boundary;
- repeated fragmented deltas;
- zero remaining capacity;
- large delta performance remains bounded;
- output always valid UTF-8 and within limit.

### Adapter transform tests

- typed TOML parse and generated source;
- unknown operation rejected;
- missing/conflicting field rejected;
- generic adapter no-op;
- exact/custom alias resolves to Laguna behavior;
- excluded/base model does not;
- deterministic precedence/fingerprint.

### Provider transcript tests

- assistant reasoning + tool call + tool result + next round includes configured reasoning field;
- reasoning disabled omits or disables thinking per adapter;
- non-Laguna adapter omits reasoning field;
- tool/argument aliases apply once and reverse-map before execution;
- malformed/unavailable wire alias enters bounded recovery.

### Privacy/negative tests

- serde output contains no private reasoning text;
- `Debug` contains only size/private marker;
- projection and ACP updates omit reasoning;
- request/error logs do not include reasoning body;
- adapter cannot alter auth headers/endpoints/permissions/tools outside aliasing.

### Serving diagnostics tests

- required parser absent;
- wrong parser configured;
- auto-tool-choice disabled;
- all requirements satisfied;
- diagnostics contain adapter ID and safe metadata only.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo check -p codegg-core --all-targets
cargo check -p codegg-providers --all-targets
cargo check -p codegg --all-targets
cargo test -p codegg-core model_profile::adapter
cargo test -p codegg-providers openai_compatible
cargo test -p codegg agent::processor
cargo test --test provider_transcripts -- --test-threads=4
cargo test --test event_processor -- --test-threads=4
cargo test --test context_plan_convergence -- --test-threads=4
python3 scripts/check_projection_disclosure.sh
```

Use captured fixtures; do not add live model/server CI or a broad model matrix.

## 12. Documentation updates

- `architecture/model-adapters.md`: typed transforms, precedence, request policy, and serving diagnostics;
- `architecture/provider.md`: adapter-selected private reasoning/thinking behavior and alias boundary;
- `architecture/cache-aware-context.md`: reasoning-mode cache identity without content disclosure;
- ACP/projection documentation: continued private-reasoning omission;
- corrective addendum, registry, and M015 closure record.

## 13. Acceptance criteria

- Reasoning accumulation is UTF-8 safe and byte-bounded.
- No valid multibyte delta can panic near the limit.
- Provider reasoning/thinking behavior is selected only by resolved adapter policy.
- Laguna/custom aliases receive correct behavior without model substring checks.
- Excluded/non-supporting models do not receive private reasoning fields.
- Tool/argument aliases cannot bypass canonical permission/execution paths.
- Transform schema is closed, typed, and build/config validated.
- Serving requirement diagnostics are actionable and bounded.
- Private reasoning remains absent from public serialization, ACP, projections, logs, and diagnostics.
- Focused adapter/provider/transcript/privacy tests pass.

## 14. Stop conditions

Stop and report if:

- provider integration requires exposing private reasoning through public protocol DTOs;
- adapter transforms require arbitrary scripting or unrestricted JSON mutation;
- correct tool aliasing cannot occur before permission/execution without redesigning the canonical tool surface;
- live provider behavior is the only possible verification method;
- a provider requires storing hidden reasoning durably outside existing message/session policy;
- changes expand into provider authentication/routing ownership.

## 15. Required closure evidence

The closure record must include:

- UTF-8 boundary fixture results;
- typed transform schema/build-validation evidence;
- before/after provider request transcript for Laguna and generic adapters;
- custom alias and exclusion evidence;
- canonical alias/permission evidence;
- serving diagnostic examples;
- privacy negative-test results;
- focused command results and exact commits;
- remaining low-severity limitations;
- explicit recommendation to promote or block Milestone 016.
