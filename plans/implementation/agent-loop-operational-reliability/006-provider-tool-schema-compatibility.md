# Agent Loop Operational Reliability M006 — Provider Tool Schema Compatibility

Status: closed

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify actual starting head)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: No new ADR unless required to change public provider capability semantics; preserve current provider retry/idempotent dispatch contracts.

Primary class: infrastructure

## 1. Objective

Prevent unsupported deferred-tool definitions from reaching an endpoint/model that rejects them, and provide bounded *request-only* recovery for a tool-schema HTTP rejection before any tool has run. Preserve full authorized coding capability through deterministic nondeferred fallback.

## 2. Why this milestone is ready

Provider capabilities, wire codecs and request preparation are existing owners. No external provider contract acceptance is required to implement a conservative fallback; reproduce reported model/gateway mismatch with captured fixtures.

## 3. Current implementation evidence

- Child test failed after queueing when a model/provider rejected `defer_loading` in tool definition. Capture the selected provider ID, model, endpoint wire surface, response status/body and version from actual run; report alone cannot identify exact adapter defect.
- `src/agent/request_preparation.rs` checks `ProviderCapabilities::for_provider(provider.id())` to partition tools when deferred loading is enabled; non-deferral path sends all definitions without explicitly clearing `defer_loading` markers already attached by disclosure logic.
- `crates/codegg-providers/src/provider_core.rs` declares deferral support for broad `openai`/`anthropic` IDs; many OpenAI-compatible gateways have stricter endpoint/model support.
- `crates/codegg-providers/src/wire.rs` carries `defer_loading` to canonical tool representation; `ToolDefinition::to_openai` also writes the flag when present. Provider feature support must be verified against *actual outgoing JSON*.

## 4. Invariants

No denied/hidden tool can be newly exposed by fallback; capability compatibility cannot grant provider/tool permission. No silent model/provider switch or changed auth endpoint; no post-execution request replay for a side effect; preserve stable tool call IDs/transcript order. Downgrade may enlarge *visible* schema but must stay within fixed provider schema/token limits and context budgets. Nondeferred fallback is explicit in diagnostics.

## 5. Scope

In: final wire-surface compatibility filtering; strip unsupported `defer_loading` flags; endpoint/model support lookup with conservative unknown fallback; one bounded safe 400/rejected-tool request retry where unacknowledged; provider/child test fixtures and actionable failure classification.

Out: wholesale provider transport redesign, arbitrary cross-provider failover, changing model routing preferences, hosted-program architecture, model-specific hard-coded bypasses without captured evidence.

## 6. Required production changes

**Capabilities:** distinguish provider family, concrete endpoint/wire surface and model features for deferral rather than trusting provider ID alone. Add a final `sanitize/validate_tool_definitions` boundary before serialization. For unsupported deferral: all authorized definitions immediate or documented capped deterministic subset with access to discovery; strip `defer_loading` fields entirely, not `false` if endpoint rejects that key too. Assert `tool_search` remains usable.

**Wire:** support OpenAI-compatible chat, Responses and Anthropic separately. Never leak an unsupported field into nested function schema or an alternate endpoint. Test exact JSON payloads. If model supports real deferred loading, preserve existing behavior.

**Retry:** recognize a typed *pre-tool* unsupported-schema HTTP error; once only, retry the **same model and endpoint** with authorized nondeferred definitions when response established no tool effect/acknowledged execution. Never downgrade a policy Deny, invalid credentials, quota error, ambiguous streaming completion, or a request that may have executed a side effect. Surface an actionable typed failure when downgrade cannot fit limits.

**Child:** propagate selected endpoint/model capability into spawned durable runs; ensure child and root preparation agree. No new scheduler.

## 7. Ordered work packages

A. Capture actual failing request and fake gateway that rejects `defer_loading` even when the nominal provider ID is `openai`; add failing main and child fixtures.

B. Build a single final wire-capability decision path and negative assertions for unsupported fields; connect to `request_preparation.rs` without overriding `ResolvedToolSurface`.

C. Add bounded request-only retry for confirmed compatibility error and a deterministic fallback; preserve original provider identity/budget and error diagnostics.

D. Verify compatibility across representative chat/Responses/Anthropic mock adapters and child spawn; update operator docs/model-profile behavior.

## 8. Failure, cancellation, restart, and contention

After a provider rejects tool schema before response, retry once only if retry-safe; cancellation/deadline overrides. Streaming uncertainty or any acknowledged tool call prohibits replay. No cross-run cache of an unsafe transient capability inference unless validated/scoped by endpoint+model+revision. On restart, capability fallback does not mutate durable authority ceilings. Concurrent child requests cannot contaminate other provider surfaces.

## 9. Compatibility and migration

No DB migration anticipated; preserve provider config/TOML and existing tool schemas. Optional capability override requires explicit provenance, validation and backward-compatible defaults. Unknown endpoints conservatively receive nondeferred wire. Avoid shipping vendor-specific extension keys to generic OpenAI-compatible endpoints.

## 10. Required tests

Focused: `ProviderCapabilities`, `request_preparation`, `wire.rs` payload serialization, provider retry context. Integration: scripted server returns 400 for deferred schema, second request carries no unsupported field and model can still call `read`/`write`/`bash` subject to authority; child spawn keeps same endpoint. Restart: saved provider config and child run remain compatible. Contention/cancel: parallel child requests, deadline during downgrade. Security negatives: no retry after any acknowledged mutation, no fallback on auth/permission denial, no unauthorized tool leak, bounded tool count and payload, malformed error.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

First run focused test filters for the named modules (`cargo test -p codegg --lib <filter> --locked`) and existing integration targets; confirm actual crate/target names at execution time. Record exact commands, stdout/result and head in closure. Hosted checks only where the existing closure policy requires them. No live-provider CI or new scheduled test lane.

## 12. Documentation updates

Update only affected `architecture/` and `docs/` surfaces; record any compatibility/config changes in `docs/tools.md` or `architecture/provider.md` where applicable. Keep existing predecessor closure history immutable.

## 13. Acceptance criteria

1. Unsupported endpoints/models never receive `defer_loading` or other unapproved deferral wire fields, including when preparer used a non-deferral branch.
2. Compatible endpoints retain deferral functionality and callable discovery.
3. A pre-execution schema 400 can recover once without switching provider/model or repeating a side effect; unsupported recovery emits typed actionable error.
4. Root and child agent fake-gateway fixtures both pass with stable tool authority and transcript semantics.

## 14. Stop conditions

Stop and record a blocker if implementation would need a new authority owner, broad sandbox changes, unsafe command execution in tests, an undocumented provider wire workaround, a nonadditive migration, or an unaccepted architectural decision. A test-only mitigation with a production gap is not closure.

## 15. Handoff

At start reconcile this plan against actual code and exact head. Preserve unrelated changes. Commit scoped production/tests/docs and add `plans/closure/agent-loop-operational-reliability/NNN-status.md` with requirement/evidence table, exact commands and outcomes, restart/contention/security proof, and residuals. Update registry and roadmap to reflect actual closure or blocker only.
