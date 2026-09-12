# Tool-Surface Upstream Compatibility M006 — MCP Modern Protocol and Metadata

Status: ready

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Planning branch: `agent/tool-surface-compat-2026-09`

Source corrective addendum:

- `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`

Predecessor references:

- `architecture/mcp.md`
- `src/mcp/local.rs`
- `src/mcp/remote.rs`
- `src/mcp/mod.rs`
- `src/provider/mod.rs` / `ToolDefinition`

Audited upstream evidence:

- eggsearch 0.3.9 current harness contract supports MCP `2026-07-28`, `server/discover`, stateless metadata, `structuredContent`, `outputSchema`, annotations, and fingerprinted tool metadata while retaining legacy initialize compatibility;
- eggsact 1.2.5 supports the same modern protocol era for MCP consumers, though CodeGG consumes eggsact in-process.

## 1. Objective

Upgrade CodeGG's generic MCP client boundary so a modern MCP server can expose current protocol metadata and result contracts without forcing every configured MCP server to support the latest protocol.

The goal is compatibility and metadata fidelity, not an MCP architecture rewrite.

## 2. Current implementation evidence

At the reviewed baseline:

- `LocalClient::initialize()` sends hard-coded `protocolVersion: "2024-11-05"`;
- `RemoteClient::initialize()` independently sends the same hard-coded version;
- `architecture/mcp.md` and `architecture/ide.md` describe that version as the CodeGG MCP protocol;
- `discover_tools()` uses `tools/list` and projects each tool to `name`, `description`, and `inputSchema` only;
- modern fields such as `outputSchema` and annotations are discarded;
- `call_tool_structured()` already preserves top-level `structuredContent`, with a legacy JSON-content fallback;
- CodeGG therefore has a useful modern response seam already, but protocol negotiation and tool metadata lag behind it.

## 3. Non-goals

M006 MUST NOT:

- require MCP `2026-07-28` from arbitrary configured servers;
- replace CodeGG's MCP implementation with rmcp or another SDK;
- redesign OAuth, SSRF protection, remote reconnection, heartbeat, or process spawning unless a narrow change is required for protocol correctness;
- expose raw eggsearch tools to the model;
- add eggsact as an MCP subprocess;
- implement eggsearch-specific request fields;
- redesign CodeGG `tool_search` ranking or hydration policy;
- add a compatibility matrix or new CI workflow.

## 4. Invariants

- Legacy servers that currently initialize successfully remain supported.
- Protocol negotiation is centralized so local and remote clients cannot silently drift.
- Modern metadata cannot grant execution permission or bypass `McpExposurePolicy`.
- `structuredContent` remains preferred when present.
- Unknown additive fields are ignored/preserved safely.
- Existing timeout, cancellation, auth, SSRF, and environment-clearing controls remain intact.
- Tool-list metadata is bounded; no unbounded server-provided description/schema material is injected directly into model context.

## 5. Expected production-code changes

Likely files:

- `src/mcp/local.rs`;
- `src/mcp/remote.rs`;
- `src/mcp/mod.rs` or a small shared protocol module;
- `src/provider/*` where `ToolDefinition` is defined;
- tool catalog/discovery code that consumes `ToolDefinition`;
- `architecture/mcp.md`, `architecture/tool.md`, and `architecture/ide.md` where protocol claims are made.

### 5.1 Central protocol policy

Create one internal representation for MCP protocol selection/negotiation. Do not leave protocol literals duplicated in local and remote clients.

The implementation should distinguish at least:

- preferred modern protocol (`2026-07-28` where supported by the server contract);
- negotiated protocol actually in use;
- legacy fallback (`2024-11-05`) for servers that reject or cannot negotiate the modern path.

Do not silently retry every initialization failure as “legacy.” Fallback should occur only for errors that plausibly indicate unsupported protocol/method/version semantics. Auth, connection, malformed-response, timeout, and policy failures must retain their original classification.

### 5.2 Modern discovery handshake

When the negotiated server supports current discovery semantics, consume `server/discover` or equivalent advertised capability according to the current MCP contract.

Requirements:

- absence/unsupported-method response is not fatal when the server otherwise supports legacy `initialize` + `tools/list`;
- modern discovery metadata is stored as integration metadata, not trusted authority;
- legacy servers continue through `tools/list` exactly as needed;
- do not make CodeGG's user-facing `tool_search` dependent on an upstream discovery facade.

### 5.3 Tool metadata preservation

Extend CodeGG's internal MCP tool metadata so `tools/list` can retain, at minimum where present:

- input schema;
- output schema;
- standard annotations;
- server-provided metadata needed for deterministic catalog identity or compatibility diagnostics.

Keep model/provider projection backward compatible. Existing provider adapters that need only `name`, `description`, and input parameters must not break merely because the internal type is richer.

Prefer optional fields with defaults over invasive call-site churn.

### 5.4 Catalog identity / cache invalidation

Audit where CodeGG caches, snapshots, fingerprints, compares, or deduplicates MCP tool inventories.

If any identity currently depends only on tool count or names, replace it with a deterministic fingerprint covering semantically relevant metadata available to CodeGG: names, descriptions, input schemas, output schemas, annotations, and applicable discovery metadata.

Do not add a persistent cache if one does not already exist. This requirement is about correctness of existing catalog identity/invalidation only.

### 5.5 Structured call result parity

Retain the existing `structuredContent` path. Add regression coverage proving that modern result envelopes remain readable when they include fields such as `resultType` and `_meta` in addition to `content`/`structuredContent`.

Tool-level `isError` semantics must remain distinguishable from JSON-RPC transport/protocol failures. If CodeGG currently loses this distinction, fix only the narrow result contract necessary to preserve it.

## 6. Ordered work packages

### WP1 — Protocol inventory and shared representation

1. Locate every hard-coded MCP protocol version.
2. Identify local/remote initialization differences.
3. Introduce a shared protocol/negotiation representation and tests.
4. Update no user-facing behavior yet beyond using the shared constant/path.

### WP2 — Negotiated modern initialization with bounded fallback

1. Implement the preferred modern initialization path.
2. Classify unsupported-version/method failures eligible for legacy fallback.
3. Ensure auth/timeout/server-fault errors do not get masked by fallback.
4. Record negotiated protocol/server version in client state for diagnostics.

### WP3 — Discovery and metadata fidelity

1. Preserve output schemas and annotations from `tools/list`.
2. Consume modern server discovery metadata when available.
3. Ensure unsupported discovery is harmless for legacy servers.
4. Preserve current exposure filtering and raw-tool hiding.

### WP4 — Catalog fingerprint audit

1. Find any MCP catalog cache/snapshot identity.
2. Use semantically relevant metadata rather than count-only identity where applicable.
3. Add deterministic tests showing schema/annotation changes invalidate identity while ordering noise does not if ordering is not semantically relevant.
4. Do not create new persistent infrastructure.

### WP5 — Result-envelope regression tests

Cover:

- legacy text-only result;
- legacy JSON content fallback;
- modern `structuredContent`;
- modern extra envelope metadata;
- tool-level `isError` versus JSON-RPC error;
- unknown additive fields.

### WP6 — Documentation and closure evidence

Update architecture docs to describe negotiated support rather than claiming a single hard-coded protocol version. Record exact upstream versions used for compatibility fixtures/manual smoke.

## 7. Focused verification

Run the narrow MCP/client tests first, then ordinary repository checks. Expected commands, adjusted to actual test target names if the current repo differs:

```bash
cargo fmt --all -- --check
cargo test --lib mcp -- --test-threads=1
cargo test --test mcp_* -- --test-threads=1
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not create a network-dependent CI test. If an eggsearch 0.3.9 binary is locally available, one manual stdio smoke may be recorded in the closure record.

## 8. Acceptance criteria

M006 is implementation-complete when:

- local and remote clients share one protocol policy;
- modern negotiation works with a fixture/current server and legacy fallback works with a legacy fixture;
- unsupported modern semantics do not hide unrelated errors;
- CodeGG retains modern tool output schemas/annotations and relevant discovery metadata;
- modern tool-call envelopes still produce the same structured result authority;
- existing exposure/permission/trust behavior is unchanged;
- architecture docs no longer state that CodeGG is unconditionally fixed to `2024-11-05`;
- focused verification and `scripts/verify.sh quick` pass.

## 9. Stop conditions

Stop and record a blocker instead of broadening scope if:

- current MCP specifications require a transport rewrite rather than additive negotiation;
- a third-party server depends on undocumented behavior that cannot coexist with modern negotiation without an architecture decision;
- implementing server discovery would require replacing CodeGG's own progressive-disclosure ownership;
- broad provider/TUI/agent changes become necessary.

## 10. Closure evidence required

The closure record must include:

- implementation commit(s);
- exact protocol fixtures or upstream binary/version used;
- modern initialization/discovery evidence;
- legacy fallback evidence;
- result-envelope compatibility evidence;
- tool metadata preservation/fingerprint evidence where applicable;
- verification commands and outcomes;
- any servers/features intentionally left on legacy behavior.
