# Provider Wire-Kernel Consolidation Milestone 002 — OpenAI-Family Shared-Wire Cutover

Status: implemented

Repository baseline: `94a4b3a0`

Source roadmap:

- `plans/subsystems/provider-wire-kernel-consolidation-roadmap.md#milestone-002--openai-family-shared-wire-cutover`

Long-term requirements:

- `plans/000-long-term-specification.md#13-provider-architecture-and-eggpool`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- None required if M001 closes with the planned ownership boundary.

Primary class: infrastructure

## 1. Objective

Move CodeGG's production OpenAI Chat and OpenAI-compatible provider family onto the M001-qualified `eggpool-wire` encode/stream path, retaining CodeGG endpoint/auth/header/model-discovery/deadline/retry ownership and preserving provider IDs/durable connection compatibility.

## 2. Why this milestone is ready

CodeGG M001 is positively closed at `plans/closure/provider-wire-kernel-consolidation/001-status.md`. It supplies the immutable upstream pin, semantic bridge, resolved wire-policy projection, and qualified OpenAI-family bridge behavior. M002 owns the production-path parity and cutover evidence for each provider in scope.

## 3. Current implementation evidence

The pre-M001 baseline has separate request builders and OpenAI SSE parsing in:

- `crates/codegg-providers/src/openai.rs`;
- `openai_compatible.rs`;
- `azure.rs`;
- `openrouter.rs`;
- `opencode_zen.rs`;
- `additional.rs` plus compatible wrappers such as Cloudflare/Copilot/GitLab/Vertex;
- `sse_parser.rs`.

`OpenAiCompatibleProvider` also owns transport-only features that must remain CodeGG-owned: auth header construction, extra-header collision checks, optional required session-affinity header, chunk timeout, HTTP status mapping, and `/models` discovery.

## 4. Invariants that must not regress

- Same provider IDs, connection kinds, credential capability matrix, setup-catalog behavior, endpoints, auth headers, extra-header collision rejection, and model-discovery behavior.
- OpenCode Go required session-affinity header still derives only from canonical CodeGG session context and fails before network I/O when missing/invalid.
- OpenAI streaming usage remains requested where CodeGG policy requires it.
- Model-specific aliases/private reasoning/thinking controls come from the M001 resolved wire policy, never a provider-local model regex table.
- Tool names are normalized back to canonical identity before permission/broker execution.
- CodeGG owns HTTP error/retry taxonomy and stream idle/cancellation policy.
- No Anthropic/Google/Bedrock/hosted Responses behavior changes.

## 5. Scope

### In scope

- Introduce or finish a thin shared `WireProvider`/helper path for OpenAI Chat wire encoding + stream decoding.
- Cut over native OpenAI, generic compatible, Azure, OpenRouter, OpenCode Zen/Go, and compatible factories/wrappers whose protocol is genuinely OpenAI Chat compatible.
- Apply only closed, qualified CodeGG post-encode transforms required by the resolved wire policy/provider contract.
- Preserve specialized endpoint/auth/header/model discovery wrappers.
- Remove OpenAI-specific duplicate serializer/parser code only after all production references move.

### Explicitly out of scope

- Anthropic/MiniMax migration.
- Google/Gemini migration.
- Bedrock.
- Hosted/stateful Responses.
- Provider-profile metadata extraction.
- Retry/fallback/circuit ownership changes.
- Changing provider IDs/storage.

## 6. Required production changes

Build provider request bodies from the M001 canonical bridge and the shared OpenAI Chat codec. Use the upstream encode options to request stream usage when CodeGG requires it.

Transport wrappers continue to construct URLs, authentication, static headers, session-affinity headers, and Eggfetch requests. The shared kernel receives no credentials/URLs/session IDs.

Provider-specific optional-field differences identified by M001 must be represented by the bounded CodeGG wire policy/closed transforms, not branches inside `eggpool-wire`. Do not use arbitrary JSON callbacks.

Normalize shared canonical stream events through the M001 event bridge and completed-call accumulator. Retire `parse_openai_buffer` only when no OpenAI-family production path or test requires it.

Preserve public Rust wrapper types where removal would be an unnecessary API break; they may delegate internally to the shared implementation.

## 7. Ordered work packages

### Work package A — Generic shared OpenAI transport core

Wire the generic compatible provider through canonical encode/decode while preserving request builder headers/session affinity/discovery.

Acceptance evidence: generic fixtures, redirect/error/timeout tests, aliases/private reasoning, and session-affinity negative tests pass.

### Work package B — Native/gateway provider cutover

Move OpenAI, Azure, OpenRouter, OpenCode Zen/Go and qualified compatible wrappers/factories to the shared path.

Acceptance evidence: provider-specific transcript matrix from M001 passes through production constructors; endpoint/auth/header assertions are unchanged.

### Work package C — OpenAI duplicate retirement

Remove or narrow obsolete OpenAI request/SSE helpers and provider-local model policy.

Acceptance evidence: repository search shows one standard OpenAI Chat codec/stream owner; remaining specialized code has an explicit owner/reason.

## 8. Failure, cancellation, restart, and contention semantics

No retry/cancellation redesign. Existing stream idle/cancellation boundaries wrap the shared decoder. Provider HTTP errors are classified before/around decode as today. A canonical decoder error after stream handoff is a stream failure and consumes the existing retry/side-effect rules; do not replay a partially observed provider turn.

## 9. Compatibility and migration

No storage/config migration. Existing providers resolve from the same setup catalog and connection factory.

Any M001 intentional compatibility correction must have a dedicated regression and be called out in closure. Unclassified differences block cutover for that provider; retain its legacy implementation temporarily rather than weakening shared semantics.

## 10. Required tests

At minimum:

- `cargo test -p codegg-providers`;
- `tests/provider_transcripts.rs` OpenAI/generic sections;
- OpenCode Go session-affinity tests;
- compatible endpoint/auth/extra-header/model discovery tests;
- malformed/interleaved tool stream tests through production providers;
- usage/finish/error mapping;
- differential fixtures from M001 now run against the production shared path;
- static/search guard preventing a second OpenAI Chat serializer/SSE parser from being added in migrated modules.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test --test provider_transcripts
cargo clippy -p codegg-providers --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Also run current dependency/security verification if the lockfile changes from the M001 pin.

## 12. Documentation updates

Update `architecture/provider.md`, model-adapter documentation where provider-local hardcoding is removed, and the roadmap/registry/closure record.

## 13. Acceptance criteria

- Every provider listed in M002 and marked qualified by M001 uses `eggpool-wire` as its OpenAI Chat grammar/stream parser.
- Transport/auth/discovery/session-affinity behavior remains CodeGG-owned and covered.
- Streaming usage/accounting is not regressed.
- Provider-local Laguna/model-name policy is no longer an independent authority.
- No second OpenAI Chat SSE parser/serializer remains for migrated providers.
- Anthropic/Google/Bedrock/hosted Responses are unchanged.

## 14. Stop conditions

Stop if M001 is not positively closed; a provider has unresolved parity differences; a provider requires non-OpenAI wire syntax; shared-kernel changes would be provider-name-specific; or transport/retry ownership would move into the kernel.

## 15. Closure evidence required

Record implementation commits, affected provider list, exact remaining legacy-provider list, provider/differential tests, session-affinity/auth/redaction checks, usage parity, code-deletion/search evidence, full workspace results, and M003 unblock disposition.

## 16. Handoff notes

Prefer retaining thin named provider wrappers over destructive public-type churn. The maintenance win comes from one grammar/parser owner, not from erasing useful provider identities.
