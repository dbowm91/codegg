# Agent Tool Runtime Reconciliation M004 — Provider Deferral Wire Qualification

Status: implemented; closed (`plans/closure/agent-tool-runtime-reconciliation/004-status.md`)

Repository baseline: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Long-term requirements: `plans/000-long-term-specification.md#46-progressive-disclosure`, `#47-correctness-before-transparent-magic`, `#29-system-invariants`, and `plans/003-planning-process.md`.

Applicable ADRs: existing provider wire kernel and ToolSurface owners. No ADR unless provider transport ownership or hosted-execution model changes.

Primary class: infrastructure

## 1. Objective

Make tool deferral an **actual provider-wire behavior**, not just a `ToolDefinition.defer_loading` label or `ProviderCapabilities` flag. Qualify native tool search for exact supported API/model combinations, ensure honest behavior on ordinary OpenAI-compatible/other providers, and quantify context and latency effects. Preserve the existing host `tool_search` as an alternative model-callable discovery tool where its semantics are actually functional.

## 2. Why this is sequenced

M002 is a hard predecessor: a deferred model tool must exist in the canonical discoverable/callable catalog, with a stable immutable schema. M003 is a soft dependency for security tests, not for protocol encoding. Provider DTOs and client transports are mature enough to capture deterministic wire behavior independently of live API credentials.

## 3. Verified research/evidence

As of 2026-10-13:

- `src/agent/request_preparation.rs` sees `caps.supports_defer_loading` and partitions immediate and deferred definitions, but returns both sets for provider requests. Providers with capability false get the full allowed definition list.
- `crates/codegg-providers/src/provider_core.rs::ProviderCapabilities::for_provider()` asserts deferral for the generic `openai` and `anthropic` IDs. That is **not** proof of support for the currently selected wire/API/model.
- `crates/codegg-providers/src/responses_api.rs::ResponsesTool::from_tool_definition()` emits a function `{type,name,description,parameters,strict}`, without `defer_loading`. `tool_search` is presently a CodeGG function name, **not** necessarily the provider built-in `type: "tool_search"`.
- Inspect the actual Anthropic Messages serializer and streaming/history structures during implementation; do not assume its DTO supports `defer_loading`, provider-native `tool_search_tool_bm25_20251119`, `tool_reference` or follow-up replay merely because the capability table says true.
- OpenAI provider documentation specifies `defer_loading: true` for deferred function definitions/namespaces **and** a provider built-in `{"type":"tool_search"}` in Responses (not merely CodeGG's own `tool_search` function). Documentation: https://developers.openai.com/api/docs/guides/tools-tool-search.
- Anthropic Messages documentation specifies that the request still carries every deferred definition with `defer_loading: true`, plus a built-in tool search variant (BM25/regex); the tool-search `server_tool_use`/`tool_search_tool_result`/`tool_reference` events must survive history round trips, and the next request must retain the full toolset. Its tool search itself must **not** be deferred. Documentation: https://platform.claude.com/docs/en/agents-and-tools/tool-use/tool-search-tool.
- Other OpenAI-compatible chat endpoints generally do not expose the same native built-in contract; *never* assume compatibility from shared JSON field names or vendor labels.

Research confidence: the public API requirements above are vendor-documented; production support must still be verified against actual code and wire capture. No live-model result has been obtained as part of this planning pass.

## 4. Mandatory invariants

One immutable `ResolvedToolSurface` controls **all** tool identities, whether eager, provider-native deferred, or host-discoverable. Neither model-selected provider path nor deferral/fallback widens parent ceilings, permissions, plugin activation, MCP exposure or tool contracts. No provider accepts a deferred callable function that CodeGG cannot resolve/authorize at dispatch.

Provider-native tool search must be distinguished from `ToolSearchTool` (ordinary client-executed CodeGG function). Provider tool references are not executable grants, and must resolve only to session-pinned, policy-eligible catalog entries. Retry/failover to a different provider recomputes a supported wire mode; never replay `tool_reference` objects to an API that does not support them.

Mode selection must use explicit provider **wire surface**, model capabilities and tested API version/features. A generic ID match, or `supports_responses_api`, is insufficient. If unqualified, fall back conservatively to a **full allowed tool array** with an ordinary CodeGG `tool_search` function, not misleading or silently omitted schemas. Host-side progressive staging may be implemented only with a coherent re-request/continuation and proven wire semantics; otherwise it remains out of scope.

## 5. Scope and required production changes

### A — Honest capability/wire matrix

Audit `ProviderCapabilities` as consumed by native OpenAI Responses, Anthropic Messages, OpenAI-compatible Chat Completions, EggPool/OpenCode Go multi-surface, and aliased providers. Replace generic `supports_defer_loading` inference with a qualified `ToolDisclosureWireMode` or equivalent resolved representation (`NativeOpenAiToolSearch`, `NativeAnthropicToolSearch`, `FullCompatibility`, optionally `HostStaged` only when proven). Matrix requires provider, selected wire surface, model eligibility, API version/feature flag and fallback. Unrecognized combinations are `FullCompatibility`.

Do not alter the shared EggPool profile contract or its role as connection/wire-surface owner; consume its chosen surface. Avoid expensive capability probing on every turn. Honor existing `tool_deferral.defer_loading`, `always_loaded`, `max_initial_tools`, disable and role-specific settings without pretending a provider supports an unavailable feature.

### B — OpenAI Responses native path (feature-gated until qualified)

Correctly serialize function tool definitions with `defer_loading` inside the documented namespace/function representation where required, and include `{"type":"tool_search"}` as a distinct provider-owned built-in entry when the selected Responses/model combination supports it. Ensure the configured model/tool count, schema limits, strict-mode requirements and tool-call/results parse on a recorded wire fixture. The built-in must **not** dispatch through CodeGG's normal `ToolRegistry` or be confused with the model-callable `tool_search` function.

Handle search-discovered function calls and continuation/history items according to the native protocol. If the codegg provider cannot preserve the necessary hosted results, leave the mode disabled and keep compatibility fallback; do not advertise completed native deferral based on HTTP 200 alone.

### C — Anthropic Messages native path (feature-gated until qualified)

Serialize every full tool definition every request, flag deferable definitions with `defer_loading:true`, and add the correct provider-owned tool search variant only for a documented eligible model/API. Keep the tool-search tool itself immediate. Preserve server tool use/search result/`tool_reference` content blocks in streamed and non-streamed responses, history and replay/compaction boundaries. Never synthesize a client `tool_result` for a provider-owned server-tool-search ID. Confirm cache-control placement is valid with deferred definitions. Use fixture-first proof of two-turn discover→call before enabling native mode.

### D — Generic compatibility and optional host-local staging

For standard OpenAI-compatible Chat Completions (including OpenCode Go-selected compatible routes), local models and unqualified providers, send the authorized complete tool definitions (current baseline behavior) and allow CodeGG `tool_search` as ordinary call; log this as **full compatibility**, not native deferral. If token pressure merits a separate host-staged mode, give it a separate milestone/ADR if it requires new continuation handling or changes user-visible call semantics. Host keyword lookup alone does not make undisclosed tools callable at the provider; explicitly test model re-request with newly advertised schema before claiming it works.

### E — Diagnostics, docs and safeguards

Update `architecture/tool.md`, `architecture/agent-tool-surface.md`, provider docs, relevant provider tests and operator config comments. Record per-turn selected `ToolDisclosureWireMode`, immediate/deferred tool counts, definition bytes, provider surface, fallback reason, caching behavior, and no credential leaks. Add static checks that a provider-native mode emits its required built-in/schemas and that unknown provider + alias cannot accidentally enable it.

## 6. Ordered work packages and closure evidence

1. **Baseline wire census**: record captured request JSON and response shapes for Anthropic Messages, OpenAI Responses, generic Chat Completions and OpenCode Go-selected surfaces. Include stream and non-stream fixtures, disabled/role/plan filters. This establishes whether the current advertised deferral is a no-op.
2. **Mode qualification/fallback**: implement matrix keyed on resolved wire surface/model and explicit support. Negative unsupported feature/HTTP 400 downgrade requires deterministic behavior, bounded retries and no authorization widening.
3. **OpenAI native slice**: serializes deferred functions/provider built-in and verifies two-turn discover→call with fixture; gated until response/history semantics are proven.
4. **Anthropic native slice**: preserve provider search/result/reference events, full definitions on next request, correct caching and no bogus client response; gated until proven.
5. **Regression/benchmark closure**: compare FullCompatibility vs native with 5/40/100 tools of varying schema sizes and mix of native/plugin/raw MCP. Record exact serialized tool bytes, provider cache impact if measurable, baseline token estimate, time-to-first-executable-tool and search-step cost. Never promise universal savings.

## 7. Failure, cancellation, restart and compatibility

No storage schema migration expected. Add provider DTO fields and protocol variants only where necessary; all additions must preserve existing chat/Responses adapters and EggPool wire negotiation. Network failure or unsupported built-in must not result in unbounded retry or implicit provider switch. For a mid-conversation unsupported-mode fallback, preserve canonical tool-call outcomes and history only if the target wire can represent them; otherwise stop the turn with a typed actionable error rather than silently discarding prior tool references. Cancellation propagates through existing provider request control. Restart rebuilds mode from pinned provider/model config; do not persist speculative support based on one success.

## 8. Required tests and verification

Wire capture tests assert **literal JSON shape**, not only internal structs:

- OpenAI Responses native mode: provider `tool_search` built-in, deferred function flags in correct nesting, mixed immediate/deferred, negative missing built-in, direct function call after search.
- Anthropic native mode: search variant, full tool array, correct defer flags, server search events and tool_reference roundtrip, no invented tool-result for hosted search, cache-control negative.
- OpenAI-compatible default: no native-only keys, no silent schema elision, survives provider aliasing and server-selected OpenCode Go surface.
- All providers: 0/5/40/100 tools, `max_initial_tools`, model/role denial, plan mode, tool_search discoverability vs executable catalog, plugin/MCP dynamic rev, model-switch and failover.
- Conservative downgrade on invalid model, unsupported tool search, wrong API version, unknown provider; distinguish retryable transport failure from unsupported feature.

Commands (verify actual selectors/paths):

```bash
cargo fmt --all -- --check
cargo test -p codegg-providers
cargo test --workspace tool_deferral
cargo test --workspace tool_search
cargo test --workspace provider
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/verify.sh quick
```

If live provider credentials/test models are not available, deterministic transport qualification may close **infrastructure fixtures** but the corresponding live native mode remains gated and must be reported explicitly. Never mark model-specific support operational without on-wire confirmation.

## 9. Acceptance and stop conditions

M004 closes when per-wire behavior is accurate; no generic provider claims unsupported native deferral; at least all fallback paths work with policy-eligible tools; each enabled native path has byte-level fixtures plus correct two-turn search/reference/call and guarded version/model capabilities; measurable payload/prompt tradeoffs recorded; no live-provider evidence is implied when not run.

Stop if a provider requires unsupported hosted state preservation, API-only feature licensing, an unverified model entitlement, cross-wire history replay that would lose tool references, a new execution authority, or source-of-truth change in EggPool profile. Disable that native mode, leave explicit status in closure, and if needed register a separate successor rather than pretending to qualify it.

Closure: `plans/closure/agent-tool-runtime-reconciliation/004-status.md` with SHAs, mode matrix, byte-level captured fixtures, benchmarks, regression commands/outcomes, live-provider availability and explicit residual limitations.
