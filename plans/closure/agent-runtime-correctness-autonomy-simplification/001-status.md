# Agent Runtime Correctness, Autonomy, and Simplification Milestone M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/001-mcp-authority-provenance-and-tool-surface-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Implementation commit:

- `fb972426` — corrected MCP authority, decision receipts, and tool-surface cache identity.

## 1. Executive finding

M001 is complete. Raw MCP origin no longer expands authority, accepted native
tool calls carry an ephemeral permission receipt, unavailable provenance
revisions are omitted, and the model-facing MCP cache uses identity/schema
content rather than tool count.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Raw unknown MCP calls remain `Ask` | `tests/permission.rs::unknown_mcp_tools_remain_ask_without_local_path`; loop auto-allow condition | pass | No-path and path-looking unknown MCP calls use normal mutating permission evaluation. |
| Explicit allow/deny precedence remains intact | `tests/permission.rs::explicit_mcp_allow_and_deny_override_default_ask`; 44 permission tests | pass | Persisted decisions continue to override the default `Ask`. |
| External origin is not a trusted read-only classification | `src/agent/loop.rs` permission path; native category mapping | pass | Managed wrappers retain typed native categories; raw MCP names have no special category. |
| Execution provenance is truthful | `PermissionDecisionReceipt`; `build_tool_execution_context` | pass | Decision ID and issued time come from the accepted receipt; workspace-derived synthetic fields were removed. |
| Equal-count MCP replacement invalidates cache | `src/agent/loop.rs::mcp_surface_revision_detects_equal_count_schema_changes` | pass | Name/description/schema changes produce a different digest; unchanged definitions reuse it. |
| Focused verification and quick check | Commands below | pass | All required focused checks completed successfully. |

## 3. Production implementation evidence

`AgentLoop::check_tool_permission` retains the narrow workspace-local file
mutation UX exception but removes `mcp__*` from that exception. Accepted
decisions are represented by `PermissionDecisionReceipt`, which carries a
real ephemeral decision ID, source, outcome, issue time, and the existing
permission-policy content fingerprint. `ToolExecutionContext` consumes that
receipt and leaves unavailable workspace policy identity/revision fields
absent.

MCP cache identity is a SHA-256 digest of sorted, provider-visible tool
definitions after defer-loading metadata is applied. The digest does not
include credentials, transport configuration, or secrets, and MCP reads
retain the existing bounded `try_read` behavior.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt -- --check
rtk cargo check -p codegg --all-targets
rtk cargo test -p codegg --test permission
rtk cargo test -p codegg --lib mcp_surface_revision -- --nocapture
rtk scripts/verify.sh quick
```

### Results

- Formatting check passed.
- Targeted all-target compilation passed.
- Permission suite passed: 44 tests.
- MCP cache regression passed: 1 test.
- Quick verification passed, including workspace all-target check and static guards.

## 5. Invariant review

- `Ask` remains live unless an explicit policy, persisted decision, or user
  choice accepts it; raw MCP naming is not an approval signal.
- Persistent deny remains authoritative through `PermissionChecker`.
- Managed read-only wrappers continue to use the existing typed category path.
- Tool Program calls still receive the accepted decision identity and policy
  fingerprint required by the canonical broker authority path.
- Provenance fields are derived from the receipt or remain `None`; no
  workspace/session identity is presented as a policy revision.
- MCP cache identity changes with provider-visible identity, schema,
  description, or defer-loading metadata.

## 6. Failure and recovery review

Permission prompt timeout/cancellation behavior is unchanged and still fails
closed to deny. MCP cache refresh retains the existing non-blocking read
attempt and bounded retry behavior. A denied MCP call remains a normal typed
tool denial and does not enter an authority-broadening recovery path.

## 7. Migration and compatibility review

No storage schema, public protocol, or MCP interoperability change was made.
Configured and persisted allows/denies remain supported. Users who previously
received blanket raw-MCP approval may now see an approval prompt; this is the
intentional security correction.

## 8. Security review

The external-tool authority escalation is removed. Cache digests exclude
credentials and raw connection configuration. Permission diagnostics retain
the existing argument handling and no new secret-bearing provenance fields
were introduced.

## 9. Documentation and operations

Updated:

- `architecture/permission.md` — raw MCP authority and receipt semantics.
- `architecture/tool.md` — receipt-aware execution context and MCP cache digest.

No new static guard, CI lane, migration, or release machinery was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The in-process MCP catalog has no separate monotonic revision field; the loop computes a bounded digest at refresh time. | No correctness gap; cache identity is provider-visible and race-safe under the existing bounded read path. | None for M001; retain the digest until a service-owned revision is justified. |

## 11. Roadmap disposition

Milestone closed. No future registered plan can be unblocked by M001 alone:
M005 still requires M002 and M004, M006 requires M005, and M009 requires
M001-M008. Those statuses remain unchanged.

## 12. Registry updates

- M001 moved from dependency-ready to recently closed.
- The subsystem roadmap marks M001 closed.
- M005 remains blocked on M002/M004; M006 and M009 remain blocked.
