# LSP Phase 2 Semantic Completion and Test Polish

## Purpose

Complete the remaining Phase 2 work after:

```text
0efc329ea76b1b3de79f70c84a06981fd7d9ab62
ec04a813eb9693effce45364a6b6741df65e7060
```

The production-runtime migration is now substantially complete:

- primary protocol tests use a real `LspClient`;
- service lifecycle tests use a real `LspService` and child process;
- the fake server is a single Cargo binary target;
- `CARGO_BIN_EXE_egglsp-test-server` provides hermetic discovery;
- malformed traffic flows from fake-server stdout into the production reader;
- production server-request dispatch, diagnostics, timeout cancellation, framing failure, single-flight initialization, and service shutdown are tested end to end;
- raw-wire protocol and semantic suites were removed.

The remaining work is narrower and concentrated at the semantic/safety boundary. Current tests validate typed LSP transport values, but they do not yet exercise all higher-level Codegg workflows and preview guarantees that consume those values.

This pass should finish Phase 2 without reopening transport or lifecycle architecture.

## Target State

At completion:

1. The real semantic-context collector is exercised against the fake server.
2. The real security-context path is exercised against the fake server.
3. The real hunk-source-context navigator/collector is exercised against the fake server.
4. Rename, formatting, and code-action responses pass through production preview conversion rather than stopping at raw `WorkspaceEdit`/`TextEdit` deserialization.
5. Representative preview safety failures are verified end to end.
6. The concurrent response test returns responses in genuinely different order from request arrival.
7. Dedicated typed hierarchy APIs are used where they exist; missing narrow typed adapters are added only when justified.
8. Fixed sleeps in production integration tests are replaced with bounded condition waits.
9. Test-support inspection APIs are either intentionally supported diagnostics APIs or gated/documented as test support.
10. Documentation accurately describes the resulting coverage without overstating untested paths.

## Scope

Likely root-crate files:

```text
src/lsp/semantic_context.rs
src/lsp/hunk_nav.rs
src/tool/lsp.rs
src/lsp/mod.rs
```

Likely `egglsp` files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/edit.rs
crates/egglsp/src/context.rs
crates/egglsp/src/hierarchy.rs
crates/egglsp/src/lib.rs
```

Test infrastructure:

```text
crates/egglsp-test-server/src/main.rs
crates/egglsp/tests/common/production_harness.rs
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/tests/production_semantic_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
```

Possible root integration tests:

```text
tests/lsp_composite_stdio.rs
src/tool/lsp/tests.rs
src/lsp/semantic_context/tests.rs
```

Documentation:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement:

- new LSP operations solely to increase test count;
- automatic server restart;
- real-server CI matrices;
- pull diagnostics;
- incremental sync;
- multi-root workspaces;
- hunk clustering or multi-hunk orchestration;
- direct workspace-edit application;
- broad relocation of all root LSP tooling into `egglsp`;
- transport or shutdown redesign.

Preserve the current preview-only boundary: tests may construct previews, but must never apply server-provided edits to disk.

# Phase 1 — Add Root-Level Composite Test Harness

## Problem

The actual semantic, security, and hunk collectors live above the raw `LspClient` layer. `crates/egglsp/tests` cannot directly prove those root-crate workflows without either duplicating their logic or moving production code.

## Preferred Approach

Add a root-package integration harness that reuses the same fake-server binary and scenario format while constructing the real `LspService` and real collector/tool path.

Recommended test file:

```text
tests/lsp_composite_stdio.rs
```

The harness should:

1. Create a temporary project root and synthetic Rust source.
2. Write a strict fake-server scenario and transcript path.
3. Construct `LspConfig::Rules` with the fake-server command and child-specific environment.
4. Construct the same `LspService` used by `LspTool`.
5. Invoke the actual collector or tool helper under test.
6. Call `shutdown_all()` in bounded teardown.
7. Include transcript tail, client keys, and lifecycle state in failure diagnostics.

Do not reproduce collector sequencing manually in the test.

## Dependency Boundary

If root integration tests cannot access the collectors because of visibility, prefer narrow `pub(crate)` or internal test-module access rather than making implementation details broadly public.

Possible options:

```rust
#[cfg(test)]
pub(crate) async fn collect_semantic_context_for_test(...)
```

or invoke the structured model-facing handler directly:

```rust
LspTool::execute_structured(...)
```

The second option is stronger when stable and not excessively coupled to unrelated model-tool plumbing.

## Acceptance Criteria

- At least one test invokes actual semantic context composition.
- At least one test invokes actual security context composition.
- At least one test invokes actual hunk source navigation/composition.
- No test manually reproduces the collector's request sequence.

# Phase 2 — Exercise the Real Semantic Context Collector

## Scenario

Use a synthetic source file containing:

```rust
pub fn entry() {
    helper();
}

fn helper() {}
```

The fake server should provide deterministic responses for the operations the collector actually requests, such as:

```text
textDocument/documentSymbol
textDocument/hover
textDocument/definition
textDocument/references
textDocument/prepareCallHierarchy
callHierarchy/incomingCalls
callHierarchy/outgoingCalls
textDocument/prepareTypeHierarchy
typeHierarchy/supertypes
typeHierarchy/subtypes
```

Only include operations enabled by the request and advertised capabilities.

Publish diagnostics through a server notification before or during collection.

## Required Assertions

Assert the final production response contains:

- source excerpt from the actual file;
- diagnostic evidence;
- diagnostic freshness/source metadata;
- document symbols;
- hover text;
- definition locations;
- references;
- call hierarchy summary when requested;
- type hierarchy summary when requested;
- deterministic notes for skipped unsupported capabilities;
- truncation/budget metadata when relevant.

Assert the collector uses capability gating:

1. Advertise a capability and confirm the associated request occurs.
2. Omit a capability and confirm the request is not sent and a note/error is recorded according to current policy.

## Failure-Degradation Case

Add one case where an optional operation returns an LSP error while required evidence still succeeds.

Assert:

- the overall semantic context response is returned;
- the optional failure is recorded;
- successful evidence remains present;
- no raw unbounded JSON appears in the final response.

# Phase 3 — Exercise the Real Security Context Path

## Scope

Invoke the production security-context workflow, including its own call-expansion policy rather than only the shared immediate hierarchy summary.

## Scenario

Construct a small call graph:

```text
entry -> validate -> sink
entry -> log
```

The fake server should return:

- diagnostics at `sink`;
- call hierarchy items and outgoing calls;
- one cycle or repeated node to prove visited-node handling;
- one optional request failure to test degradation.

## Required Assertions

Assert:

- shared diagnostic evidence and freshness metadata are present;
- call expansion obeys configured depth;
- node count is bounded;
- repeated/cyclic nodes do not recurse indefinitely;
- ordering is deterministic;
- security-specific notes/filtering remain intact;
- capability-gated operations are skipped when unsupported;
- optional failures do not erase successful evidence.

Do not redesign security-context semantics in this pass. The test should codify existing behavior.

# Phase 4 — Exercise the Real Hunk Source Context Path

## Scope

Invoke the existing `HunkSourceNavigator`, `HunkSourceNavigationCollector`, or the model-facing `hunkSourceContext` path.

## Synthetic Diff

Use a deterministic unified diff that changes a line inside `entry()` and another line near `helper()`.

This phase should preserve current first-anchor behavior. Do not add hunk clustering.

## Fake-Server Responses

Provide:

```text
document symbols
definition
references
hover where currently used
```

## Required Assertions

Assert:

- diff hunk parsing maps to the expected source line;
- the current anchor-selection policy is preserved;
- symbol selection is stable;
- definition/reference evidence is included;
- source excerpts are bounded;
- request count remains within the current policy;
- output ordering is deterministic.

Add one no-match case where the hunk cannot be mapped to a symbol and verify the current fallback/skip behavior.

# Phase 5 — Drive Production Workspace Edit Preview Conversion

## Problem

Current tests stop after deserializing raw `WorkspaceEdit`, `TextEdit`, or code-action results. They do not exercise the preview-only safety boundary.

## Required Production Paths

Use the same functions invoked by model-facing operations:

```text
renamePreview
formatPreview
source action / code-action preview
preview_workspace_edit
preview_text_edits_for_file
```

Prefer testing through the model-facing structured handler where practical. Otherwise call the authoritative `egglsp::edit` conversion functions directly after obtaining responses through the real child process.

The important chain is:

```text
fake server response
    -> production LspClient
    -> typed LSP response
    -> production preview conversion
    -> WorkspaceEditPreview/FileEditPreview/TextEditPreview
```

## Required Success Cases

### Rename preview

Return a `WorkspaceEdit` with:

- edits in the source file;
- edits in a second file inside the root;
- UTF-16 positions involving at least one non-ASCII character.

Assert:

- two file previews are produced;
- original and preview text are correct;
- patch/diff summaries are stable;
- disk files remain unchanged.

### Formatting preview

Return several edits in reverse source order.

Assert:

- reverse-order application produces the correct preview;
- final newline semantics are preserved;
- disk remains unchanged.

### Code-action/source-action preview

Return an edit-bearing `CodeAction` for `source.organizeImports`.

Assert:

- the correct action is selected;
- edit preview is produced;
- command-only behavior remains rejected;
- no command is executed.

## Required Safety-Failure Cases

Drive representative fake-server responses through production conversion:

1. Out-of-root URI -> `PathOutsideRoot`.
2. Overlapping edits -> `OverlappingEdits`.
3. Invalid UTF-16 position -> `Utf16Position`.
4. Unsupported resource operation -> `UnsupportedEdit`.
5. Command-only code action -> `CommandOnlySourceAction`.
6. Ambiguous matching source actions -> `AmbiguousSourceAction`.
7. Code action with no edit -> `NoEditForSourceAction`.

Not every unit-test permutation needs child-process coverage. One representative integration case per safety category is sufficient.

## Filesystem Assertions

For every preview test:

- read all relevant files before the operation;
- execute the preview path;
- read them again;
- assert byte-for-byte equality.

# Phase 6 — Make Response Reordering Genuine

## Problem

The current concurrent test starts calls concurrently but the fake server responds immediately in request arrival order.

## Minimal Scenario-Engine Extension

Add named request capture without expanding into a general scripting language.

Suggested step/action:

```rust
ExpectRequest {
    method: String,
    capture_id_as: Option<String>,
    params: ValueMatcher,
    then: Vec<Action>,
}

SendCapturedResult {
    captured_id: String,
    result: Value,
}

SendCapturedError {
    captured_id: String,
    code: i64,
    message: String,
    data: Option<Value>,
}
```

The fake server stores captured IDs in scenario execution state.

## Corrected Test

1. Launch four production client calls concurrently.
2. Capture all four request IDs without responding.
3. Emit diagnostics/progress/log notifications.
4. Respond in reverse order.
5. Assert each caller receives its own typed result.
6. Assert pending count returns to zero.

Add scenario-engine self-tests for:

- captured ID lookup;
- unknown captured name failure;
- duplicate capture-name failure in strict mode.

## Acceptance Criteria

The transcript must show request arrival order differs from response order.

# Phase 7 — Use Dedicated Typed Hierarchy Adapters

## Audit

Determine whether `LspClient` already exposes dedicated methods for:

```text
prepare call hierarchy
incoming calls
outgoing calls
prepare type hierarchy
supertypes
subtypes
```

Where methods exist, update integration tests to call them.

Where they do not exist but production collectors currently duplicate generic request/deserialization logic, add narrow typed adapters to `LspClient` or a hierarchy module.

Recommended signatures:

```rust
pub async fn prepare_call_hierarchy(
    &self,
    uri: &Url,
    position: Position,
) -> Result<Vec<CallHierarchyItem>, LspError>;

pub async fn incoming_calls(
    &self,
    item: CallHierarchyItem,
) -> Result<Vec<CallHierarchyIncomingCall>, LspError>;
```

Use equivalent methods for outgoing/type hierarchy.

Do not add wrappers that are used only by tests and not by production collectors.

## Acceptance Criteria

- Integration tests no longer manually serialize hierarchy params when a production adapter exists.
- Collector and test use the same typed adapter path.

# Phase 8 — Replace Fixed Sleeps with Condition-Based Waits

## Current Risk

Several tests wait a fixed 100–250 ms for diagnostics or server activity. These may become flaky under loaded CI.

## Required Helpers

Add bounded polling helpers:

```rust
async fn wait_for_diagnostics_len(...)
async fn wait_for_transcript_contains(...)
async fn wait_for_transport_state(...)
async fn wait_for_registration_count(...)
async fn wait_for_client_keys(...)
```

Use 10–25 ms polling intervals and 3–5 second absolute deadlines.

Where a deterministic event can be observed through the transcript, prefer transcript/event waiting over arbitrary sleep.

Timing tests whose subject is elapsed duration may retain bounded timing assertions.

## Acceptance Criteria

- No fixed sleep remains solely to “give the server time.”
- Failure messages include the final observed state and transcript tail.

# Phase 9 — Decide the Inspection API Boundary

## Current Surface

Integration support added public methods such as:

```text
pending_request_count
transport_state_snapshot
dynamic_registration_snapshot
```

The `LspClient` struct also exposes several internal fields publicly.

## Decision

Choose one of two explicit policies.

### Policy A — Supported diagnostics API

Keep snapshot methods public because they are useful for operational monitoring and debugging.

Then:

- document their stability and semantics;
- keep them read-only;
- avoid exposing raw mutable maps;
- consider a consolidated `LspClientHealthSnapshot`.

### Policy B — Test-support surface

Gate integration-only methods behind a feature:

```toml
[features]
lsp-test-support = []
```

and:

```rust
#[cfg(any(test, feature = "lsp-test-support"))]
```

Integration test targets enable the feature.

Mark remaining unavoidable fields `pub(crate)` where possible.

## Preferred Direction

Use Policy A only for genuinely useful runtime health data. Dynamic registration details and raw pending counts are likely better as test-support or `#[doc(hidden)]` until a public observability API is intentionally designed.

Do not perform a broad public API cleanup in this pass; make only low-risk visibility changes needed to clarify intent.

# Phase 10 — Documentation and Coverage Matrix

## Correct Current Overstatement

Until composite and preview integration tests land, documentation should not claim full semantic/security/hunk composite coverage.

After implementation, replace broad count-driven claims with a coverage matrix:

```text
Production protocol
Production service lifecycle
Production semantic typed operations
Production edit preview safety
Root semantic context composite
Root security context composite
Root hunk source context composite
Fixture scenario-engine self-tests
```

## Test Count

Counts may be included as supplemental information, but completion should be defined by invariant coverage.

## Phase Closure

Mark Phase 2 complete only when:

- protocol/runtime migration remains green;
- preview boundary is exercised;
- all three composite paths are exercised;
- genuine out-of-order routing is proven.

# Suggested Implementation Order

1. Add root composite harness and one semantic-context smoke test.
2. Add security-context and hunk-source-context tests.
3. Add production preview conversion success cases.
4. Add representative preview safety-failure cases.
5. Add captured-ID support to the fake server.
6. Correct the out-of-order response test.
7. Replace generic hierarchy requests with typed adapters.
8. Replace fixed sleeps with bounded wait helpers.
9. Decide/gate inspection APIs.
10. Update documentation and run full verification.

# Verification Commands

Run `egglsp` production integration tests:

```bash
cargo test -p egglsp --test production_protocol_stdio -- --test-threads=1
cargo test -p egglsp --test production_protocol_stdio -- --test-threads=8
cargo test -p egglsp --test production_semantic_stdio -- --test-threads=1
cargo test -p egglsp --test production_semantic_stdio -- --test-threads=8
cargo test -p egglsp --test production_service_stdio -- --test-threads=1
cargo test -p egglsp --test production_service_stdio -- --test-threads=8
cargo test -p egglsp --test scenario_engine
```

Run root composite tests:

```bash
cargo test --test lsp_composite_stdio -- --test-threads=1
cargo test --test lsp_composite_stdio -- --test-threads=8
```

Run LSP-focused unit tests:

```bash
cargo test -p egglsp
cargo test lsp::
cargo test tool::lsp::
```

Run clean-checkout binary verification:

```bash
cargo clean
cargo test -p egglsp --test production_protocol_stdio initialization_handshake
cargo test --test lsp_composite_stdio semantic_context
```

Run full validation:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

# Review Checklist

## Composite workflows

- [x] Actual semantic context collector invoked against fake server.
- [x] Actual security context path invoked against fake server.
- [x] Actual hunk source context path invoked against fake server.
- [x] Capability gating is asserted.
- [x] Optional-operation degradation is asserted.
- [x] Budget/truncation behavior remains bounded.

## Preview safety

- [x] Rename response flows through `WorkspaceEditPreview` conversion.
- [x] Formatting response flows through preview conversion.
- [x] Edit-bearing source action flows through preview conversion.
- [x] Multi-file and UTF-16 success case covered.
- [x] Out-of-root edit rejected.
- [x] Overlapping edit rejected.
- [x] Unsupported resource operation rejected.
- [x] Command-only/no-edit/ambiguous source actions rejected.
- [x] Files remain byte-for-byte unchanged.

## Routing

- [x] Fake server captures request IDs.
- [x] Responses are emitted in a different order than requests.
- [x] Each production caller receives the correct result.
- [x] Pending count returns to zero.

## Typed APIs

- [x] Dedicated hierarchy adapters are used where available.
- [x] New adapters are shared by production collectors and tests.
- [x] No test-only typed wrapper is added.

## Reliability

- [x] Fixed synchronization sleeps removed.
- [x] Bounded wait helpers include useful failure diagnostics.
- [x] Tests pass under one and eight test threads.

## API and docs

- [ ] Inspection methods have an explicit supported/test-only policy.
- [x] Public mutable internals are not expanded.
- [x] Documentation distinguishes protocol, service, semantic, preview, and composite coverage.
- [x] Phase 2 completion claims match actual tests.

# Completion Criteria

This pass is complete when:

1. The actual semantic context collector is tested end to end with a real fake-server child.
2. The actual security context workflow is tested end to end.
3. The actual hunk source context workflow is tested end to end.
4. Rename, formatting, and source-action responses pass through production preview conversion.
5. Representative preview safety failures are proven through child-process responses.
6. No preview test mutates disk.
7. Concurrent request routing is proven with genuinely reversed response order.
8. Hierarchy integration uses the same typed adapters as production collectors.
9. Fixed synchronization sleeps are replaced with bounded state waits.
10. Test inspection APIs are intentionally classified.
11. Documentation no longer overstates coverage.
12. Phase 2 can be closed without additional transport or lifecycle work.

## Handoff Result

After this pass, Phase 2 will cover the full chain from child-process LSP traffic through Codegg’s semantic and safety boundaries:

```text
fake LSP server
    -> production LspClient/LspService
    -> typed semantic operations
    -> semantic/security/hunk collectors
    -> workspace-edit preview conversion
    -> bounded, non-mutating model-facing responses
```

The next roadmap step should then move beyond harness validation into operational evaluation, server lifecycle supervision, and broader workflow adoption rather than further Phase 2 hardening.
