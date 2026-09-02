# Runtime Assets Milestone 006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/006-plugin-declarative-asset-mcp-contributions.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md`

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Implementation commit:

- `35cf6f5` — declarative plugin asset and MCP contribution bridge.

## 1. Executive finding

M006 is fully implemented and closed. Plugin manifests now carry an additive,
bounded passive-contribution section. Active contributions are resolved from
the immutable M005 activation view and consumed by the existing asset snapshot
builder and `McpService`. Executable plugin behavior remains owned by
`PluginService`, and execution remains scheduler-owned. No duplicate plugin,
asset, or MCP runtime was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Passive schema and legacy compatibility | `PluginContributions` in `src/plugin/manifest.rs`; defaulted serde field; plugin manifest tests | pass |
| Bounded/path-safe contribution validation | Manifest validation, install-time validation, canonical containment checks in `src/plugin/contributions.rs` | pass |
| Runtime asset bridge | `AssetContext` -> `ProjectAssetSnapshotBuilder` plugin source inputs; `tests/plugin_contributions.rs` | pass |
| Deterministic provenance and namespacing | `SourceKind::Plugin`, `PluginFile`, `plugin:<id>:<name>` identities, plugin/version metadata | pass |
| Native precedence | Project-native assets retain their normal names; plugin assets are namespaced and do not replace them | pass |
| Workspace isolation and immutable pins | `active_plugin_assets_are_namespaced...`, `workspace_without_activation...`, and daemon refresh wiring | pass |
| MCP ownership and origin | `McpServerOrigin`, `clone_configured_servers`, plugin reconciliation in `src/mcp/mod.rs` | pass |
| MCP collision/disable behavior | `mcp_reconciliation_removes_only_plugin_origin_and_rejects_config_collision` | pass |
| Restart/install reconstruction | Installed plugin manifests are rehydrated without runtime execution; activation remains M005 durable state | pass |
| Operator visibility | Plugin info/doctor contribution counts and bounded diagnostics; `docs/PLUGINS.md` | pass |

## 3. Production implementation evidence

- `src/plugin/manifest.rs` defines declarative skills, agents, instructions,
  and local/remote MCP declarations with explicit bounds and credential-key
  rejection.
- `src/plugin/contributions.rs` resolves only active installed plugins,
  canonicalizes contribution paths, rejects symlink/path escapes, and records
  plugin ID/version provenance plus bounded diagnostics.
- `src/agent/asset_snapshot_builder.rs`, `src/skills/registry.rs`, and
  `src/agent/instructions.rs` route passive inputs through the existing
  snapshot/registry/parser owners. Plugin identities are deterministic and
  project-native identities retain precedence.
- `src/core/daemon.rs` attaches the resolved contribution set to the explicit
  workspace asset context. `src/agent/turn_runtime.rs` uses a turn-scoped
  MCP service cloned from configured servers before reconciling pinned plugin
  servers, preventing cross-workspace plugin-server leakage.
- `src/mcp/mod.rs` owns plugin-origin metadata, collision checks, connection,
  removal, and reconciliation. No plugin-specific MCP client exists.
- Installed plugin rehydration in `src/plugin/mod.rs` reads manifests only;
  passive discovery does not invoke process/WASM plugin runtimes.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo check -p codegg --lib
rtk cargo test -p codegg --lib plugin -- --test-threads=1
rtk cargo test --test plugin_contributions -- --test-threads=1
rtk cargo test --test mcp -- --test-threads=1
rtk cargo test --test asset_snapshot -- --test-threads=1
rtk cargo clippy -p codegg --all-targets --locked -- -D warnings
rtk scripts/verify.sh quick
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_discovery_invariants.py
rtk python3 scripts/check_scheduler_bypass.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk scripts/verify.sh full
```

### Results

- Focused plugin tests passed: 367 tests.
- New integration tests passed: 5/5.
- MCP tests passed: 26/26; asset snapshot tests passed: 8/8.
- Root all-target Clippy passed with `-D warnings`.
- `scripts/verify.sh quick` passed.
- The default-feature workspace test matrix in `scripts/verify.sh full`
  completed with zero failures, including all root integration tests and
  workspace-crate tests reached before the feature-gated phase.
- The feature-gated full-test phase was attempted with
  `server,plugins,lsp-test-support`; this local run did not produce progress
  after entering the feature build and was interrupted. The feature-gated
  path is not required by the M006 verification contract; no feature-gated
  failure was observed.
- Daemon-CWD, project-agent-PWD, discovery-invariant, and scheduler-bypass
  guards passed. The project-catalog guard reports the pre-existing repository
  version mismatch (`STORAGE_LAYOUT_VERSION` 45 versus guard expectation 44);
  its other six checks pass and M006 does not alter storage layout.
- `git diff --check` passed before both commits.

## 5. Invariant review

- Passive discovery parses manifests and bounded asset files only; it never
  starts a plugin runtime or executes a sibling script/resource.
- All asset construction flows through `ProjectAssetSnapshotBuilder`, and all
  MCP lifecycle operations flow through `McpService`.
- Plugin ID/version, contribution kind/path, source class, and MCP origin are
  retained for inspection. Canonical names use `plugin:<plugin-id>:<name>`.
- Project-native definitions are not silently shadowed by plugin defaults.
- Workspace activation is taken from explicit M005 context, and each turn
  receives a complete immutable contribution set.
- Disabling or removing a plugin changes later refresh generations and MCP
  reconciliation; existing pinned asset snapshots are not mutated.
- Plugin activation and runtime trust do not grant agent, tool, provider, or
  MCP authority. Existing policy and exposure checks remain authoritative.

## 6. Failure, cancellation, restart, and contention review

- Invalid declared paths are diagnosed and omitted individually; malformed
  manifest declarations fail install validation. One plugin's bad contribution
  cannot corrupt another installed plugin.
- Canonical path containment and symlink checks prevent contribution escapes.
- MCP connection failures are reported per server and do not invalidate
  unrelated assets or configured/plugin origins.
- Configured MCP servers cannot be overwritten by plugin declarations, and
  stale removal targets only plugin-owned origins.
- Restart rehydrates installed manifests and resolves durable activation; a
  disabled or missing plugin contributes nothing and produces no ghost asset.
- Turn-scoped MCP services begin from configured servers, so one workspace's
  plugin reconciliation cannot mutate another workspace's plugin surface.

## 7. Migration and compatibility review

- `contributions` is optional and serde-defaulted; existing manifests and
  executable capability variants remain valid.
- Existing native/foreign skill source ordering remains intact; plugin source
  is an explicit additional class with documented lower precedence and
  namespaced identity.
- Existing configured MCP definitions retain their authority and origin.
- No database migration, marketplace change, scheduler change, or credential
  storage migration was introduced.

## 8. Security review

- Relative paths, bounded counts/lengths, canonical containment, and symlink
  checks are enforced before contribution use.
- Credential-looking environment/header keys are rejected for plugin MCP
  declarations, and diagnostics do not include secret values or asset bodies.
- MCP declarations are translated to normal service configuration; plugin
  metadata does not bypass normal transport, DNS/redirect, exposure, or
  permission controls.
- Passive asset parsing does not invoke process/WASM runtimes and does not
  grant executable authority to contributed agents.

## 9. Documentation and operations

Updated:

- `architecture/plugin.md`
- `architecture/mcp.md`
- `docs/PLUGINS.md`
- `plans/implementation/runtime-assets/006-plugin-declarative-asset-mcp-contributions.md`
- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md`
- `plans/registry.md`

Plugin info/doctor now expose bounded contribution inventory and validation
diagnostics, while runtime asset and MCP structures retain origin metadata for
operator diagnosis.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| low | `check_project_catalog_invariants.py` expects storage layout version 44 while the repository is at 45 | Pre-existing, unrelated to M006; six of seven guard checks pass. Separate guard-maintenance work remains the owner. |

No M006 correctness, security, compatibility, or ownership finding remains.

## 11. Roadmap disposition

M006 is closed. The plugin declarative contribution addendum is closed with
the implementation commit above and this accepted closure record. Its future
work remains explicitly deferred: remote catalogs, dependency resolution,
automatic updates, executable plugin schedulers, browser integration, and
arbitrary hook expansion.

## 12. Registry updates and unblock audit

- M006 moved from `ready` to `closing` when implementation landed, then from
  `closing` to `closed` with this record.
- M006 was removed from dependency-ready work and added to recently closed
  control points.
- The addendum and implementation plan now identify M006 as closed/implemented.
- All registry `blocked` rows and the affected runtime-assets roadmap were
  audited. No registered future plan depends on M006, so no future plan could
  be promoted or otherwise unblocked by this closure. The existing runtime
  safety M012 blocker remains unchanged because it depends on runtime safety
  M011, not runtime-assets M006.
