# Plugin Ecosystem Interoperability M001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/plugin-ecosystem-interoperability/001-first-class-plugin-tools.md`

Source subsystem roadmap:

- `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md`

Repository baseline reviewed: `2f64c9e16f9ee96ea4d47ed897201f4c24d4606f`

Implementation commit:

- `c320117` — Implement context discovery and plugin interoperability foundations

## 1. Executive finding

M001 is complete. CodeGG-native plugin tool capabilities are represented in
manifest and protocol contracts, validated and indexed by `PluginRegistry`,
adapted into the canonical deferred `ToolRegistry`, and dispatched only by
`PluginService` after activation, policy, runtime, effect, and output checks.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Typed Tool capability | `PluginToolSpec` in manifest/protocol with bounded validation | pass |
| Typed invocation | `PluginCapabilityInvocation::Tool` and `PluginService::invoke_tool` | pass |
| Namespaced indexing | `PluginToolRegistration` and deterministic `plugin__...` names | pass |
| Canonical tool adapter | `src/tool/plugin.rs`, deferred registration in turn registry | pass |
| Activation/parent authority | pinned activation filtering, `PluginExecute` capability, normal tool surface | pass |
| Conservative effects/output | host-owned Mutating category, UI-effect filtering, 256 KiB cap | pass |

## 3. Production implementation evidence

Plugin tools are never routed through command aliases or called directly from
the agent loop. The adapter calls only `PluginService::invoke_tool`, which
constructs a typed protocol invocation, checks the pinned activation and plugin
policy, dispatches through the existing Process/WASM runtime owners, filters
UI effects, and bounds serialized output. Passive and unsupported builtin
runtime cases fail explicitly.

The turn registry receives only descriptors from the activation-pinned plugin
set. Tools are deferred and remain outside the core/curated palettes; normal
tool search and parent capability ceilings govern discoverability/callability.

## 4. Verification executed

```text
rtk cargo test -p codegg --lib plugin::registry::tests --locked        # 27 passed
rtk cargo test -p codegg --lib plugin::service::tests --locked         # 1 passed
rtk scripts/verify.sh quick                                             # passed
```

## 5. Invariant review

- ToolBroker/ToolRegistry remain the only model-tool boundary.
- Plugin declarations cannot lower host risk or permission requirements.
- Namespaces prevent native/raw-MCP/other-plugin shadowing.
- In-flight turns consume an activation-pinned descriptor set.
- Existing commands/hooks/panels/status/event capabilities remain additive and
  compatible.

## 6. Failure and recovery review

Invalid or duplicate tool declarations reject registration before indexing.
Disabled/passive/unsupported plugins fail closed. Runtime errors and bounded
diagnostics use existing plugin/tool error paths. Restart rehydrates the same
manifest/index contract without executing plugin code.

## 7. Migration and compatibility review

The Tool capability is additive to `manifest.toml` and the protocol. Existing
manifests without tools remain valid. No plugin dependency/lockfile, JavaScript
runtime, automatic installation, or storage migration was introduced.

## 8. Security review

Plugin-authored effect hints are diagnostic only. Every executable tool is
conservatively mutating, plugin execution is separately ceilinged, runtime
policy remains authoritative, and UI effects are discarded unless policy
allows them. Output is serialized and bounded before it reaches model context.

## 9. Documentation and operations

Plugin service/registry and tool adapter comments document the ownership
boundary. Existing plugin management and progressive-disclosure surfaces carry
the new namespaced/deferred tools without exposing secrets or raw runtime state.

## 10. Unresolved findings

None that prevent strict closure. Builtin plugin tools remain explicitly
unsupported until a builtin handler contract is separately specified.

## 11. Roadmap disposition

M001 is closed. M002 remains independently ready and is the next requested
plugin milestone. M003 and M004 remain blocked until M002's unified portable
package/install contract is closed.

## 12. Registry updates

The implementation plan is marked `implemented`, the roadmap records M001
closed/M002 ready, and the registry row is closed in this status-change commit.
