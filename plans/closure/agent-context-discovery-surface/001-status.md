# Agent Context Discovery Surface M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-context-discovery-surface/001-mcp-resource-and-prompt-projection.md`

Source subsystem roadmap:

- `plans/subsystems/agent-context-discovery-surface-roadmap.md`

Repository baseline reviewed: `2f64c9e16f9ee96ea4d47ed897201f4c24d4606f`

Implementation commit:

- `c320117` — Implement context discovery and plugin interoperability foundations

## 1. Executive finding

M001 is complete. MCP resource descriptors and reads now have bounded native
deferred tools with integrity-bound handles, current-turn MCP visibility, origin
provenance, UTF-8-safe text limits, and metadata-only blob projection. Prompt
descriptors and structured messages preserve source role data, while invocation
is available only through an explicit host/user-selection API with required
argument validation. No model-facing prompt activation path was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Preserve prompt/resource protocol content | `src/mcp/mod.rs`, `src/mcp/local.rs`, `src/mcp/remote.rs` structured DTOs and multi-entry parsing | pass |
| Bounded resource discovery/read | `src/tool/mcp_resource.rs` search/read tools and host caps | pass |
| Non-forgeable exact reads | `mcpres:v1` handle digest plus current descriptor revalidation | pass |
| Explicit prompt activation | `McpService::invoke_user_selected_prompt` validates descriptor and required args | pass |
| Plugin/workspace MCP coherence | turn construction derives one activation-reconciled MCP view before tool/loop construction | pass |
| Trust and binary handling | provenance framing, bounded text, blob metadata-only projection | pass |

## 3. Production implementation evidence

`McpService` remains the sole transport/connection owner. `McpResourceSearchTool`
and `McpResourceReadTool` are registered in the canonical `ToolRegistry`, are
deferred and read-only, and consume the turn's `SearchRuntimeContext` MCP view.
Search emits descriptors only. Read accepts an exact admitted handle, lists the
current resource catalog again, and then reads through `McpService`.

`McpPromptResult` retains bounded message role/text/MIME metadata. The explicit
activation method performs connected-server descriptor lookup, object-argument
validation, required-argument validation, and structured retrieval. Source role
labels remain data and are not provider message authority.

## 4. Verification executed

```text
rtk cargo test -p codegg-core memory::tests --locked                 # 9 passed
rtk cargo test -p codegg --lib plugin::package::tests --locked       # 2 passed
rtk cargo test -p codegg --lib plugin::install::tests --locked       # 33 passed
rtk scripts/verify.sh quick                                          # passed
```

The quick verification covered formatting, generated-agent consistency,
core-boundary, sandbox, execution ownership, TUI authority, HTTP disposition,
audit coverage, scheduler-bypass guards, and workspace all-target checks.

## 5. Invariant review

- McpService remains the only protocol and connection owner.
- Resource tools cannot accept arbitrary URI/server pairs or expose raw blobs.
- Plugin MCP resources use the same activation-pinned view as the agent loop.
- Prompt invocation is not present in the model tool registry.
- Resource and prompt outputs are bounded and source-framed; secrets and headers
  are not included in projections.

## 6. Failure and recovery review

Disconnected or failed servers are omitted from descriptor discovery and return
the existing typed MCP errors on exact reads. A stale or forged handle fails
closed during revalidation. Existing reconnect/shutdown behavior remains in
the local and remote clients. No new persistent resource store or subscription
engine was introduced.

## 7. Migration and compatibility review

The changes are additive. Existing raw MCP tool exposure, local/remote
transport negotiation, OAuth, plugin MCP reconciliation, and string-only
`get_prompt`/`read_resource` compatibility methods remain available. No storage
migration or provider protocol change was required.

## 8. Security review

Resource reads retain server/origin boundaries and use bounded handles. Binary
content is not emitted as unbounded base64. Prompt roles are explicitly
untrusted metadata. No credential, header, OAuth token, process environment,
or internal endpoint diagnostic is included in model-facing descriptors.

## 9. Documentation and operations

`architecture/mcp.md` and `docs/MCP.md` document resource handle projection,
blob behavior, and host-only prompt activation. The implementation records
bounded/truncation information in structured tool provenance and diagnostics.

## 10. Unresolved findings

None that prevent strict closure. Resource-template expansion and subscriptions
remain explicit non-goals in the implementation plan.

## 11. Roadmap disposition

M001 is closed. M002 remains independently ready and was not blocked by this
milestone; it is the next context-discovery milestone in the requested order.

## 12. Registry updates

The implementation plan is marked `implemented`, the roadmap now records M001
closed/M002 ready, and the registry row is closed in the same status-change
commit as this closure record.
