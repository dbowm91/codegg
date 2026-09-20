# Plugin Ecosystem Interoperability M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/plugin-ecosystem-interoperability/002-portable-agent-plugin-import.md`

Source subsystem roadmap:

- `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md`

Repository baseline reviewed: `2f64c9e16f9ee96ea4d47ed897201f4c24d4606f`

Implementation commit:

- `c320117` — Implement context discovery and plugin interoperability foundations

## 1. Executive finding

M002 is complete. CodeGG now detects native, Agent Plugins 1.0, and bounded
Claude-compatible package directories; imports only passive skills/MCP
contributions understood by existing owners; preserves unsupported-component
diagnostics and format/schema provenance; and installs the original package
without manufacturing a native manifest. Restart rehydrates the same package
through the canonical loader.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Format detector and precedence | `src/plugin/package.rs::detect_and_load` | pass |
| Local Agent Plugins 1.0 validation | bounded JSON, recognized `$schema`, metadata bounds | pass |
| Passive skills/MCP translation | fixed `skills/<name>/SKILL.md` discovery and root `mcp.json` translation | pass |
| PLUGIN_ROOT/PLUGIN_DATA | contribution resolution expands both host-owned placeholders | pass |
| Unsupported component safety | commands/hooks/LSP/extensions are diagnosed and ignored | pass |
| Unified installation/restart | install and default service rehydration use the same loader | pass |
| Provenance | source format/schema/unsupported components retained in registry/management metadata | pass |

## 3. Production implementation evidence

`PluginManifest::Passive` is a truthful non-executable runtime. Portable
packages cannot declare executable CodeGG capabilities. `install_from_path_into`
validates/canonicalizes, parses, validates contributions, locks by package
identity, copies the complete source tree, and preserves rollback behavior.
The default plugin service and management path parse the installed destination
again, so passive packages survive restart without executing package scripts.

Local MCP entries are translated into existing `PluginMcpServerContribution`
records; remote entries remain subject to the existing McpService DNS,
redirect, OAuth, and network policies. Placeholder expansion occurs only at
host contribution resolution, with `PLUGIN_DATA` rooted under CodeGG-managed
plugin data rather than the package tree.

## 4. Verification executed

```text
rtk cargo test -p codegg --lib plugin::package::tests --locked        # 2 passed
rtk cargo test -p codegg --lib plugin::install::tests --locked        # 34 passed
rtk cargo test -p codegg --lib plugin::registry::tests --locked       # 27 passed
rtk scripts/verify.sh quick                                             # passed
```

## 5. Invariant review

- No package script, command, hook, or runtime is executed during parsing or
  installation.
- Native `manifest.toml` compatibility is retained and takes precedence.
- Skills and MCP remain owned by ProjectAssetSnapshotBuilder and McpService.
- Canonical/symlink containment and existing install lock/rollback checks remain
  authoritative.
- Installation does not imply activation.

## 6. Failure and recovery review

Invalid package metadata fails the package as a whole; invalid individual skill
or MCP entries produce component-local diagnostics where safe. Copy failures
remove the partial destination. Installed passive packages with malformed
metadata are skipped during restart and never become executable authority.

## 7. Migration and compatibility review

No storage migration or runtime dependency was added. Existing native packages,
install locks, archive/path policies, and plugin activation persistence remain
compatible. Claude import is explicitly compatibility-only and does not claim
full Agent Plugins conformance.

## 8. Security review

JSON is size-bounded, skills are immediate-child regular files with canonical
root containment, and imported MCP declarations cannot bypass existing remote
network or local process policy. Secrets are not logged or copied into model
metadata. `PLUGIN_ROOT` is canonical and `PLUGIN_DATA` is separate/host-owned.

## 9. Documentation and operations

Package/parser and installer comments document passive semantics and provenance.
Plugin management views expose package format, schema, and unsupported component
inventory without dumping package bodies or credentials.

## 10. Unresolved findings

None that prevent strict closure. Remote catalogs, dependency solving, and
foreign executable command/hook semantics remain deferred to later milestones.

## 11. Roadmap disposition

M002 is closed. Dependency audit found both downstream hard gates satisfied:
M003 Playwright packaging can consume the canonical importer, and M004 catalog
installation can call the unified installer. Both are moved from blocked to
ready in the same registry update. M001's soft dependency is also closed.

## 12. Registry updates

The implementation plan is marked `implemented`, M003 and M004 are registered
as `ready`, and the closure record/registry updates are committed together.
