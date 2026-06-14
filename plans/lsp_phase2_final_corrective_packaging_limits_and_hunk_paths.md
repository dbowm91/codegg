# LSP Phase 2 Final Corrective Pass: Packaging, Security Limits, Hunk Paths, and Health API

## Purpose

Apply the final narrow corrections after:

```text
b41e02932cb80b7c98442ff1d7640da9fbd32184
3b2218bdcce8891219c0bbe936f0380275948fc5
d30ec29a980fb97e1db58177c5ccd1cf15dc0474
```

The main Phase 2 architecture is complete. Production `LspClient`, `LspService`, semantic collectors, hunk navigation, workspace-edit preview conversion, and the actual `LspTool::execute("securityContext")` path are exercised against the scripted child process.

This corrective pass addresses six remaining issues:

1. The root fake-server fixture is currently an ordinary ungated shipping binary target.
2. Security call-expansion tests do not prove node-limit, depth-limit, and truncation behavior.
3. The final security packet does not yet have end-to-end diagnostic-filtering coverage.
4. Hunk request-path normalization silently falls back after normalization failure and does not resolve relative paths against the allowed root.
5. Hunk normalization tests use nonexistent paths and do not prove the intended containment semantics; `a/` and `b/` empty paths are accepted.
6. `LspClientHealthSnapshot` converts the typed transport state into a debug string.

This plan is deliberately explicit so a smaller model can execute it without redesigning adjacent systems.

## Target State

At completion:

- Test-server binaries are available to integration tests but are excluded from ordinary production installs/build artifacts unless a test-support feature is enabled.
- A security-context integration test proves `max_call_nodes`, `call_depth`, truncation, and node-depth invariants.
- A security-context integration test proves security-relevant diagnostics and diagnostic evidence survive the full tool path.
- Hunk path normalization propagates errors, resolves relative request paths against the allowed root, uses component-aware containment, and rejects traversal/empty diff paths.
- Hunk path unit tests use real temporary files and assert exact results.
- `LspClientHealthSnapshot` retains `ClientTransportSnapshot` as a typed field.
- Documentation claims match the exact coverage.

## Scope

Primary files:

```text
Cargo.toml
crates/egglsp/Cargo.toml
crates/egglsp/src/client.rs
crates/egglsp/src/lib.rs
src/lsp/hunk_nav_collector.rs
tests/lsp_composite_stdio.rs
```

Documentation:

```text
README.md
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

## Non-Goals

Do not change:

- LSP framing or JSON-RPC routing;
- initialization coordinator behavior;
- shutdown semantics;
- semantic context architecture;
- security-context production policies;
- hunk clustering or multi-file support;
- workspace-edit application;
- fake-server scenario language except where needed for diagnostics or the limit test;
- public API beyond the health-snapshot type correction.

# Phase 1 — Gate Test-Server Binary Targets

## Current Problem

The root package currently declares:

```toml
[[bin]]
name = "codegg-lsp-test-server"
path = "crates/egglsp-test-server/src/main.rs"
```

without `required-features`.

This makes the fixture an ordinary binary target of the `codegg` package. It may be compiled or installed with normal production commands and can be included in release/package output.

The `egglsp` package also owns a test-server target. Both targets intentionally use the same implementation source, but neither should be part of a normal product install.

## Required Feature

Add a workspace/package feature named:

```toml
lsp-test-support = []
```

Use the same feature name in both packages where possible.

### Root `Cargo.toml`

Change the binary target to:

```toml
[[bin]]
name = "codegg-lsp-test-server"
path = "crates/egglsp-test-server/src/main.rs"
required-features = ["lsp-test-support"]
```

Add:

```toml
[features]
default = []
lsp-test-support = []
```

If the root package already has a `[features]` section, merge the new feature instead of adding a second section.

### `crates/egglsp/Cargo.toml`

Gate the package-local fixture binary similarly:

```toml
[[bin]]
name = "egglsp-test-server"
path = "../egglsp-test-server/src/main.rs"
required-features = ["lsp-test-support"]
```

Add or merge:

```toml
[features]
lsp-test-support = []
```

## Integration-Test Invocation

Because Cargo only exposes `CARGO_BIN_EXE_*` for enabled binary targets, update documented test commands to enable the feature.

Root tests:

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio
```

`egglsp` tests:

```bash
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio
cargo test -p egglsp --features lsp-test-support --test production_semantic_stdio
cargo test -p egglsp --features lsp-test-support --test production_service_stdio
```

## Workspace Test Behavior

Decide explicitly how `cargo test --workspace` should behave.

Preferred approach:

- Keep ordinary unit tests runnable without the feature.
- Mark integration test targets that require the binary with `required-features = ["lsp-test-support"]` in `Cargo.toml` using explicit `[[test]]` entries.
- Run the LSP integration matrix separately with the feature in CI.

Example:

```toml
[[test]]
name = "lsp_composite_stdio"
path = "tests/lsp_composite_stdio.rs"
required-features = ["lsp-test-support"]
```

Add equivalent entries to `crates/egglsp/Cargo.toml` for the three production stdio tests and scenario-engine test if needed.

Do not make normal `cargo test --workspace` fail because the fixture binary feature is disabled.

## Packaging Verification

Run:

```bash
cargo metadata --no-deps --format-version 1
cargo package --allow-dirty --no-verify
```

Inspect package metadata/output sufficiently to confirm the fake-server binary is not a default install target.

Also verify:

```bash
cargo install --path . --no-track --root /tmp/codegg-install-check
```

or the nearest safe equivalent does not install `codegg-lsp-test-server` without the feature.

Clean the temporary installation directory afterward.

## Acceptance Criteria

- Normal root package builds do not compile/install the root test-server binary.
- Normal `egglsp` package builds do not compile the package-local fixture binary.
- Integration tests work when `lsp-test-support` is enabled.
- No target-directory scanning or nested Cargo invocation is reintroduced.
- Both package-local `CARGO_BIN_EXE_*` paths remain deterministic.

# Phase 2 — Add a Real Security Node-Limit and Truncation Test

## Current Problem

The existing security call graph contains only three unique nodes and requests:

```text
call_depth = 2
max_call_nodes = 8
```

The test proves cycle suppression but not node-limit or truncation behavior.

## Required Scenario

Create a scenario variant with a root node that has more children than the requested node limit.

Recommended graph:

```text
entry -> validate
entry -> sink
entry -> audit
entry -> log
sink -> entry
```

Unique nodes:

```text
entry
validate
sink
audit
log
```

Keep the existing cycle edge to continue exercising visited-node handling.

The fake server must provide strict expected requests for any nodes the production BFS reaches before truncation.

## Required Test

Add:

```rust
#[tokio::test]
async fn security_context_tool_enforces_call_node_limit_and_truncation()
```

Invoke:

```json
{
  "operation": "securityContext",
  "file_path": "...",
  "line": 4,
  "column": 5,
  "security_preset": "unsafe_review",
  "security_categories": ["unsafe", "process"],
  "include_call_hierarchy": true,
  "call_depth": 2,
  "max_call_nodes": 2,
  "call_direction": "outgoing"
}
```

## Required Assertions

Deserialize the final tool result and assert:

```rust
let expansion = &parsed["results"]["call_expansion"];
let nodes = expansion["nodes"].as_array().unwrap();
```

Then assert:

- `nodes.len() <= 2`.
- Every node has a numeric `depth` field.
- Every node depth is `<= 2`.
- `expansion["truncated"] == true`.
- `parsed["results"]["limits"]["call_expansion_truncated"] == true`.
- The root node is retained.
- The service remains usable or shuts down normally after the truncated result.

Do not only assert `nodes.len() <= 2`; the truncation flags are mandatory.

## Depth-Limit Assertion

If the current serialized node format does not expose depth, inspect the production `CallExpansionNode` type.

- If it already has depth, assert it.
- If depth is intentionally not serialized, assert behavior through the strict scenario: include a depth-3 child whose request must never occur with `call_depth = 2`.

Do not change public output solely to make the test easier.

## Scenario Strictness

The scenario must be strict. Unexpected expansion beyond the allowed depth or node budget should fail the fake server.

## Acceptance Criteria

- The test fails if `max_call_nodes` is ignored.
- The test fails if truncation flags are not set.
- The test fails if the BFS requests a node beyond the depth limit.
- The existing cycle and error-degradation tests remain green.

# Phase 3 — Add Security Diagnostic Filtering Coverage

## Current Problem

The actual `securityContext` tests do not publish diagnostics and therefore do not prove:

```text
security_relevant_diagnostics
diagnostic_evidence
freshness metadata
security diagnostic filtering
```

## Scenario Change

Create a dedicated scenario or extend the main security scenario with a server notification after initialization and before semantic collection completes:

```json
{
  "type": "SendNotification",
  "method": "textDocument/publishDiagnostics",
  "params": {
    "uri": "__SOURCE_URI__",
    "version": 1,
    "diagnostics": [
      {
        "range": {
          "start": {"line": 14, "character": 4},
          "end": {"line": 17, "character": 5}
        },
        "severity": 1,
        "source": "security-lint",
        "code": "COMMAND_INJECTION",
        "message": "untrusted input reaches shell command execution"
      },
      {
        "range": {
          "start": {"line": 9, "character": 4},
          "end": {"line": 9, "character": 20}
        },
        "severity": 3,
        "source": "style-lint",
        "code": "STYLE_ONLY",
        "message": "consider a shorter function"
      }
    ]
  }
}
```

The first diagnostic should match current security filtering. The second should be non-security noise.

If the fake-server scenario language cannot send a notification at the required point without another expected client message, add the notification to the `then` actions of an existing expected request, preferably `textDocument/documentSymbol` or initialize completion.

## Required Test

Add:

```rust
#[tokio::test]
async fn security_context_tool_filters_and_preserves_diagnostic_evidence()
```

Use the real `LspTool::execute("securityContext")` path.

## Synchronization

Ensure diagnostics are present before invoking the tool or before the collector reads them.

Preferred approaches:

1. Send diagnostics during initialize and wait through a bounded service/client diagnostics condition.
2. Send diagnostics as an action on the first semantic request if the collector reads diagnostics afterward.
3. Open the file first and wait for the diagnostic cache through a bounded polling helper.

Do not use an arbitrary sleep.

## Required Assertions

Assert:

- `security_relevant_diagnostics` is non-empty.
- It contains the `COMMAND_INJECTION` diagnostic or its message.
- It does not contain the style-only diagnostic, if current filtering policy excludes it.
- `diagnostic_evidence` exists.
- Evidence metadata indicates the diagnostics source and usable/fresh status according to current schema.
- Risk markers and call expansion still remain present.

If the production filter intentionally retains both diagnostics, assert the documented filtering result rather than forcing a new policy. The test must encode current behavior, not redesign it.

## Acceptance Criteria

- The final security packet contains diagnostic evidence from the production cache.
- At least one security diagnostic survives the production security filter.
- The test uses bounded condition waiting.

# Phase 4 — Correct Hunk Request-Path Normalization

## Current Problem

The current code does:

```rust
normalize_request_relative_path(...)
    .unwrap_or_else(|_| target_path.clone())
```

This silently discards containment/canonicalization failures.

It also canonicalizes a relative request path directly, which resolves against the process working directory rather than the allowed root.

## Required Function Behavior

Replace `normalize_request_relative_path` with behavior equivalent to:

```rust
fn normalize_request_relative_path(
    request_file: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, String> {
    let canonical_root = allowed_root
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize root {}: {e}", allowed_root.display()))?;

    let resolved_file = if request_file.is_absolute() {
        request_file.to_path_buf()
    } else {
        canonical_root.join(request_file)
    };

    let canonical_file = resolved_file
        .canonicalize()
        .map_err(|e| format!("failed to canonicalize {}: {e}", resolved_file.display()))?;

    let relative = canonical_file
        .strip_prefix(&canonical_root)
        .map_err(|_| format!("path {} is outside allowed root {}", canonical_file.display(), canonical_root.display()))?;

    if relative.as_os_str().is_empty() {
        return Err("request file resolves to the project root, not a file".to_string());
    }

    Ok(relative.to_path_buf())
}
```

## Propagate Failure

In `collect()`, replace the fallback with:

```rust
let target_relative = normalize_request_relative_path(
    &target_path,
    self.semantic_collector.allowed_root(),
)
.map_err(|e| format!("hunkSourceContext: invalid file path: {e}"))?;
```

Do not continue with the original absolute/relative path after normalization failure.

## Diff Path Normalization

Update `normalize_diff_relative_path`:

1. Trim input.
2. Strip exactly one leading `a/` or `b/` prefix.
3. Check emptiness **after** stripping.
4. Reject absolute paths.
5. Reject `ParentDir`, `RootDir`, and `Prefix` components.
6. Ignore or normalize `CurDir` components.
7. Build a clean relative `PathBuf` from only `Normal` components.
8. Reject a result with no normal components.

Recommended implementation pattern:

```rust
let mut normalized = PathBuf::new();
for component in Path::new(stripped).components() {
    match component {
        Component::Normal(part) => normalized.push(part),
        Component::CurDir => {}
        Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
            return Err(...);
        }
    }
}
if normalized.as_os_str().is_empty() {
    return Err(...);
}
```

## Remove Dead String Helper

Delete `normalize_hunk_path()` and its old string-prefix tests unless another production caller still uses it.

Do not keep dead compatibility code solely for old unit tests.

## Acceptance Criteria

- Relative request paths resolve under the allowed root.
- Absolute paths outside the root return an error.
- No normalization failure is silently ignored.
- `a/` and `b/` are rejected as empty paths.
- Diff traversal and absolute/prefixed paths are rejected.

# Phase 5 — Replace Hunk Path Tests with Real Files

## Current Problem

Existing tests use nonexistent `/tmp/project` paths and sometimes discard the result. They do not prove canonical containment or prefix-collision behavior.

## Required Test Helper

Use `tempfile::TempDir` and create real paths:

```rust
fn make_real_path_fixture() -> (TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let file = root.join("src/lib.rs");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "fn main() {}\n").unwrap();
    (temp, root, file)
}
```

## Required Unit Tests

### Absolute path under root

```rust
let result = normalize_request_relative_path(&file, &root).unwrap();
assert_eq!(result, PathBuf::from("src/lib.rs"));
```

### Relative request path

```rust
let result = normalize_request_relative_path(Path::new("src/lib.rs"), &root).unwrap();
assert_eq!(result, PathBuf::from("src/lib.rs"));
```

### Prefix collision

Create two real sibling directories:

```text
.../project/src/lib.rs
.../project-other/file.rs
```

Assert the second file is rejected for root `project`.

### Traversal

Pass relative `../project-other/file.rs` and assert rejection.

### Root itself

Pass the root directory as the request path and assert rejection.

### Diff path cases

Assert:

```text
a/src/lib.rs -> src/lib.rs
b/src/lib.rs -> src/lib.rs
./src/lib.rs -> src/lib.rs
a/ -> error
b/ -> error
../outside.rs -> error
a/b/../../outside.rs -> error
/etc/passwd -> error
```

On Windows, add a conditional test for a drive-prefixed path if the test runner supports it.

## Collector-Level Test

Add a small test proving `collect()` returns a clear `hunkSourceContext: invalid file path` error for an outside-root request instead of falling through to semantic collection.

## Acceptance Criteria

- Tests assert exact success/error results.
- No test merely verifies “does not panic.”
- Prefix collision is proven with real existing paths.

# Phase 6 — Make `LspClientHealthSnapshot` Typed

## Current Problem

The snapshot stores:

```rust
pub transport: String
```

constructed through `format!("{:?}", ...)`.

This loses the typed failure reason structure and couples consumers to debug formatting.

## Required Change

Change:

```rust
pub struct LspClientHealthSnapshot {
    pub transport: ClientTransportSnapshot,
    pub pending_requests: usize,
}
```

Then change:

```rust
pub async fn health_snapshot(&self) -> LspClientHealthSnapshot {
    LspClientHealthSnapshot {
        transport: self.transport_state_snapshot().await,
        pending_requests: self.pending_request_count().await,
    }
}
```

Ensure `ClientTransportSnapshot` derives any traits needed by the snapshot:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
```

Do not convert the state to a string.

## Tests

Add unit tests for:

### Running state

```rust
let snapshot = client.health_snapshot().await;
assert_eq!(snapshot.transport, ClientTransportSnapshot::Running);
assert_eq!(snapshot.pending_requests, 0);
```

### Failed state

Use an existing deterministic failing-writer or failed-transport test seam.

Assert:

```rust
match snapshot.transport {
    ClientTransportSnapshot::Failed { reason } => assert!(reason.contains(...)),
    _ => panic!(...),
}
```

## API Documentation

Retain rustdoc stating:

- the snapshot is observational;
- values can change immediately;
- it is not a synchronization primitive.

## Dynamic Registration Snapshot

Do not redesign this API in this pass.

Add `#[doc(hidden)]` to `dynamic_registration_snapshot()` if it is intended only for tests/internal diagnostics, while leaving it public for cross-crate integration tests.

Alternatively feature-gate it under `lsp-test-support` if that can be done without awkward production code branching.

Preferred smaller-model path: add `#[doc(hidden)]` and preserve behavior.

## Acceptance Criteria

- No debug-string transport field remains.
- Consumers can pattern-match the typed state and failure reason.
- Existing tests compile with minimal assertion updates.

# Phase 7 — Update Security Test Setup for Parallel Safety

## Current Problem

Security tests use fixed directories such as:

```text
target/test-security-call-graph
target/test-security-call-graph-error
```

Separate test processes can collide.

## Required Helper

Create a workspace-local unique temporary directory because macOS `/var` symlink paths previously conflicted with root/symlink checks.

Use:

```rust
fn workspace_local_tempdir(prefix: &str) -> tempfile::TempDir
```

Implementation approach:

```rust
let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/lsp-tests");
std::fs::create_dir_all(&base).unwrap();
tempfile::Builder::new().prefix(prefix).tempdir_in(base).unwrap()
```

Use this helper for:

- main security call-graph test;
- call-hierarchy error test;
- node-limit/truncation test;
- diagnostic-filtering test.

Remove custom fixed-path cleanup guards.

## Acceptance Criteria

- Security tests can run in multiple test processes without sharing directories.
- Paths remain workspace-local and canonicalize without macOS `/var` alias issues.

# Phase 8 — Documentation Corrections

## Packaging

Document that fixture binaries require:

```text
lsp-test-support
```

and are not normal production binaries.

Update running commands accordingly.

## Security Coverage

Only claim node-limit/truncation coverage after the new small-limit test passes.

Document separate tests for:

```text
risk filtering + cycle suppression
call hierarchy error degradation
node-limit/depth/truncation
security diagnostic filtering/evidence
```

## Hunk Paths

Document:

- relative request paths resolve against `allowed_root`;
- canonical containment is required;
- diff paths reject traversal/absolute components;
- normalization errors are propagated.

## Health Snapshot

Document `LspClientHealthSnapshot.transport` as `ClientTransportSnapshot`, not a string.

## Coverage Counts

Update counts only after all new tests land. Prefer naming invariants rather than emphasizing totals.

# Suggested Implementation Order

A smaller model should follow this order exactly:

1. Add `lsp-test-support` features and gate both fixture binaries.
2. Add explicit `[[test]] required-features` entries so ordinary workspace tests remain valid.
3. Run clean package and integration smoke checks.
4. Change `LspClientHealthSnapshot.transport` to the typed enum and fix tests.
5. Rewrite `normalize_request_relative_path` and propagate errors from `collect()`.
6. Rewrite `normalize_diff_relative_path`; remove `normalize_hunk_path`.
7. Replace hunk unit tests with real `TempDir` fixtures.
8. Add the security node-limit/truncation scenario and test.
9. Add the security diagnostics scenario and test.
10. Replace fixed security test directories with `tempdir_in(target/lsp-tests)`.
11. Update documentation.
12. Run all focused and workspace verification commands.

# Verification Commands

## Packaging and feature behavior

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo build --bin codegg
cargo build -p egglsp
```

These should not require or build the test-server binaries as ordinary product targets.

Then run fixture-backed tests explicitly:

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1
cargo test --features lsp-test-support --test lsp_composite_stdio -- --test-threads=8

cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio -- --test-threads=1
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio -- --test-threads=8
cargo test -p egglsp --features lsp-test-support --test production_semantic_stdio
cargo test -p egglsp --features lsp-test-support --test production_service_stdio
cargo test -p egglsp --features lsp-test-support --test scenario_engine
```

## Focused tests

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio security_context_tool_enforces_call_node_limit_and_truncation
cargo test --features lsp-test-support --test lsp_composite_stdio security_context_tool_filters_and_preserves_diagnostic_evidence
cargo test lsp::hunk_nav_collector::tests
cargo test -p egglsp health_snapshot
```

## Clean-checkout proof

```bash
cargo clean
cargo test --features lsp-test-support --test lsp_composite_stdio security_context_tool_enforces_call_node_limit_and_truncation

cargo clean
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio initialization_handshake
```

## Full validation

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If `--all-features` causes unrelated optional-provider failures, run the LSP targets with `lsp-test-support` explicitly and record any unrelated workspace failures exactly.

# Review Checklist

## Packaging

- [ ] Root fixture binary requires `lsp-test-support`.
- [ ] `egglsp` fixture binary requires `lsp-test-support`.
- [ ] Integration tests declare required features where needed.
- [ ] Normal package install/build does not expose fixture binaries.
- [ ] No nested Cargo or target scanning exists.

## Security limits

- [ ] A graph wider than `max_call_nodes` is used.
- [ ] Node count obeys the requested cap.
- [ ] Truncation flags are true.
- [ ] Depth limit is asserted structurally or through strict scenario behavior.
- [ ] Cycle suppression test remains green.
- [ ] Error degradation test remains green.

## Security diagnostics

- [ ] Fake server publishes a security diagnostic.
- [ ] Production cache receives it without fixed sleep.
- [ ] Final security packet contains security-relevant diagnostics.
- [ ] Diagnostic evidence/freshness metadata is asserted.
- [ ] Non-security diagnostic filtering matches current policy.

## Hunk normalization

- [ ] Relative request paths resolve under `allowed_root`.
- [ ] Absolute outside-root paths return an error.
- [ ] No fallback ignores normalization errors.
- [ ] Diff `a/` and `b/` empty paths are rejected.
- [ ] Parent traversal and absolute/prefixed paths are rejected.
- [ ] Dead string helper is removed.
- [ ] Tests use real temporary files.
- [ ] Prefix collision is proven.

## Health API

- [ ] `LspClientHealthSnapshot.transport` is typed.
- [ ] Running and failed snapshot tests exist.
- [ ] Dynamic registration snapshot is marked hidden/test-support.

## Parallel safety

- [ ] Security tests use unique workspace-local temporary directories.
- [ ] Single-thread and eight-thread suites pass.

## Documentation

- [ ] Feature-gated fixture commands are accurate.
- [ ] Security limit and diagnostic coverage is not overstated.
- [ ] Hunk normalization behavior is accurate.
- [ ] Health snapshot schema is accurate.

# Completion Criteria

This corrective pass is complete when:

1. Fake-server binaries are test-support artifacts rather than default shipping binaries.
2. Security call expansion has end-to-end node-limit, depth-limit, and truncation evidence.
3. Security diagnostic filtering and diagnostic evidence are exercised through the real `securityContext` operation.
4. Hunk request and diff paths are normalized component-wise and failures are propagated.
5. Hunk tests prove containment using real files and reject empty/traversal paths.
6. `LspClientHealthSnapshot` preserves typed transport state.
7. Security integration tests are parallel-safe.
8. Ordinary workspace tests and explicit feature-enabled LSP integration tests both pass.
9. Documentation accurately records the final behavior.
10. Phase 2 can be closed without qualification.

## Handoff Result

After this pass, the LSP Phase 2 test system will retain its production-path coverage without leaking test fixtures into normal packaging, and the remaining security-limit, diagnostic-evidence, hunk-path, and health-observability claims will be directly supported by deterministic tests.
