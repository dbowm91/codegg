# Upstream Tool-Surface Compatibility Corrective Addendum

Status: ready

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Planning branch: `agent/tool-surface-compat-2026-09`

Normative references:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/003-planning-process.md#2.4-milestone-implementation-plans`
- `plans/003-planning-process.md#7-corrective-passes`
- `plans/003-planning-process.md#9-registry-requirements`

Predecessor subsystem evidence:

- `plans/subsystems/search-eggsearch-integration-roadmap.md`
- `plans/closure/search-eggsearch-integration/005-status.md`
- `architecture/mcp.md`
- `architecture/deterministic_tools.md`
- `architecture/native_crates.md`

Audited upstream baselines:

- eggsearch `0.3.9`, released 2026-09-11;
- eggsact `1.2.5`, released 2026-09-11;
- CodeGG still initializes MCP clients with protocol `2024-11-05` and consumes eggsact in-process through `eggsact::agent::ToolRegistry`.

## 1. Corrective trigger

The search/eggsearch workstream closed against an eggsearch 0.3.6-era contract. Subsequent upstream releases remained backward compatible but expanded the integration surface materially. Eggsearch 0.3.8 added extractive evidence controls, query-focused fetch, cache policy controls, deterministic freshness/cache metadata, and additional provider capabilities. Eggsearch 0.3.9 added persistent Streamable HTTP MCP and integration/operational support. Its current harness contract documents MCP `2026-07-28`, `server/discover`, `structuredContent`, `outputSchema`, annotations, cacheable tool metadata, and fingerprint-based catalog identity.

Eggsact 1.2.5 likewise added MCP `2026-07-28`, modern result envelopes, progressive discovery facades, additional typed preflight support, and a larger built-in profile set. CodeGG does not use eggsact through MCP, which is still the correct ownership choice, but CodeGG duplicates the upstream profile list and silently falls back to `Profile::Default` when a configured profile is unknown.

The current CodeGG implementation is therefore compatible through legacy/additive behavior, but no longer fully aligned with either upstream contract.

## 2. Ownership boundary

CodeGG owns:

- the model-facing native tool palette;
- generic MCP client transport, protocol negotiation, metadata preservation, and policy;
- tool discovery/hydration policy and CodeGG's `tool_search` UX;
- eggsearch argument ergonomics, framing, trust/provenance projection, and context budgeting;
- eggsact selection policy, audience/profile choice, CodeGG-facing names, and harness-side preflight orchestration.

Eggsearch owns:

- external search/fetch/provider behavior;
- MCP schemas, structured evidence metadata, output schemas, annotations, and discovery metadata for eggsearch tools;
- response-detail projection semantics and search/fetch cache semantics.

Eggsact owns:

- deterministic tool definitions and schemas;
- profile definitions and profile membership;
- deterministic preflight implementations and machine-code contracts.

CodeGG MUST NOT import eggsact's `tool_search`/`tool_invoke` facade as a second model-facing discovery system. CodeGG already owns progressive disclosure and must use upstream metadata only as input to that policy.

## 3. Invariants

- Legacy MCP servers that support only CodeGG's current initialization path remain usable.
- Modern MCP support is additive and negotiated; CodeGG must not require `2026-07-28` from arbitrary configured servers.
- Raw `mcp__eggsearch__*` tools remain hidden by default.
- `structuredContent` remains authoritative for machine-readable tool results when present; text remains a bounded display/fallback projection.
- Unknown additive upstream fields remain non-fatal.
- Eggsearch remains the sole normal owner of external search/provider execution.
- Eggsact remains an in-process dependency for CodeGG deterministic tools and preflight; no eggsact subprocess/MCP hop is introduced.
- Unknown eggsact profiles must not silently change execution policy to `default`.
- Upstream capability expansion does not automatically expand CodeGG's immediate model-visible palette.
- No new CI lane, compatibility matrix, scheduled network test, dependency bot, or release automation is introduced for this corrective work.

## 4. Non-goals

- Rewriting CodeGG's MCP stack around a third-party SDK.
- Requiring every MCP server to implement `server/discover`.
- Mirroring every eggsearch option into every CodeGG native wrapper.
- Auto-executing eggsearch `next_actions` without CodeGG policy.
- Exposing all eggsact 1.2.x utilities merely because they exist.
- Importing eggsact's progressive-discovery facade into CodeGG.
- Deleting the explicit legacy built-in search fallback.
- Redesigning Tool Programs, provider routing, agent orchestration, or the TUI.

## 5. Dependency graph

```text
M006 — Generic MCP modern protocol and metadata compatibility
   |
   | hard for modern eggsearch discovery/metadata consumption
   v
M007 — Eggsearch 0.3.9 request/control and projection alignment

M008 — Eggsact 1.2.5 in-process/profile compatibility
   (independent; may execute in parallel)
```

Dependencies:

- M006 -> M007: **hard** for modern protocol/discovery metadata. M007 request-field translation may be developed against fixtures in parallel, but closure requires the final generic MCP behavior.
- eggsact 1.2.5 public in-process API: **interface** dependency for M008.
- locally available upstream binaries: **operational** only for bounded manual smoke evidence; ordinary CI must remain offline/deterministic.

## 6. Milestones

### M006 — Generic MCP modern protocol and metadata compatibility

Class: infrastructure / compatibility

Objective: modernize CodeGG's local and remote MCP clients so modern servers can negotiate and expose current metadata without breaking legacy servers.

Required outcome:

- central protocol-version policy rather than duplicated string literals;
- capability-aware negotiation with a safe legacy fallback path;
- preservation of `outputSchema`, annotations, and relevant server/discovery metadata in CodeGG's MCP tool representation;
- optional `server/discover` use only when negotiated/supported;
- no regression to current `structuredContent` consumption;
- cache/catalog identity based on meaningful tool metadata where CodeGG caches or compares tool inventories, not tool count alone;
- architecture docs and focused transport tests updated.

Implementation plan: `plans/implementation/tool-surface-upstream-compatibility/006-mcp-modern-protocol-and-metadata.md`.

### M007 — Eggsearch 0.3.9 request/control and projection alignment

Class: capability compatibility / evidence quality

Objective: expose the high-value additive eggsearch controls that CodeGG's stable facade currently drops, while aligning response-detail handling with CodeGG's existing structured-result/context policy.

Required outcome:

- audit every CodeGG eggsearch wrapper against 0.3.9 schemas;
- add justified translation for search `excerpt_count` and fetch `focus`, `cache_policy`, and `max_cache_age_seconds`, including batch-item cache policy where supported;
- deliberately handle `response_detail` rather than accidentally relying on upstream defaults;
- retain full structured evidence internally while bounding model-visible output;
- preserve current trust, stable-ID, structured-warning, retrieval-state, and next-action data without auto-execution;
- update compatibility fixtures to reject unknown/stale field assumptions;
- no provider-specific implementation returns to CodeGG.

Implementation plan: `plans/implementation/tool-surface-upstream-compatibility/007-eggsearch-0.3.9-surface-alignment.md`.

### M008 — Eggsact 1.2.5 in-process/profile compatibility

Class: correctness / simplification

Objective: remove stale duplicated eggsact capability knowledge and consume the current in-process API without broadening CodeGG's model-facing tool surface unnecessarily.

Required outcome:

- update dependency/lock baseline intentionally to current compatible eggsact 1.2.5 or document why the resolved version differs;
- replace CodeGG's hard-coded four-profile allowlist with upstream profile parsing/metadata;
- reject or explicitly diagnose unknown configured profiles instead of silently falling back to `Profile::Default`;
- verify current audience/exposure semantics remain correct;
- assess and, if valuable, use typed `DependencyPreflight` or other typed wrappers on harness-side paths without duplicating tool semantics;
- keep eggsact MCP discovery facades out of CodeGG's model-facing registry;
- document which new eggsact utilities remain intentionally deferred.

Implementation plan: `plans/implementation/tool-surface-upstream-compatibility/008-eggsact-1.2.5-inprocess-compatibility.md`.

## 7. Compatibility and migration

No durable storage migration is expected.

MCP protocol modernization must be negotiated. A configured legacy MCP server that works before M006 must continue to work afterward. Modern metadata is additive to CodeGG's internal representation; provider-facing `ToolDefinition` projection must continue to provide the fields existing model adapters require.

Eggsearch wrapper additions should preserve existing CodeGG argument names. New upstream controls should use CodeGG-native names identical to upstream where there is no reason to alias. Stale or conflicting aliases must fail with actionable validation rather than being silently discarded.

Eggsact profile parsing is a configuration compatibility change: currently invalid/unknown names silently become `default`. That behavior is unsafe and must be replaced with explicit failure/diagnostics. Existing valid built-in profile names continue unchanged.

## 8. Security, failure, and resource semantics

- Modern MCP negotiation must not weaken existing remote URL/SSRF validation, auth, timeout, reconnect, or process-environment controls.
- `server/discover` and tool metadata are untrusted integration metadata; they do not grant tool execution authority.
- `outputSchema` validation, if added, must fail safely and must not cause raw external content to bypass CodeGG framing.
- Eggsearch cache controls must not create a second CodeGG cache or persistent search index.
- `response_detail=compact` must never be interpreted as evidence absence when structured retrieval metadata says providers failed or evidence is incomplete.
- Eggsact profile/audience failures must be deterministic and visible; no security-relevant fallback to a broader profile is allowed.

## 9. Verification policy

Verification remains intentionally narrow. Each milestone should use focused unit/integration tests plus the repository's ordinary quick verification posture. No new CI architecture is authorized.

Normal broad posture:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Where a real upstream binary is available, one local manual smoke may be recorded in closure evidence. It must not become a required network-dependent CI job.

## 10. Closure criteria

This corrective addendum closes only when all three milestones have closure records or an explicit evidence-backed deferred disposition.

At closure:

- modern eggsearch can use the current MCP metadata path without forcing legacy-only behavior;
- CodeGG can reach the selected high-value eggsearch 0.3.9 controls through its stable wrappers;
- eggsact profile selection cannot silently widen/change policy;
- CodeGG still owns one progressive-disclosure system and one normal external-search integration boundary;
- architecture/docs name the actual supported upstream baselines and compatibility strategy.
