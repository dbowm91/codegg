# Search and Eggsearch Integration Milestone 003 — Structured Contract and Compatibility Closure

Status: blocked — hard dependency on M002 closure; operational real-binary evidence required for final closure

Repository baseline:

- CodeGG audited baseline: `40dbd1981abf1a8d96d7ab9f5ebefb4b763053f2`
- roadmap addition: `24c4df7ecdf8477cf27d51e0e92acd777d61427d`
- M001 plan addition: `1cd5a465c54e9b7791091e8534a99fc453f656f6`
- M002 plan addition: `ded8f4d17d077cdfe101beac61843a8050ec8937`
- eggsearch audited baseline: 0.3.6, release commit `4ccb374af00348bba75761f6bbd1e192d385a2b9`

Source roadmap:

- `plans/subsystems/search-eggsearch-integration-roadmap.md#milestone-003--structured-contract-consumption-and-compatibility-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#46-progressive-disclosure`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`

Applicable ADRs:

- None expected. If preserving eggsearch structured responses requires a material redesign of generic MCP response semantics for all servers, stop and split that concern rather than silently broadening this milestone.

Predecessor plans:

- `plans/implementation/search-eggsearch-integration/001-current-eggsearch-contract-repair.md`
- `plans/implementation/search-eggsearch-integration/002-external-search-ownership-consolidation.md`

Required predecessor closure:

- `plans/closure/search-eggsearch-integration/001-status.md`
- `plans/closure/search-eggsearch-integration/002-status.md`

Primary class: infrastructure / compatibility closure

## 1. Objective

Close the search/eggsearch corrective workstream by making CodeGG consume eggsearch as a structured evidence service rather than flattening its responses into opaque text, improving compatibility/capability diagnostics, and demonstrating the final wrapper set against one real current eggsearch MCP process.

M003 must preserve the stable model-facing CodeGG tool surface and its trust framing while retaining upstream machine-readable data for internal consumers. It must use existing `StructuredToolResult::value` where possible, avoid truncating/corrupting the structured value, and ensure future incompatibility is reported as a contract/capability problem rather than being masked by permissive mocks.

The final evidence requirement is intentionally small: one local real-binary compatibility smoke against eggsearch 0.3.6 or the current audited compatible successor. Do not create a permanent live-network CI matrix.

## 2. Why this milestone is blocked

M003 has two prerequisites.

Hard dependency:

- M002 must be closed so the structured response work applies to the final single-owner external search architecture rather than preserving metadata for direct provider paths that are about to be removed.

Operational closure dependency:

- a current eggsearch binary must be runnable locally to capture real MCP initialize/tool discovery and invoke the supported wrapper set.

The operational dependency does not justify CI expansion. Implementation may proceed using deterministic fixtures after M002 closes; strict M003 closure waits for the bounded local real-process evidence.

## 3. Current implementation evidence

### 3.1 CodeGG already has a structured tool-result field

`src/tool/backend.rs` defines:

```text
StructuredToolResult {
    output: String,
    success: bool,
    value: Option<serde_json::Value>,
    provenance: Option<ToolProvenance>,
}
```

and provides `StructuredToolResult::with_value(...)`.

Current eggsearch wrappers generally call a dispatch function returning `String`, then construct `StructuredToolResult::with_provenance(...)`, leaving `value = None`.

This means CodeGG already has the receiving contract needed for structured eggsearch values; M003 should use it rather than inventing another evidence-result type unless current repository evidence proves that insufficient.

### 3.2 MCP result handling currently flattens content

The current local MCP client extracts text parts from `tools/call` content and joins them into a `String`. The generic `McpService::call_tool` therefore exposes a text-oriented result to search adapters.

Eggsearch 0.3.6 emits JSON tool results and documents stable machine-readable fields intended for harnesses. CodeGG currently wraps/caps the serialized result as model-facing text, so internal consumers do not reliably retain the parsed object.

### 3.3 Current CodeGG output clamping can destroy JSON shape

`src/search_backend/framing.rs` clamps the raw serialized response before applying external-content framing. For large JSON outputs this is safe as a string display operation but the truncated text is no longer a parseable complete upstream object.

This is acceptable for a legacy model-facing string only if the full parsed structured value is retained separately before display projection/truncation.

### 3.4 Current eggsearch harness contract contains useful structured metadata

The current eggsearch contract includes, depending on tool/response:

- deterministic `stable_id` values for source/fetch/evidence linkage;
- `structured_warnings` with stable code/severity/scope fields;
- trust markers including sanitization/injection signals;
- `next_actions` with tool/reason/priority/input templates;
- `routing_decision` describing selected/skipped/degraded providers;
- retrieval summary/dimension states;
- structured repository fetch locators and source metadata;
- security applicability/confidence metadata;
- research claims/conflicts/gaps where provided;
- provider/server/tool capabilities through `provider_status`.

CodeGG does not need to implement UI behavior for all of these in M003. It does need to stop destroying them at the integration boundary.

### 3.5 Bootstrap compatibility classification is currently shallow

`src/search_backend/bootstrap.rs` currently discovers tool names and classifies the integration as complete/partial/incompatible based primarily on required/recommended tool presence.

This is useful but insufficient to distinguish:

- an eggsearch process with the right tool names but incompatible request schema;
- a server with specialized tool names but degraded underlying capabilities;
- provider-specific credential/routing degradation that should not be treated as global search failure;
- a current compatible server whose optional native providers are unavailable but keyless fallback works.

M001/M002 address request correctness and ownership. M003 should make doctor/bootstrap diagnostics consume current capability/provenance information without becoming a second routing engine.

### 3.6 Existing tests are mostly fake-service tests

M001 is expected to strengthen offline request validation. M003 must add one real-process compatibility path because fake tool definitions cannot prove that CodeGG interoperates with the actual current eggsearch executable and MCP serialization.

## 4. Invariants that must not regress

- M001 request-contract repairs remain intact.
- M002 single-owner external search architecture remains intact.
- Eggsearch remains the default search backend; built-in fallback remains explicit/secondary.
- Raw eggsearch MCP tools remain hidden by default.
- Model-facing output remains bounded and explicitly trust-framed.
- Structured upstream data retained internally is evidence/data, not instructions.
- `StructuredToolResult::value` must represent the parsed upstream result, not the already-truncated/framed model display string.
- Truncating model-facing text must not mutate or invalidate the retained structured value.
- CodeGG must tolerate additive unknown upstream fields.
- Baseline provider credential absence remains provider-scoped degradation rather than a global CodeGG failure when eggsearch keyless operation is available.
- CodeGG does not auto-execute eggsearch `next_actions` outside normal tool policy/permission boundaries.
- No unbounded structured value is injected into model context solely because it is retained internally.
- No per-call eggsearch process spawn.

## 5. Scope

### In scope

- eggsearch-specific structured result parsing/transport at the existing MCP/search boundary;
- use of `StructuredToolResult::value` by eggsearch wrappers;
- safe separation of full structured value from bounded/framed display output;
- provenance/version/capability metadata where available through existing or narrowly additive MCP bootstrap state;
- `provider_status` parsing for doctor/search capability diagnostics;
- current eggsearch stable IDs/warnings/trust/routing metadata preservation;
- one local real eggsearch MCP compatibility smoke covering the supported wrapper set;
- documentation of the supported contract and upgrade/diagnostic procedure;
- final roadmap/registry/closure reconciliation.

### Explicitly out of scope

- building a new evidence database or graph in CodeGG;
- UI panels for every warning/next-action/retrieval field;
- automatically following `next_actions`;
- allowing upstream metadata to bypass CodeGG permission/tool policy;
- a generic MCP protocol rewrite for unrelated servers;
- network-dependent CI, scheduled compatibility checks, multiple eggsearch-version CI lanes, or release gates;
- broad browser/PDF feature adoption;
- new provider clients;
- removal of legacy built-in fallback;
- research-synthesis redesign;
- release automation.

## 6. Required production changes

### Core/domain

Define one small internal eggsearch call result representation if needed, containing at minimum:

- parsed upstream `serde_json::Value`;
- bounded/framed display `String`;
- truncation flag;
- upstream server/version metadata when known.

Prefer a search-backend-specific type over changing every generic MCP/tool caller. The wrapper `execute()` path may continue returning the legacy display string; `execute_structured()` must retain the parsed value.

Do not duplicate upstream domain structs merely to deserialize every field. `serde_json::Value` plus narrow typed readers for capability/warning fields is sufficient unless an existing CodeGG consumer genuinely needs typed structures.

### Storage and migrations

No durable storage migration is required.

Do not automatically persist complete raw eggsearch values beyond existing tool/context artifact behavior. Structured retention for the current execution path is enough.

### Protocol and DTOs

Preserve the model-facing CodeGG tool schemas accepted in M001.

For structured outputs:

1. capture the complete MCP result payload before CodeGG display truncation;
2. parse the eggsearch JSON deterministically;
3. retain the parsed object as `StructuredToolResult::value`;
4. derive bounded model display from the parsed/raw result using existing trust framing/output caps;
5. mark provenance truncation only when display/output was actually shortened;
6. do not overwrite the structured value with a truncated string representation.

If current MCP `Content::json` arrives through the existing text path as valid serialized JSON, a narrow parse at the search adapter is preferred over broad MCP changes.

If the existing generic client discards structured MCP content in a way that makes faithful parsing impossible, add the smallest additive MCP method/result form needed and leave existing `call_tool -> String` behavior intact for other callers.

### Runtime and concurrency

No new background tasks.

Parsing occurs per call after MCP response receipt and before display clamping.

Respect existing per-tool timeouts and parent cancellation/deadline behavior.

Large structured responses must remain bounded in model context. Internal parsed values should use existing context/artifact projection mechanisms where later consumers need persistence; do not bypass existing context policy.

### Frontend or operator surface

Enhance `codegg doctor search` / bootstrap reporting to distinguish:

- eggsearch executable/process unavailable;
- MCP initialization failure;
- missing required tools;
- missing recommended specialized tools;
- current server version when available;
- provider-status server/tool capabilities where available;
- provider-specific degraded/unroutable states without misclassifying the whole server as unavailable;
- inability to parse the expected structured compatibility response.

Do not print secrets, full provider config, or massive raw `provider_status` JSON.

### Security and authorization

Retain CodeGG's external-content framing for model output.

Preserve upstream `trust_markers` and structured warnings in the internal value. Do not downgrade a prompt-injection warning because CodeGG also adds outer framing.

If eggsearch identifies local workspace content as `local_trusted`, preserve that provenance distinction internally but continue treating content as non-instructional.

`next_actions` are suggestions only. They may be surfaced to existing orchestration later, but M003 must not execute them automatically.

### Documentation and static guards

Update:

- `architecture/search_backend.md`;
- `architecture/tool.md`;
- `architecture/mcp.md` if an additive structured MCP method is introduced;
- `architecture/config.md` / doctor guidance as appropriate;
- README install/doctor guidance if needed.

No new static guard is required. Focused tests plus one real-process smoke are sufficient.

## 7. Ordered work packages

### Work package A — Preserve the complete upstream value before display projection

Intent:

Separate machine-readable evidence from model-facing bounded text.

Required changes:

- identify exact current MCP result representation for eggsearch `Content::json`;
- parse the complete result before `clamp_output`;
- create/return an integration result containing parsed value + display string + truncation metadata;
- preserve the legacy string-returning tool path.

Acceptance evidence:

- a large valid eggsearch fixture produces a complete parseable structured `value` while the model-facing output is truncated/framed;
- malformed/non-JSON upstream output returns an actionable integration error or controlled legacy fallback behavior defined by the implementation, never a panic.

### Work package B — Populate `StructuredToolResult::value` for all eggsearch wrappers

Intent:

Use CodeGG's existing structured tool contract rather than creating a parallel evidence channel.

Required changes:

- update wrapper `execute_structured()` methods to use parsed upstream values;
- retain provenance/truncation timing;
- keep `execute()` output unchanged in trust/framing semantics;
- cover `websearch`, `webfetch`, `repo_search`, `repo_fetch`, `repo_map`, `security_search`, `research_search`, `batch_fetch`, and `evidence_bundle`.

Acceptance evidence:

- focused tests assert `value.is_some()` and inspect representative stable IDs/warnings/routing/trust fields;
- legacy output remains bounded/framed.

### Work package C — Consume provider/capability diagnostics narrowly

Intent:

Make doctor/bootstrap compatibility reporting reflect the current server contract without duplicating eggsearch routing decisions.

Required changes:

- parse the bounded subset of `provider_status` needed for diagnostics;
- record server/tool capability presence and degraded/routable provider state where supplied;
- retain existing required/recommended tool-name discovery;
- report current server version if the MCP initialize state can provide it through a small additive change;
- avoid treating missing optional credentials as global incompatibility.

Acceptance evidence:

- doctor tests cover compatible keyless/degraded provider state, missing required tool state, and unavailable server state;
- no doctor output leaks secrets or dumps unbounded JSON.

### Work package D — Preserve machine-readable warning/identity/trust fields

Intent:

Ensure CodeGG can later chain evidence without another refetch caused by adapter data loss.

Required changes:

No new database is required. Tests must demonstrate that parsed values preserve representative:

- `stable_id`;
- `structured_warnings` including severity/code;
- `trust_markers`;
- `routing_decision`;
- `next_actions`;
- structured repo locator/fetch identity where present;
- security/research-specific metadata where present.

Do not build behavior for every field; preservation is the acceptance boundary.

Acceptance evidence:

- round-trip fixture tests prove these fields survive wrapper execution unchanged in `StructuredToolResult::value`.

### Work package E — Add one real-binary compatibility smoke

Intent:

Prove interoperability with actual current eggsearch rather than another permissive fake.

Required changes:

Provide one bounded local verification path that:

1. records `eggsearch --version` or MCP server-info version;
2. starts/uses an actual eggsearch MCP stdio process through CodeGG's normal bootstrap path;
3. discovers the tool inventory;
4. invokes representative valid requests through every supported CodeGG eggsearch wrapper;
5. proves requests reach actual upstream deserialization/handlers;
6. validates structured result parsing;
7. avoids requiring broad live Internet success where a tool can validate/execute structurally without it;
8. clearly distinguishes provider/network failure from request-schema/MCP incompatibility.

Use eggsearch 0.3.6 or, if a newer version is current at execution time, audit that version's published contract and record it. Do not silently claim 0.3.6 evidence when another binary was run.

This smoke is local/opt-in. It must not become a network-required CI lane.

Acceptance evidence:

- closure record contains exact eggsearch version, CodeGG revision, command(s), tool inventory, per-wrapper disposition, and failures if any.

### Work package F — Documentation and final planning reconciliation

Intent:

Close the workstream with accurate operator and maintainer guidance.

Required changes:

- document structured value/display separation;
- document current compatibility baseline and doctor workflow;
- update subsystem roadmap milestone statuses after closure;
- update `plans/registry.md` only after accepted closure evidence.

Acceptance evidence:

- docs no longer imply tool-name presence alone proves compatibility;
- no unresolved medium/high search ownership or current-contract defect remains.

## 8. Failure, cancellation, restart, and contention semantics

### Structured parse failure

If eggsearch returns content that should be JSON under the supported contract but cannot be parsed, return an actionable integration/compatibility error. Do not panic and do not fabricate an empty structured value that looks successful.

If a narrowly documented legacy text response remains supported for a specific older compatibility path, mark `value = None` and identify the degraded compatibility mode explicitly. Do not silently treat it as full current compatibility.

### Display truncation

Display truncation affects only the model-facing/output string and provenance `truncated` flag. It must not mutate the retained parsed object.

### Cancellation/timeouts

Preserve existing timeout and parent cancellation behavior. Parsing after a completed MCP response must not launch follow-up work.

### Restart

Existing eggsearch MCP bootstrap/restart behavior remains authoritative. Do not add a separate process supervisor in M003.

### Contention

Concurrent calls continue sharing existing MCP service state. Parsed result values are per-call and immutable after construction.

## 9. Compatibility and migration

No durable migration.

M003 establishes the compatibility policy for future eggsearch updates:

- tool discovery validates required surface presence;
- request tests validate CodeGG's supported argument subset;
- structured response parsing tolerates additive fields;
- doctor/capability diagnostics surface degraded optional capabilities;
- a real local compatibility smoke is run when intentionally updating the documented supported eggsearch baseline;
- breaking upstream changes require a new corrective plan rather than silent schema edits.

Do not hard-pin users to 0.3.6 if a later compatible version is current. The closure record must name the exact version actually verified.

## 10. Required tests

### Focused unit tests

- parse valid eggsearch JSON result into complete `serde_json::Value`;
- preserve stable IDs/warnings/trust/routing/next-actions fixture fields;
- derive truncated display without mutating structured value;
- malformed structured response fails safely;
- provider-status capability extraction handles missing/additive fields.

### Integration tests

For every eggsearch-backed CodeGG wrapper:

- `execute_structured()` returns `value = Some(...)` for a valid structured fake response;
- output retains the correct external trust frame;
- provenance backend/implementation/truncation remain correct.

Doctor/bootstrap integration tests:

- compatible tool set + keyless/degraded optional providers;
- missing required tool;
- missing specialized/recommended tool;
- server unavailable;
- malformed provider-status data.

### Restart and recovery tests

No new restart test unless implementation changes MCP lifecycle.

### Contention and cancellation tests

No new contention suite unless shared state changes. Existing search backend global-state tests remain serialized.

### Security and negative tests

- prompt-injection/trust marker fields survive structured retention;
- model output remains framed even when structured value indicates local provenance;
- no `next_actions` execution occurs merely because the field is present;
- doctor output does not include credential values.

### Migration and compatibility tests

- M001 legacy request aliases still work through the structured path;
- unknown additive response fields do not fail parsing;
- older text-only behavior, if deliberately retained, is explicit and test-covered rather than accidental.

### Real-process compatibility smoke

Required for closure, local/opt-in, against the exact recorded current eggsearch binary.

It must exercise actual MCP initialization/tool discovery and actual CodeGG wrapper request serialization/deserialization boundary.

## 11. Required verification commands

Expected deterministic minimum after implementation:

```bash
cargo fmt --all -- --check
git diff --check
cargo test --test search_backend_arg_mapping -- --test-threads=1
cargo test --test search_backend_eggsearch -- --test-threads=1
cargo test --test fake_eggsearch_mcp -- --test-threads=1
# focused doctor/structured-result tests added by this milestone
scripts/verify.sh quick
```

Required closure-only real-process evidence should use the repository's implemented helper/command. The closure record must show the exact command rather than this plan inventing a permanent interface before implementation.

A representative acceptable shape is:

```bash
eggsearch --version
# run the dedicated local CodeGG <-> eggsearch compatibility smoke
```

Do not add this as a network-required CI step.

Run `scripts/verify.sh full` only if implementation materially changes generic MCP behavior, shared tool execution, or another broad subsystem. If the correction remains search-specific/additive, `quick` plus focused tests and the real-process smoke are sufficient.

## 12. Documentation updates

- `architecture/search_backend.md`: structured response/value contract, compatibility verification, capability diagnostics.
- `architecture/tool.md`: structured eggsearch wrapper behavior and trust/provenance.
- `architecture/mcp.md`: only if an additive generic MCP structured call/result helper is introduced.
- `architecture/config.md`: doctor/config compatibility notes if affected.
- `README.md`: current eggsearch installation and `codegg doctor search` workflow if needed.
- `AGENTS.md`: focused local compatibility command only if it becomes a supported contributor workflow.
- `plans/subsystems/search-eggsearch-integration-roadmap.md`: final milestone/closure status after acceptance.
- `plans/registry.md`: remove blocked/ready entries and mark roadmap closed only after accepted closure.

## 13. Acceptance criteria

M003 is accepted only when all are true:

1. M001 and M002 are accepted/closed and their invariants remain present.
2. Every eggsearch-backed CodeGG wrapper preserves a complete parsed upstream value in `StructuredToolResult::value` for valid current structured responses.
3. Model-facing output remains bounded, external-content framed, and backward-compatible in its trust semantics.
4. Display truncation does not truncate/corrupt the retained structured value.
5. Representative deterministic `stable_id`, `structured_warnings`, `trust_markers`, `routing_decision`, `next_actions`, and domain metadata survive wrapper execution unchanged.
6. Additive unknown upstream fields do not cause failure.
7. Malformed expected structured responses fail safely and diagnostically rather than panicking or appearing successful.
8. `codegg doctor search` distinguishes server unavailability, required/recommended tool coverage, and provider/capability degradation without requiring optional credentials for baseline compatibility.
9. Server/version information is surfaced when available through a narrow implementation; absence of version metadata alone does not break a compatible server.
10. No eggsearch `next_action` bypasses CodeGG tool policy or executes automatically in this milestone.
11. Focused deterministic tests and `scripts/verify.sh quick` are green.
12. A real local current eggsearch MCP process is exercised through CodeGG's actual integration boundary; the closure record names exact CodeGG revision, eggsearch version, tool inventory, commands, and per-wrapper result.
13. The real-process smoke does not become a permanent network-dependent CI lane or version matrix.
14. No unresolved critical/high/medium defect remains in search ownership, current request compatibility, structured response preservation, or compatibility diagnostics.
15. Documentation and registry accurately reflect the closed single-owner architecture.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- M002 is not closed;
- actual current eggsearch response transport cannot be preserved without a broad generic MCP redesign affecting unrelated servers;
- current eggsearch has introduced a breaking contract change beyond the audited plan assumptions;
- retaining full structured values would bypass CodeGG context/output safety rather than using existing structured/context mechanisms;
- implementation begins automatically executing upstream `next_actions` outside normal permission policy;
- a new evidence database/index becomes necessary to satisfy the plan;
- closure cannot run an actual current eggsearch binary locally;
- real-process evidence shows request incompatibility not owned by already-completed M001 semantics — create a narrow corrective follow-up rather than hiding it in closure prose;
- verification would require new CI infrastructure rather than bounded local evidence.

## 15. Closure evidence required

Create `plans/closure/search-eggsearch-integration/003-status.md` containing:

- accepted implementation commit(s) and exact final CodeGG tree;
- M001 and M002 closure references;
- exact eggsearch binary version and, when known, upstream commit/release;
- description of the structured-result implementation path;
- evidence that parsed value survives display truncation;
- field-preservation matrix for IDs, warnings, trust, routing, next actions, and representative domain metadata;
- doctor/capability diagnostics evidence;
- deterministic focused test commands/outcomes;
- `scripts/verify.sh quick` outcome;
- real-process compatibility smoke commands and outputs summarized per wrapper;
- exact discovered eggsearch MCP tool inventory;
- any provider/network failures clearly separated from request/MCP compatibility failures;
- confirmation that no new CI lane/version matrix/scheduled compatibility job was added;
- unresolved findings classified by severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

If strict closure is accepted, update:

- `plans/subsystems/search-eggsearch-integration-roadmap.md` to closed with all three closure links;
- `plans/registry.md` to remove M001-M003 from ready/blocked work and record the recently closed control point as appropriate.

## 16. Handoff notes

- Use `StructuredToolResult::value` before inventing new result plumbing.
- Parse before display clamping.
- Keep the full structured value internal; model context remains bounded/projected.
- Treat eggsearch's machine-readable metadata as evidence metadata, never instructions.
- Do not confuse optional provider degradation with server incompatibility.
- The real-binary smoke is a compatibility proof, not a reason to create network CI.
- If the current eggsearch release has advanced beyond 0.3.6 at execution time, inspect its published contract and record the exact version verified.
- Preserve unrelated user changes and keep the final pass narrow.
