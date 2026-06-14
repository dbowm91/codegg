# LSP Phase 2 Final Closure: Security Orchestration and Hermetic Root Tests

## Purpose

Close the final Phase 2 gaps after:

```text
3a178868643351d0ce6bfd09a06e97afd296de1e
9ca35b82444374d946be0d2132cd759bab7660c7
abe83f9aa2425a55799295a6114fc57db49a2426
```

The current implementation now has strong production-path evidence for:

- real `LspClient` and `LspService` child-process integration;
- initialization, bidirectional JSON-RPC, diagnostics, timeout cancellation, malformed framing, EOF, and shutdown;
- genuinely out-of-order response routing through captured request IDs;
- typed call/type hierarchy adapters;
- `SemanticContextCollector` capability gating and failure degradation;
- `HunkSourceNavigationCollector` execution against a scripted server;
- rename, formatting, and source-action preview conversion;
- major preview safety boundaries and no-mutation guarantees.

Two blockers remain before Phase 2 should be marked closed:

1. The test named as security-context coverage calls `SemanticContextCollector` with `SecurityReview` intent, but does not invoke the real `LspTool` `securityContext` orchestration. It therefore does not cover risk-marker scanning, security filtering, presets, recursive call expansion, call-node limits, cycle suppression, or the final `SecurityContextPacket`.
2. Root-package composite tests find the fake-server binary by searching `target/debug` when `CARGO_BIN_EXE_*` is unavailable. A clean `cargo test --test lsp_composite_stdio` can fail or select a stale artifact.

This pass should fix those blockers and complete the small portability, diagnostics, API-boundary, and documentation cleanup identified during review. Do not reopen transport, initialization, or shutdown architecture.

## Target State

At completion:

1. A root integration test invokes `LspTool::execute` with `operation: "securityContext"` against the real fake-server-backed `LspService`.
2. The test validates the final serialized security packet, including security-specific fields and call expansion.
3. Root composite tests receive a Cargo-built package-local fake-server artifact on a clean checkout without target-directory scanning or nested Cargo invocation.
4. Hunk path comparison uses path components rather than string-prefix stripping.
5. Preview coverage documentation distinguishes child-process cases from local conversion-function cases.
6. Debug output is emitted only on failure.
7. Inspection APIs have an explicit supported or test-support classification.
8. Phase 2 closure claims are based on invariant coverage, not only test counts.

## Scope

Primary implementation files:

```text
src/tool/lsp.rs
src/lsp/hunk_nav_collector.rs
src/lsp/semantic_context.rs
crates/egglsp/src/client.rs
```

Test infrastructure:

```text
Cargo.toml
crates/egglsp/Cargo.toml
crates/egglsp-test-server/src/main.rs
tests/lsp_composite_stdio.rs
crates/egglsp/tests/common/production_harness.rs
```

Documentation:

```text
README.md
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
plans/lsp_phase2_semantic_completion_and_test_polish.md
```

## Non-Goals

Do not implement:

- additional LSP protocol methods;
- automatic server restart;
- real-server CI matrices;
- hunk clustering;
- direct edit application;
- multi-root workspaces;
- a broad tool framework refactor;
- a new generic security-analysis engine;
- transport or lifecycle redesign;
- large-scale public API stabilization for all of `egglsp`.

# Phase 1 — Make the Fake Server Hermetic for Root Integration Tests

## Current Problem

`tests/lsp_composite_stdio.rs` currently resolves the server in this order:

```text
EGGLSP_TEST_SERVER
CARGO_BIN_EXE_egglsp-test-server
target/debug/egglsp-test-server
```

The root package does not own the `egglsp-test-server` binary target, so Cargo does not reliably define the package-local `CARGO_BIN_EXE_*` variable for the root integration test. The target-directory fallback is not hermetic.

## Preferred Packaging Solution

Keep one fake-server implementation source, but expose package-local binary targets for both packages that need integration-test artifact discovery.

Recommended arrangement:

```text
crates/egglsp-test-server/src/server.rs      # reusable implementation
crates/egglsp-test-server/src/main.rs        # optional thin shared entry
crates/egglsp/src/bin/egglsp-test-server.rs  # egglsp package wrapper
src/bin/codegg-lsp-test-server.rs            # root package wrapper
```

Both wrappers should contain only:

```rust
fn main() {
    egglsp_test_server_support::run_or_exit();
}
```

If introducing a small support crate is excessive, both package-local `[[bin]]` targets may point at the same existing source file, provided:

- the binary names are distinct;
- there is one implementation source;
- no code is duplicated;
- Cargo builds each package-local artifact deterministically.

Suggested names:

```text
egglsp-test-server       # owned by egglsp package tests
codegg-lsp-test-server   # owned by root package tests
```

The root composite harness should use:

```rust
env!("CARGO_BIN_EXE_codegg-lsp-test-server")
```

or `option_env!` only when an explicit manual override remains supported.

## Required Removal

Delete root-test fallback logic that searches:

```text
target/debug
target/release
```

Do not launch `cargo build` from test code.

An explicit override may remain:

```text
CODEGG_LSP_TEST_SERVER=/absolute/path
```

Use a root-specific name to avoid ambiguity with the `egglsp` package harness.

## Build-Graph Requirements

Ensure:

```bash
cargo clean
cargo test --test lsp_composite_stdio composite_harness_initialization_smoke
```

builds the root package's test-server target automatically.

Also preserve:

```bash
cargo clean
cargo test -p egglsp --test production_protocol_stdio initialization_handshake
```

## Acceptance Criteria

- No test scans the target directory.
- No test starts a nested Cargo process.
- Root and `egglsp` integration tests each receive a package-local Cargo artifact.
- The fake-server scenario engine has one implementation source.
- Clean-checkout commands work independently and in either order.

# Phase 2 — Add a Testable Security-Context Orchestration Entry Point

## Current Problem

The real security workflow is implemented inside `LspTool::execute` and includes logic beyond `SemanticContextCollector`:

```text
security preset/default resolution
risk-marker scanning
category filtering
security-relevant symbol filtering
security-relevant diagnostic filtering
diagnostic evidence adaptation
call hierarchy summary
recursive call expansion BFS
node/depth/direction limits
cycle suppression
security notes
final SecurityContextPacket serialization
```

The current integration test only calls the shared semantic collector with `SecurityReview` intent.

## Preferred Test Path

Test the production tool entry point directly:

```rust
use codegg::tool::{Tool, lsp::LspTool};

let tool = LspTool::new(service.clone()).with_allowed_root(root.clone());
let json = tool.execute(serde_json::json!({
    "operation": "securityContext",
    "file_path": source_path,
    "line": 12,
    "column": 13,
    "security_preset": "unsafe_review",
    "security_categories": ["unsafe", "process"],
    "include_call_hierarchy": true,
    "call_depth": 2,
    "max_call_nodes": 4,
    "call_direction": "outgoing"
})).await?;
```

Deserialize the returned JSON into a test-local mirror type or assert through structured `serde_json::Value` paths.

## Optional Refactor

If testing through the string-returning `Tool::execute` path is too opaque, extract the existing branch into one production helper:

```rust
impl LspTool {
    async fn execute_security_context(
        &self,
        parsed: &LspInput,
    ) -> Result<SecurityContextPacket, ToolError>;
}
```

Keep the helper private or `pub(crate)`. The `Tool::execute` branch should serialize the returned packet. Tests should still include at least one call through `Tool::execute` to prove model-facing dispatch and serialization.

Do not duplicate orchestration in the test.

## Acceptance Criteria

- The test invokes the `securityContext` operation string through the real tool.
- The final `SecurityContextPacket` is asserted, not merely the shared semantic response.
- No test-only implementation of risk scanning or call expansion is introduced.

# Phase 3 — Build a Deterministic Security Call Graph Scenario

## Synthetic Source

Use source that contains both risk markers and a small call graph:

```rust
use std::process::Command;

pub fn entry(input: &str) {
    validate(input);
    sink(input);
}

fn validate(input: &str) {
    if input.is_empty() { return; }
}

fn sink(input: &str) {
    unsafe {
        let _ = Command::new("sh").arg("-c").arg(input).output();
    }
    entry(input); // deliberate cycle
}
```

## Fake-Server Capabilities

Advertise:

```text
documentSymbolProvider
definitionProvider
referencesProvider
callHierarchyProvider
```

Publish at least one security-relevant diagnostic for `sink` if the production filter recognizes its source/message/severity.

## Call Hierarchy Sequence

The server should support the actual security path's expected calls:

```text
textDocument/documentSymbol
textDocument/definition
textDocument/references
textDocument/prepareCallHierarchy
callHierarchy/incomingCalls and/or outgoingCalls
```

The call graph should encode:

```text
entry -> validate
entry -> sink
sink -> entry
```

Use stable item URIs/ranges so the production call-expansion node identity function sees repeated `entry` as the same node.

## Assertions on Final Packet

Assert the serialized result contains:

```text
file
target
excerpt
risk_markers
security_relevant_symbols
security_relevant_diagnostics
diagnostic_evidence
definitions
references
call_hierarchy
call_expansion
preset
notes
limits
```

Required semantic assertions:

- `risk_markers` contains at least `unsafe` and process/command execution evidence.
- `security_relevant_symbols` contains `sink` and/or `entry` according to current filtering policy.
- `security_relevant_diagnostics` contains the scripted security diagnostic when applicable.
- `preset` equals the requested preset name.
- preset notes appear in `notes` when current behavior adds them.
- `call_expansion.root` is `entry` or the target symbol selected by the current production path.
- expansion contains `validate` and `sink`.
- the `sink -> entry` cycle does not create a duplicate unbounded subtree.
- all node depths are `<= call_depth`.
- total nodes are `<= max_call_nodes`.
- edge count remains within the production cap.
- `direction` matches the requested direction.
- `limits.call_expansion_truncated` matches whether the chosen node cap was reached.

## Separate Limit Case

Add a second focused test with:

```text
call_depth = 2
max_call_nodes = 2
```

Assert:

- packet returns successfully;
- node count is exactly or at most 2 according to current root counting;
- `call_expansion.truncated` and `limits.call_expansion_truncated` are true;
- the service remains healthy afterward.

## Optional Failure-Degradation Case

Have one outgoing-call request return an LSP error.

Assert:

- the packet is still returned;
- the expansion `errors` list records the failure;
- already collected nodes/evidence remain present.

This case is valuable but not required to close Phase 2 if equivalent unit coverage already exists.

# Phase 4 — Correct the Existing Security Test and Documentation

## Existing Test

Rename:

```text
security_context_workflow_uses_semantic_collector
```

to accurately state what it covers, for example:

```text
semantic_context_security_review_intent_collects_security_source
```

Keep it only if its distinct `SecurityReview`-intent coverage is useful.

Add the new actual tool test under a name such as:

```text
security_context_tool_exercises_risk_filtering_and_call_expansion
```

## Documentation Rule

Only the new `LspTool::execute("securityContext")` test may be cited as security-context orchestration coverage.

The semantic-intent test should be listed separately as shared collector coverage.

# Phase 5 — Replace String-Based Hunk Path Comparison

## Current Problem

`HunkSourceNavigationCollector` currently:

1. converts the allowed root and request path to strings;
2. strips the root with `str::strip_prefix`;
3. compares the result with slash-normalized diff paths.

This is vulnerable to prefix ambiguity and is not portable across path separators.

## Required Helpers

Introduce path-aware helpers:

```rust
fn normalize_request_relative_path(
    request_file: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, String>;

fn normalize_diff_relative_path(raw: &str) -> Result<PathBuf, String>;
```

### Request path

- If relative, join it to `allowed_root`.
- Use `Path::strip_prefix(allowed_root)` on normalized/canonical paths.
- Do not use textual prefix removal.
- Reject a file outside the allowed root.

Because the requested source file exists during collection, `std::fs::canonicalize` is acceptable. Canonicalize both root and file when possible.

If canonicalization fails, use a lexical component normalizer that:

- removes `.`;
- resolves `..` without permitting escape above the root;
- preserves normal path components;
- rejects platform prefix/root components in diff-relative paths.

### Diff path

- Strip only exact leading `a/` or `b/` path components.
- Convert `/` separators to `PathBuf` components.
- reject `..`, absolute paths, drive prefixes, and empty terminal paths;
- compare `Path` components rather than strings.

## Required Tests

Add unit tests for:

- absolute request path under root matches `src/lib.rs`;
- relative request path matches `src/lib.rs`;
- `/tmp/project-other/file.rs` does not match root `/tmp/project`;
- `a/src/lib.rs` and `b/src/lib.rs` normalize correctly;
- `a/../outside.rs` is rejected;
- multi-file diff still rejects a second file;
- separator normalization behaves correctly on Windows-compatible input.

Keep the existing real hunk collector integration test green.

# Phase 6 — Make Preview Coverage Labels Precise

## Classification

Classify preview tests into:

### Child-process production-chain tests

```text
rename success
format success
source-action success
out-of-root rename rejection
overlapping rename rejection
```

These follow:

```text
fake server -> LspClient -> typed response -> preview conversion
```

### Local production-function tests

```text
command-only source action
no-edit source action
ambiguous source action
unsupported resource operation
```

These directly exercise production selection/conversion functions with locally constructed typed values.

## Optional Upgrade

Converting the local cases into fake-server scenarios is optional. Do it only if straightforward; it is not a Phase 2 blocker.

## Documentation

Do not describe every preview-safety test as child-process integration. State both categories explicitly.

# Phase 7 — Remove Debug Noise and Tighten Composite Tests

## Debug Output

Remove unconditional `eprintln!` calls that dump hierarchy and transcript information during successful runs.

Provide a helper:

```rust
fn assert_with_transcript(condition: bool, message: &str, transcript: &Path)
```

or include transcript tails only in panic/error branches.

## Low-Signal Smoke Tests

Review:

```text
composite_service_layer_construction
composite_semantic_context_collector_construction
semantic_context_minimal_service_client
```

Keep them if they prove distinct construction boundaries, but do not include them in completion counts for semantic invariants.

Prefer a coverage matrix with named invariants over statements such as “19 composite tests.”

## Failure Diagnostics

Ensure actual security and hunk tests include:

```text
scenario name
transcript tail
service client keys
transport/lifecycle state where accessible
```

on failure.

# Phase 8 — Finalize the Inspection API Policy

## Current Methods

```text
pending_request_count
transport_state_snapshot
dynamic_registration_snapshot
```

## Recommended Policy

### Supported runtime health API

Keep and document:

```rust
transport_state_snapshot()
pending_request_count()
```

These are useful for operational health and do not expose mutation.

Optionally consolidate later into:

```rust
LspClientHealthSnapshot {
    transport,
    pending_requests,
}
```

Do not require that consolidation for Phase 2 closure.

### Test/internal protocol-state API

Treat detailed dynamic registration state as test-support/internal unless a public consumer currently needs it.

Preferred options:

```rust
#[doc(hidden)]
pub async fn dynamic_registration_snapshot(...)
```

or feature-gate it:

```toml
[features]
lsp-test-support = []
```

```rust
#[cfg(any(test, feature = "lsp-test-support"))]
```

Because root integration tests are a separate package target, enable the feature explicitly if feature-gating is chosen.

## Documentation

Add rustdoc explaining snapshot consistency and that pending count is observational, not a synchronization primitive.

# Phase 9 — CI and Clean-Checkout Proof

## Required Commands

Run independently from a clean target directory:

```bash
cargo clean
cargo test -p egglsp --test production_protocol_stdio initialization_handshake
```

Then separately:

```bash
cargo clean
cargo test --test lsp_composite_stdio security_context_tool_exercises_risk_filtering_and_call_expansion
```

Then:

```bash
cargo clean
cargo test --test lsp_composite_stdio hunk_source_context_collector_exercises_real_workflow
```

These commands must not require a prior manual binary build.

## Scheduler Coverage

Run root composite tests with:

```bash
cargo test --test lsp_composite_stdio -- --test-threads=1
cargo test --test lsp_composite_stdio -- --test-threads=8
```

Run `egglsp` production suites as before.

## Full Validation

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

If workspace-wide failures are unrelated and pre-existing, record exact diagnostics. All changed LSP and root test targets must be clean.

# Phase 10 — Final Documentation and Phase Closure

## Coverage Matrix

Document these categories separately:

```text
fixture scenario-engine self-tests
production LspClient protocol tests
production LspService lifecycle tests
production typed semantic tests
child-process preview conversion tests
local preview safety-function tests
SemanticContextCollector integration
securityContext tool orchestration integration
HunkSourceNavigationCollector integration
```

## Correct Claims

Update all references that currently equate `SecurityReview` semantic intent with full `securityContext` coverage.

Document:

- actual security tool test name;
- call-depth/node-limit/cycle coverage;
- package-local fake-server binary artifact strategy;
- path-aware hunk normalization;
- inspection API classification.

## Plan Checkboxes

Do not mark the final Phase 2 plan complete until the clean-checkout tests and actual security tool test pass.

# Suggested Implementation Order

1. Add the root package-local fake-server binary target and remove target scanning.
2. Prove clean-checkout root test startup.
3. Add a real `LspTool::execute("securityContext")` test with risk markers and call expansion.
4. Add the call-node truncation/cycle assertion case.
5. Rename the existing semantic-intent security test accurately.
6. Replace hunk string-prefix logic with path-aware normalization and tests.
7. Remove unconditional debug output.
8. Classify preview tests and inspection APIs accurately.
9. Update documentation and coverage matrix.
10. Run clean, single-thread, multi-thread, and workspace verification.

# Review Checklist

## Hermeticity

- [ ] Root tests use a package-local `CARGO_BIN_EXE_*` artifact.
- [ ] `egglsp` tests retain package-local artifact discovery.
- [ ] No target-directory scanning remains.
- [ ] No nested Cargo process runs inside tests.
- [ ] Fake-server implementation code is not duplicated.

## Actual security context

- [ ] Test calls `LspTool::execute` with `operation = securityContext`.
- [ ] Final `SecurityContextPacket` is asserted.
- [ ] Risk markers are asserted.
- [ ] Security-relevant symbols/diagnostics are asserted.
- [ ] Preset and notes are asserted.
- [ ] Call hierarchy is asserted.
- [ ] Recursive call expansion is asserted.
- [ ] Depth and node limits are asserted.
- [ ] Cycle suppression is asserted.
- [ ] Truncation state is asserted.
- [ ] Existing semantic-intent test is renamed accurately.

## Hunk portability

- [ ] Root containment uses `Path::strip_prefix`, not string prefixes.
- [ ] Diff paths reject traversal and absolute components.
- [ ] Prefix-collision test exists.
- [ ] Multi-file rejection remains correct.
- [ ] Root hunk integration test remains green.

## Test quality

- [ ] Debug output appears only on failure.
- [ ] Child-process and local preview tests are labeled separately.
- [ ] Completion is described by invariants rather than raw counts.

## API surface

- [ ] Transport/pending snapshots are documented as observational health APIs or gated.
- [ ] Dynamic registration snapshot is explicitly internal/test-support or intentionally public.
- [ ] No new mutable internal state is exposed.

## Documentation

- [ ] Full security orchestration is no longer conflated with semantic collector intent.
- [ ] Root fake-server artifact strategy is documented accurately.
- [ ] Phase 2 closure claims match clean-checkout test evidence.

# Completion Criteria

Phase 2 is complete when:

1. Root composite tests launch the fake server hermetically from a clean checkout.
2. The real `securityContext` tool operation is exercised end to end.
3. The final security packet proves risk scanning, filtering, preset behavior, and call expansion.
4. Call expansion depth, node limits, cycle suppression, and truncation are tested.
5. The actual semantic and hunk collectors remain covered.
6. Hunk path matching is path-aware and traversal-safe.
7. Preview test classifications are accurate.
8. Debug-only output is removed from successful test runs.
9. Inspection APIs have an explicit policy.
10. Single-thread, multi-thread, clean-checkout, and workspace verification pass for changed targets.
11. Documentation accurately marks Phase 2 closed.

## Handoff Result

After this pass, Phase 2 will have complete evidence across the production chain and the root-level semantic/security boundary:

```text
Cargo-built fake server
    -> production LspClient/LspService
    -> SemanticContextCollector / HunkSourceNavigationCollector
    -> actual LspTool securityContext orchestration
    -> risk filtering and bounded call expansion
    -> preview-only, non-mutating outputs
```

The next roadmap phase should move to operational lifecycle supervision, real-server compatibility evaluation, and broader workflow adoption rather than further scripted-harness hardening.
