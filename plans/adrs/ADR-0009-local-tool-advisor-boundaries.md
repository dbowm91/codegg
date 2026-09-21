# ADR-0009: Local Tool-Advisor Boundaries

Status: accepted

Date: 2026-09-20

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`

Affected subsystem roadmaps:

- `plans/subsystems/tool-selection-advisor-roadmap.md`
- `plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md` (closed predecessor evidence; not reopened)

## Context

CodeGG now has a coherent four-state tool contract: tools may be registered, advertised, discoverable, and callable as distinct states. The host resolves one policy-filtered `ResolvedToolSurface` per turn, `tool_search` exposes policy-allowed deferred tools, and the broker/permission machinery remains the execution authority.

The remaining weakness is semantic tool selection. Keyword/BM25 discovery is inexpensive and deterministic, but smaller primary models may fail to ask for `tool_search`, may formulate a poor search query, or may overlook a specialized capability that would improve the task. Research into typed-decision models such as Jev and Laya suggests that this subproblem does not require a general autoregressive model: a small bidirectional encoder can score typed choices and abstain.

The project wants to explore a very small local model that assists discovery and disclosure while preserving CodeGG's lightweight, pure-Rust distribution model. Model use and training must be optional. CodeGG must continue to work correctly when no advisor model, training feature, training data, or telemetry configuration exists.

Tool-selection training data may contain prompts, repository context, tool names, and downstream outcomes. That information can be sensitive. Local training is the initial supported training mode. Future remote contribution/telemetry must therefore have an explicit consent and data-minimization boundary rather than reuse operational audit logs or silently upload normal session content.

## Decision drivers

- Preserve CodeGG's current behavior and execution authority when the advisor is disabled or unavailable.
- Keep production/runtime implementation pure Rust; no Python, PyTorch, embedded interpreter, or required native inference service.
- Keep model use opt-in during the experimental period.
- Keep training opt-in and local-first.
- Permit future remote training contribution without retrofitting privacy/consent after data capture exists.
- Support tools unknown at model-training time, especially MCP/plugin tools, by scoring textual descriptors rather than fixed class IDs.
- Make advisor output observable, reproducible, calibrated, and easy to disable.
- Avoid creating a second permission system, tool broker, or catalog.

## Considered options

### Option A — Put all tool schemas in every main-model prompt

This avoids a second model but increases context cost and tool-selection entropy, especially for smaller models. It also defeats the existing progressive-disclosure architecture.

### Option B — Use a general local generative function-calling model

A sub-billion generative model can select tools and construct arguments, but CodeGG already has a capable primary model for argument construction. This duplicates responsibility, costs substantially more memory/CPU, and increases the trusted surface.

### Option C — Small local discriminative/reranking advisor

A compact bidirectional model receives bounded task state plus policy-allowed textual tool descriptors and returns calibrated relevance/abstention scores. The host may use those scores for search ranking or optional disclosure. Execution remains entirely with existing CodeGG authority.

This option is selected.

### Option D — Remote tool-routing service

This can centralize training and inference but violates locality-by-default, adds latency/availability coupling, and unnecessarily transmits code/task context. It is rejected as the required runtime path.

## Decision

CodeGG will treat a learned tool advisor as an **optional local advisory component**, not as an agent, tool executor, permission authority, or required runtime dependency.

The following contracts are durable:

1. **Disabled equivalence.** With tool-advisor use disabled, missing, corrupt, unsupported, or intentionally omitted, CodeGG follows the established non-advisor tool surface and discovery path. Model absence must never prevent normal startup or an agent turn.

2. **Authority monotonicity.** Advisor input is built only from tools already admitted to the turn's policy-allowed/discoverable universe. Advisor output may rank, withhold from initial presentation, or promote an already allowed tool into model visibility. It may not register a tool, make a denied tool discoverable, alter permissions, bypass parent ceilings, execute a tool, or synthesize execution authority.

3. **Textual-candidate generalization.** Tool identity is represented by canonical name plus bounded descriptive/semantic metadata. The learned output must not be a fixed classifier over the current built-in tool list. Unknown-tool and leave-one-tool-out evaluation is required before learned disclosure is considered qualified.

4. **Pure-Rust runtime.** Production inference, tokenization, artifact loading, calibration, and scoring must execute in Rust in-process. No Python runtime or mandatory sidecar is allowed. Any selected ML dependency must be usable without required C/C++ inference runtimes on the supported default path. Optional platform acceleration may be considered later only if the pure-Rust CPU path remains functional.

5. **Optional model/runtime.** Initial implementation must be feature/config gated. No model weights are required for the ordinary CodeGG install during the first workstream. A later decision may bundle a qualified small model, but its use must still be configurable and failure must fall back to ordinary discovery.

6. **Local-first training.** Initial training, evaluation, calibration, and dataset construction execute locally. Training code is an opt-in feature/binary or equivalent separable surface so ordinary CodeGG builds do not pay training-only dependency or binary-size cost.

7. **Separate training-data channel.** Tool-advisor training events are not security audit records and must not be added to existing audit logs merely for convenience. They have a versioned schema, bounded retention, inspect/export/purge controls, and explicit content classification.

8. **No telemetry by default.** Remote transmission is disabled by default and requires a separate explicit opt-in from both advisor use and local training/capture. No default remote collection endpoint is configured in the first workstream.

9. **Content consent is distinct from metadata consent.** A future remote sink must distinguish non-content operational/model metadata from task/repository text suitable for training. Transmitting training content requires an explicit content opt-in. Secrets/credentials and raw tool arguments/results are excluded by default and must pass an approved redaction boundary before any remote-capable envelope is created.

10. **Observable advice.** Advisor decisions carry model/artifact version, surface fingerprint, candidate scores, abstention score, mode, latency, and resulting disclosure action so evaluation can explain whether a failure came from retrieval, model ranking, thresholding, or the main agent.

11. **Progressive rollout.** Learned inference first runs in benchmark/observe-only modes. Semantic reranking and proactive promotion remain explicitly enabled experimental modes until downstream task evidence demonstrates benefit. This workstream does not make learned promotion the default.

## Consequences

### Positive

- Smaller primary models can receive stronger tool-discovery assistance without adding a general second LLM.
- Existing permission and broker boundaries remain authoritative.
- CodeGG retains a no-model operational path.
- The same compact decision machinery may later be reusable for other bounded decisions, but those uses require separate planning.
- Training and telemetry privacy are designed before data collection becomes entrenched.

### Negative

- The project must own a dataset schema, evaluation harness, model artifact contract, calibration, and compatibility/versioning.
- Pure-Rust training/inference constrains framework choices and may initially give up some accelerator convenience.
- Tool-use trajectories are noisy labels; evaluation must distinguish "tool used" from "tool should have been used."

### Neutral or deferred

- No durable ML framework is selected by this ADR. Candle, Burn, tract, or a smaller custom runtime may be evaluated, provided the selected path satisfies the contracts above. Current external evidence makes Candle a useful first candidate because it provides Rust BERT, safetensors, and training; Burn is a training/inference candidate; tract is principally an inference candidate and would require a separate training owner.
- Model architecture, exact parameter count, quantization, and eventual bundled-weight policy remain empirical decisions.
- Remote aggregation/training infrastructure is not created by this ADR.

## Compatibility and migration

There is no database or protocol migration required by the decision itself.

Advisor configuration must default to disabled. Unknown or removed advisor configuration must fail toward the non-advisor path rather than making sessions unusable.

Model artifacts require an explicit manifest including at least artifact/model version, architecture identifier, tokenizer version/hash, descriptor schema version, context schema version, calibration version, maximum input limits, weight hash, and training/evaluation provenance fingerprint.

Training-event schemas must be versioned independently of model artifacts so old local datasets can be inspected, migrated, or deliberately discarded.

## Security and reliability implications

The advisor receives potentially sensitive task context, so local inference should consume the smallest context projection required for selection. It must never receive secrets merely because a downstream tool could use them.

Remote-capable training data requires explicit consent, bounded queues, redaction, destination visibility, TLS-only network transport, and inspect/purge behavior. Enabling the advisor must never implicitly enable capture or upload.

Advisor inference failure, timeout, corrupt weights, unsupported artifact version, memory pressure, or panic boundary must produce a diagnostic and fall back to ordinary tool discovery for the current turn. Repeated failures may circuit-break the advisor for the session/process, but may not fail the agent loop.

## Verification

Conforming implementations must prove:

- disabled/no-model equivalence against current tool-surface tests;
- denied/hidden/disabled/plan/parent-ceiling tools never enter advisor candidates;
- advisor output cannot invoke tools or mutate authority;
- deterministic artifact/version rejection and fallback;
- unknown-tool/leave-one-tool-out evaluation;
- local training can run without Python;
- ordinary default builds do not require training dependencies;
- remote telemetry is network-silent without explicit opt-in;
- metadata-only and training-content consent paths are distinguishable;
- local captured data can be inspected, exported, and purged.

## Supersession

None.
