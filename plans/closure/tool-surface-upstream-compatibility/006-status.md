# Tool-Surface Upstream Compatibility M006 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tool-surface-upstream-compatibility/006-mcp-modern-protocol-and-metadata.md`

Source subsystem roadmap: `plans/subsystems/tool-surface-upstream-compatibility-corrective-addendum.md`

Repository baseline reviewed: `2798501b61edcd43acb215924500635dc705bbb2`

Implementation commits:

- `65ed1c6` — negotiate modern MCP protocol and preserve metadata;
- `dd0a8ec` — move implementation into closure review;
- this closure commit — accepted evidence and dependency reconciliation.

## 1. Executive finding

M006 is fully implemented and strictly closed. CodeGG's local stdio and
remote HTTP MCP clients now use one shared protocol policy: they probe modern
`2026-07-28` servers with `server/discover`, retain the negotiated protocol and
bounded untrusted metadata, and use legacy `2024-11-05` initialization only
after an explicit protocol-shape probe error. Modern tool metadata and result
envelopes are preserved without changing exposure or execution authority.

The deterministic fixtures used here model the audited MCP `2026-07-28`
contract. No network-dependent test or upstream binary was added.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| One protocol policy for local and remote clients | `src/mcp/protocol.rs`; both clients call the shared negotiation/metadata helpers | pass |
| Modern discovery and negotiated version | `modern_probe_preserves_protocol_tool_metadata_and_result_envelope`; `parse_discovery` tests | pass |
| Safe legacy compatibility | `explicit_legacy_probe_error_falls_back_to_initialize`; explicit classifier tests | pass |
| Unrelated failures are not masked by fallback | classifier rejects connection, timeout, auth/HTTP, and non-protocol errors | pass |
| Tool metadata fidelity | `McpTool` retains bounded output schema, annotations, and `_meta`; parser regression test | pass |
| Catalog identity correctness | `McpService::catalog_fingerprint` includes semantic metadata and ignores tool ordering | pass |
| Structured result parity | local modern fixture covers `structuredContent`, `resultType`, `_meta`, unknown fields, and `isError` | pass |
| JSON-RPC/tool error distinction | JSON-RPC errors remain `Err(McpError::Server)`; tool `isError` remains an `Ok` result flag | pass |
| Exposure and authority invariants | `list_filtered_tools` remains policy-gated; metadata is documented as untrusted and cannot grant permission | pass |
| Architecture documentation | `architecture/mcp.md` and `architecture/ide.md` describe negotiated support and the IDE server's legacy binding | pass |

## 3. Production implementation evidence

- `src/mcp/protocol.rs` centralizes protocol constants, modern `_meta`
  construction, discovery parsing, fallback classification, bounded strings/
  schemas/metadata, and shared MCP tool parsing.
- `src/mcp/local.rs` probes stdio with `server/discover`, records negotiated
  protocol/server metadata, falls back to legacy initialization only for
  explicit method/parameter protocol errors, and retains modern per-request
  metadata on subsequent calls.
- `src/mcp/remote.rs` applies the same policy to HTTP, adding modern
  `MCP-Protocol-Version`, `Mcp-Method`, and tool-call `Mcp-Name` headers while
  retaining legacy session headers and existing DNS/redirect/OAuth controls.
- `src/mcp/mod.rs` carries rich internal `McpTool` metadata and server
  discovery state, projects the existing provider-compatible definition shape,
  and computes deterministic semantic catalog fingerprints.
- `McpToolCallResult` now preserves bounded result metadata and explicit
  tool-level error/type fields while keeping `structuredContent` authoritative.

## 4. Verification executed

### Commands and outcomes

```text
rtk cargo fmt --all -- --check                         pass
rtk cargo check -p codegg --tests                      pass
rtk cargo test -p codegg --lib mcp -- --test-threads=1 pass — 33 tests
rtk cargo test -p codegg --test mcp --test mcp_reconnect -- --test-threads=1
                                                        pass — 44 tests
rtk cargo clippy --workspace --all-targets \
  --features server,plugins,lsp-test-support -- -D warnings
                                                        pass
rtk scripts/verify.sh quick                            pass
rtk git diff --check                                   pass
```

The first test-link attempt used the normal workspace command and was blocked
by the host's pre-existing x86_64/arm64 `/opt/local` `liblzma` mismatch. The
same focused tests passed with the repository-built x86_64 `liblzma.a` supplied
as a local linker argument. The first quick verification attempt was SIGTERM'd
while cold-compiling dependencies; the warm rerun passed all guards and the
workspace check. These are environment observations, not M006 correctness
findings.

No eggsearch 0.3.9 binary was locally available for manual stdio smoke. The
deterministic local fixture covers the modern and legacy wire contracts, and
the remote implementation compiles against the same shared policy. No
network-dependent CI test was introduced.

## 5. Invariant review

- Legacy servers remain supported through explicit fallback to the existing
  initialization path.
- Modern discovery metadata, annotations, schemas, and result metadata are
  integration data only. They do not affect `McpExposurePolicy`, permissions,
  trust, or execution routing.
- Raw MCP tools remain hidden whenever the existing policy says so; provider
  projection remains name/description/input-schema compatible.
- Tool and metadata payloads are bounded before storage or provider-facing
  projection.
- Existing local environment clearing, remote SSRF/DNS revalidation,
  redirects, OAuth, timeout, reconnect, heartbeat, and cancellation paths are
  retained.

## 6. Failure and recovery review

The modern probe distinguishes explicit unsupported method/parameter protocol
errors from connection, timeout, authentication, malformed-response, and
server failures. Only the former initiate legacy retry. JSON-RPC error codes
and data are retained in diagnostics. Remote reconnect re-enters the same
negotiation path; legacy session state is not sent on modern requests.

## 7. Migration and compatibility review

No durable storage or configuration migration is required. Existing provider
adapters continue to receive the original `ToolDefinition` fields. Existing
mock/test constructors use defaults for the additive internal metadata fields.
Servers that do not advertise modern discovery continue to use the legacy
`initialize`/`tools/list` flow after an eligible probe error.

## 8. Security review

The new fields are treated as untrusted server data and bounded at the MCP
edge. Server identity, annotations, output schemas, discovery instructions,
and result metadata cannot grant permissions or expose hidden raw tools.
No OAuth, URL validation, environment, process, or execution authority was
weakened. No raw eggsearch surface was exposed.

## 9. Documentation and operations

Architecture docs now describe modern/legacy negotiation, stateless modern
HTTP metadata, legacy session handling, result envelopes, and semantic catalog
identity. The audited compatibility baselines remain MCP `2026-07-28`,
eggsearch `0.3.9`, and eggsact `1.2.5`; only the generic MCP portion is owned
by M006. No compatibility matrix, CI lane, or scheduled upstream smoke was
created.

## 10. Unresolved findings

No critical, high, or medium M006 correctness, security, migration, or
resource finding remains. The host linker mismatch and cold-build SIGTERM
described in section 4 are low-severity environment limitations and have
passing bounded workarounds; they do not require a corrective pass.

## 11. Roadmap disposition

M006 is closed. The corrective roadmap remains active because its independent
M008 and newly unblocked M007 milestones remain future work. M007 may now
consume the final generic MCP metadata/negotiation contract; M008 remains
independent and ready.

## 12. Registry updates

The registry audit checked every row in **Blocked work** and the corrective
roadmap dependency graph:

- M007 was the only registered plan blocked on M006. Its other dependency set
  is empty, so it moved from `blocked` to `ready` in the same closure commit.
- M008 was already `ready` and remains ready.
- No other registered blocked plan names M006 as a hard or interface
  dependency, so no unrelated plan was promoted.
- M006 was removed from active closure work and recorded under recently closed
  work with this closure record and implementation commit.
