# Runtime Assets Milestone 007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/007-plugin-mcp-transport-alias-corrective-pass.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-plugin-contributions-corrective-addendum.md`

Repository baseline reviewed: `89f57f5`

Implementation commits:

- `eb9c4d9` — canonicalize plugin MCP transport aliases and add production-path coverage.
- `fb3289a` — activate the M007 implementation plan.
- `89f57f5` — move M007 into closure review.

Original corrected milestone and closure:

- `plans/implementation/runtime-assets/006-plugin-declarative-asset-mcp-contributions.md`
- `plans/closure/runtime-assets/006-status.md`

## 1. Executive finding

M007 is fully implemented and closed. Plugin MCP contributions now accept the
documented `local`/`stdio` and `remote`/`http` spellings through one typed
translation seam. Reconciliation passes only canonical `local`/`remote` values
to the existing `McpService` lifecycle, while validation, activation,
namespacing, origin ownership, collision handling, removal, and MCP security
behavior remain unchanged.

M007 corrects the narrow M006 coverage gap without reopening M006 or adding a
plugin-specific MCP client, transport, registry, or configuration layer.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One accepted transport mapping | `PluginMcpTransport` and `PluginMcpServerContribution::canonical_server_type()` in `src/plugin/manifest.rs`; table-driven unit test | pass | `local`/`stdio` → `local`; `remote`/`http` → `remote` |
| Validation and runtime agree | Manifest validation calls the same typed parser used by reconciliation | pass | Unsupported values fail before connection |
| Field requirements remain equivalent | Alias table tests cover command-required local forms and URL-required remote forms | pass | URL-only local and command-only remote declarations fail |
| Successful stdio production path | `mcp_reconciliation_connects_stdio_alias_through_local_lifecycle` in `tests/plugin_contributions.rs` | pass | Reaches real child-process initialize and `tools/list` discovery |
| HTTP alias reaches normal remote seam | `mcp_reconciliation_routes_http_alias_to_remote_validation_path` | pass | Reaches existing remote URL validation without external network or loopback SSRF bypass |
| Canonical local/remote compatibility | Existing `mcp_reconnect` config-dispatch tests plus canonicalization matrix | pass | No configured-MCP alias behavior was changed |
| Collision and origin protection | Existing reconciliation test, with explicit stale-removal assertion | pass | Configured server remains authoritative; stale plugin ownership is removed only for plugin origin |
| Activation-disabled isolation | `inactive_plugin_mcp_contributions_do_not_resolve_or_materialize` | pass | Inactive plugin contributes no MCP declaration |
| Passive ownership and security | Reconciliation still calls `McpService::connect_from_config`; existing credential, DNS/redirect, OAuth, exposure, and permission paths remain authoritative | pass | No new runtime or security bypass |
| Storage/protocol compatibility | No schema, storage, or public protocol changes | pass | `contributions` remains optional and backward-compatible |

## 3. Production implementation evidence

- `src/plugin/manifest.rs` owns the accepted plugin MCP transport vocabulary
  and canonicalization. Validation derives its local/remote field requirements
  from the typed transport instead of maintaining a second string list.
- `src/mcp/mod.rs` canonicalizes each validated plugin declaration immediately
  before the existing `connect_from_config()` call. Directly malformed resolved
  input is reported as a bounded per-server failure rather than dispatched as
  an unsupported MCP type.
- `src/plugin/mod.rs` re-exports the transport type as part of the plugin
  manifest API; no lifecycle ownership moved from `McpService`.
- `tests/plugin_contributions.rs` adds real stdio lifecycle coverage, a
  deterministic HTTP/remote validation-seam check, alias validation tests,
  stale-origin assertions, and activation-disabled isolation coverage.
- `architecture/plugin.md`, `architecture/mcp.md`, and `docs/PLUGINS.md`
  document the accepted spellings and translation ownership.

## 4. Verification executed

### Commands run

```bash
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo fmt --all -- --check
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo test -p codegg plugin --lib
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo test --test plugin_contributions
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo test --test mcp
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo test --test mcp_reconnect
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo clippy -p codegg --all-targets --locked -- -D warnings
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig cargo clippy -p codegg --all-targets --locked -- -D warnings -A clippy::type-complexity -A clippy::unnecessary-unwrap
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig scripts/verify.sh quick
PKG_CONFIG_PATH=/usr/local/lib/pkgconfig git diff --check
```

### Results

- Formatting passed.
- Focused plugin library tests passed: 369 tests, 0 failures.
- Plugin contribution integration tests passed: 8 tests, 0 failures.
- MCP integration tests passed: 26 tests and 18 reconnect tests, 0 failures.
- `scripts/verify.sh quick` passed, including built-in-agent freshness,
  core-boundary, sandbox-contract, execution-ownership, and workspace
  all-target checking.
- The exact required Clippy command was attempted and found six pre-existing
  `clippy::type-complexity` findings in
  `crates/codegg-core/src/snapshot/checkpoint.rs` and one pre-existing
  `clippy::unnecessary-unwrap` finding in `src/agent/tool_batch.rs`. `git
  blame` attributes these findings to the base revision `4dd1220c`; none is in
  the M007 change set.
- A supplemental all-target Clippy run allowing only those two baseline lint
  classes passed with no issues in the M007 change set.
- The default linker metadata selected an arm64 MacPorts `liblzma` for this
  x86_64 shell; setting `PKG_CONFIG_PATH=/usr/local/lib/pkgconfig` selected
  the matching x86_64 dependency for local compilation and test execution.
- `git diff --check` passed.

## 5. Invariant review

- Plugin declarations remain passive data and are materialized only through
  `McpService`; no plugin runtime is invoked by discovery or translation.
- `McpService` remains the sole MCP connection/lifecycle owner.
- Configured servers and other plugin origins cannot be overwritten; stale
  removal still targets only plugin-origin servers absent from the desired set.
- Namespaced server identities and `McpServerOrigin::Plugin` metadata are
  preserved after successful alias reconciliation.
- Canonicalization changes only transport spelling. It does not alter command,
  args, environment, URL, headers, timeout, origin, activation, or exposure
  semantics.
- Unsupported transport strings remain bounded validation failures and cannot
  reach a connection attempt.

## 6. Failure and recovery review

- Translation is pure and deterministic; it introduces no durable alias state
  or additional failure state.
- Per-server connection failures remain degraded diagnostics and do not
  invalidate unrelated plugin assets or configured servers.
- Reconciliation remains idempotent for existing same-plugin servers and
  preserves current collision/removal behavior.
- Restart reconstructs aliases from the persisted manifest and existing
  activation state; no in-memory alias registry is authoritative.
- Concurrent workspace/turn reconciliation continues to use existing
  configured-server cloning and plugin isolation. No global lock, scheduler,
  or second MCP runtime was added.
- The stdio integration fixture uses a fixed `/bin/sh -c` script supplied by
  the test itself; production still launches the declared executable through
  argv without shell interpretation.

## 7. Migration and compatibility review

- No database or storage migration is required.
- No public protocol DTO changed; `contributions` remains optional under
  serde-defaulted manifest parsing.
- Existing canonical configured MCP values `local` and `remote` retain their
  existing `McpService` behavior. Plugin-only aliases are translated before
  that canonical configuration boundary.
- No remote marketplace, credential-storage, dependency, hook, or scheduler
  behavior was introduced.

## 8. Security review

- Alias handling does not relax credential-key rejection for plugin headers or
  environment entries.
- The normal local process and remote HTTP clients continue to enforce their
  existing argument handling, URL validation, DNS revalidation, redirect,
  OAuth, header, exposure, and permission policies.
- Diagnostics and tests do not include credential values or environment/header
  secrets.
- Plugin metadata cannot grant MCP tool permission or raw MCP exposure.

## 9. Documentation and operations

Updated:

- `architecture/plugin.md`
- `architecture/mcp.md`
- `docs/PLUGINS.md`
- `plans/implementation/runtime-assets/007-plugin-mcp-transport-alias-corrective-pass.md`
- `plans/subsystems/runtime-assets-plugin-contributions-corrective-addendum.md`
- `plans/registry.md`

The routine operator/developer verification remains the focused plugin/MCP
tests plus `scripts/verify.sh quick`. No new CI lane or remote-network matrix
was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Base revision `4dd1220c` has six checkpoint `type-complexity` and one tool-batch `unnecessary-unwrap` Clippy findings | Exact repository-wide `-D warnings` Clippy remains red independently of M007 | Track under the existing runtime-safety checked-edit-history corrective work; do not reopen or expand M007 |

No M007 correctness, compatibility, security, ownership, migration, or
reconciliation finding remains.

## 11. Roadmap disposition

M007 is closed and is the current strict disposition for the plugin MCP alias
compatibility correction identified after M006. M006 remains an immutable
historical record of the original contribution bridge and is not rewritten.
The corrective addendum is closed; its deferred dependency resolution,
marketplace, new transport, browser, hook, scheduler, and rollback work
remains deferred.

The blocked-work and dependency-graph audit found no registered implementation
plan whose hard or interface blocker is this exact runtime-assets M007. The
ready runtime-safety M013 corrective plan remains independent and unchanged;
no future plan was unblocked by this closure.

## 12. Registry updates

- M007 implementation plan moved `ready` → `active` → `implemented`.
- The corrective addendum moved to `closing` for evidence review and is now
  marked `closed`.
- The runtime-assets corrective subsystem row was removed from active roadmaps
  and added to recently closed control points with this closure record.
- M007 was removed from dependency-ready plans.
- The blocked-work section was audited. No downstream registered plan named
  this exact M007 as a blocker, so no downstream status changed.
- The pre-existing runtime-safety M013 ready status and its named Clippy/base
  revision blocker remain unchanged.
