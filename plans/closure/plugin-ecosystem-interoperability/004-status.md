# Plugin Ecosystem Interoperability M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/plugin-ecosystem-interoperability/004-extension-catalog-and-install.md`

Source subsystem roadmap:

- `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md`

Repository baseline reviewed: `2f64c9e16f9ee96ea4d47ed897201f4c24d4606f`

Implementation commits:

- `c23b22c` — Add Playwright bundle and extension catalog
- `fefa622` — Implement extension catalog discovery and host proposals

## 1. Executive finding

M004 is complete. Marketplace stubs are replaced by a bounded, provenance-aware
catalog service with bundled and explicit local/HTTPS source loading, deterministic
merging, read-only extension discovery, digest-checked remote package staging,
and canonical package installation. Models can discover entries or create a
host-visible proposal, but cannot supply URLs/paths/digests or install/enable
code.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Typed catalog/source model | `ExtensionCatalogSource`, `ExtensionCatalogEntry`, artifact/format/provenance fields | pass |
| Bounded catalog loading | schema/version, byte, entry, string/component bounds | pass |
| Hardened source fetch | HTTPS-only URL loading through ordinary HTTP client and response cap | pass |
| Canonical remote install | SHA-256 check, safe tar extraction, package identity/version check, M002 installer | pass |
| Model discovery | deferred read-only `extension_search` over resolved catalog snapshot | pass |
| Host-mediated proposal | deferred `extension_install_request` returns exact metadata and explicit-confirmation state only | pass |
| Activation separation | installer copies/registers but does not enable or connect the package | pass |

## 3. Production implementation evidence

`MarketplaceService` owns catalog metadata only; it does not replace
`PluginRegistry` or activation ownership. Catalog entries are bounded and
retain source ID/tier. The explicit `install_entry` host method resolves one
exact entry, stages a local or HTTPS artifact, verifies identity/version and
optional SHA-256, then delegates to `install_from_path_into`.

`extension_search` is deferred and read-only. `extension_install_request`
accepts only exact catalog ID/version and returns a structured pending proposal;
it never downloads or installs. The host/UI can use the same exact entry with
the explicit installer and separately choose enable/activation.

## 4. Verification executed

```text
rtk cargo test -p codegg --lib plugin::marketplace::tests --locked     # 2 passed
rtk cargo test -p codegg --lib plugin::playwright::tests --locked       # 1 passed
rtk cargo test -p codegg --lib tool::tests --locked                     # 21 passed
rtk cargo check -p codegg --locked                                      # passed
```

The final repository-wide quick verification is run after this closure change
and recorded in the delivery summary.

## 5. Invariant review

- Catalog metadata is never execution authority.
- PluginManager/installer and PluginActivationStore retain install/activation
  ownership.
- Model input cannot choose arbitrary package URL/path/digest or answer its own
  approval request.
- Catalog tier is provenance/UI metadata, not a trust bypass.
- Installed packages remain inactive until explicit activation.
- Existing installed plugins are unaffected by catalog outage or refresh failure.

## 6. Failure and recovery review

Malformed sources fail independently and do not disable installed plugins.
Remote non-HTTPS, oversized, malformed, digest-mismatched, traversal-bearing,
multi-root, and identity-mismatched artifacts fail before canonical install.
Partial staging is temporary and existing installer rollback/locks remain the
commit boundary.

## 7. Migration and compatibility review

The old `MarketplacePlugin` local listing/search compatibility DTO remains
available, now using the unified package detector so passive packages appear
in local discovery. No database migration, automatic update path, package
manager integration, or trust-root change was added.

## 8. Security review

Catalog/package bytes and metadata are bounded. Remote catalogs/artifacts use
the hardened ordinary HTTP owner and HTTPS policy. Archive extraction rejects
links/traversal and package identity is read back from the parsed package.
Catalog URLs never come from model input, and package/catalog bodies and secrets
are not emitted in discovery results.

## 9. Documentation and operations

Catalog provenance, package format, component summaries, prerequisites, and
security notes are structured in the discovery result. The Playwright package
and `docs/playwright.md` document the explicit install/enable split. Existing
plugin management remains the host surface for the final install/activation
action.

## 10. Unresolved findings

None that prevent strict closure. Automatic updates, dependency solving,
central signing PKI, and arbitrary Git/package-manager installation remain
explicit non-goals.

## 11. Roadmap disposition

M004 is closed. The plugin ecosystem and harness interoperability roadmap is
now closed with M001-M004 closure records. No future plan in this workstream
remains blocked or ready; later catalog trust/update work would require a new
bounded plan and likely an ADR before implementation.

## 12. Registry updates

The implementation plan is marked `implemented`, the roadmap is marked
`closed`, and the registry records the workstream and M004 closure in this
status-change commit.
