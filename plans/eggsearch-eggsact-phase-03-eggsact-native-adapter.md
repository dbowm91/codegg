# Phase 3: Eggsact Dependency and Native Adapter Foundation

## Goal

Add `eggsact` to Codegg as an in-process deterministic utility dependency and build a small adapter layer that maps eggsact's agent API into Codegg's `Tool` and `StructuredToolResult` contracts.

This phase should not expose a large new model-facing tool surface yet. Its purpose is to establish the dependency, adapter, config, provenance, and test foundation.

## Design position

Use eggsact as a direct Rust library, not as an internal MCP server, for the default Codegg integration.

Rationale:

- Eggsact has an in-process `agent::ToolRegistry` designed for Rust consumers.
- In-process calls avoid stdio process management and JSON-RPC correlation overhead.
- Deterministic local preflight tools should have local trusted provenance, not external MCP provenance.
- Harness-side preflights need low latency and predictable resource budgets.

MCP mode can remain useful for external clients and debugging, but it should not be the ordinary Codegg path.

## Scope

- Add `eggsact` dependency.
- Add an adapter module for in-process tool calls.
- Map eggsact responses into Codegg output/provenance.
- Add configuration for profile, audience, and enablement.
- Add tests with a small representative tool set.

## Implementation steps

### 1. Add dependency

Add `eggsact` to the root `Cargo.toml` or to a small internal crate if Codegg wants stronger separation.

Possible dependency shapes:

```toml
eggsact = "1.1"
```

or, during local co-development:

```toml
eggsact = { git = "https://github.com/eggstack/eggsact" }
```

Prefer crates.io once the desired API is available there. Avoid path dependencies in committed mainline config unless the workspace intentionally vendors or checks out sibling repos.

Before committing, verify:

- MSRV compatibility with Codegg's current Rust policy.
- Dependency tree impact.
- No unnecessary feature flags pull in external services.
- License compatibility.

### 2. Create adapter module

Add `src/tool/eggsact.rs` or `src/eggsact_adapter.rs`.

Recommended structure:

```rust
pub struct EggsactRuntime {
    registry: eggsact::agent::ToolRegistry,
    profile: eggsact::agent::Profile,
    audience: eggsact::agent::ToolAudience,
}

pub struct EggsactCallOptions {
    pub tool_name: String,
    pub budget: Option<eggsact::mcp::budget::ToolBudget>,
    pub cancel_flag: Option<Arc<AtomicBool>>,
}

pub fn call_json(&self, tool: &str, args: Value) -> Result<EggsactCallResult, ToolError>;
```

If direct access to `ToolBudget` is not public enough for Codegg's needs, start with eggsact's default budget resolution and add explicit budget support later.

### 3. Map errors carefully

Map eggsact error classes into Codegg `ToolError` without flattening away useful information.

Suggested mapping:

- Unknown tool -> `ToolError::NotFound` or `ToolError::Execution` with `unknown eggsact tool`.
- Tool unavailable in profile -> `ToolError::Execution` with profile name.
- Tool not allowed for audience -> `ToolError::Execution` with audience and exposure.
- Invalid arguments -> `ToolError::Execution` with validation detail.
- Tool-level `ok=false` response -> successful Codegg tool execution that returns structured failure content, unless the Codegg wrapper contract expects hard failure.

Do not panic on eggsact responses with missing optional fields.

### 4. Map responses into Codegg output

Initially serialize eggsact `ToolResponse` to pretty JSON or a compact readable text form. Prefer a stable text envelope that includes:

- `ok`.
- `machine_code` if present.
- `result` if present.
- `findings` if present.
- `limits_applied` if present.

The model-facing format should be deterministic and bounded.

### 5. Attach provenance

Every eggsact-backed Codegg wrapper should use structured execution provenance:

- `backend = "native"`
- `implementation = "eggsact/<tool_name>"`
- `version = Some(env!("CARGO_PKG_VERSION"))` only if the adapter can accurately report eggsact's crate version; otherwise `None`.
- `trust = LocalTrusted` for pure deterministic tools.
- `trust = LocalUntrusted` only if a future eggsact tool reads user-supplied files or handles untrusted local data in a way that should not be instruction-trusted.
- `truncated` based on eggsact limits or Codegg-side projection.

### 6. Add config types

Add a config section such as:

```toml
[deterministic_tools]
enabled = true
backend = "native"       # "native" | "disabled" initially; "mcp" reserved
profile = "codegg_core_min"
model_audience = "model"
harness_audience = "harness"
expose_expert_tools = false
max_output_chars = 12000
```

Alternatively extend `[tool_backends]` with domains in a later phase and keep this phase's config private/minimal. The important point is that the adapter should not hard-code `Profile::Full` for model-facing use.

### 7. Add a small smoke wrapper

For this phase, add one or two hidden or test-only wrappers to verify the adapter:

- `deterministic_text_equal` wrapping eggsact `text_equal`.
- `deterministic_validate_json` wrapping eggsact `validate_json`.

These may be hidden from model definitions until Phase 4. The goal is adapter correctness, not final palette design.

### 8. Tests

Add tests for:

- Adapter initializes with `Profile::CodeggCoreMin` or configured default.
- `text_equal` succeeds.
- `validate_json` reports valid and invalid JSON deterministically.
- Unknown tool returns a Codegg error.
- Tool unavailable in profile returns a Codegg error.
- Model audience does not execute harness-only tools.
- Harness audience can execute a representative harness-only tool if available.
- Provenance is native/local trusted.

## Acceptance criteria

- Codegg builds with eggsact as a dependency.
- Adapter can call eggsact in-process without spawning a process.
- At least two representative tools are exercised in tests.
- Errors preserve enough detail for debugging.
- Structured provenance identifies eggsact as native deterministic infrastructure.
- No broad model-facing eggsact palette is exposed yet.

## Risks

The main risk is API coupling to eggsact internals. Keep the adapter small and depend only on eggsact's documented agent API. If an eggsact field is not public or stable, do not reach around it; request or add a small upstream API instead.
