# Plugin Ecosystem Interoperability M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/plugin-ecosystem-interoperability/003-playwright-browser-integration-bundle.md`

Source subsystem roadmap:

- `plans/subsystems/plugin-ecosystem-interoperability-roadmap.md`

Repository baseline reviewed: `2f64c9e16f9ee96ea4d47ed897201f4c24d4606f`

Implementation commit:

- `c23b22c` — Add Playwright bundle and extension catalog
- `014ce8d` — Fix strict workspace lint findings

## 1. Executive finding

M003 is complete. CodeGG now ships repository-owned, Agent Plugins-compatible
Playwright CLI and opt-in MCP packages. The CLI skill is the default,
token-efficient workflow; the MCP companion is inert until explicitly enabled
and uses `--no-install` so missing npm packages fail closed. No browser engine,
Node runtime, or Playwright dependency entered the CodeGG binary.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Portable package A | `assets/portable-plugins/playwright-browser-testing` | pass |
| Optional MCP package | `assets/portable-plugins/playwright-mcp` with explicit opt-in README | pass |
| CLI-first workflow | bounded skill covering open/snapshot/interact/assert/artifacts/close | pass |
| Dependency diagnostics | `PlaywrightSupportReport` PATH-only detection and explicit version-probe state | pass |
| Security/profile policy | skill/docs prohibit implicit downloads, profile reuse, credentials, and page instruction authority | pass |
| No native browser surface | no browser tools/core palette/dependencies added | pass |

## 3. Production implementation evidence

Both packages are parsed by the canonical M002 passive importer. The browser
skill retains CodeGG's existing shell/test/image permissions; it does not grant
`allowed-tools` authority. The companion MCP configuration is separate and
uses an explicitly installed package with `npx --no-install`. The host
diagnostic only inspects PATH and reports that version probing must occur via
the scheduler-owned explicit doctor/shell surface; it never executes Node/npm
as a side effect of discovery.

## 4. Verification executed

```text
rtk cargo test -p codegg --lib plugin::package::tests --locked       # 2 passed
rtk cargo test -p codegg --lib plugin::marketplace::tests --locked    # 2 passed
rtk cargo test -p codegg --lib plugin::playwright::tests --locked     # 1 passed
rtk cargo check -p codegg --locked                                    # passed
```

The upstream guidance used for the package split distinguishes the concise
CLI/skill workflow from the persistent/introspective MCP workflow:
[Playwright CLI guidance](https://github.com/microsoft/playwright/blob/main/docs/src/getting-started-cli.md)
and [Playwright MCP guidance](https://github.com/microsoft/playwright-mcp).

## 5. Invariant review

- No browser engine or Node dependency is bundled.
- Installing a package does not download, connect, or activate a browser.
- MCP remains subject to existing McpService exposure and activation policy.
- Browser content and artifacts are treated as untrusted/bounded external data.
- Default/profile credentials are never implicitly reused.

## 6. Failure and recovery review

Missing Node/CLI/MCP package produces an actionable unavailable state rather
than an implicit download. A missing `--no-install` package prevents MCP
startup. The package is passive and can be disabled/uninstalled through normal
plugin management without special browser cleanup code.

## 7. Migration and compatibility review

The integration is additive repository assets and one diagnostic type. Node-less
CodeGG remains fully functional. Existing MCP, shell, test, image, asset, and
run-store ownership is unchanged; no storage migration was added.

## 8. Security review

The skill explicitly warns against prompt injection from pages, copied command
execution, credential entry, profile reuse, and unreviewed downloads. The
integration does not claim that an external Playwright/browser process is a
CodeGG sandbox boundary.

## 9. Documentation and operations

`docs/playwright.md` documents the package split, prerequisites, explicit setup,
artifact policy, and trust boundary. The package's SKILL.md is intentionally
small and high-signal; verbose browser reference remains upstream-owned.

## 10. Unresolved findings

None that prevent strict closure. Exact version probing remains an explicit
host command because passive discovery must not execute external processes.

## 11. Roadmap disposition

M003 is closed. M004 remains ready and can use the bundled catalog/importer;
there is no remaining hard dependency in this workstream.

## 12. Registry updates

The implementation plan is marked `implemented`, the roadmap records M003
closed/M004 ready, and the registry closure row is committed with this record.
