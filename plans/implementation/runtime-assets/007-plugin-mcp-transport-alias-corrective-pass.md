# Runtime Assets Milestone 007 — Plugin MCP Transport Alias Corrective Pass

Status: implemented

Repository baseline: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Source corrective roadmap:

- `plans/subsystems/runtime-assets-plugin-contributions-corrective-addendum.md`

Original milestone and closure corrected by this pass:

- M006: `plans/implementation/runtime-assets/006-plugin-declarative-asset-mcp-contributions.md`
- M006 closure: `plans/closure/runtime-assets/006-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Applicable architecture:

- `architecture/plugin.md`
- `architecture/mcp.md`
- `docs/PLUGINS.md`

Primary class: corrective capability / compatibility

## 1. Objective

Fix the plugin MCP transport contract so every transport alias accepted by `PluginMcpServerContribution` is translated deterministically into the existing `McpService` transport vocabulary, and add production-path tests that exercise actual reconciliation instead of only validation/collision branches.

This is a narrow correction to M006. Preserve the existing passive contribution schema, plugin activation model, `McpService`, MCP exposure/security behavior, runtime-asset ownership, and namespaced plugin server identities. Do not create a plugin-specific MCP client, transport, registry, or configuration layer.

## 2. Discovered defect

`PluginContributions::validate()` currently accepts four server-type spellings:

```text
local
stdio
remote
http
```

and correctly requires `command` for local/stdio and `url` for remote/http.

`McpService::reconcile_plugin_servers()` forwards `declaration.server_type` unchanged into `McpService::connect_from_config()`.

`connect_from_config()` currently accepts only:

```text
local
remote
```

Therefore a plugin declaration using the documented/validated aliases `stdio` or `http` passes manifest validation and contribution resolution but fails at connection time with `unknown server type`.

This is a real capability defect because validation promises compatibility that the runtime path does not implement.

## 3. Why original verification missed the defect

The M006 integration fixture uses `server_type: "stdio"` only in a configured-server collision case. Reconciliation detects the collision and returns before calling `connect_from_config()`, so the alias never reaches the failing production branch.

The manifest negative fixture also uses `type = "http"`, but it is intentionally rejected for an embedded credential header before connection. It verifies secret-safe validation, not transport compatibility.

The M006 closure therefore had validation, namespacing, origin, collision, and removal coverage without a direct successful reconciliation fixture for each accepted alias.

## 4. Invariants that must not regress

- plugin MCP declarations remain passive manifest data until translated by CodeGG.
- `McpService` remains the only MCP connection/lifecycle owner.
- configured MCP servers cannot be overwritten by plugin declarations.
- plugin server identities remain deterministically namespaced and origin-tagged.
- plugin activation does not bypass MCP exposure, transport security, DNS/redirect policy, OAuth, or tool permissions.
- legacy canonical values `local` and `remote` continue to work.
- accepted aliases `stdio` and `http` must have exactly the same semantics as their canonical equivalents.
- malformed/unsupported transport strings fail during validation, before attempting a connection.
- no additional remote marketplace or credential-storage behavior is introduced.

## 5. Required production changes

### 5.1 Canonical transport translation

Establish one canonical translation boundary before `connect_from_config()`.

Preferred mapping:

```text
local  -> local
stdio  -> local
remote -> remote
http   -> remote
```

Do not duplicate this mapping in manifest validation, contribution resolution, and MCP service call sites independently.

Preferred implementation shapes, in order:

1. a small typed transport enum shared by plugin contribution translation and `McpService` configuration where it can be introduced without broad churn;
2. a narrow `PluginMcpServerContribution::canonical_server_type()` / equivalent helper returning the existing `local`/`remote` vocabulary;
3. extending `connect_from_config()` to accept aliases only if that is already the canonical config parsing boundary for non-plugin MCP configuration.

Choose the smallest shape that leaves one source of truth and avoids changing unrelated configured-MCP behavior.

### 5.2 Validation consistency

Validation and runtime translation must use the same accepted set.

Required behavior:

- `local` and `stdio` require a command and reject URL-only declarations;
- `remote` and `http` require a URL and reject command-only declarations;
- unsupported values fail validation with a bounded diagnostic;
- canonicalization must not alter server names, environment, headers, args, timeout, plugin origin, or activation semantics.

### 5.3 Reconciliation path

`McpService::reconcile_plugin_servers()` must translate the alias before calling the normal connection path and must continue to:

- reject configured-server collisions;
- reject another plugin's ownership collision;
- preserve an existing same-plugin server according to current reconciliation semantics;
- remove only stale plugin-origin servers;
- retain per-server failure diagnostics without invalidating unrelated plugin assets.

Do not add a second `PluginMcpService` or a wrapper that owns connection lifecycle.

## 6. Protocol, storage, and migration

No database/storage migration is required.

No public protocol change is expected. `PluginManifest` remains backward compatible and `contributions` remains optional.

If documentation currently describes both alias and canonical forms, keep that compatibility. If it describes only one vocabulary, document accepted aliases explicitly after the implementation is made true.

## 7. Ordered work packages

### WP A — Centralize transport canonicalization

Add one small translation function/type and make validation and runtime reconciliation agree on it.

Acceptance evidence:

- table-driven unit tests cover all four accepted spellings;
- unknown values are rejected;
- local/stdio and remote/http field requirements remain equivalent.

### WP B — Exercise successful production-path reconciliation

Add integration coverage that reaches the normal `connect_from_config()` path or the narrowest production translation immediately before it.

For stdio/local:

- prefer a deterministic fake/local executable fixture already used by MCP tests, or a harmless test helper executable under repository control;
- do not rely on an external network service.

For HTTP/remote:

- use an existing local fake HTTP/SSE MCP fixture if available;
- otherwise test canonical config translation directly and keep transport-network behavior covered by the existing MCP suite rather than adding brittle network infrastructure.

Acceptance evidence:

- a `stdio` plugin declaration is translated to the normal local/stdio MCP lifecycle rather than rejected as unknown;
- an `http` declaration is translated to the normal remote lifecycle/config path;
- canonical `local` and `remote` still pass the same tests.

### WP C — Regression and closure

Retain collision/origin/removal tests and add explicit assertions that alias translation does not bypass those rules.

Acceptance evidence:

- configured collision remains non-destructive;
- plugin origin metadata is correct after successful connection;
- disabling/removing the plugin still reconciles only its servers;
- no duplicate MCP runtime or registry is introduced.

## 8. Failure, restart, and contention semantics

- translation is pure/deterministic and does not create an additional failure state;
- connection failures retain existing per-server diagnostics;
- daemon restart reconstructs the same canonical transport from the persisted plugin manifest and durable M005 activation state;
- no hidden in-memory alias state is authoritative;
- concurrent workspace reconciliation continues to use the current turn/workspace MCP isolation semantics.

## 9. Security review requirements

This corrective pass must confirm that alias handling changes only transport spelling.

It must not:

- relax credential-key rejection;
- inject shell interpretation into stdio command/args handling;
- bypass HTTP URL/DNS/redirect checks;
- broaden raw MCP tool exposure;
- let plugin metadata grant tool permission;
- emit environment/header secret values into diagnostics or tests.

## 10. Required tests

At minimum:

- validation table for `local`, `stdio`, `remote`, `http`, and unsupported value;
- canonicalization table for all accepted values;
- successful stdio alias reconciliation path;
- HTTP alias translation/reconciliation path at the narrowest deterministic production seam;
- canonical `local`/`remote` regression;
- configured-server collision remains non-destructive;
- plugin-origin removal affects only stale plugin servers;
- activation-disabled workspace does not materialize contributed MCP servers.

## 11. Required verification

```bash
cargo fmt --check --all
cargo test -p codegg plugin --lib
cargo test --test plugin_contributions
cargo test --test mcp
cargo clippy -p codegg --all-targets --locked -- -D warnings
scripts/verify.sh quick
git diff --check
```

Use existing test selectors if names differ. Do not add a new CI lane or remote marketplace/network matrix.

## 12. Documentation updates

Update as needed:

- `architecture/plugin.md` — canonical/accepted MCP transport spellings;
- `architecture/mcp.md` — translation ownership if currently ambiguous;
- `docs/PLUGINS.md` — valid manifest examples;
- corrective roadmap and registry;
- new closure record `plans/closure/runtime-assets/007-status.md`.

Do not rewrite the M006 closure record. M007 exists because M006's accepted verification did not exercise the alias connection branch.

## 13. Acceptance criteria

- every transport string accepted by plugin manifest validation reaches a supported canonical `McpService` transport path;
- `stdio` behaves exactly as `local` and `http` behaves exactly as `remote` for configuration semantics;
- runtime and validation share one accepted transport mapping;
- successful production-path tests cover the aliases, not only collision/negative branches;
- configured/plugin origin collision and removal behavior remains unchanged;
- no new MCP runtime/client/registry is introduced;
- focused tests, Clippy, and `scripts/verify.sh quick` pass;
- M007 closure records the original M006 coverage gap and confirms no remaining correctness issue in this scope.

## 14. Stop conditions

Stop and report when:

- fixing aliases requires redesigning the MCP transport stack;
- a proposed solution creates plugin-specific connection ownership;
- HTTP alias coverage would require unstable external network dependencies rather than a local fixture/translation test;
- configured MCP behavior would need a breaking compatibility change;
- implementation begins unrelated plugin marketplace, dependency, hook, or scheduler work.

## 15. Closure evidence required

The M007 closure record must include:

- implementation commit(s);
- explicit link to M006 and `plans/closure/runtime-assets/006-status.md`;
- explanation of why prior tests did not reach the failing connection path;
- accepted-alias validation/canonicalization matrix;
- successful runtime-path evidence for stdio and HTTP aliases at the appropriate seam;
- collision/origin/removal regression outcomes;
- exact verification commands/outcomes;
- unresolved findings by severity and final disposition.
