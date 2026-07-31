# Agent Runtime, Model Adaptation, and ACP Milestone 008 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-runtime-model-adaptation-acp/008-reasoning-preservation-and-poolside-laguna-adapter.md`

Source subsystem roadmap: `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-008--reasoning-preservation-and-poolside-laguna-vertical-slice`

Repository baseline reviewed: `7eb55265`

Implementation commit: `7eb55265` — feat(agent): preserve Laguna reasoning across tool rounds

## 1. Executive finding

Milestone 008 is implemented and strictly closed. Provider-neutral assistant
history now carries bounded, private reasoning parts. Event processing attaches
reasoning to the same assistant response as its tool calls, while serializers
only round-trip it for Laguna-compatible OpenAI endpoints. The built-in Laguna
adapter resolves model matching, aliases, thinking, interleaving, parallelism,
recovery, and serving requirements through the existing generated adapter
registry.

The retention decision is intentionally in-memory/provider-history only. The
private body is not added to session projections, ACP payloads, audit metadata,
or durable session storage. Serde skips the body and debug formatting emits
only bounded metadata. A restart therefore does not claim resumable Laguna
reasoning continuation; the next turn safely drops unsupported private state.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Private reasoning capture and assistant association | `src/agent/processor.rs`; `tests/event_processor.rs::reasoning_is_private_and_attached_to_the_same_assistant_round` |
| Bounded retention and context accounting | `MAX_REASONING_BYTES`; `src/context/volatile_tail.rs` |
| Laguna two-round round trip | `tests/provider_transcripts.rs::laguna_two_round_history_preserves_private_reasoning_and_aliases` |
| Existing-provider compatibility and disclosure boundary | Generic OpenAI-compatible regression in `tests/provider_transcripts.rs`; non-Laguna serializers omit reasoning |
| Declarative Laguna model/alias/serving contract | `crates/codegg-core/assets/model-adapters/laguna.toml`; adapter unit tests |
| Actionable serving diagnostics | `serving_requirement_diagnostics` and adapter mismatch test |
| Fingerprint/pinning compatibility | Existing `ResolvedModelAdapter` fingerprint path; Laguna resolution test |

## 3. Production implementation evidence

- Added `ContentPart::Reasoning` with explicit private visibility and bounded
  capture. It is excluded from visible text aggregation.
- Added Laguna-only `reasoning_content` serialization and
  `chat_template_kwargs.enable_thinking` to the OpenAI-compatible request
  path. Generic requests remain unchanged.
- Added the generated built-in `poolside-laguna-agentic` adapter with current
  Poolside model matching, `bash -> shell`, `command -> cmd`, single-call
  parallelism, recovery policy, and `poolside_v1` serving requirements.
- Added server metadata comparison diagnostics and context-token accounting.
- Debug output redacts reasoning content; serde does not serialize the private
  body. No projection or ACP mapping was changed to include it.

Poolside's current primary model card documents preserved `reasoning_content`,
the `shell`/`cmd` tool contract, `enable_thinking`, and vLLM
`poolside_v1`/automatic-tool-choice requirements. Live serving was not run;
the plan explicitly makes it optional rather than a routine closure gate.

## 4. Verification executed

- `cargo fmt --all -- --check` — passed.
- `bash scripts/check-core-boundary.sh` — passed.
- `cargo test --test event_processor` — 15 passed.
- `cargo test --test provider_transcripts` — 21 passed.
- `cargo test -p codegg-providers` — 99 passed.
- `cargo test -p codegg-core model_profile::adapter --lib` — 5 passed.
- `cargo test --test compaction` — completed through the serialized Cargo build
  queue; no failure was reported.
- `cargo check -p codegg` — passed.

These are local results. No external Laguna endpoint or hosted serving stack
was available or required.

## 5. Invariant review

Private reasoning is not visible assistant text, is bounded, remains attached
to tool-call history, and is omitted by non-Laguna providers. Canonical
permission/tool authority remains independent of the wire alias asset.

## 6. Failure and recovery review

Partial stream buffers are owned by `EventProcessor` and are discarded on
reset/retry. Oversized reasoning is bounded at a UTF-8 boundary. Unsupported
providers drop the private part rather than sending an unknown field.

## 7. Migration and compatibility review

Existing `Message` constructors and serialized visible content remain
compatible. The private body is non-durable by decision, so restart behavior is
documented as non-resumable rather than silently inventing persistence.

## 8. Security review

Reasoning bodies do not enter projection/ACP structures. `Debug` exposes only
byte count and a private marker, and serde skips the body. The generic provider
test proves a non-Laguna request does not emit `reasoning_content`.

## 9. Documentation and operations

The implementation plan, roadmap, registry, and this closure record document
the retention boundary and optional live-serving validation. The adapter's
serving requirements provide actionable diagnostics without attempting remote
server remediation.

## 10. Unresolved findings

None at critical, high, or medium severity. Optional live serving validation is
not a closure blocker and remains a documented operational follow-up.

## 11. Roadmap disposition

M008 is closed. The blocked-work audit found that M009's requirements (M001,
M002, M007, and final reasoning integration) are now satisfied, so M009 was
promoted to `ready`. M010 remains blocked on M009 strict closure, and M011
remains blocked on M004 through M010 strict closure. No other registered plan
became ready.

## 12. Registry updates

- Marked the implementation plan `implemented`.
- Added this strict closure record.
- Marked the roadmap milestone closed.
- Removed M008 from dependency-ready work and added M009 as ready.
- Audited all registered blocked work and preserved M010/M011 blockers.
