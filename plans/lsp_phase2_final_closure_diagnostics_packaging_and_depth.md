# LSP Phase 2 Final Closure: Diagnostics, Packaging, Depth Limits, and Test Portability

## Purpose

Close the final Phase 2 issues after:

```text
b52ed68cf2310f9f00332bbcbaa3c37e4d1cd4dc
```

The production LSP architecture and nearly all planned Phase 2 coverage are complete. The remaining work is limited to five closure items:

1. Correct the security diagnostic-evidence test so it does not consume one strict fake-server scenario with two semantic workflows.
2. Strengthen diagnostic-evidence assertions beyond simple non-null checks.
3. Verify and correct `egglsp` packageability because its gated test binary currently points outside the package directory.
4. Add independent call-depth enforcement coverage rather than conflating it with node-budget truncation.
5. Remove remaining debug noise and make hunk path tests fully portable and semantically exact.

This plan is intentionally narrow. Do not redesign LSP transport, service lifecycle, semantic collection, security filtering, or hunk navigation.

## Target State

At completion:

- The security diagnostic test initializes the service once, waits for diagnostics through a bounded condition, and invokes `securityContext` exactly once.
- The final security packet is asserted for diagnostic content, evidence source, freshness/usability, and filtering behavior.
- `cargo package -p egglsp` succeeds with all enabled target paths contained inside the packaged crate.
- Node-budget truncation and call-depth enforcement are proven by separate strict scenarios.
- No unconditional debug output remains in successful integration tests.
- Hunk traversal and outside-root tests use real sibling files and work on all supported platforms.
- Phase 2 documentation can be marked complete without qualification.

## Scope

Primary files:

```text
crates/egglsp/Cargo.toml
crates/egglsp/src/bin/egglsp-test-server.rs        # likely new thin wrapper
crates/egglsp-test-server/src/main.rs              # shared implementation source today
tests/lsp_composite_stdio.rs
src/lsp/hunk_nav_collector.rs
README.md
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Possible support files if needed:

```text
crates/egglsp-test-server/src/lib.rs
crates/egglsp-test-server-support/Cargo.toml
crates/egglsp-test-server-support/src/lib.rs
```

Prefer the smallest packaging-safe structure.

## Non-Goals

Do not change:

- JSON-RPC framing or routing;
- initialization or shutdown semantics;
- security-context filtering policy;
- call-expansion algorithm behavior;
- hunk multi-file policy;
- workspace-edit preview behavior;
- public tool schemas;
- real-server compatibility work.

# Phase 1 — Correct the Security Diagnostic Test Sequence

## Current Problem

`security_context_tool_filters_and_preserves_diagnostic_evidence` currently:

1. starts a strict scenario containing one semantic request sequence;
2. calls `semanticContext`;
3. ignores that result;
4. calls `securityContext` on the same service.

The first call consumes `documentSymbol`, `definition`, and `references`. The second call then sends another semantic sequence while the fake server expects shutdown, causing strict-scenario mismatch or process exit.

The first call is not a harmless file-open operation and must be removed.

## Required Test Flow

Use this sequence:

1. Create the fake-server-backed `LspService`.
2. Trigger client initialization and document opening without invoking a complete semantic collector twice.
3. Wait until the published diagnostics are visible in the production diagnostics cache.
4. Invoke `LspTool::execute` with `operation = "securityContext"` exactly once.
5. Assert the final packet.
6. Shut down the service.

## Preferred Initialization Method

Use the service directly:

```rust
service.open_file(source_path.to_str().unwrap(), SECURITY_CALL_GRAPH_SOURCE).await?;
```

or the exact current service API equivalent.

If `open_file` both initializes and sends `didOpen`, that is preferred.

Do not use `semanticContext` as a setup operation.

## Bounded Diagnostic Wait

Add or reuse a helper:

```rust
async fn wait_for_diagnostics(
    service: &LspService,
    source_path: &Path,
    timeout_duration: Duration,
) -> Result<LspDiagnosticSnapshot, String>
```

Behavior:

- poll every 10–25 ms;
- use a 3–5 second absolute deadline;
- query the authoritative service/client diagnostic snapshot;
- return only when at least one diagnostic is present;
- on timeout, include client keys and transcript tail.

Do not use a fixed sleep.

## Scenario Ordering

The scenario may continue to emit `publishDiagnostics` as an action on `initialized`, provided the production reader receives it before the bounded wait completes.

After setup, the strict scenario should expect exactly one semantic/security request sequence followed by shutdown.

## Acceptance Criteria

- `securityContext` is invoked exactly once.
- The strict scenario is consumed exactly once.
- The test fails clearly if diagnostics never reach the cache.
- No ignored setup result remains.

# Phase 2 — Strengthen Diagnostic Evidence Assertions

## Current Problem

The test only verifies:

```rust
diagnostic_evidence != null
```

This does not prove that the scripted security diagnostic is represented correctly or that evidence metadata remains meaningful.

## Required Assertions

Inspect the actual serialized `SecurityContextPacket` schema and assert the fields it currently exposes.

At minimum assert:

### Security-relevant diagnostics

- array is non-empty;
- one item contains either code `COMMAND_INJECTION` or the scripted message;
- source is `security-lint` if source is serialized;
- severity matches the scripted error severity if severity is serialized.

### Non-security diagnostic behavior

Explicitly encode current policy:

- if style-only diagnostics are filtered, assert `STYLE_ONLY` and its message are absent;
- if current policy retains them, assert that documented behavior instead.

Do not change production filtering policy in this pass.

### Diagnostic evidence

Assert the actual evidence structure, for example where available:

```text
source == pushed
freshness == fresh or possibly_stale according to current lifecycle
usable == true
count >= 1
file/uri matches source file
```

If the schema contains nested diagnostic records, assert that the security diagnostic appears there.

If field names differ, use the production DTO names rather than introducing test-only schema.

### Other packet evidence

Retain assertions that:

- risk markers are non-empty;
- preset is `unsafe_review`;
- notes and limits are present.

## Failure Diagnostics

On assertion failure, print only a bounded pretty-printed section of:

```text
security_relevant_diagnostics
diagnostic_evidence
transcript tail
```

Do not unconditionally print during passing tests.

## Acceptance Criteria

- The test proves the security diagnostic survives filtering.
- The expected treatment of the style-only diagnostic is explicit.
- Evidence source/freshness/usability is asserted where exposed.
- The test would fail if `diagnostic_evidence` were an unrelated empty object.

# Phase 3 — Make `egglsp` Packageable

## Current Problem

`crates/egglsp/Cargo.toml` declares:

```toml
[[bin]]
name = "egglsp-test-server"
path = "../egglsp-test-server/src/main.rs"
required-features = ["lsp-test-support"]
```

The source is outside the `egglsp` package directory. Workspace builds can resolve it, but a packaged `egglsp` crate archive generally cannot contain or reference files above its package root.

Feature gating avoids normal compilation but does not guarantee `cargo package -p egglsp` succeeds or produces a valid archive.

## First Verification Step

Run before changing structure:

```bash
cargo package -p egglsp --allow-dirty
```

Record the exact result.

If packaging fails, or if the generated package manifest references a missing source path, apply the corrective structure below.

## Preferred Corrective Structure

Create a package-local binary wrapper:

```text
crates/egglsp/src/bin/egglsp-test-server.rs
```

The wrapper must not duplicate the full scenario engine.

Preferred implementation options, in order:

### Option A — Shared support library crate

Create a small unpublished workspace crate:

```text
crates/egglsp-test-server-support/
```

with:

```rust
pub fn run_or_exit() { ... }
```

Both root and `egglsp` package-local binaries depend on this support crate and call one function.

Mark it:

```toml
publish = false
```

This is the cleanest option if root and `egglsp` both need package-local `CARGO_BIN_EXE_*` artifacts.

### Option B — Package-local copy of a thin wrapper plus shared module

If the existing fake-server code can be exposed as a library module without creating another crate, create:

```text
crates/egglsp-test-server/src/lib.rs
```

and move the implementation there.

Each package-local `main.rs` should call the library entry point.

Be careful: a package cannot depend on an unpackaged sibling source by relative module inclusion in its published archive. Use a real path dependency marked `publish = false` only if `egglsp` itself is not intended to publish. If `egglsp` may be published, prefer Option A with an explicit publish policy or place a minimal self-contained wrapper implementation inside `egglsp`.

## `egglsp` Manifest

Update the binary target to a contained path:

```toml
[[bin]]
name = "egglsp-test-server"
path = "src/bin/egglsp-test-server.rs"
required-features = ["lsp-test-support"]
```

No target in `crates/egglsp/Cargo.toml` may point above the package root.

## Root Binary

Retain the root package-local binary for `CARGO_BIN_EXE_codegg-lsp-test-server`, but make it a thin wrapper over the same shared implementation.

Do not duplicate the scenario engine.

## Required Packaging Checks

Run:

```bash
cargo package -p egglsp --allow-dirty
cargo package --allow-dirty --no-verify
```

Then inspect the packaged `egglsp` manifest/archive sufficiently to confirm:

- all target paths exist inside the archive;
- the gated binary target is valid when enabled;
- ordinary package verification does not require the test feature.

Also run:

```bash
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio initialization_handshake
```

## Acceptance Criteria

- `cargo package -p egglsp` succeeds.
- No `egglsp` target path escapes `crates/egglsp`.
- Package-local `CARGO_BIN_EXE_egglsp-test-server` remains available.
- Root package-local artifact remains available.
- Fake-server implementation remains single-source.

# Phase 4 — Add Independent Call-Depth Enforcement Coverage

## Current Problem

The node-limit test uses:

```text
max_call_nodes = 2
call_depth = 2
```

The node cap stops traversal before depth becomes relevant. Therefore the test proves node-budget truncation but does not independently prove `call_depth` enforcement.

## Keep Existing Node-Limit Test

Retain and rename/document it narrowly if necessary:

```text
security_context_tool_enforces_call_node_limit_and_truncation
```

It should continue to assert:

- node count obeys `max_call_nodes`;
- root is retained;
- expansion and packet truncation flags are true.

Remove wording that claims it independently proves depth limiting.

## New Depth-Limit Scenario

Create a strict chain wider in depth than the requested limit:

```text
entry -> level1 -> level2 -> level3
```

Use:

```text
call_depth = 2
max_call_nodes = 16
```

The generous node budget ensures only depth can stop traversal.

## Scenario Requirements

The fake server should expect expansion requests for:

```text
entry
level1
```

and, depending on current depth semantics, possibly collect `level2` as a terminal node without expanding it.

It must not accept a request that expands `level2` into `level3` when that would exceed depth 2.

Use strict mode so an unexpected depth-3 expansion fails the test.

## Required Test

Add:

```rust
#[tokio::test]
async fn security_context_tool_enforces_call_depth_limit()
```

Invoke `securityContext` with:

```json
{
  "call_depth": 2,
  "max_call_nodes": 16,
  "call_direction": "outgoing"
}
```

Assert:

- every serialized node depth is `<= 2` if depth is serialized;
- no `level3` node appears if current semantics exclude it;
- strict transcript contains no expansion request beyond allowed depth;
- truncation flag matches current production behavior for depth exhaustion.

Do not assume depth exhaustion sets the same truncation flag as node-budget exhaustion unless production code does so. Assert current semantics.

## Acceptance Criteria

- The new test fails if `call_depth` is ignored while node budget remains generous.
- Node-limit and depth-limit invariants are documented as separate tests.

# Phase 5 — Remove Debug Noise

## Current Problem

`security_context_tool_enforces_call_node_limit_and_truncation` unconditionally prints the call-expansion JSON with `eprintln!` during successful runs.

## Required Change

Remove the unconditional print.

If diagnostics are useful, add a helper:

```rust
fn expansion_debug(parsed: &serde_json::Value) -> String
```

Use it only inside assertion messages:

```rust
assert!(condition, "...\n{}", expansion_debug(&parsed));
```

Bound output length if the structure can grow.

Search the composite suite for other unconditional:

```text
eprintln!
println!
dbg!
```

Remove or gate all successful-run debug output.

## Acceptance Criteria

- Passing tests produce no unsolicited debug output.
- Failing assertions still include actionable bounded diagnostics.

# Phase 6 — Make Hunk Path Tests Portable and Exact

## Current Problems

Two tests remain weaker than intended:

1. The traversal test uses a nonexistent sibling path, so failure may come from canonicalization rather than containment.
2. The collector-level outside-root test hard-codes `/etc/passwd`, which is not portable to Windows.

## Traversal Test Correction

Create a real sibling path:

```text
TempDir/
  project/src/lib.rs
  project-other/file.rs
```

Then call:

```rust
normalize_request_relative_path(
    Path::new("../project-other/file.rs"),
    &root,
)
```

Assert the error specifically contains `outside allowed root` or equivalent containment wording.

This proves traversal resolves to an existing path and is rejected by containment, not merely by missing-file canonicalization.

## Collector-Level Outside-Root Test

Replace `/etc/passwd` with a real file under the same `TempDir` but outside `root`:

```text
TempDir/project/src/lib.rs
TempDir/outside.rs
```

Build the request using `outside.rs.to_string_lossy()`.

Assert:

- collector returns `hunkSourceContext: invalid file path` or the current exact prefix;
- error identifies outside-root containment failure;
- no platform-specific path assumption exists.

## Additional Exactness

Where practical, assert exact normalized results and error categories rather than broad `is_err()` checks.

Do not change production normalization logic unless these stronger tests expose a real defect.

## Acceptance Criteria

- Hunk path tests pass on Unix and Windows-supported builds.
- Traversal rejection is proven with an existing target.
- Outside-root collector rejection uses only temporary test paths.

# Phase 7 — Documentation Corrections

## Security Coverage

Document four separate invariants:

```text
risk filtering and cycle suppression
call hierarchy error degradation
node-budget truncation
depth-limit enforcement
security diagnostic filtering and evidence
```

Do not describe the node-budget test as independent depth proof.

## Diagnostic Test

Document that diagnostics are:

- published by the fake server;
- observed through the production cache;
- consumed by one `securityContext` invocation;
- asserted for filtering and evidence metadata.

## Packaging

Document the package-local wrapper/support architecture and confirm:

```text
cargo package -p egglsp
```

passes.

## Hunk Tests

Document that containment tests use real temporary sibling files and are platform-neutral.

## Phase Closure

Only mark Phase 2 complete after all focused verification commands pass.

# Suggested Implementation Order

A smaller model should execute this order exactly:

1. Fix the diagnostic test to remove the setup `semanticContext` call.
2. Add bounded diagnostic-cache waiting through the service/client API.
3. Strengthen diagnostic filtering and evidence assertions.
4. Remove unconditional debug output from composite tests.
5. Add the independent depth-chain scenario and test.
6. Run `cargo package -p egglsp --allow-dirty` and capture the exact failure/success.
7. If required, move the `egglsp` binary target to a package-local wrapper and extract one shared implementation source.
8. Re-run package and package-local integration tests.
9. Replace the two weak/nonportable hunk tests with real sibling files.
10. Update documentation and coverage matrix.
11. Run clean, single-thread, multi-thread, package, and workspace verification.

# Verification Commands

## Focused security tests

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio \
  security_context_tool_filters_and_preserves_diagnostic_evidence -- --nocapture

cargo test --features lsp-test-support --test lsp_composite_stdio \
  security_context_tool_enforces_call_node_limit_and_truncation

cargo test --features lsp-test-support --test lsp_composite_stdio \
  security_context_tool_enforces_call_depth_limit
```

Run the complete composite suite:

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1
cargo test --features lsp-test-support --test lsp_composite_stdio -- --test-threads=8
```

## Packaging

```bash
cargo package -p egglsp --allow-dirty
cargo package --allow-dirty --no-verify
```

## `egglsp` fixture verification

```bash
cargo clean
cargo test -p egglsp --features lsp-test-support \
  --test production_protocol_stdio initialization_handshake
```

## Hunk tests

```bash
cargo test lsp::hunk_nav_collector::tests
```

## Full validation

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo check --workspace --all-targets --all-features
cargo test --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If unrelated optional features fail, record exact diagnostics and run all changed LSP targets explicitly with `lsp-test-support`.

# Review Checklist

## Diagnostic sequence

- [ ] No setup `semanticContext` call remains.
- [ ] File initialization/opening uses the service/client API directly.
- [ ] Diagnostics are awaited with a bounded condition.
- [ ] `securityContext` is invoked exactly once.
- [ ] Strict scenario completes without mismatch.

## Diagnostic evidence

- [ ] Security diagnostic code/message is asserted.
- [ ] Style-only diagnostic behavior is asserted explicitly.
- [ ] Evidence source/freshness/usability/count is asserted where serialized.
- [ ] Non-null-only assertion is removed.

## Packaging

- [ ] `cargo package -p egglsp` succeeds.
- [ ] No `egglsp` target path points outside the package directory.
- [ ] Fake-server implementation remains single-source.
- [ ] Package-local `CARGO_BIN_EXE_*` discovery remains intact.

## Limits

- [ ] Node-budget truncation test remains green.
- [ ] Separate depth-chain test exists.
- [ ] Depth test uses a generous node budget.
- [ ] Strict scenario fails if traversal exceeds depth.
- [ ] Documentation separates node and depth invariants.

## Test quality

- [ ] No unconditional debug prints remain.
- [ ] Traversal test uses an existing sibling file.
- [ ] Outside-root collector test uses a temporary sibling file, not `/etc/passwd`.
- [ ] Tests are platform-neutral.

## Documentation

- [ ] Diagnostic flow is described accurately.
- [ ] `egglsp` packageability is documented and verified.
- [ ] Security limit coverage is not overstated.
- [ ] Phase 2 completion statement matches actual evidence.

# Completion Criteria

Phase 2 can be closed without qualification when:

1. The diagnostic integration test uses one strict scenario and one `securityContext` invocation.
2. Diagnostic filtering and evidence metadata are asserted meaningfully.
3. `cargo package -p egglsp` succeeds with contained target paths.
4. Call-depth enforcement has independent strict-scenario evidence.
5. Node-budget truncation remains covered separately.
6. Passing tests emit no debug noise.
7. Hunk containment tests use real, platform-neutral temporary paths.
8. Single-thread, multi-thread, clean-checkout, package, and workspace validation pass for changed targets.
9. Documentation accurately marks Phase 2 complete.

## Handoff Result

After this pass, Phase 2 will have no remaining qualification around security diagnostic sequencing, evidence quality, packageability, depth enforcement, or path-test portability. The next roadmap work should begin Phase 3 real-server compatibility and operational lifecycle evaluation rather than further scripted-harness correction.
