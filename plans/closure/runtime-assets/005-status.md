# Runtime Assets Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/005-durable-context-aware-plugin-activation.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md#6-dependency-graph`

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Implementation commits:

- `0c13856` — add durable global/workspace activation, context-bound service
  views, management persistence, stale-install protection, and documentation.
- `15c761c` — add scoped command and hook visibility regression coverage.

## 1. Executive finding

M005 is strictly closed. Plugin installation remains represented by the
existing registry/source metadata, while activation is a separate durable
daemon-owned state file. Global defaults and stable workspace overrides resolve
deterministically into an immutable `ResolvedPluginActivationSet`. The daemon
pins that set into the `PluginService` used to construct each turn, allowing
concurrent workspaces to differ without duplicating plugin runtimes or
registries. Management operations persist state transactionally, expose scope
and source, and remove activation records before uninstall. M006 is therefore
dependency-ready and was moved to `ready` in the same governance change.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Durable activation is separate from installation | `src/plugin/activation.rs`; `PluginActivationStore`; manager enable/disable tests | pass | Activation is stored in daemon-scoped `plugin-activation.json`; manifests are not mutated. |
| Global/default and workspace override precedence | `global_and_workspace_precedence_is_deterministic`; `PluginActivationScope` resolution | pass | Workspace override wins, then global, then migration default. |
| Activation survives restart/reconstruction | `records_survive_store_reconstruction` | pass | A new store instance resolves the persisted decision. |
| Legacy missing records preserve expected behavior | `MigrationDefault` resolver path and management views | pass | Non-builtin plugins without a record remain active, matching prior registration behavior. |
| Builtin compatibility is protected | `builtin_policy_is_not_disabled_by_third_party_records`; manager builtin guard | pass | Builtin source metadata is resolved through explicit builtin policy and cannot be disabled by management. |
| Workspace isolation covers commands and hooks | `pinned_services_isolate_workspaces_and_ignore_later_changes` | pass | Opposite workspace states produce opposite command/hook visibility from the same registry. |
| In-flight activation is immutable | Same pinned-service test | pass | Changing workspace A after pinning does not change the existing service; a later service sees the new state. |
| Stale or unknown activation fails safely | `stale_identity_is_inactive_and_diagnosed`; resolver unknown-record diagnostics | pass | Version/install-path mismatch stays inactive and emits bounded diagnostics. |
| Enable/disable persistence is transactional | `PluginActivationStore::set` rollback; manager live-state rollback on durable failure | pass | Registry changes are committed before durable state and reverted if persistence fails. |
| Uninstall/reinstall cannot revive unrelated state | manager uninstall cleanup; install version/path identity matching | pass | All records are removed before unregister; changed version/path records are stale and inactive. |
| Existing runtime/policy/permission authority remains intact | `PluginService` still dispatches runtimes and policy checks; only registry visibility is context-filtered | pass | No new runtime, permission bypass, scheduler, or frontend authority was added. |
| M006 receives a stable interface without passive contribution code | Public `ResolvedPluginActivationSet`; no manifest contribution changes | pass | M006 is ready and remains the next implementation milestone. |

## 3. Production implementation evidence

- `src/plugin/activation.rs` owns the versioned durable activation schema,
  atomic JSON writes, scoped records, install identity checks, precedence,
  builtin policy, unknown/stale diagnostics, and immutable resolved views.
- `PluginRegistry` retains the installed catalog and adds capability queries
  against an explicit active-ID set. Its legacy live `enabled` field remains a
  compatibility surface, not the durable authority.
- `PluginService` shares the registry, runtimes, policy, and activation store;
  `for_workspace` resolves and pins one activation set. Command and hook
  dispatch use that pin, so active turns do not observe later management
  changes.
- `CoreDaemon::TurnSubmit` resolves the service against its explicit
  `ExecutionContext` workspace ID before passing it to `TurnRunInput`.
  Process-global current-directory inference was not introduced.
- `PluginManager` persists global enable/disable, provides workspace-scoped
  management APIs, reports active scope/source/diagnostics, and removes all
  activation records before `remove`/`uninstall` unregister the plugin.
- Builtin plugins retain explicit `PluginSourceMetadata::builtin()` policy.
  Existing executable invocation still flows through `PluginService`, trust,
  policy, permissions, and the existing runtime implementations.

## 4. Verification executed

### Commands run

```bash
rtk cargo check -p codegg --lib
rtk cargo fmt --all
rtk cargo test -p codegg plugin:: --lib
rtk cargo test -p codegg plugin --lib
rtk cargo test -p codegg plugin::service::tests::pinned_services_isolate_workspaces_and_ignore_later_changes --lib
rtk cargo clippy -p codegg --lib -- -D warnings
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk scripts/verify.sh quick
rtk git diff --check
```

### Results

- The focused plugin suite passed with 365 tests.
- The scoped service isolation test passed independently.
- Targeted Clippy passed with `-D warnings`.
- Daemon-CWD and execution-ownership guards passed.
- `scripts/verify.sh quick` passed, including formatting, generated-agent
  checks, core-boundary/sandbox/execution-ownership guards, and locked
  workspace all-target compilation.
- Diff hygiene passed before the governance-only closure edits. The final
  governance commit is documentation-only and is checked again before push.
- All results are local evidence; no additional hosted CI lane was required by
  this plan.

## 5. Invariant review

- **Installation versus activation:** installed plugin metadata remains in the
  registry/source records; activation records are separate and never written
  into plugin manifests.
- **Explicit identity:** workspace overrides use stable workspace IDs. The
  activation store rejects path-like/invalid workspace IDs; turn resolution
  receives the daemon's typed explicit workspace identity.
- **Immutable in-flight behavior:** `PluginService::for_workspace` stores an
  `Arc<ResolvedPluginActivationSet>` and all context-bound registry queries use
  that set.
- **Shared runtime ownership:** workspaces share one installed registry and
  runtime implementation; no per-workspace clone or divergent execution
  authority was created.
- **Builtin compatibility:** builtin source metadata is resolved separately
  and management refuses to disable required builtin plugins.
- **Policy and permissions:** activation only filters visibility. Existing
  `PluginService` policy and runtime permission checks remain in the invocation
  path.
- **Safe stale state:** activation records retain observed version and install
  path; mismatches are inactive and diagnostic rather than executable.

## 6. Failure and recovery review

- **Atomic writes:** the store writes a temporary file and renames it into
  place. In-memory state is restored when persistence fails.
- **Management contention:** a shared async mutex serializes writes and
  monotonically assigns revisions, yielding deterministic last-committed
  results for same-scope requests.
- **Live/durable disagreement:** global management transitions validate the
  live registry before writing durable state and roll live state back on write
  failure.
- **Restart:** store reconstruction reloads the file; missing plugin inventory
  is diagnosed as unknown and never activated.
- **Uninstall race:** activation records are removed before unregister and
  filesystem deletion. A persistence failure leaves the plugin registered and
  installed for retry.
- **In-flight change:** an already pinned service remains unchanged while later
  service construction observes the new record.
- **Malformed state:** unsupported activation schema is treated as unavailable
  and the default plugin service is disabled rather than silently activating
  untrusted or stale plugin state.

## 7. Migration and compatibility review

- The activation file is schema-versioned at version 1 and additive; absent
  records follow the documented migration default for non-builtin plugins.
- Existing manifests, plugin capability variants, runtime types, and protocol
  invocation types remain unchanged.
- Existing unscoped management methods remain available and now mean global
  durable overrides; workspace-specific methods add scope without removing the
  prior call shape.
- The registry's legacy `enabled` field and query methods remain compatible for
  non-context callers. Daemon turns use the new immutable resolved view.
- No database migration or installed-plugin manifest migration is required.
  Older binaries will not understand the new activation file and may retain
  their historical in-memory behavior; the new binary fails closed on an
  unsupported activation schema.

## 8. Security review

- Activation does not grant plugin trust, filesystem, network, environment,
  secret, UI, hook, or tool permissions. Existing policy checks remain
  authoritative.
- Workspace activation keys reject path separators, whitespace, control
  characters, and other invalid identity content through the shared identity
  parser, preventing path spoofing as a workspace identity.
- Stale install identity checks prevent a changed version or install path from
  inheriting an old activation record.
- Atomic writes and bounded JSON records avoid partial durable state and do not
  store plugin bodies, credentials, or executable content.
- Unknown plugin records produce diagnostics and are never included in active
  IDs. Builtin compatibility is explicit rather than inherited from an
  arbitrary trust label.

## 9. Documentation and operations

Updated:

- `architecture/plugin.md` — installation/activation authority, scope,
  immutable service pinning, and management APIs.
- `docs/PLUGINS.md` — operator commands, persistence, precedence, stale state,
  and uninstall behavior.
- `plans/implementation/runtime-assets/005-durable-context-aware-plugin-activation.md`
  — implemented status.
- `plans/implementation/runtime-assets/006-plugin-declarative-asset-mcp-contributions.md`
  — ready-for-handoff dependency status.
- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md` — M005
  closed and M006 ready.
- `plans/registry.md` — M005 recently closed and M006 promoted to ready.

Operator visibility is provided by management list/info activation scope,
source, and stale diagnostics. The existing doctor/install path checks remain
in place; no new CI lane or release mechanism was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None in M005 scope | — | — |

## 11. Roadmap disposition

M005 is closed. The durable/context-aware activation contract is complete and
the next capability milestone, M006, is ready for handoff. M006 must consume
`ResolvedPluginActivationSet` and route passive contributions through the
existing runtime-asset and MCP owners; it must not read the activation file
directly or create duplicate runtimes.

## 12. Registry updates

- Added `plans/closure/runtime-assets/005-status.md` as the accepted closure
  gate.
- Removed M005 from dependency-ready work and added it to recently closed
  control points.
- Changed the runtime-assets plugin-contributions roadmap to `active`, with
  M005 `closed` and M006 `ready`.
- Removed M006 from blocked work and registered it under dependency-ready
  implementation plans.
- Audited the registry and affected dependency graph: M006 was the only
  registered plan hard-blocked on M005, and all other listed dependencies and
  interfaces are already stable. No additional future plan was unblocked.
