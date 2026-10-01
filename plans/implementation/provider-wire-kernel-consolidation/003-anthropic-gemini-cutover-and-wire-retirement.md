# Provider Wire-Kernel Consolidation Milestone 003 — Anthropic/Gemini Cutover and Wire Retirement

Status: ready for handoff

Repository baseline: `dcea3ea`

Source roadmap:

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md#milestone-003--anthropicgemini-cutover-and-duplicate-wire-retirement`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required if M001/M002 preserve the roadmap boundary.

Primary class: infrastructure

## 1. Objective

Finish shared-wire consolidation by moving Anthropic-compatible and native Google/Gemini production paths onto `eggpool-wire`, then retire CodeGG's remaining duplicate standard OpenAI/Anthropic/Gemini serializers and SSE parsers while leaving Bedrock and hosted/stateful Responses specialized.

## 2. Why this milestone is ready

M002 is closed at `plans/closure/provider-wire-kernel-consolidation/002-status.md`. Its production cutover proves the shared transport/bridge seam, and the upstream pin and CodeGG policy projection remain stable.

## 3. Current implementation evidence

At the pre-cutover baseline:

- `anthropic.rs` manually builds Messages requests and decodes Anthropic SSE through CodeGG parser state.
- MiniMax uses an Anthropic-compatible endpoint through a dedicated factory/auth contract.
- `google.rs` manually constructs `contents`, `functionCall`/`functionResponse`, generation config, multimodal image payloads, and its own SSE parser.
- After M002, `sse_parser.rs` is Anthropic-only; OpenAI parsing has been retired.
- `bedrock.rs` owns AWS SigV4 + ConverseStream syntax and must remain specialized.
- `responses_api.rs` owns hosted-program/continuation semantics above ordinary stateless wire translation and must remain specialized.

The M001 parity matrix is authoritative for exact intentional differences, including Anthropic system-message behavior.

## 4. Invariants that must not regress

- Anthropic x-api-key/version/auth/base-URL behavior remains CodeGG-owned.
- MiniMax keeps its existing endpoint/credential contract unless separately changed by provider planning.
- System instructions reach Anthropic exactly once; any correction to legacy dropped-system behavior is explicit and tested.
- Google tool call/result pairing, multimodal input, finish reasons, usage, and reasoning/thought output remain semantically correct.
- Private reasoning remains policy-gated.
- Bedrock SigV4/ConverseStream is not routed through a false compatible codec.
- Hosted/stateful Responses continuation/tool-program state remains outside `eggpool-wire`.
- Standard wire parser deletion happens only after repository-wide reference proof.

## 5. Scope

### In scope

- Anthropic provider shared Messages encode/decode.
- Qualified Anthropic-compatible providers such as MiniMax.
- Google provider migration to the qualified Gemini surface selected by actual endpoint semantics.
- Production event bridge/accumulator use for tools/reasoning/usage.
- Delete or collapse duplicate standard wire code after all consumers move.
- Final ownership/static guards and architecture documentation.

### Explicitly out of scope

- Bedrock codec extraction.
- Stateful/hosted Responses redesign.
- Provider metadata/profile extraction.
- EggPool runtime routing.
- New provider capability.

## 6. Required production changes

### Anthropic family

Use the shared canonical request bridge and `AnthropicMessages` codec. Preserve CodeGG auth/version headers, endpoint selection, model discovery/static model policy, timeout/cancellation, and error classification.

System/developer content must follow the shared codec contract. Add a regression based on the production agent path where `request.system` has already become a `Message::System`. If this differs from legacy behavior, document it as an intentional correctness repair.

Map thinking budget/adaptive/disable only through qualified shared semantics and CodeGG model policy.

### Gemini family

Select the existing shared Gemini surface that matches CodeGG's Google endpoint. Do not translate through OpenAI syntax.

Qualify:

- text + images/data URLs;
- system/control placement;
- function declarations;
- function calls/results and IDs/names;
- multiple tool calls;
- temperature/top-p/max output;
- structured output if CodeGG currently exposes it;
- thought/reasoning deltas;
- finish reason and usage.

Any provider-specific path/auth query remains CodeGG transport configuration.

### Duplicate retirement

After Anthropic/Gemini production paths are green, perform repository-wide reference checks and remove/narrow:

- standard OpenAI/Anthropic parsing portions of `sse_parser.rs`;
- provider-local Google SSE/parser helpers;
- provider-local standard request builders superseded by the shared codec;
- redundant tool-format helper methods only if no specialized consumer remains.

Do not delete compatibility/public wrapper types merely for line-count reduction.

Add a guard/test that the standard shared families do not reintroduce a second codec/parser implementation outside the bridge/kernel.

## 7. Ordered work packages

### Work package A — Anthropic family cutover

Acceptance evidence: native Anthropic + compatible fixtures, thinking/tools/images/system/usage/error cases, and existing connection/auth tests pass.

### Work package B — Gemini cutover

Acceptance evidence: Google request/stream transcript corpus passes, including multi-tool, image, reasoning/thought, usage, malformed/incomplete and arbitrary chunking cases.

### Work package C — Final duplicate retirement and static ownership guard

Acceptance evidence: repository search and guard identify `eggpool-wire` as sole standard OpenAI/Anthropic/Gemini grammar/parser owner; remaining Bedrock/Responses specialized parsers are explicitly exempted by path/purpose rather than broad allowlisting.

### Work package D — Full provider/architecture closure

Acceptance evidence: full CodeGG verification green; architecture/provider docs match actual ownership; closure identifies any deferred provider-profile metadata work without registering it as completed.

## 8. Failure, cancellation, restart, and contention semantics

Same as M002: CodeGG owns network lifecycle/retry; shared decoding is incremental/per-request. Provider-error, incomplete, malformed, and EOF distinctions map into CodeGG failures without synthesized success. Dropping a stream frees decoder/accumulator state.

No new background task/global cache is introduced.

## 9. Compatibility and migration

No durable storage/config migration.

Existing provider IDs/endpoints/auth remain stable. If shared Gemini/Anthropic behavior corrects a proven legacy bug, record the old/new fixture and why the new behavior matches the intended semantic request; do not silently bundle unrelated API changes.

Bedrock/hosted Responses continue on existing code paths.

## 10. Required tests

Focused:

- all Anthropic provider/unit/transcript tests;
- MiniMax compatible request/auth tests;
- Google provider tests;
- provider transcript tests for system/tools/results/images/reasoning;
- upstream conformance vectors for Anthropic and Gemini;
- production bridge complete-tool-call tests;
- malformed/incomplete/EOF/error/usage tests;
- static ownership guard for duplicate standard parsers/codecs.

Broad:

- `cargo test -p codegg-providers`;
- `cargo test --test provider_transcripts`;
- full workspace Clippy/tests;
- current security/dependency verification if source/dependency graph changes.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test --test provider_transcripts
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run repository static guards and current quick/CI verification appropriate to provider changes. Do not claim live provider evidence unless actually obtained.

## 12. Documentation updates

- `architecture/provider.md` — final single-owner wire architecture.
- `architecture/model-adapters.md` — resolved wire-policy projection and reasoning/alias authority.
- `architecture/resilience.md` only if parser ownership text references removed components; do not change resilience behavior.
- roadmap/registry/closure.

## 13. Acceptance criteria

- Ordinary OpenAI/OpenAI-compatible, Anthropic-compatible, and Google/Gemini provider paths all use the shared wire kernel for supported finite encoding and stream decode.
- No duplicate standard OpenAI/Anthropic/Gemini SSE parser/request grammar remains in CodeGG.
- CodeGG still owns provider transport, auth, discovery, session identity, model policy, retries, and agent/tool semantics.
- Anthropic system instructions have an explicit regression proving correct production-path behavior.
- Google tool/multimodal/reasoning/usage behavior is qualified.
- Bedrock and hosted/stateful Responses remain functional specialized paths.
- Full workspace verification passes and the roadmap can close.

## 14. Stop conditions

Stop if M002 is not closed; the chosen Gemini surface cannot represent current CodeGG semantics without material loss; a provider requires unmodeled native syntax; a proposed deletion is still referenced by Bedrock/hosted Responses/other specialized code; or parity requires moving app/provider policy into `eggpool-wire`.

## 15. Closure evidence required

Record implementation commits, provider-family migration list, before/after duplicate ownership/search results, Anthropic system regression, Gemini tool/multimodal/usage evidence, Bedrock/Responses non-regression, provider/full-workspace verification, docs/guard updates, and severity-classified residual findings.

## 16. Handoff notes

Delete duplicate code only after the production call graph proves it unused. The target is one standard wire implementation, not artificial uniformity across genuinely different protocols.
